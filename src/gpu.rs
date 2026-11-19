//! GPU side: one wgpu device, the atlas, and a transparent surface per monitor. Each frame the vertices are
//! uploaded once and drawn into every surface with that monitor's view. At reduced quality the frame is drawn
//! into a smaller texture first and stretched onto the surface.

use crate::atlas::{build_atlas, ATLAS_H, ATLAS_W};
use crate::draw::Vertex;
use crate::platform::Rect;

/// What the main thread hands over for each monitor: its surface and where it sits.
pub struct TargetDesc {
    pub surface: wgpu::Surface<'static>,
    pub rect: Rect,         // in overlay units
    pub size: (u32, u32),   // surface size in physical pixels
}

struct Target {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    rect: Rect,
    view_buf: wgpu::Buffer,
    view_group: wgpu::BindGroup,
    reduced: Option<(wgpu::TextureView, wgpu::BindGroup)>,
    cleared: bool,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ViewUniform {
    origin: [f32; 2],
    inv_size: [f32; 2],
}

pub enum FrameError {
    /// Surfaces need recreating (lost); the caller should ask the main thread for new ones.
    Lost,
}

pub struct Gpu {
    instance: wgpu::Instance,
    adapter: Option<wgpu::Adapter>,
    device: Option<(wgpu::Device, wgpu::Queue)>,
    pipeline: Option<wgpu::RenderPipeline>,
    blit: Option<wgpu::RenderPipeline>,
    atlas_group: Option<wgpu::BindGroup>,
    tex_layout: Option<wgpu::BindGroupLayout>,
    view_layout: Option<wgpu::BindGroupLayout>,
    sampler: Option<wgpu::Sampler>,
    vbuf: Option<wgpu::Buffer>,
    vcap: u64,
    targets: Vec<Target>,
    format: wgpu::TextureFormat,
    quality_scale: f32,
}

/// The surface alpha modes that keep the overlay see-through, best first. Not `Inherit`: on X11 a window
/// without an alpha channel reports it too, and there it means opaque.
const TRANSPARENT_ALPHA_MODES: [wgpu::CompositeAlphaMode; 2] =
    [wgpu::CompositeAlphaMode::PreMultiplied, wgpu::CompositeAlphaMode::PostMultiplied];

fn pick_alpha_mode(supported: &[wgpu::CompositeAlphaMode]) -> Option<wgpu::CompositeAlphaMode> {
    TRANSPARENT_ALPHA_MODES.into_iter().find(|m| supported.contains(m))
}

pub fn new_instance() -> wgpu::Instance {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::DX12 | wgpu::Backends::METAL | wgpu::Backends::VULKAN;
    // a swapchain made for DirectComposition is the one that can be transparent on Windows
    desc.backend_options.dx12.presentation_system = wgpu::Dx12SwapchainKind::DxgiFromVisual;
    wgpu::Instance::new(desc)
}

impl Gpu {
    pub fn new(instance: wgpu::Instance) -> Self {
        Self {
            instance,
            adapter: None,
            device: None,
            pipeline: None,
            blit: None,
            atlas_group: None,
            tex_layout: None,
            view_layout: None,
            sampler: None,
            vbuf: None,
            vcap: 0,
            targets: Vec::new(),
            format: wgpu::TextureFormat::Bgra8Unorm,
            quality_scale: 1.0,
        }
    }

    pub fn ready(&self) -> bool {
        self.device.is_some() && !self.targets.is_empty()
    }

    /// Output pixels per overlay unit on the densest monitor, after the quality scale.
    pub fn pixels_per_unit(&self) -> f32 {
        let mut best = 0.0f32;
        for t in &self.targets {
            let w = (t.rect.r - t.rect.l).max(1.0);
            best = best.max(t.config.width as f32 / w);
        }
        if best <= 0.0 {
            1.0
        } else {
            best * self.quality_scale
        }
    }

    fn init_device(&mut self, surface: &wgpu::Surface<'static>) -> Result<(), String> {
        let adapter = pollster::block_on(self.instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(surface),
            force_fallback_adapter: false,
            ..Default::default()
        }))
        .map_err(|e| format!("no GPU adapter: {e}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("parapet"),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .map_err(|e| format!("no GPU device: {e}"))?;

        #[cfg(feature = "debug-overlay")]
        eprintln!("parapet: adapter {:?}", adapter.get_info());
        let caps = surface.get_capabilities(&adapter);
        self.format = caps
            .formats
            .iter()
            .copied()
            .find(|f| matches!(f, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm))
            .unwrap_or(caps.formats[0]);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("parapet"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let view_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("view"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sprites"),
            bind_group_layouts: &[Some(&tex_layout), Some(&view_layout)],
            immediate_size: 0,
        });
        let premultiplied = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x2];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sprites"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attrs,
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: Some(premultiplied),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let blit_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit"),
            bind_group_layouts: &[Some(&tex_layout)],
            immediate_size: 0,
        });
        let blit = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit"),
            layout: Some(&blit_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("blit_vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("blit_fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        // the atlas, premultiplied, sampled bilinearly like Direct2D's sprite batches did
        let atlas_pixels = build_atlas();
        #[cfg(feature = "debug-overlay")]
        crate::atlas::dump_png(&atlas_pixels, &crate::settings::path().with_extension("ini.atlas.png"));
        let size = wgpu::Extent3d { width: ATLAS_W, height: ATLAS_H, depth_or_array_layers: 1 };
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("atlas"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &atlas,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &atlas_pixels,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(ATLAS_W * 4), rows_per_image: Some(ATLAS_H) },
            size,
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let atlas_view = atlas.create_view(&wgpu::TextureViewDescriptor::default());
        let atlas_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("atlas"),
            layout: &tex_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&atlas_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });

        self.adapter = Some(adapter);
        self.pipeline = Some(pipeline);
        self.blit = Some(blit);
        self.atlas_group = Some(atlas_group);
        self.tex_layout = Some(tex_layout);
        self.view_layout = Some(view_layout);
        self.sampler = Some(sampler);
        self.device = Some((device, queue));
        Ok(())
    }

    /// Replaces all surfaces; returns the old ones so the main thread can drop them next to their windows.
    pub fn set_targets(&mut self, descs: Vec<TargetDesc>, quality_scale: f32) -> Result<Vec<wgpu::Surface<'static>>, String> {
        let old: Vec<_> = self.targets.drain(..).map(|t| t.surface).collect();
        if descs.is_empty() {
            return Ok(old);
        }
        if self.device.is_none() {
            self.init_device(&descs[0].surface)?;
        }
        self.quality_scale = quality_scale;
        for d in descs {
            let t = self.make_target(d)?;
            self.targets.push(t);
        }
        Ok(old)
    }

    fn make_target(&self, d: TargetDesc) -> Result<Target, String> {
        let (device, _) = self.device.as_ref().unwrap();
        let adapter = self.adapter.as_ref().unwrap();
        let caps = d.surface.get_capabilities(adapter);
        // an opaque surface would cover the whole monitor in black: refuse it rather than draw it
        let alpha_mode = pick_alpha_mode(&caps.alpha_modes)
            .ok_or_else(|| format!("the GPU can only draw opaque windows here ({:?})", caps.alpha_modes))?;
        let present_mode = [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate]
            .into_iter()
            .find(|m| caps.present_modes.contains(m))
            .unwrap_or(wgpu::PresentMode::Fifo);
        let mut config = d
            .surface
            .get_default_config(adapter, d.size.0.max(1), d.size.1.max(1))
            .ok_or("the GPU cannot present to this window")?;
        config.usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        config.format = self.format;
        config.present_mode = present_mode;
        config.desired_maximum_frame_latency = 1;
        config.alpha_mode = alpha_mode;
        config.view_formats = vec![];
        d.surface.configure(device, &config);
        let view_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("view"),
            size: std::mem::size_of::<ViewUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("view"),
            layout: self.view_layout.as_ref().unwrap(),
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: view_buf.as_entire_binding() }],
        });
        let mut t = Target { surface: d.surface, config, rect: d.rect, view_buf, view_group, reduced: None, cleared: false };
        self.make_reduced(&mut t);
        Ok(t)
    }

    fn make_reduced(&self, t: &mut Target) {
        t.reduced = None;
        if self.quality_scale >= 0.999 {
            return;
        }
        let (device, _) = self.device.as_ref().unwrap();
        let w = ((t.config.width as f32 * self.quality_scale).round() as u32).max(1);
        let h = ((t.config.height as f32 * self.quality_scale).round() as u32).max(1);
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("reduced frame"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("reduced frame"),
            layout: self.tex_layout.as_ref().unwrap(),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(self.sampler.as_ref().unwrap()) },
            ],
        });
        t.reduced = Some((view, group));
    }

    pub fn set_quality(&mut self, quality_scale: f32) {
        if (quality_scale - self.quality_scale).abs() < 1e-3 {
            return;
        }
        self.quality_scale = quality_scale;
        let mut targets = std::mem::take(&mut self.targets);
        for t in &mut targets {
            self.make_reduced(t);
        }
        self.targets = targets;
    }

    pub fn resize(&mut self, index: usize, size: (u32, u32)) {
        let Some((device, _)) = self.device.as_ref() else { return };
        let Some(t) = self.targets.get_mut(index) else { return };
        if (t.config.width, t.config.height) == size || size.0 == 0 || size.1 == 0 {
            return;
        }
        t.config.width = size.0;
        t.config.height = size.1;
        t.surface.configure(device, &t.config);
        let mut t = self.targets.remove(index);
        self.make_reduced(&mut t);
        self.targets.insert(index, t);
    }

    /// Draws the frame into every surface. `verts` empty draws a clear frame.
    pub fn render(&mut self, verts: &[Vertex]) -> Result<(), FrameError> {
        let Some((device, queue)) = self.device.as_ref() else { return Ok(()) };
        let bytes: &[u8] = bytemuck::cast_slice(verts);
        if !bytes.is_empty() && (self.vbuf.is_none() || self.vcap < bytes.len() as u64) {
            self.vcap = (bytes.len() as u64).next_power_of_two().max(1 << 20);
            self.vbuf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("vertices"),
                size: self.vcap,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if !bytes.is_empty() {
            queue.write_buffer(self.vbuf.as_ref().unwrap(), 0, bytes);
        }
        let count = verts.len() as u32;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        let mut frames = Vec::with_capacity(self.targets.len());
        let mut lost = false;
        for t in &mut self.targets {
            if count == 0 && t.cleared {
                continue;
            }
            let frame = match t.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
                wgpu::CurrentSurfaceTexture::Outdated => {
                    t.surface.configure(device, &t.config);
                    continue;
                }
                wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => continue,
                _ => {
                    lost = true;
                    continue;
                }
            };
            let w = (t.rect.r - t.rect.l).max(1.0);
            let h = (t.rect.b - t.rect.t).max(1.0);
            queue.write_buffer(
                &t.view_buf,
                0,
                bytemuck::bytes_of(&ViewUniform { origin: [t.rect.l, t.rect.t], inv_size: [1.0 / w, 1.0 / h] }),
            );
            let surface_view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
            let target_view = t.reduced.as_ref().map(|r| &r.0).unwrap_or(&surface_view);
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("sprites"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                    })],
                    ..Default::default()
                });
                if count > 0 {
                    pass.set_pipeline(self.pipeline.as_ref().unwrap());
                    pass.set_bind_group(0, self.atlas_group.as_ref().unwrap(), &[]);
                    pass.set_bind_group(1, &t.view_group, &[]);
                    pass.set_vertex_buffer(0, self.vbuf.as_ref().unwrap().slice(..bytes.len() as u64));
                    pass.draw(0..count, 0..1);
                }
            }
            if let Some((_, group)) = &t.reduced {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("upscale"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &surface_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(self.blit.as_ref().unwrap());
                pass.set_bind_group(0, group, &[]);
                pass.draw(0..3, 0..1);
            }
            t.cleared = count == 0;
            frames.push(frame);
        }
        queue.submit(Some(encoder.finish()));
        for f in frames {
            queue.present(f);
        }
        if lost {
            Err(FrameError::Lost)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::CompositeAlphaMode as A;

    #[test]
    fn prefers_premultiplied_alpha() {
        assert_eq!(pick_alpha_mode(&[A::Opaque, A::Inherit, A::PreMultiplied]), Some(A::PreMultiplied));
    }

    #[test]
    fn refuses_inherit_which_an_opaque_x11_window_reports() {
        assert_eq!(pick_alpha_mode(&[A::Opaque, A::Inherit]), None);
    }

    #[test]
    fn refuses_a_surface_that_can_only_be_opaque() {
        assert_eq!(pick_alpha_mode(&[A::Opaque]), None);
    }
}
