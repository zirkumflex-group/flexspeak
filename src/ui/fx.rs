//! GPU effect layers from rune-fx, drawn in the window's own pass. A view marks an element with
//! [`liquid_metal_ring`]; the compositor finds the marked instances each frame and draws the
//! matching layer in the element's place.

use rune::El;
use rune::compose::{Compositor, DeviceKey, Frame};
use rune::render::{EffectPhase, EffectSite, KIND_EFFECT};
use rune_fx::{FxFrame, FxLayer, FxOptions, FxRenderer, FxSlot, LiquidMetalParams, Shape};

use crate::App;

/// Effect marks: which layer an `El::effect` slot draws.
const LIQUID_METAL: u32 = 1;

/// A ring of flowing chrome `width` points wide, inside the element's edge. Under reduced
/// motion it rests on one pose and never animates.
pub fn liquid_metal_ring(width: f32, reduced: bool) -> El {
    rune::effect(LIQUID_METAL).effect_data([width, f32::from(u8::from(reduced)), 0.0, 0.0])
}

fn layer(kind: u32, data: [f32; 4], shape: Shape, time: f32) -> Option<FxLayer> {
    match kind {
        LIQUID_METAL => Some(
            rune_fx::Fx::fill(LiquidMetalParams::chrome().border(data[0]))
                .on(shape)
                .reduced(data[1] > 0.5)
                .at(time),
        ),
        _ => None,
    }
}

#[derive(Default)]
pub struct Effects {
    renderer: Option<(DeviceKey, FxRenderer)>,
    /// Whether a layer drawn last frame moves on its own, so the window keeps drawing.
    moving: bool,
}

impl Compositor<App> for Effects {
    fn animating(&self) -> bool {
        self.moving
    }

    fn render(&mut self, f: &mut Frame<'_, App>) {
        let (device, queue, size) = (f.device, f.queue, f.size);
        let time = f.rt.t as f32;
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
            let Some(layer) = layer(kind, instance.uv, Shape::rounded(x, y, w, h, radius), time)
            else {
                continue;
            };
            at.push(n as u32);
            layers.push(layer);
            slots.push(
                FxSlot::default()
                    .scale(scale)
                    .clip(instance.clip, instance.fade),
            );
        }
        self.moving = layers.iter().any(FxLayer::is_animating);
        if layers.is_empty() {
            f.draw_ui();
            return;
        }
        // Resources belong to one device; a window shown again on another GPU makes them anew.
        let key = f.device_key();
        if self.renderer.as_ref().is_none_or(|(k, _)| *k != key) {
            self.renderer = Some((
                key,
                FxRenderer::new(device, f.format, FxOptions { dither: true }),
            ));
        }
        let Some((_, fx)) = self.renderer.as_mut() else {
            return;
        };
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
        let clear = f.rt.clear_color();
        f.ui.prepare(device, queue, &mut f.rt.text.atlas, &f.rt.instances, size);
        f.ui.draw_with_backdrop(
            f.encoder,
            f.target,
            clear,
            &mut |phase, site: &EffectSite<'_>| {
                let Some(k) = at.iter().position(|n| *n == site.index) else {
                    return;
                };
                if let EffectPhase::Draw(pass) = phase {
                    fx.draw_layer_in(pass, site.format, site.origin, k);
                }
            },
        );
    }
}
