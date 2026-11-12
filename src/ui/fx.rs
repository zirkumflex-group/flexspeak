//! The window's compositor: Rune's own frame, with received video drawn in the place of each
//! video element, and the Monster cards' rune-fx layers and 3D cans in the place of theirs. A
//! view marks an element with [`card_fx`]; the compositor finds the marked instances each frame
//! and draws the matching layer there.

use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;

use rune::El;
use rune::compose::{Compositor, DeviceKey, Frame};
use rune::render::{EffectPhase, EffectSite, KIND_EFFECT};
use rune_fx::{
    AuroraParams, ElectricParams, FireParams, FireStyle, Fx, FxBlend, FxFrame, FxLayer, FxOptions,
    FxRenderer, FxSlot, GlowParams, GlowStyle, GrainParams, MeshGradientParams, MetaballParams,
    PlasticParams, PulseParams, PulseStyle, Shape, ShimmerParams, SparkleParams, Stops,
    SwirlParams, VoronoiParams, WaveParams, rgb,
};

use crate::App;
use crate::monster::{Beat, FLAVOURS, Rarity};

/// The first mark of the Monster card layers; see `CardFx`.
const CARD_MARKS: u32 = 16;

/// One layer of a Monster card, in the order a card stacks them. Which of them a card gets, and
/// how each is tuned, depends on its rarity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardFx {
    /// Flames around the card, behind it.
    Fire,
    /// The card's painted background, a different style for each rarity.
    Art,
    Aurora,
    Grain,
    Shimmer,
    Sparkles,
    Electric,
    /// The card's plastic frame, in its rarity's colour.
    Frame,
    /// The face-down card a reveal starts from.
    Back,
    RevealGlow,
    RevealSparkles,
    RevealElectric,
    RevealFire,
    /// The shower of stars as a rare card lands.
    Burst,
    Ripple,
}

impl CardFx {
    const ALL: [Self; 15] = [
        Self::Fire,
        Self::Art,
        Self::Aurora,
        Self::Grain,
        Self::Shimmer,
        Self::Sparkles,
        Self::Electric,
        Self::Frame,
        Self::Back,
        Self::RevealGlow,
        Self::RevealSparkles,
        Self::RevealElectric,
        Self::RevealFire,
        Self::Burst,
        Self::Ripple,
    ];

    fn mark(self) -> u32 {
        CARD_MARKS + self as u32
    }

    fn from_mark(mark: u32) -> Option<Self> {
        Self::ALL
            .get(mark.checked_sub(CARD_MARKS)? as usize)
            .copied()
    }
}

/// Which layers a card of this rarity draws: more of them the rarer it is.
pub fn card_layers(rarity: Rarity) -> (&'static [CardFx], &'static [CardFx]) {
    use CardFx::*;
    match rarity {
        Rarity::Common => (&[Art, Grain], &[]),
        Rarity::Uncommon => (&[Art, Grain], &[Shimmer, Frame]),
        Rarity::Rare => (&[Art, Aurora, Grain], &[Shimmer, Sparkles, Frame]),
        Rarity::Mythic => (&[Art, Aurora, Grain], &[Shimmer, Sparkles, Frame, Electric]),
        Rarity::Legendary => (
            &[Fire, Art, Aurora, Grain],
            &[Shimmer, Sparkles, Frame, Electric],
        ),
    }
}

/// An effect slot for one layer of a card. `art` is the flavour's place in `FLAVOURS`, and
/// `seed` keeps cards of the same flavour from moving in step.
pub fn card_fx(part: CardFx, art: usize, rarity: Rarity, seed: u16, reduced: bool) -> El {
    rune::effect(part.mark()).effect_data([
        art as f32,
        rarity.index() as f32,
        f32::from(seed),
        f32::from(u8::from(reduced)),
    ])
}

/// The card width the layers below are tuned at; other sizes scale from it.
const CARD_WIDTH: f32 = 236.0;

/// The card's frame: a glossy plastic rim in the colour of its rarity, which still reads as
/// that colour when the card is small.
fn frame(rarity: Rarity, k: f32) -> Fx {
    let width = match rarity {
        Rarity::Legendary => 5.0,
        Rarity::Mythic => 4.5,
        Rarity::Rare => 4.0,
        _ => 3.5,
    };
    Fx::fill(
        PlasticParams {
            color: rgb(rarity.color()),
            ..PlasticParams::glossy()
        }
        .border(width * k),
    )
}

/// A layer of a card being opened. `level` is how lit it is, 1 at full and lower in the hush.
fn reveal_layer(part: CardFx, rarity: Rarity, level: f32, shape: Shape, k: f32) -> Option<Fx> {
    let color = rgb(rarity.color());
    let hot = rgb(0xfff4dc);
    Some(match part {
        CardFx::Back => Fx::fill(MeshGradientParams::graphite()),
        CardFx::RevealGlow => Fx::glow(GlowParams {
            style: if rarity >= Rarity::Mythic {
                GlowStyle::Aura
            } else {
                GlowStyle::Soft
            },
            color,
            core: hot,
            third: color,
            radius: 20.0 * k,
            intensity: level
                * match rarity {
                    Rarity::Common | Rarity::Uncommon => 0.5,
                    Rarity::Rare => 0.8,
                    Rarity::Mythic => 1.3,
                    Rarity::Legendary => 1.6,
                },
            aura_speed: 0.3,
            ..Default::default()
        }),
        CardFx::RevealSparkles => Fx::sparkles(SparkleParams {
            color,
            count: if rarity == Rarity::Legendary { 16 } else { 6 },
            life: 1.2,
            size: 9.0 * k,
            margin: 6.0 * k,
            ..Default::default()
        }),
        CardFx::RevealElectric => Fx::electric(ElectricParams {
            color,
            core: hot,
            cool: color,
            amplitude: if rarity == Rarity::Legendary {
                4.0
            } else {
                1.6
            } * k,
            glow: 4.0 * k,
            occlusion: 0.0,
            intensity: level,
            ..ElectricParams::charged()
        }),
        CardFx::RevealFire => Fx::fire(FireParams {
            style: FireStyle::Border,
            low: rgb(0x8a3a05),
            mid: color,
            hot,
            height: 26.0 * k * level,
            scale: 13.0 * k,
            speed: 2.4,
            intensity: level,
            embers: 18,
            ember_rise: 44.0 * k,
            sides: 0.6,
            ..Default::default()
        }),
        CardFx::Burst => Fx::sparkles(SparkleParams {
            color,
            count: match rarity {
                Rarity::Legendary => 64,
                Rarity::Mythic => 32,
                _ => 24,
            },
            life: 1.0,
            size: 12.0 * k,
            margin: 14.0 * k,
            burst: Some(0.0),
            ..Default::default()
        }),
        CardFx::Ripple => Fx::pulse(PulseParams {
            style: PulseStyle::Ripple {
                at: [
                    shape.rect.x + shape.rect.w / 2.0,
                    shape.rect.y + shape.rect.h / 2.0,
                ],
            },
            color,
            core: hot,
            reach: 240.0 * k,
            width: 3.0,
            duration: 0.5,
            ..Default::default()
        }),
        _ => return None,
    })
}

fn card_layer(
    mark: u32,
    data: [f32; 4],
    shape: Shape,
    time: f32,
    since_hit: f32,
) -> Option<FxLayer> {
    let part = CardFx::from_mark(mark)?;
    let k = shape.rect.w / CARD_WIDTH;
    if let Some(fx) = reveal_layer(
        part,
        *Rarity::ALL.get(data[1] as usize)?,
        data[2] / 100.0,
        shape,
        k,
    ) {
        // A burst and a ripple run once, on the clock of the card landing.
        let at = if matches!(part, CardFx::Burst | CardFx::Ripple) {
            since_hit
        } else {
            time
        };
        return Some(fx.on(shape).seed(7).reduced(data[3] > 0.5).at(at));
    }
    let [base, main, high, second] = FLAVOURS.get(data[0] as usize)?.palette;
    let rarity = *Rarity::ALL.get(data[1] as usize)?;
    let seed = data[2] as u32;
    let fx = match part {
        CardFx::Fire => Fx::fire(FireParams {
            style: FireStyle::Border,
            low: rgb(second),
            mid: rgb(main),
            hot: rgb(high),
            height: 26.0 * k,
            scale: 13.0 * k,
            speed: 1.3,
            embers: 14,
            ember_rise: 44.0 * k,
            sides: 0.6,
            ..Default::default()
        }),
        CardFx::Art => art(rarity, [base, main, high, second], k),
        CardFx::Aurora => Fx::aurora(AuroraParams {
            a: rgb(main),
            b: rgb(high),
            c: rgb(second),
            scale: 110.0 * k,
            speed: 0.3,
            intensity: match rarity {
                Rarity::Rare => 0.3,
                Rarity::Mythic => 0.45,
                _ => 0.35,
            },
            feather: 30.0 * k,
        }),
        CardFx::Grain => Fx::grain(GrainParams {
            amount: 0.05,
            fps: if rarity >= Rarity::Mythic { 12.0 } else { 0.0 },
            feather: 0.0,
        }),
        CardFx::Shimmer => Fx::shimmer(ShimmerParams {
            color: rgb(if rarity == Rarity::Legendary {
                0xffe6a8
            } else {
                0xffffff
            }),
            period: 2.6,
            band: 46.0 * k,
            angle: 0.5,
            intensity: match rarity {
                Rarity::Uncommon => 0.16,
                Rarity::Rare => 0.24,
                _ => 0.3,
            },
            pause: 1.1,
            ..Default::default()
        }),
        CardFx::Sparkles => Fx::sparkles(SparkleParams {
            color: rgb(if rarity == Rarity::Legendary {
                0xffe6a8
            } else {
                high
            }),
            count: match rarity {
                Rarity::Rare => 6,
                Rarity::Mythic => 10,
                _ => 16,
            },
            life: 1.5,
            size: 9.0 * k,
            margin: 4.0 * k,
            ..Default::default()
        }),
        CardFx::Electric => {
            if rarity == Rarity::Legendary {
                Fx::electric(ElectricParams {
                    color: rgb(main),
                    core: rgb(high),
                    cool: rgb(second),
                    amplitude: 4.0 * k,
                    glow: 5.0 * k,
                    fork_len: 18.0 * k,
                    intensity: 1.1,
                    // Its default darkens what's under the arcs, which reads as gaps in the frame.
                    occlusion: 0.0,
                    ..Default::default()
                })
            } else {
                Fx::electric(ElectricParams {
                    color: rgb(main),
                    core: rgb(high),
                    cool: rgb(second),
                    amplitude: 1.6 * k,
                    glow: 4.0 * k,
                    ..ElectricParams::charged()
                })
            }
        }
        CardFx::Frame => frame(rarity, k),
        // Drawn by `reveal_layer` above.
        CardFx::Back
        | CardFx::RevealGlow
        | CardFx::RevealSparkles
        | CardFx::RevealElectric
        | CardFx::RevealFire
        | CardFx::Burst
        | CardFx::Ripple => return None,
    };
    Some(
        fx.on(shape)
            .seed(seed)
            .reduced(data[3] > 0.5)
            // Offset by the seed, so a row of the same card doesn't move as one.
            .at(time + (seed % 97) as f32),
    )
}

/// The card's background. Each rarity is a different painting style.
fn art(rarity: Rarity, [base, main, high, second]: [u32; 4], k: f32) -> Fx {
    match rarity {
        Rarity::Common => Fx::fill(MeshGradientParams {
            colors: Stops::hex(&[base, second, main, base, second]),
            drift: 0.07,
            speed: 0.14,
            sharpness: 1.3,
            warp: 0.2,
            ..MeshGradientParams::dusk()
        }),
        Rarity::Uncommon => Fx::fill(WaveParams {
            colors: Stops::hex(&[high, main, second, base, base]),
            amplitude: 0.06,
            frequency: 1.4,
            speed: 0.25,
            softness: 0.01,
            ..WaveParams::sea()
        }),
        Rarity::Rare => Fx::fill(SwirlParams {
            colors: Stops::hex(&[base, second, main, high]),
            scale: 130.0 * k,
            speed: 0.22,
            twist: 0.7,
            warp: 1.2,
        }),
        Rarity::Mythic => Fx::fill(VoronoiParams {
            colors: Stops::hex(&[base, second, main, high, second, main]),
            edge_color: rgb(0x08080c),
            scale: 24.0 * k,
            speed: 0.22,
            edge: 1.6 * k,
            depth: 0.35,
        }),
        Rarity::Legendary => Fx::fill(MetaballParams {
            colors: Stops::hex(&[main, second, high]),
            base: rgb(base),
            count: 7,
            size: 0.17,
            speed: 0.5,
            gloss: 0.8,
        }),
    }
}

/// Foil lights what's under it; everything else paints over.
fn layer_blend(mark: u32) -> FxBlend {
    match CardFx::from_mark(mark) {
        Some(CardFx::Shimmer | CardFx::Aurora) => FxBlend::Screen,
        _ => FxBlend::Normal,
    }
}

fn new_renderer(device: &wgpu::Device, format: wgpu::TextureFormat) -> FxRenderer {
    FxRenderer::new(device, format, FxOptions { dither: true })
}

/// What a card takes to draw, as `warm` builds it.
type Warmed = (FxRenderer, super::can3d::Warm);

/// Builds every card shader and the cans' pipelines for targets of `format`. Done on another
/// thread from the window's first frame, because built when the first card showed they held
/// that frame for 300 to 400 ms on a MacBook. Isolated layers draw into `Rgba16Float`, and
/// `set_srgb_blend` moves the target to the other sRGB variant of `format`, as `prepare` does.
fn warm(device: &wgpu::Device, format: wgpu::TextureFormat, srgb: bool) -> Warmed {
    let mut fx = new_renderer(device, format);
    let blended = if srgb {
        format.remove_srgb_suffix()
    } else {
        format.add_srgb_suffix()
    };
    fx.add_format(device, blended);
    fx.add_format(device, wgpu::TextureFormat::Rgba16Float);
    fx.build_every_pipeline();
    let cans = super::can3d::warm(device, &[format, wgpu::TextureFormat::Rgba16Float]);
    (fx, cans)
}

/// How long after the window's first frame the card shaders start building, so they don't
/// compete with launch. A card shown sooner starts them at once and waits for them.
const WARM_AFTER: std::time::Duration = std::time::Duration::from_secs(1);

/// Whether this frame shows a card layer or a can.
fn cards_shown(f: &Frame<'_, App>) -> bool {
    f.rt.instances.iter().enumerate().any(|(n, instance)| {
        let kind = f.rt.marks.get(n).copied().unwrap_or(0) & 0xff;
        instance.params[1] == KIND_EFFECT
            && (kind == super::can3d::CAN || CardFx::from_mark(kind).is_some())
    })
}

#[derive(Default)]
pub struct Effects {
    videos: super::video::Videos,
    cans: super::can3d::Cans,
    renderer: Option<(DeviceKey, FxRenderer)>,
    /// The card shaders being built ahead on another thread, for the device of that key, and
    /// the way to tell it to start at once.
    warming: Option<(DeviceKey, Sender<()>, JoinHandle<Warmed>)>,
    /// The device they were last built for, so each GPU the window lands on builds them once.
    warmed_for: Option<DeviceKey>,
    /// Whether a layer drawn last frame moves on its own, so the window keeps drawing.
    moving: bool,
    /// The reveal that last landed, and when, for effects that run once from that moment.
    landed: Option<(u64, f32)>,
}

impl Effects {
    /// Starts building the card shaders for this frame's device, and takes them once they're
    /// built, or as soon as a card is on screen, waiting for the rest then if it must.
    fn warm(&mut self, f: &Frame<'_, App>) {
        let key = f.device_key();
        if self.warmed_for != Some(key) {
            self.warmed_for = Some(key);
            let (device, format, srgb) = (f.device.clone(), f.format, f.srgb_blend());
            let (start, started) = channel();
            match std::thread::Builder::new()
                .name("card shaders".to_owned())
                .spawn(move || {
                    // Compiled alongside launch's own first frame, they doubled it.
                    let _ = started.recv_timeout(WARM_AFTER);
                    warm(&device, format, srgb)
                }) {
                Ok(thread) => self.warming = Some((key, start, thread)),
                Err(error) => tracing::warn!("Card shaders aren't built ahead: {error}"),
            }
        }
        let Some((_, start, thread)) = &self.warming else {
            return;
        };
        if !thread.is_finished() {
            if !cards_shown(f) {
                return;
            }
            let _ = start.send(());
        }
        let (built_for, _, thread) = self.warming.take().expect("checked above");
        match thread.join() {
            // The window moved to another GPU meanwhile: what it builds there is used.
            Ok(_) if built_for != key => {}
            Ok((fx, cans)) => {
                if self.renderer.as_ref().is_none_or(|(k, _)| *k != key) {
                    self.renderer = Some((key, fx));
                }
                self.cans.adopt(key, cans);
            }
            Err(_) => tracing::warn!("Building the card shaders ahead failed"),
        }
    }
}

impl Compositor<App> for Effects {
    fn animating(&self) -> bool {
        self.moving
    }

    fn render(&mut self, f: &mut Frame<'_, App>) {
        // The first frame built the frosted backdrop's pipelines; the next can drop the speck.
        f.rt.state.ui.warm_blur = false;
        self.warm(f);
        let video_visible = self.videos.prepare(f);
        let cans = self.cans.prepare(f);
        let key = f.device_key();
        let (device, queue, size) = (f.device, f.queue, f.size);
        let time = f.rt.t as f32;
        let hit = f.rt.state.monster.reveal.as_ref();
        if let Some(reveal) = hit.filter(|reveal| reveal.beat() == Beat::Hit)
            && self.landed.is_none_or(|(id, _)| id != reveal.id)
        {
            self.landed = Some((reveal.id, time));
        }
        let since_hit = self.landed.map_or(0.0, |(_, landed)| time - landed);
        let mut at = Vec::new();
        let mut layers = Vec::new();
        let mut slots = Vec::new();
        for (n, instance) in f.rt.instances.iter().enumerate() {
            if instance.params[1] != KIND_EFFECT {
                continue;
            }
            let kind = f.rt.marks.get(n).copied().unwrap_or(0) & 0xff;
            let scale = instance.params[3];
            let [x, y, w, h] = instance.rect.map(|v| v / scale);
            let radius = instance.radii.iter().fold(0.0f32, |r, v| r.max(v.abs())) / scale;
            let Some(layer) = card_layer(
                kind,
                instance.uv,
                Shape::rounded(x, y, w, h, radius),
                time,
                since_hit,
            ) else {
                continue;
            };
            at.push(n as u32);
            layers.push(layer);
            slots.push(
                FxSlot::default()
                    .scale(scale)
                    .clip(instance.clip, instance.fade)
                    .blend(layer_blend(kind)),
            );
        }
        self.moving = layers.iter().any(FxLayer::is_animating) || cans.moving;
        if layers.is_empty() && !video_visible && !cans.visible {
            f.draw_ui();
            return;
        }
        // Normally `warm` built it already. Resources belong to one device, so a renderer
        // that couldn't be built ahead, or was built for another GPU, is made here.
        if !layers.is_empty() && self.renderer.as_ref().is_none_or(|(k, _)| *k != key) {
            self.renderer = Some((key, new_renderer(device, f.format)));
        }
        let mut fx = self
            .renderer
            .as_mut()
            .filter(|_| !layers.is_empty())
            .map(|(_, fx)| fx);
        if let Some(fx) = fx.as_mut() {
            fx.set_srgb_blend(f.srgb_blend());
            fx.add_format(device, wgpu::TextureFormat::Rgba16Float);
            let frame = FxFrame {
                viewport: [size.0, size.1],
                scale: 1.0,
                layers: &layers,
                backdrop: None,
                sources: &[],
            };
            fx.prepare_with_slots(device, queue, &frame, &slots);
        }
        let clear = f.rt.clear_color();
        f.ui.prepare(device, queue, &mut f.rt.text.atlas, &f.rt.instances, size);
        f.ui.draw_with_backdrop(
            f.encoder,
            f.target,
            clear,
            &mut |mut phase, site: &EffectSite<'_>| {
                if let EffectPhase::Draw(pass) = &mut phase {
                    self.videos.draw(device, pass, site);
                    self.cans.draw(key, device, pass, site);
                }
                let Some(k) = at.iter().position(|n| *n == site.index) else {
                    return;
                };
                if let (EffectPhase::Draw(pass), Some(fx)) = (phase, fx.as_mut()) {
                    fx.draw_layer_in(pass, site.format, site.origin, k);
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_card_layer_resolves_for_every_flavour_and_rarity() {
        let shape = Shape::rounded(0.0, 0.0, 150.0, 210.0, 11.0);
        for flavour in 0..FLAVOURS.len() {
            for rarity in Rarity::ALL {
                for part in CardFx::ALL {
                    let data = [flavour as f32, rarity.index() as f32, 65535.0, 0.0];
                    assert!(card_layer(part.mark(), data, shape, 0.0, 0.0).is_some());
                }
            }
        }
    }

    #[test]
    fn a_mark_outside_the_card_range_draws_nothing() {
        let shape = Shape::rounded(0.0, 0.0, 10.0, 10.0, 1.0);
        let data = [0.0; 4];
        assert!(card_layer(CARD_MARKS - 1, data, shape, 0.0, 0.0).is_none());
        assert!(card_layer(CARD_MARKS + CardFx::ALL.len() as u32, data, shape, 0.0, 0.0).is_none());
    }

    #[test]
    fn rarer_cards_take_more_layers() {
        let count = |rarity| {
            let (behind, over) = card_layers(rarity);
            behind.len() + over.len()
        };
        for pair in Rarity::ALL.windows(2) {
            assert!(count(pair[0]) < count(pair[1]), "{pair:?}");
        }
    }

    #[test]
    fn reduced_motion_stills_every_card_layer() {
        let shape = Shape::rounded(0.0, 0.0, 150.0, 210.0, 11.0);
        for rarity in Rarity::ALL {
            // Bursts and ripples run once, and the view leaves them out under Reduce Motion.
            for part in CardFx::ALL
                .into_iter()
                .filter(|part| !matches!(part, CardFx::Burst | CardFx::Ripple))
            {
                let data = [0.0, rarity.index() as f32, 7.0, 1.0];
                let layer = card_layer(part.mark(), data, shape, 3.0, 0.5).unwrap();
                assert!(!layer.is_animating(), "{part:?} at {rarity:?}");
            }
        }
    }
}
