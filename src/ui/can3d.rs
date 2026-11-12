//! The can on each Monster card: pixel art that is still a real 3D model. It has its own small
//! pipeline, drawn in painter order like a video, because rune-fx's layers are flat fills. See
//! `can3d.wgsl` for the model. Every pixel of a cell would trace the same ray, so each frame
//! first traces every can's cells into a small texture, and drawing a can only looks them up.
//!
//! A can's pose is worked out here each frame: its idle turn, the lean a hovering pointer gives
//! it, where a drag has spun the large one, and the tab levering open when a card is revealed.

use std::collections::HashMap;
use std::time::Instant;

use rune::compose::{DeviceKey, Frame};
use rune::render::{EffectSite, KIND_EFFECT};
use rune::{Drag, Phase};
use rune_fx::rgb;
use wgpu::util::DeviceExt;

use crate::App;
use crate::monster::{FLAVOURS, Rarity};

pub const CAN: u32 = 3;

/// What a can's fourth data slot can say about it, added together.
pub const REDUCED: u32 = 1;
/// The large can: a drag spins it, and it opens when its card is revealed.
pub const SHOWCASE: u32 = 2;
const MYSTERY: u32 = 4;
/// The pointer is over its card.
pub const HOVERED: u32 = 8;

/// A can. `art` is the flavour's place in `FLAVOURS`, and `seed` keeps cans out of step.
pub fn can(art: usize, rarity: Rarity, seed: u16, look: u32) -> rune::El {
    rune::effect(CAN).effect_data([
        art as f32,
        rarity.index() as f32,
        f32::from(seed),
        look as f32,
    ])
}

/// The unprinted can on a card's back, before it's known what it is. It trembles harder with
/// each rarity the reveal has climbed past, and `level` stills it for the hush.
pub fn mystery(rung: Option<Rarity>, level: f32, reduced: bool) -> rune::El {
    let pressure = rung.map_or(0.0, |rung| rung.index() as f32);
    rune::effect(CAN).effect_data([
        0.0,
        pressure,
        level * 100.0,
        (MYSTERY | if reduced { REDUCED } else { 0 }) as f32,
    ])
}

/// The camera's pitch when nothing is happening: slightly above the can.
const REST_PITCH: f32 = 0.2;
const YAW_PER_POINT: f32 = 0.012;
const PITCH_PER_POINT: f32 = 0.008;
/// How far a drag can tip the can: enough to see into the lid, and under the base.
const PITCH_RANGE: std::ops::RangeInclusive<f32> = -0.75..=1.15;
const MAX_VELOCITY: f32 = 14.0;

/// Where dragging has left the large can.
#[derive(Debug, Clone, Copy, Default)]
pub struct Spin {
    yaw: f32,
    pitch: f32,
    /// Radians a second it's still turning at since it was let go.
    velocity: f32,
    held: bool,
    last: (f32, f32),
    moved: Option<Instant>,
    /// The idle animation's own clock, which stops while the can is held.
    clock: f32,
}

impl Spin {
    /// A can already turned and tipped, for previews.
    pub fn posed(yaw: f32, pitch: f32) -> Self {
        Self {
            yaw,
            pitch,
            held: true,
            ..Self::default()
        }
    }

    pub fn drag(&mut self, drag: &Drag) {
        let now = Instant::now();
        match drag.phase {
            Phase::Down => {
                self.held = true;
                self.velocity = 0.0;
                self.last = (0.0, 0.0);
            }
            Phase::Move => {
                let seconds = self
                    .moved
                    .map_or(0.0, |moved| now.duration_since(moved).as_secs_f32());
                self.turn(drag.dx - self.last.0, drag.dy - self.last.1, seconds);
                self.last = (drag.dx, drag.dy);
            }
            Phase::Up => {
                // A can held still before it's let go stays still.
                let paused = self
                    .moved
                    .is_none_or(|moved| now.duration_since(moved).as_secs_f32() > 0.09);
                self.release(paused);
            }
        }
        self.moved = Some(now);
    }

    /// Turns with the pointer: the side under it follows it across, and pulling down tips the
    /// top toward you.
    fn turn(&mut self, dx: f32, dy: f32, seconds: f32) {
        self.yaw -= dx * YAW_PER_POINT;
        self.pitch =
            (self.pitch + dy * PITCH_PER_POINT).clamp(*PITCH_RANGE.start(), *PITCH_RANGE.end());
        if seconds > 0.0 {
            let now = (-dx * YAW_PER_POINT / seconds).clamp(-MAX_VELOCITY, MAX_VELOCITY);
            self.velocity = self.velocity * 0.5 + now * 0.5;
        }
    }

    fn release(&mut self, paused: bool) {
        self.held = false;
        if paused {
            self.velocity = 0.0;
        }
    }

    /// Lets a released can coast to a stop and stand back up.
    fn advance(&mut self, dt: f32) {
        if self.held {
            return;
        }
        self.clock += dt;
        self.yaw += self.velocity * dt;
        self.velocity *= (-2.4 * dt).exp();
        if self.velocity.abs() < 0.01 {
            self.velocity = 0.0;
        }
        self.pitch *= (-3.5 * dt).exp();
    }
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How long a can takes to open and stand back up, in seconds.
const OPENING_SECONDS: f32 = 1.6;

/// A can being opened, `since` seconds after its card turned over: it tips toward you with
/// its mouth forward, the tab levers up and pushes the mouth in, the tab is pressed back down,
/// and the can stands up again, sealed as it's kept.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Opening {
    /// How far the can has turned to face you and tipped, 0 to 1.
    face: f32,
    /// The tab's angle, in radians.
    lift: f32,
    /// How far the mouth is pushed in, in millimetres.
    depth: f32,
}

fn opening(since: f32) -> Opening {
    let sealing = 1.0 - smoothstep(1.3, 1.55, since);
    Opening {
        face: smoothstep(0.0, 0.22, since) * (1.0 - smoothstep(0.85, 1.3, since)),
        lift: 1.05
            * smoothstep(0.12, 0.34, since)
            * (1.0 - 0.88 * smoothstep(0.42, 0.6, since))
            * sealing,
        depth: 13.0 * smoothstep(0.26, 0.4, since) * sealing,
    }
}

/// How a can is posed at a moment: its turn and its lean, in radians. Plain cans sway on the
/// spot; from Rare up they turn all the way round.
fn pose(rarity: Rarity, seed: f32, time: f32) -> (f32, f32) {
    let t = time + seed % 97.0;
    if rarity >= Rarity::Rare {
        (t * 0.8, (t * 1.3).sin() * 0.04)
    } else {
        ((t * 0.9).sin() * 0.6, (t * 1.7).sin() * 0.05)
    }
}

/// What the frame knows that every can's pose may depend on.
#[derive(Debug, Clone, Copy, Default)]
struct Now {
    time: f32,
    spin: Spin,
    /// Seconds since the card being revealed turned face up.
    since_turn: Option<f32>,
}

/// How the pointer has stirred one can.
#[derive(Debug, Clone, Copy, Default)]
struct Hover {
    /// 0 at rest, 1 once the pointer has settled on it.
    amount: f32,
    /// The extra turning it has done while hovered, which it keeps.
    turn: f32,
    wanted: bool,
    seen: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Placed {
    spin: f32,
    roll: f32,
    pitch: f32,
    zoom: f32,
    lift: f32,
    depth: f32,
    bare: bool,
}

fn placed(data: [f32; 4], hover: Hover, now: &Now) -> Placed {
    let look = data[3] as u32;
    let rarity = Rarity::ALL
        .get(data[1] as usize)
        .copied()
        .unwrap_or(Rarity::Common);
    let mut at = Placed {
        spin: 0.4,
        roll: 0.0,
        pitch: REST_PITCH,
        zoom: 1.0,
        lift: 0.0,
        depth: 0.0,
        bare: look & MYSTERY != 0,
    };
    if look & SHOWCASE != 0 {
        at.spin += now.spin.yaw;
        at.pitch += now.spin.pitch;
    }
    if look & REDUCED != 0 {
        return at;
    }
    if look & MYSTERY != 0 {
        let (pressure, level) = (data[1], data[2] / 100.0);
        at.spin = (now.time * 0.9).sin() * 0.25;
        at.roll =
            (now.time * 1.7).sin() * 0.03 + (now.time * 41.0).sin() * 0.007 * pressure * level;
        return at;
    }
    if look & SHOWCASE == 0 {
        let (spin, roll) = pose(rarity, data[2], now.time);
        at.spin = spin + hover.turn;
        at.roll = roll;
        at.pitch += 0.3 * hover.amount;
        at.zoom += 0.03 * hover.amount;
        return at;
    }
    let (spin, roll) = pose(rarity, data[2], now.spin.clock);
    at.spin = spin + now.spin.yaw;
    at.roll = roll;
    if let Some(since) = now.since_turn.filter(|since| *since < OPENING_SECONDS) {
        let open = opening(since);
        // The nearest turn that puts the mouth at the front.
        let front = (at.spin / std::f32::consts::TAU).round() * std::f32::consts::TAU;
        at.spin += (front - at.spin) * open.face;
        at.pitch += (REST_PITCH + 0.72 - at.pitch) * open.face;
        at.roll *= 1.0 - open.face;
        at.zoom += 0.1 * open.face;
        at.lift = open.lift;
        at.depth = open.depth;
    }
    at
}

/// Cans of the same flavour, rarity and seed share how the pointer has stirred them.
fn hover_key(data: [f32; 4]) -> u64 {
    (data[0] as u64) << 40
        | (data[1] as u64) << 32
        | (data[2] as u64) << 8
        | u64::from(data[3] as u32 & (SHOWCASE | MYSTERY))
}

/// The bytes of a can's `Can` uniform.
const UNIFORM_SIZE: u64 = 176;
/// The byte in a can's uniform where its `viewport` starts.
const VIEWPORT_AT: usize = 48;
/// Cells a can's box is tall, as `can3d.wgsl` divides it.
const CELLS: f32 = 72.0;
/// How wide a shelf of traced cans is in `cells` before the next starts below it.
const SHELF: u32 = 2048;
/// Traced cells: a colour, and whether the cell is drawn, kept exactly.
const CELLS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;

struct Gpu {
    layout: wgpu::BindGroupLayout,
    cells_layout: wgpu::BindGroupLayout,
    shader: wgpu::ShaderModule,
    pipelines: Vec<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
    /// Traces every cell of a can into `cells`.
    trace: wgpu::RenderPipeline,
    /// Bytes from one can's uniform slot to the next, as the device aligns dynamic offsets.
    stride: u64,
    /// Every can's uniforms this frame, a slot each, and how many slots it holds.
    uniforms: Option<(wgpu::Buffer, wgpu::BindGroup, u64)>,
    /// Every can's traced cells this frame, and its size.
    cells: Option<(wgpu::TextureView, wgpu::BindGroup, [u32; 2])>,
}

impl Gpu {
    fn new(device: &wgpu::Device) -> Gpu {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("can"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(UNIFORM_SIZE),
                },
                count: None,
            }],
        });
        let cells_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("can cells"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("can"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}",
                    rune::render::CLIP_WGSL,
                    include_str!("can3d.wgsl")
                )
                .into(),
            ),
        });
        let trace_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("can cells"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let trace = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("can cells"),
            layout: Some(&trace_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("cover"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("trace"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: CELLS_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let align = u64::from(device.limits().min_uniform_buffer_offset_alignment);
        Gpu {
            layout,
            cells_layout,
            shader,
            pipelines: Vec::new(),
            trace,
            stride: wgpu::util::align_to(UNIFORM_SIZE, align),
            uniforms: None,
            cells: None,
        }
    }

    /// Builds the pipeline that draws cans into targets of `format`, unless it's built.
    fn add_format(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) {
        if self.pipelines.iter().any(|(f, _)| *f == format) {
            return;
        }
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("can"),
            bind_group_layouts: &[Some(&self.layout), Some(&self.cells_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("can"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.pipelines.push((format, pipeline));
    }
}

/// The cans' shaders and pipelines, built ahead of time on another thread (`warm`), so the
/// first card on screen doesn't wait for them.
pub struct Warm(Gpu);

/// Builds what drawing cans takes on `device`, with a pipeline for each of `formats`.
pub fn warm(device: &wgpu::Device, formats: &[wgpu::TextureFormat]) -> Warm {
    let mut gpu = Gpu::new(device);
    for &format in formats {
        gpu.add_format(device, format);
    }
    Warm(gpu)
}

/// Where this frame put a can: its uniform slot, and its cells in `cells`.
#[derive(Debug, Clone, Copy)]
struct Slot {
    /// The can's slot in the painter stream.
    index: u32,
    /// Where its uniform slot starts, in bytes.
    offset: u32,
    /// Left, top, width and height, in texels.
    region: [u32; 4],
}

#[derive(Default)]
pub struct Cans {
    gpu: Option<(DeviceKey, Gpu)>,
    now: Now,
    last_time: Option<f32>,
    hovers: HashMap<u64, Hover>,
    /// The reveal whose card last turned face up, and when.
    turned: Option<(u64, f32)>,
    /// This frame's cans, in painter order.
    slots: Vec<Slot>,
    /// What this frame wrote to the uniform slots.
    bytes: Vec<u8>,
    /// The frame target's size, which the slots' viewports were written for.
    viewport: [u32; 2],
}

/// What the frame's cans need from it.
pub struct Seen {
    pub visible: bool,
    /// Whether any of them moves on its own, so the window keeps drawing.
    pub moving: bool,
}

impl Cans {
    /// Takes what `warm` built for the device `key`, unless that's already made.
    pub fn adopt(&mut self, key: DeviceKey, warm: Warm) {
        if self.gpu.as_ref().is_none_or(|(k, _)| *k != key) {
            self.gpu = Some((key, warm.0));
        }
    }

    /// Finds this frame's cans and moves their poses on by the time since the last one.
    pub fn prepare(&mut self, f: &mut Frame<'_, App>) -> Seen {
        let time = f.rt.t as f32;
        let dt = self
            .last_time
            .map_or(0.0, |last| (time - last).clamp(0.0, 0.1));
        self.last_time = Some(time);
        let face_up = f.rt.state.monster.reveal.as_ref();
        if let Some(reveal) = face_up.filter(|reveal| reveal.beat().face_up())
            && self.turned.is_none_or(|(id, _)| id != reveal.id)
        {
            self.turned = Some((reveal.id, time));
        }
        let mut seen = Seen {
            visible: false,
            moving: false,
        };
        for (n, instance) in f.rt.instances.iter().enumerate() {
            if instance.params[1] != KIND_EFFECT
                || f.rt.marks.get(n).copied().unwrap_or(0) & 0xff != CAN
            {
                continue;
            }
            let look = instance.uv[3] as u32;
            seen.visible = true;
            seen.moving |= look & REDUCED == 0;
            let hover = self.hovers.entry(hover_key(instance.uv)).or_default();
            hover.seen = true;
            hover.wanted |= look & HOVERED != 0 && look & REDUCED == 0;
        }
        for hover in self.hovers.values_mut() {
            let target = f32::from(u8::from(hover.wanted));
            hover.amount += (target - hover.amount) * (1.0 - (-10.0 * dt).exp());
            hover.turn += hover.amount * dt * 3.0;
        }
        // A can that has scrolled away forgets its extra turn, where nobody sees it jump back.
        self.hovers.retain(|_, hover| {
            let keep = hover.seen || hover.amount > 0.01;
            hover.seen = false;
            hover.wanted = false;
            keep
        });
        let spin = &mut f.rt.state.ui.can_spin;
        if seen.moving {
            spin.advance(dt);
        }
        self.now = Now {
            time,
            spin: *spin,
            since_turn: self.turned.map(|(_, turned)| time - turned),
        };
        if seen.visible {
            self.trace(f);
        } else {
            self.slots.clear();
        }
        seen
    }

    /// Writes every can's uniforms into its slot and traces its cells, once for the frame, so
    /// the pixels of a cell only look up what its ray found.
    fn trace(&mut self, f: &mut Frame<'_, App>) {
        self.slots.clear();
        self.bytes.clear();
        let device = f.device;
        let stride = self.gpu(f.device_key(), device).stride;
        let limit = device.limits().max_texture_dimension_2d;
        let shelf_width = SHELF.min(limit);
        self.viewport = [f.size.0, f.size.1];
        let srgb_blend = f.srgb_blend();
        // The shelf being filled: its left edge so far, its top and its height.
        let (mut x, mut y, mut height) = (0u32, 0u32, 0u32);
        let mut size = [1u32; 2];
        for (n, instance) in f.rt.instances.iter().enumerate() {
            if instance.params[1] != KIND_EFFECT
                || f.rt.marks.get(n).copied().unwrap_or(0) & 0xff != CAN
            {
                continue;
            }
            let data = instance.uv;
            let Some(flavour) = FLAVOURS.get(data[0] as usize) else {
                continue;
            };
            let [left, top, w, h] = instance.rect;
            // Clipped away, it's never drawn; Rune scissors a can to its clip's whole pixels.
            let c = instance.clip;
            let shown = left < c[2].ceil()
                && top < c[3].ceil()
                && left + w > c[0].floor()
                && top + h > c[1].floor()
                && c[2].ceil() > c[0].floor()
                && c[3].ceil() > c[1].floor();
            if !(w > 0.0 && h > 0.0 && shown) {
                continue;
            }
            // A cell more on each side, for a pixel whose interpolated position lands just
            // outside the box.
            let cell = h / CELLS;
            let (cols, rows) = ((w / cell).ceil() + 2.0, (h / cell).ceil() + 2.0);
            if !(cols <= shelf_width as f32 && rows <= limit as f32) {
                continue;
            }
            let (cols, rows) = (cols as u32, rows as u32);
            if x + cols > shelf_width {
                (x, y, height) = (0, y + height, 0);
            }
            if y + rows > limit {
                break;
            }
            let region = [x, y, cols, rows];
            x += cols;
            height = height.max(rows);
            size = [size[0].max(x), size[1].max(y + height)];

            let hover = self
                .hovers
                .get(&hover_key(data))
                .copied()
                .unwrap_or_default();
            let at = placed(data, hover, &self.now);
            // The can on a card's back has no rarity yet; its second slot is how hard it shakes.
            let finish = if at.bare { 0.0 } else { data[1] };
            let start = self.bytes.len();
            for v in instance.rect.into_iter().chain(instance.clip) {
                self.bytes.extend(v.to_ne_bytes());
            }
            for v in instance.fade {
                self.bytes.extend(v.to_ne_bytes());
            }
            // Drawn into the frame target, whose top-left is the window's; `draw` writes its own
            // for a can inside an isolated layer.
            for v in [self.viewport[0] as f32, self.viewport[1] as f32, 0.0, 0.0] {
                self.bytes.extend(v.to_ne_bytes());
            }
            for colour in flavour.palette {
                let [r, g, b] = rgb(colour).0;
                for v in [r, g, b, 1.0] {
                    self.bytes.extend(v.to_ne_bytes());
                }
            }
            for v in [
                at.spin,
                at.roll,
                finish,
                f32::from(srgb_blend),
                instance.fill[3],
                at.pitch,
                at.lift,
                at.depth,
                at.zoom,
                f32::from(u8::from(at.bare)),
                // Cell (0, 0), past the margin.
                (region[0] + 1) as f32,
                (region[1] + 1) as f32,
            ] {
                self.bytes.extend(v.to_ne_bytes());
            }
            debug_assert_eq!((self.bytes.len() - start) as u64, UNIFORM_SIZE);
            self.bytes.resize(start + stride as usize, 0);
            self.slots.push(Slot {
                index: n as u32,
                offset: start as u32,
                region,
            });
        }
        if self.slots.is_empty() {
            return;
        }

        let Some((_, gpu)) = self.gpu.as_mut() else {
            return;
        };
        let count = self.slots.len() as u64;
        if gpu
            .uniforms
            .as_ref()
            .is_none_or(|(_, _, slots)| *slots < count)
        {
            let slots = count.next_power_of_two();
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("can placement"),
                size: slots * stride,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = uniform_group(device, &gpu.layout, &buffer);
            gpu.uniforms = Some((buffer, group, slots));
        }
        if gpu
            .cells
            .as_ref()
            .is_none_or(|(_, _, had)| had[0] < size[0] || had[1] < size[1])
        {
            // Grown a little past what's needed, so a few more cans don't make it anew.
            let had = gpu.cells.as_ref().map_or([0; 2], |(_, _, had)| *had);
            let grown = |had: u32, need: u32| had.max(need.next_multiple_of(256)).min(limit);
            let size = [grown(had[0], size[0]), grown(had[1], size[1])];
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("can cells"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: CELLS_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("can cells"),
                layout: &gpu.cells_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                }],
            });
            gpu.cells = Some((view, group, size));
        }
        let (Some((buffer, uniforms, _)), Some((view, _, _))) = (&gpu.uniforms, &gpu.cells) else {
            return;
        };
        f.queue.write_buffer(buffer, 0, &self.bytes);
        let mut pass = f.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("can cells"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&gpu.trace);
        for slot in &self.slots {
            let [x, y, w, h] = slot.region.map(|v| v as f32);
            pass.set_viewport(x, y, w, h, 0.0, 1.0);
            pass.set_bind_group(0, uniforms, &[slot.offset]);
            pass.draw(0..4, 0..1);
        }
    }

    fn gpu(&mut self, key: DeviceKey, device: &wgpu::Device) -> &mut Gpu {
        if self.gpu.as_ref().is_none_or(|(k, _)| *k != key) {
            self.gpu = Some((key, Gpu::new(device)));
        }
        &mut self.gpu.as_mut().expect("just made").1
    }

    pub fn draw(
        &mut self,
        key: DeviceKey,
        device: &wgpu::Device,
        pass: &mut wgpu::RenderPass<'_>,
        site: &EffectSite<'_>,
    ) {
        if site.mark & 0xff != CAN {
            return;
        }
        // `trace` placed and traced it this frame, or it isn't drawn.
        let Ok(k) = self
            .slots
            .binary_search_by_key(&site.index, |slot| slot.index)
        else {
            return;
        };
        let offset = self.slots[k].offset;
        // Inside an isolated layer the can is placed in that layer's target, so it gets its own
        // copy of its uniforms.
        let layered = (site.origin != [0.0; 2] || site.size != self.viewport).then(|| {
            let start = offset as usize;
            let mut bytes = self.bytes[start..start + UNIFORM_SIZE as usize].to_vec();
            let viewport = [
                site.size[0] as f32,
                site.size[1] as f32,
                site.origin[0],
                site.origin[1],
            ];
            for (n, v) in viewport.into_iter().enumerate() {
                let at = VIEWPORT_AT + n * 4;
                bytes[at..at + 4].copy_from_slice(&v.to_ne_bytes());
            }
            bytes
        });
        let gpu = self.gpu(key, device);
        gpu.add_format(device, site.format);
        let (Some((_, uniforms, _)), Some((_, cells, _))) = (&gpu.uniforms, &gpu.cells) else {
            return;
        };
        let Some((_, pipeline)) = gpu.pipelines.iter().find(|(f, _)| *f == site.format) else {
            return;
        };
        pass.set_pipeline(pipeline);
        if let Some(bytes) = layered {
            let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("can placement"),
                contents: &bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let group = uniform_group(device, &gpu.layout, &uniform);
            pass.set_bind_group(0, &group, &[0]);
        } else {
            pass.set_bind_group(0, uniforms, &[offset]);
        }
        pass.set_bind_group(1, cells, &[]);
        pass.draw(0..4, 0..1);
    }
}

/// Binds one can's slot of `buffer`, which the draw picks with its dynamic offset.
fn uniform_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("can"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer,
                offset: 0,
                size: wgpu::BufferSize::new(UNIFORM_SIZE),
            }),
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(rarity: Rarity, look: u32) -> [f32; 4] {
        [0.0, rarity.index() as f32, 0.0, look as f32]
    }

    #[test]
    fn plain_cans_sway_and_rare_ones_turn_all_the_way() {
        for step in 0..400 {
            let (spin, _) = pose(Rarity::Uncommon, 0.0, step as f32 * 0.1);
            assert!(spin.abs() <= 0.6, "a sway stays on the spot");
        }
        let (start, _) = pose(Rarity::Rare, 0.0, 0.0);
        let (later, _) = pose(Rarity::Rare, 0.0, 10.0);
        assert!(later - start > std::f32::consts::TAU);
    }

    #[test]
    fn reduced_motion_rests_on_one_pose() {
        let at = |time| {
            let now = Now {
                time,
                since_turn: Some(0.4),
                ..Now::default()
            };
            placed(
                data(Rarity::Legendary, REDUCED | SHOWCASE),
                Hover::default(),
                &now,
            )
        };
        assert_eq!(at(1.0), at(99.0));
        assert_eq!(at(1.0).lift, 0.0, "and doesn't play the opening");
    }

    #[test]
    fn a_can_opens_tab_first_then_seals_and_stands_back_up() {
        let start = opening(0.0);
        assert_eq!((start.face, start.lift, start.depth), (0.0, 0.0, 0.0));

        let levered = opening(0.36);
        assert!(levered.face > 0.99, "tipped toward you");
        assert!(levered.lift > 0.9, "the tab is up");
        assert!(opening(0.2).depth == 0.0, "before the mouth gives");

        let open = opening(0.8);
        assert!(open.depth > 12.9, "the mouth is pushed in");
        assert!(open.lift < 0.2, "and the tab is pressed back down");

        let done = opening(OPENING_SECONDS);
        assert_eq!((done.face, done.lift, done.depth), (0.0, 0.0, 0.0));
    }

    #[test]
    fn only_the_large_can_opens_when_a_card_turns() {
        let now = Now {
            time: 5.0,
            since_turn: Some(0.5),
            ..Now::default()
        };
        let large = placed(data(Rarity::Rare, SHOWCASE), Hover::default(), &now);
        assert!(large.depth > 0.0 && large.pitch > REST_PITCH + 0.5);
        let small = placed(data(Rarity::Rare, 0), Hover::default(), &now);
        assert_eq!(
            (small.depth, small.lift, small.pitch),
            (0.0, 0.0, REST_PITCH)
        );
    }

    #[test]
    fn an_opening_can_turns_its_mouth_to_the_front() {
        let now = Now {
            time: 0.0,
            spin: Spin {
                clock: 3.0,
                ..Spin::default()
            },
            since_turn: Some(0.5),
        };
        let at = placed(data(Rarity::Rare, SHOWCASE), Hover::default(), &now);
        let turns = at.spin / std::f32::consts::TAU;
        assert!((turns - turns.round()).abs() < 0.001);
    }

    #[test]
    fn the_can_on_a_cards_back_is_bare_and_shakes_only_once_it_has_climbed() {
        let roll = |pressure: f32, level: f32| {
            let now = Now {
                time: 0.5,
                ..Now::default()
            };
            placed(
                [0.0, pressure, level * 100.0, MYSTERY as f32],
                Hover::default(),
                &now,
            )
            .roll
        };
        let calm = roll(0.0, 1.0);
        assert_ne!(roll(4.0, 1.0), calm, "a Legendary's back trembles");
        assert_eq!(roll(4.0, 0.0), calm, "and holds still for the hush");
        let now = Now::default();
        assert!(placed([0.0, 4.0, 100.0, MYSTERY as f32], Hover::default(), &now).bare);
    }

    #[test]
    fn a_hovered_can_leans_toward_you() {
        let now = Now::default();
        let hovered = Hover {
            amount: 1.0,
            turn: 0.5,
            ..Hover::default()
        };
        let at = placed(data(Rarity::Common, HOVERED), hovered, &now);
        let rest = placed(data(Rarity::Common, 0), Hover::default(), &now);
        assert!(at.pitch > rest.pitch);
        assert_eq!(at.spin, rest.spin + 0.5);
    }

    #[test]
    fn dragging_turns_the_can_with_the_pointer_and_tips_it() {
        let mut spin = Spin {
            held: true,
            ..Spin::default()
        };
        spin.turn(100.0, 50.0, 0.016);
        assert!(spin.yaw < 0.0, "the near side follows the pointer right");
        assert!(spin.pitch > 0.0, "pulling down shows the lid");
        spin.turn(0.0, 10_000.0, 0.016);
        assert_eq!(spin.pitch, *PITCH_RANGE.end(), "but not past the limit");
    }

    #[test]
    fn a_flung_can_coasts_to_a_stop_and_stands_back_up() {
        let mut spin = Spin {
            held: true,
            ..Spin::default()
        };
        spin.turn(60.0, 40.0, 0.016);
        let (yaw, pitch) = (spin.yaw, spin.pitch);
        spin.advance(0.1);
        assert_eq!((spin.yaw, spin.pitch), (yaw, pitch), "held, it stays put");
        assert_eq!(spin.clock, 0.0, "and its idle turn waits");

        spin.release(false);
        spin.advance(0.1);
        assert!(spin.yaw < yaw, "it keeps turning the way it was flung");
        for _ in 0..200 {
            spin.advance(0.05);
        }
        assert_eq!(spin.velocity, 0.0);
        assert!(spin.pitch.abs() < 0.001);
        assert!(spin.clock > 0.0);
    }

    #[test]
    fn a_can_held_still_before_release_doesnt_coast() {
        let mut spin = Spin {
            held: true,
            ..Spin::default()
        };
        spin.turn(60.0, 0.0, 0.016);
        spin.release(true);
        let yaw = spin.yaw;
        spin.advance(0.1);
        assert_eq!(spin.yaw, yaw);
    }
}
