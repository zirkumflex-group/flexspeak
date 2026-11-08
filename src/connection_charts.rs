// Globally unique versions also distinguish histories belonging to different sessions.
pub(crate) fn next_revision() -> u64 {
    static REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    REVISION.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use rune::{Color, Shape, shape::LineCap};

use crate::usage::{ConnectionUsage, Health};

const WINDOW: Duration = Duration::from_secs(60);
const MAX_SAMPLES: usize = 60;

#[derive(Clone, Copy)]
pub enum Metric {
    LossIn,
    LossOut,
    Ping,
    Idle,
}

impl Metric {
    fn value(self, sample: ConnectionUsage) -> Option<f64> {
        let value = match self {
            Self::LossIn => sample.packet_loss_in.map(f64::from),
            Self::LossOut => sample.packet_loss_out.map(f64::from),
            Self::Ping => sample.ping_ms,
            Self::Idle => sample.idle_seconds.map(|value| value as f64),
        };
        value.filter(|value| value.is_finite() && *value >= 0.0)
    }

    fn health(self, sample: ConnectionUsage) -> Health {
        match self {
            Self::LossIn => Health::loss(sample.packet_loss_in),
            Self::LossOut => Health::loss(sample.packet_loss_out),
            Self::Ping => Health::ping(sample.ping_ms, sample.ping_deviation_ms),
            Self::Idle => Health::Unknown,
        }
    }

    fn minimum_scale(self) -> f64 {
        match self {
            Self::LossIn | Self::LossOut => 5.0,
            Self::Ping => 200.0,
            Self::Idle => 60.0,
        }
    }
}

pub struct ConnectionHistory {
    samples: VecDeque<(Instant, ConnectionUsage)>,
    now: Instant,
    pub revision: u64,
}

impl ConnectionHistory {
    pub fn new() -> Self {
        Self {
            samples: VecDeque::new(),
            now: Instant::now(),
            revision: next_revision(),
        }
    }

    pub fn push(&mut self, now: Instant, sample: ConnectionUsage) {
        self.samples.push_back((now, sample));
        self.advance(now);
    }

    pub fn advance(&mut self, now: Instant) {
        self.revision = next_revision();
        self.now = now;
        while self
            .samples
            .front()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) > WINDOW)
            || self.samples.len() > MAX_SAMPLES
        {
            self.samples.pop_front();
        }
    }

    pub fn buffer_bytes(&self) -> usize {
        self.samples.capacity() * std::mem::size_of::<(Instant, ConnectionUsage)>()
    }
}

/// Colours a chart paints with.
pub struct ChartPaint {
    pub grid: Color,
    pub band: Color,
    /// Good, fair, poor and unknown.
    pub health: [Color; 4],
}

impl ChartPaint {
    fn health(&self, health: Health) -> Color {
        let [good, fair, poor, unknown] = self.health;
        match health {
            Health::Good => good,
            Health::Fair => fair,
            Health::Poor => poor,
            Health::Unknown => unknown,
        }
    }
}

impl ConnectionHistory {
    /// The last minute of `metric` as a line `width` × `height` points: health-coloured
    /// segments, a translucent ping-variation band, and gaps where readings are missing.
    pub fn chart(&self, metric: Metric, width: f32, height: f32, paint: &ChartPaint) -> Shape {
        let mut shape = Shape::new();
        let inner_w = (width - 4.0).max(1.0);
        let inner_h = (height - 4.0).max(1.0);
        let deviation = |sample: &ConnectionUsage| {
            sample
                .ping_deviation_ms
                .filter(|v| v.is_finite() && *v >= 0.0)
        };
        let upper = self
            .samples
            .iter()
            .filter_map(|(_, sample)| {
                metric.value(*sample).map(|value| {
                    value
                        + if matches!(metric, Metric::Ping) {
                            deviation(sample).unwrap_or(0.0)
                        } else {
                            0.0
                        }
                })
            })
            .fold(metric.minimum_scale(), f64::max)
            * 1.05;
        let y = |value: f64| 2.0 + inner_h * (1.0 - (value / upper).clamp(0.0, 1.0) as f32);
        for fraction in [0.0, 0.5, 1.0] {
            let gy = 2.0 + inner_h * fraction;
            shape = shape.line(
                [2.0, gy],
                [inner_w + 2.0, gy],
                1.0,
                LineCap::Butt,
                paint.grid,
            );
        }
        struct SamplePoint {
            at: Instant,
            point: [f32; 2],
            band: Option<(f32, f32)>,
        }
        let mut previous: Option<SamplePoint> = None;
        for (at, sample) in &self.samples {
            let Some(value) = metric.value(*sample) else {
                previous = None;
                continue;
            };
            let age = self.now.saturating_duration_since(*at).as_secs_f32();
            let x = 2.0 + inner_w * (1.0 - age / WINDOW.as_secs_f32()).clamp(0.0, 1.0);
            let point = [x, y(value)];
            let band = if matches!(metric, Metric::Ping) {
                deviation(sample)
                    .map(|deviation| (y(value + deviation), y((value - deviation).max(0.0))))
            } else {
                None
            };
            let color = paint.health(metric.health(*sample));
            if let Some(old) = previous
                && at.saturating_duration_since(old.at) <= Duration::from_secs(15)
            {
                if let (Some((old_top, old_bottom)), Some((top, bottom))) = (old.band, band) {
                    shape = shape.fill(
                        vec![
                            [old.point[0], old_top],
                            [x, top],
                            [x, bottom],
                            [old.point[0], old_bottom],
                        ],
                        paint.band,
                    );
                }
                shape = shape.line(old.point, point, 1.8, LineCap::Round, color);
            } else {
                // A lone sample stays visible; missing readings and long gaps never get joined.
                shape = shape.line(point, point, 3.0, LineCap::Round, color);
            }
            previous = Some(SamplePoint {
                at: *at,
                point,
                band,
            });
        }
        shape
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timelines_are_bounded_expire_and_keep_missing_values_as_gaps() {
        let start = Instant::now();
        let mut history = ConnectionHistory::new();
        for index in 0..80 {
            history.push(
                start + Duration::from_millis(index * 100),
                ConnectionUsage {
                    ping_ms: Some(index as f64),
                    ..Default::default()
                },
            );
        }
        assert_eq!(history.samples.len(), MAX_SAMPLES);
        assert_eq!(history.samples.front().unwrap().1.ping_ms, Some(20.0));
        history.push(start + Duration::from_secs(8), ConnectionUsage::default());
        assert_eq!(Metric::Ping.value(history.samples.back().unwrap().1), None);
        assert_eq!(
            Metric::Ping.value(ConnectionUsage {
                ping_ms: Some(f64::NAN),
                ..Default::default()
            }),
            None
        );
        history.advance(start + Duration::from_secs(69));
        assert!(history.samples.is_empty());
    }
}

#[cfg(test)]
mod revision_tests {
    use super::*;

    #[test]
    fn chart_versions_distinguish_histories_samples_and_time() {
        let mut history = ConnectionHistory::new();
        assert_ne!(history.revision, ConnectionHistory::new().revision);
        let initial = history.revision;
        let now = history.now;
        history.push(now, ConnectionUsage::default());
        assert_ne!(history.revision, initial);
        let sampled = history.revision;
        history.advance(now + Duration::from_secs(1));
        assert_ne!(history.revision, sampled);
    }
}
