//! GPU effect layers from rune-fx, drawn in the window's own pass. A view marks an element with
//! [`liquid_metal_ring`]; the compositor finds the marked instances each frame and draws the
//! matching layer in the element's place.

use std::thread::{self, JoinHandle};

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
    rune::effect(LIQUID_METAL).effect_data(ring_data(width, reduced))
}

fn ring_data(width: f32, reduced: bool) -> [f32; 4] {
    [width, f32::from(u8::from(reduced)), 0.0, 0.0]
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

/// What an element fading as a group, like Settings opening, is drawn into.
const LAYER_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

fn new_renderer(device: &wgpu::Device, format: wgpu::TextureFormat) -> FxRenderer {
    FxRenderer::new(device, format, FxOptions { dither: true })
}

/// A renderer with the ring's pipelines already built. Building them stalls for hundreds of
/// milliseconds when the shader cache is cold, and on Windows, where shaders compile from
/// source, on every launch; this runs on its own thread so that never freezes the window.
fn prepared_renderer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
    srgb: bool,
) -> FxRenderer {
    let mut fx = new_renderer(device, format);
    fx.set_srgb_blend(srgb);
    fx.add_format(device, LAYER_FORMAT);
    let shape = Shape::rounded(0.0, 0.0, 8.0, 8.0, 2.0);
    let Some(ring) = layer(LIQUID_METAL, ring_data(3.0, false), shape, 0.0) else {
        return fx;
    };
    // `prepare` builds the pipeline for the window's format.
    fx.prepare(
        device,
        queue,
        &FxFrame {
            viewport: [8, 8],
            scale: 1.0,
            layers: &[ring],
            backdrop: None,
            sources: &[],
        },
    );
    // Drawing builds it for a layer's format too. The commands are never submitted.
    let target = device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("fx warm-up"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: LAYER_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &target,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Discard,
            },
        })],
        ..Default::default()
    });
    fx.draw_layer_in(&mut pass, LAYER_FORMAT, [0.0, 0.0], 0);
    drop(pass);
    fx
}

#[derive(Default)]
pub struct Effects {
    videos: super::video::Videos,
    renderer: Option<(DeviceKey, FxRenderer)>,
    /// The renderer being prepared off the UI thread, once the first frame has started it.
    warming: Option<(DeviceKey, JoinHandle<FxRenderer>)>,
    warming_started: bool,
    /// Whether a layer drawn last frame moves on its own, so the window keeps drawing.
    moving: bool,
}

impl Compositor<App> for Effects {
    fn animating(&self) -> bool {
        self.moving
    }

    fn render(&mut self, f: &mut Frame<'_, App>) {
        // The first frame built the frosted backdrop's pipelines; the next can drop the speck.
        f.rt.state.ui.warm_blur = false;
        let video_visible = self.videos.prepare(f);
        let key = f.device_key();
        if !std::mem::replace(&mut self.warming_started, true) {
            let (device, queue) = (f.device.clone(), f.queue.clone());
            let (format, srgb) = (f.format, f.srgb_blend());
            self.warming = thread::Builder::new()
                .name("effects warm-up".into())
                .spawn(move || prepared_renderer(&device, &queue, format, srgb))
                .ok()
                .map(|handle| (key, handle));
        }
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
        if layers.is_empty() && !video_visible {
            f.draw_ui();
            return;
        }
        // Resources belong to one device; a window shown again on another GPU makes them anew.
        if self.renderer.as_ref().is_none_or(|(k, _)| *k != key) {
            // Only waits if an effect is on screen before the warm-up has finished.
            let warmed = self
                .warming
                .take()
                .filter(|(k, _)| *k == key)
                .and_then(|(_, handle)| handle.join().ok());
            self.renderer = Some((
                key,
                warmed.unwrap_or_else(|| new_renderer(device, f.format)),
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
            &mut |mut phase, site: &EffectSite<'_>| {
                if let EffectPhase::Draw(pass) = &mut phase {
                    self.videos.draw(device, pass, site);
                }
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
