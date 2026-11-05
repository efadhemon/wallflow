//! GPU side: two textures (Y and interleaved UV) uploaded straight from the
//! decoder's NV12 output, and one draw that converts to RGB while it scales.
//! The same code runs on Vulkan, Metal and Direct3D 12 through wgpu.

use crate::fit::{FillMode, Placement, place};
use crate::probe::VideoInfo;
use anyhow::{Context, Result, anyhow, bail};
use std::sync::Arc;
use winit::window::Window;

const SHADER: &str = r#"
struct Params {
    pos_scale: vec2<f32>,
    uv_offset: vec2<f32>,
    uv_scale: vec2<f32>,
    bt709: f32,
    full_range: f32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var tex_y: texture_2d<f32>;
@group(0) @binding(2) var tex_uv: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// A full-screen triangle strip; the corners come from the vertex index.
@vertex
fn vs(@builtin(vertex_index) index: u32) -> VsOut {
    let corner = vec2<f32>(f32(index & 1u), f32((index >> 1u) & 1u));
    let ndc = (corner * 2.0 - 1.0) * params.pos_scale;
    var out: VsOut;
    out.pos = vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
    out.uv = params.uv_offset + corner * params.uv_scale;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    var y = textureSample(tex_y, samp, in.uv).r;
    var c = textureSample(tex_uv, samp, in.uv).rg - vec2<f32>(0.5, 0.5);
    if (params.full_range < 0.5) {
        y = (y - 16.0 / 255.0) * (255.0 / 219.0);
        c = c * (255.0 / 224.0);
    }
    var rgb: vec3<f32>;
    if (params.bt709 > 0.5) {
        rgb = vec3<f32>(y + 1.5748 * c.y, y - 0.1873 * c.x - 0.4681 * c.y, y + 1.8556 * c.x);
    } else {
        rgb = vec3<f32>(y + 1.402 * c.y, y - 0.3441 * c.x - 0.7141 * c.y, y + 1.772 * c.x);
    }
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    pos_scale: [f32; 2],
    uv_offset: [f32; 2],
    uv_scale: [f32; 2],
    bt709: f32,
    full_range: f32,
}

/// Everything shared by all monitors: one device, one pipeline, and the two
/// plane textures, so a frame is decoded and uploaded once however many
/// screens show it.
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter: wgpu::Adapter,
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    view_y: wgpu::TextureView,
    view_uv: wgpu::TextureView,
    tex_y: wgpu::Texture,
    tex_uv: wgpu::Texture,
    video: VideoInfo,
    /// Nothing is drawn until the first frame is uploaded, so a black flash
    /// never replaces the static wallpaper underneath.
    has_frame: bool,
}

/// One window (monitor): its surface and where the video sits on it.
pub struct Target {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    params: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    mode: FillMode,
}

impl Gpu {
    /// Opens the GPU that can present to `window` and returns the shared state
    /// together with that window's target.
    pub fn new(
        instance: &wgpu::Instance,
        window: Arc<Window>,
        video: VideoInfo,
        mode: FillMode,
    ) -> Result<(Self, Target)> {
        let surface = instance
            .create_surface(window.clone())
            .context("creating the window surface")?;

        // A wallpaper never needs the discrete GPU; waking it costs battery.
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .context("no GPU adapter can draw to this window")?;
        let adapter_info = adapter.get_info();
        log::info!(
            "rendering on {} ({:?})",
            adapter_info.name,
            adapter_info.backend
        );

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("wallflow-live"),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .context("opening the GPU")?;

        // The shader writes display-referred values, so skip the sRGB encode.
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|format| !format.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or_else(|| anyhow!("the GPU cannot present to this window"))?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nv12"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nv12"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(1),
                texture_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("nv12"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nv12"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
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
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });

        let tex_y = plane(
            &device,
            "y",
            video.width,
            video.height,
            wgpu::TextureFormat::R8Unorm,
        );
        let tex_uv = plane(
            &device,
            "uv",
            video.width.div_ceil(2),
            video.height.div_ceil(2),
            wgpu::TextureFormat::Rg8Unorm,
        );

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("video"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let gpu = Self {
            view_y: tex_y.create_view(&Default::default()),
            view_uv: tex_uv.create_view(&Default::default()),
            device,
            queue,
            adapter,
            format,
            pipeline,
            layout,
            sampler,
            tex_y,
            tex_uv,
            video,
            has_frame: false,
        };
        let target = gpu.target_for(surface, window.inner_size(), mode)?;
        Ok((gpu, target))
    }

    /// A target for another monitor's window, sharing this GPU.
    pub fn add_target(
        &self,
        instance: &wgpu::Instance,
        window: Arc<Window>,
        mode: FillMode,
    ) -> Result<Target> {
        let size = window.inner_size();
        let surface = instance
            .create_surface(window)
            .context("creating the window surface")?;
        self.target_for(surface, size, mode)
    }

    fn target_for(
        &self,
        surface: wgpu::Surface<'static>,
        size: winit::dpi::PhysicalSize<u32>,
        mode: FillMode,
    ) -> Result<Target> {
        let mut config = surface
            .get_default_config(&self.adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| anyhow!("the GPU cannot present to this window"))?;
        if !surface
            .get_capabilities(&self.adapter)
            .formats
            .contains(&self.format)
        {
            bail!("this monitor's surface does not support {:?}", self.format);
        }
        config.format = self.format;
        config.present_mode = wgpu::PresentMode::Fifo;
        config.desired_maximum_frame_latency = 1;
        surface.configure(&self.device, &config);

        let params = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nv12"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.view_y),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&self.view_uv),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        let target = Target {
            surface,
            config,
            params,
            bind_group,
            mode,
        };
        self.write_params(&target);
        Ok(target)
    }

    fn write_params(&self, target: &Target) {
        let Placement {
            pos_scale,
            uv_offset,
            uv_scale,
        } = place(
            target.mode,
            (self.video.width, self.video.height),
            (target.config.width, target.config.height),
        );
        let params = Params {
            pos_scale,
            uv_offset,
            uv_scale,
            bt709: if self.video.bt709 { 1.0 } else { 0.0 },
            full_range: if self.video.full_range { 1.0 } else { 0.0 },
        };
        self.queue
            .write_buffer(&target.params, 0, bytemuck::bytes_of(&params));
    }

    pub fn resize(&self, target: &mut Target, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        target.config.width = width;
        target.config.height = height;
        target.surface.configure(&self.device, &target.config);
        self.write_params(target);
    }

    pub fn set_mode(&self, target: &mut Target, mode: FillMode) {
        target.mode = mode;
        self.write_params(target);
    }

    /// Copies one NV12 frame into the two plane textures.
    pub fn upload(&mut self, nv12: &[u8]) {
        let (w, h) = (self.video.width, self.video.height);
        let luma = (w * h) as usize;
        write_plane(&self.queue, &self.tex_y, &nv12[..luma], w, h, 1);
        write_plane(
            &self.queue,
            &self.tex_uv,
            &nv12[luma..],
            w.div_ceil(2),
            h.div_ceil(2),
            2,
        );
        self.has_frame = true;
    }

    /// Draws the latest frame into one window. Returns false when it could not
    /// be shown (surface lost or occluded) so the caller can back off.
    pub fn draw(&self, target: &Target) -> bool {
        if !self.has_frame {
            return false;
        }
        let frame = match target.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                target.surface.configure(&self.device, &target.config);
                return false;
            }
            other => {
                log::debug!("skipping a frame: {other:?}");
                return false;
            }
        };

        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &target.bind_group, &[]);
            pass.draw(0..4, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        true
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn plane(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn write_plane(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    data: &[u8],
    width: u32,
    height: u32,
    bytes_per_pixel: u32,
) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * bytes_per_pixel),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}
