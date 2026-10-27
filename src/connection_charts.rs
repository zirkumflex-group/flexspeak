use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use iced::{Color, Point, Rectangle, Renderer, Theme, mouse, widget::canvas};

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

pub fn health_color(health: Health) -> Color {
    match health {
        Health::Good => Color::from_rgb8(115, 210, 151),
        Health::Fair => Color::from_rgb8(234, 187, 99),
        Health::Poor => Color::from_rgb8(244, 119, 139),
        Health::Unknown => Color::from_rgb8(145, 154, 178),
    }
}

pub struct ConnectionHistory {
    samples: VecDeque<(Instant, ConnectionUsage)>,
    now: Instant,
    caches: [canvas::Cache; 4],
}

impl ConnectionHistory {
    pub fn new() -> Self {
        Self {
            samples: VecDeque::new(),
            now: Instant::now(),
            caches: std::array::from_fn(|_| canvas::Cache::new()),
        }
    }

    pub fn push(&mut self, now: Instant, sample: ConnectionUsage) {
        self.samples.push_back((now, sample));
        self.advance(now);
    }

    pub fn advance(&mut self, now: Instant) {
        self.now = now;
        while self
            .samples
            .front()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) > WINDOW)
            || self.samples.len() > MAX_SAMPLES
        {
            self.samples.pop_front();
        }
        for cache in &self.caches {
            cache.clear();
        }
    }

    pub fn buffer_bytes(&self) -> usize {
        self.samples.capacity() * std::mem::size_of::<(Instant, ConnectionUsage)>()
    }
}

pub struct ConnectionChart<'a> {
    pub history: &'a ConnectionHistory,
    pub metric: Metric,
}

impl<Message> canvas::Program<Message> for ConnectionChart<'_> {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        vec![
            self.history.caches[self.metric as usize].draw(renderer, bounds.size(), |frame| {
                let width = (frame.width() - 4.0).max(1.0);
                let height = (frame.height() - 4.0).max(1.0);
                let upper = self
                    .history
                    .samples
                    .iter()
                    .filter_map(|(_, sample)| {
                        self.metric.value(*sample).map(|value| {
                            value
                                + if matches!(self.metric, Metric::Ping) {
                                    sample
                                        .ping_deviation_ms
                                        .filter(|v| v.is_finite() && *v >= 0.0)
                                        .unwrap_or(0.0)
                                } else {
                                    0.0
                                }
                        })
                    })
                    .fold(self.metric.minimum_scale(), f64::max)
                    * 1.05;
                let y = |value: f64| 2.0 + height * (1.0 - (value / upper).clamp(0.0, 1.0) as f32);
                for fraction in [0.0, 0.5, 1.0] {
                    let y = 2.0 + height * fraction;
                    frame.stroke(
                        &canvas::Path::line(Point::new(2.0, y), Point::new(width + 2.0, y)),
                        canvas::Stroke::default()
                            .with_color(Color::from_rgba(0.65, 0.7, 0.85, 0.10)),
                    );
                }
                struct SamplePoint {
                    at: Instant,
                    point: Point,
                    band: Option<(f32, f32)>,
                }
                let mut previous: Option<SamplePoint> = None;
                for (at, sample) in &self.history.samples {
                    let Some(value) = self.metric.value(*sample) else {
                        previous = None;
                        continue;
                    };
                    let age = self
                        .history
                        .now
                        .saturating_duration_since(*at)
                        .as_secs_f32();
                    let x = 2.0 + width * (1.0 - age / WINDOW.as_secs_f32()).clamp(0.0, 1.0);
                    let point = Point::new(x, y(value));
                    let band = if matches!(self.metric, Metric::Ping) {
                        sample
                            .ping_deviation_ms
                            .filter(|v| v.is_finite() && *v >= 0.0)
                            .map(|deviation| {
                                (y(value + deviation), y((value - deviation).max(0.0)))
                            })
                    } else {
                        None
                    };
                    let color = health_color(self.metric.health(*sample));
                    if let Some(old) = previous
                        && at.saturating_duration_since(old.at) <= Duration::from_secs(15)
                    {
                        if let (Some((old_top, old_bottom)), Some((top, bottom))) = (old.band, band)
                        {
                            let area = canvas::Path::new(|path| {
                                path.move_to(Point::new(old.point.x, old_top));
                                path.line_to(Point::new(x, top));
                                path.line_to(Point::new(x, bottom));
                                path.line_to(Point::new(old.point.x, old_bottom));
                                path.close();
                            });
                            frame.fill(&area, Color::from_rgba(0.75, 0.54, 0.96, 0.20));
                        }
                        frame.stroke(
                            &canvas::Path::line(old.point, point),
                            canvas::Stroke::default().with_width(1.8).with_color(color),
                        );
                    } else {
                        // A lone sample stays visible; missing readings and long gaps never get joined.
                        frame.fill(&canvas::Path::circle(point, 1.5), color);
                    }
                    previous = Some(SamplePoint {
                        at: *at,
                        point,
                        band,
                    });
                }
            }),
        ]
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
