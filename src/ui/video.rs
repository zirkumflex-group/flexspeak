//! I420 video stays planar until the compositor draws it in painter order.
use crate::{App, streaming::MAX_WATCHES};
use rune::compose::{DeviceKey, Frame};
use rune::render::EffectSite;
use wgpu::util::DeviceExt;

pub const VIDEO: u32 = 2;
/// A stream's picture, fitted inside its box; or filling it instead when that crops no more
/// than `crop` of it (0.02 is 2%), so a box a hair off the stream's shape shows no bars.
pub fn filling(slot: usize, rotation: f32, crop: f32) -> rune::El {
    rune::effect(VIDEO).effect_data([slot as f32, rotation, crop, 0.0])
}
struct Picture {
    size: (u32, u32),
    stamp: (u64, u64),
    textures: [wgpu::Texture; 3],
    views: [wgpu::TextureView; 3],
}
struct Gpu {
    layout: wgpu::BindGroupLayout,
    shader: wgpu::ShaderModule,
    pipelines: Vec<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
    pictures: [Option<Picture>; MAX_WATCHES],
}
#[derive(Default)]
pub struct Videos {
    gpu: Option<(DeviceKey, Gpu)>,
}
impl Videos {
    pub fn prepare(&mut self, f: &Frame<'_, App>) -> bool {
        let surfaces = &f.rt.state.server().streaming.surfaces;
        // The streams some painter draws this frame, exits and drag ghosts included. Only
        // those upload, and only those wake the window for each new frame.
        let mut drawn = [false; MAX_WATCHES];
        for (n, i) in f.rt.instances.iter().enumerate() {
            if i.params[1] == rune::render::KIND_EFFECT
                && f.rt.marks.get(n).copied().unwrap_or(0) & 0xff == VIDEO
                && let Some(slot) = drawn.get_mut(i.uv[0] as usize)
            {
                *slot = true;
            }
        }
        for (slot, drawn) in drawn.iter().enumerate() {
            let surface = surfaces.get(slot);
            surface.set_shown(*drawn);
            // An ended stream's picture goes, even when no video is left to draw.
            if surface.latest().is_none()
                && let Some((_, gpu)) = &mut self.gpu
            {
                gpu.pictures[slot] = None;
            }
        }
        if !drawn.contains(&true) {
            return false;
        }
        let key = f.device_key();
        if self.gpu.as_ref().is_none_or(|(k, _)| *k != key) {
            let entries: Vec<_> = (0..4)
                .map(|binding| wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: if binding == 3 {
                        wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        }
                    } else {
                        wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        }
                    },
                    count: None,
                })
                .collect();
            let layout = f
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("I420"),
                    entries: &entries,
                });
            let shader = f.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("I420 BT.601 limited"),
                source: wgpu::ShaderSource::Wgsl(
                    format!(
                        "{}\n{}",
                        rune::render::CLIP_WGSL,
                        include_str!("video.wgsl")
                    )
                    .into(),
                ),
            });
            self.gpu = Some((
                key,
                Gpu {
                    layout,
                    shader,
                    pipelines: Vec::new(),
                    pictures: std::array::from_fn(|_| None),
                },
            ));
        }
        let gpu = &mut self.gpu.as_mut().unwrap().1;
        for slot in (0..MAX_WATCHES).filter(|&slot| drawn[slot]) {
            let surface = surfaces.get(slot);
            surface.with_planes(|frame, planes| {
                let size = (frame.width, frame.height);
                if gpu.pictures[slot].as_ref().is_none_or(|p| p.size != size) {
                    let textures = std::array::from_fn(|n| {
                        f.device.create_texture(&wgpu::TextureDescriptor {
                            label: Some("I420 plane"),
                            size: extent(size, n),
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: wgpu::TextureFormat::R8Unorm,
                            usage: wgpu::TextureUsages::TEXTURE_BINDING
                                | wgpu::TextureUsages::COPY_DST,
                            view_formats: &[],
                        })
                    });
                    let views =
                        std::array::from_fn(|n| textures[n].create_view(&Default::default()));
                    gpu.pictures[slot] = Some(Picture {
                        size,
                        stamp: (0, 0),
                        textures,
                        views,
                    });
                }
                let p = gpu.pictures[slot].as_mut().unwrap();
                let stamp = (frame.generation, frame.sequence);
                if p.stamp == stamp {
                    return;
                }
                for (n, plane) in planes.iter().enumerate() {
                    let e = extent(size, n);
                    f.queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &p.textures[n],
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        plane,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(e.width),
                            rows_per_image: Some(e.height),
                        },
                        e,
                    );
                }
                p.stamp = stamp;
                surface.note_presented();
            });
        }
        true
    }
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        pass: &mut wgpu::RenderPass<'_>,
        site: &EffectSite<'_>,
    ) {
        if site.mark & 0xff != VIDEO {
            return;
        }
        let Some((_, gpu)) = &mut self.gpu else {
            return;
        };
        let slot = site.data[0] as usize;
        if slot >= MAX_WATCHES || gpu.pictures[slot].is_none() {
            return;
        }
        if !gpu.pipelines.iter().any(|(fmt, _)| *fmt == site.format) {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("I420"),
                bind_group_layouts: &[Some(&gpu.layout)],
                immediate_size: 0,
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("I420"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &gpu.shader,
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
                    module: &gpu.shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: site.format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
            gpu.pipelines.push((site.format, pipeline));
        }
        let p = gpu.pictures[slot].as_ref().unwrap();
        let mut bytes = Vec::with_capacity(112);
        for v in site.instance.rect.into_iter().chain(site.instance.clip) {
            bytes.extend(v.to_ne_bytes());
        }
        for v in site.instance.fade {
            bytes.extend(v.to_ne_bytes());
        }
        for v in [
            site.size[0] as f32,
            site.size[1] as f32,
            site.origin[0],
            site.origin[1],
            p.size.0 as f32,
            p.size.1 as f32,
            site.data[1],
            f32::from(site.srgb_blend),
            site.instance.fill[3],
            site.data[2],
            0.0,
            0.0,
        ] {
            bytes.extend(v.to_ne_bytes());
        }
        // A distinct uniform per occurrence: thumbnails and drag ghosts can share a picture.
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("I420 placement"),
            contents: &bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("I420"),
            layout: &gpu.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&p.views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&p.views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&p.views[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        pass.set_pipeline(
            &gpu.pipelines
                .iter()
                .find(|(fmt, _)| *fmt == site.format)
                .unwrap()
                .1,
        );
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..4, 0..1);
    }
}
fn extent((w, h): (u32, u32), n: usize) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: if n == 0 { w } else { w.div_ceil(2) },
        height: if n == 0 { h } else { h.div_ceil(2) },
        depth_or_array_layers: 1,
    }
}
