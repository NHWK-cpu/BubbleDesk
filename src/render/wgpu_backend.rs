//! Backend render GPU berbasis wgpu (DX12) untuk Windows (spec §11).
//!
//! Mengelola device, surface swapchain, tekstur latar belakang wallpaper,
//! instanced draw call quad untuk bubble SDF, dan debug overlay.

use super::atlas::ATLAS_PAGE_SIZE;
use super::text::{
    GlyphInstance, LabelBgInstance, TextRenderer, GLYPH_ATLAS_SIZE, OVERLAY_HEIGHT, OVERLAY_WIDTH,
};
use super::{FrameData, RenderError, Renderer};
use crate::platform::wallpaper::WallpaperImage;
use bytemuck::{Pod, Zeroable};
use std::sync::Arc;
use winit::window::Window;

/// Representasi data seragam (uniform) layar untuk shader.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ScreenUniformRaw {
    screen_size: [f32; 2],
    background_dim: f32,
    time: f32,
}

/// Representasi byte mentah instance bubble untuk vertex buffer.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct BubbleInstanceRaw {
    pub center: [f32; 2],
    pub radius: f32,
    pub visual_scale: f32,
    pub ring_color: [f32; 4],
    pub ring_width: f32,
    pub glow: f32,
    pub flags: u32,
    pub icon_page: i32,
    pub icon_uv: [f32; 4],
}

impl BubbleInstanceRaw {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 8,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32,
                },
                wgpu::VertexAttribute {
                    offset: 16,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 32,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32,
                },
                wgpu::VertexAttribute {
                    offset: 36,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32,
                },
                wgpu::VertexAttribute {
                    offset: 40,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Uint32,
                },
                wgpu::VertexAttribute {
                    offset: 48,
                    shader_location: 7,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 44,
                    shader_location: 8,
                    format: wgpu::VertexFormat::Sint32,
                },
            ],
        }
    }
}

fn label_bg_desc() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<LabelBgInstance>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &[
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: 8,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x2,
            },
        ],
    }
}

fn glyph_instance_desc() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<GlyphInstance>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &[
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: 8,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: 16,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: 24,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: 32,
                shader_location: 4,
                format: wgpu::VertexFormat::Float32x4,
            },
        ],
    }
}

/// Instance data untuk posisi dan ukuran panel overlay.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct OverlayInstanceRaw {
    rect_pos: [f32; 2],
    rect_size: [f32; 2],
}

impl OverlayInstanceRaw {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 8,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

/// Renderer grafis berbasis wgpu.
pub struct WgpuRenderer {
    _window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: wgpu::SurfaceConfiguration,

    screen_uniform_buffer: wgpu::Buffer,
    screen_bind_group: wgpu::BindGroup,

    background_texture: wgpu::Texture,
    background_bind_group_layout: wgpu::BindGroupLayout,
    background_bind_group: wgpu::BindGroup,
    background_sampler: wgpu::Sampler,
    background_pipeline: wgpu::RenderPipeline,

    bubble_pipeline: wgpu::RenderPipeline,
    bubble_instance_buffer: wgpu::Buffer,
    bubble_capacity: usize,

    atlas_dummy_texture: wgpu::Texture,
    atlas_textures: [Option<wgpu::Texture>; 2],
    atlas_bind_group_layout: wgpu::BindGroupLayout,
    atlas_bind_group: wgpu::BindGroup,
    atlas_sampler: wgpu::Sampler,

    label_bg_pipeline: wgpu::RenderPipeline,
    label_bg_buffer: wgpu::Buffer,
    label_bg_capacity: usize,

    glyph_texture: wgpu::Texture,
    glyph_bind_group: wgpu::BindGroup,
    text_pipeline: wgpu::RenderPipeline,
    text_instance_buffer: wgpu::Buffer,
    text_capacity: usize,

    overlay_texture: wgpu::Texture,
    overlay_bind_group: wgpu::BindGroup,
    overlay_pipeline: wgpu::RenderPipeline,
    overlay_vertex_buffer: wgpu::Buffer,

    text_renderer: TextRenderer,
    scratch_bubbles: Vec<BubbleInstanceRaw>,
    scratch_label_bgs: Vec<LabelBgInstance>,
    scratch_glyphs: Vec<GlyphInstance>,
    width: u32,
    height: u32,
}

impl WgpuRenderer {
    /// Menginisialisasi wgpu dan pipeline grafis pada jendela winit.
    pub fn new(
        window: Arc<Window>,
        initial_width: u32,
        initial_height: u32,
    ) -> Result<Self, RenderError> {
        let width = initial_width.max(1);
        let height = initial_height.max(1);

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            // DX12 sebagai backend utama Windows (spec §11)
            backends: wgpu::Backends::DX12,
            ..Default::default()
        });

        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| RenderError::Other(format!("gagal membuat surface: {e}")))?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| {
            RenderError::Other("tidak menemukan adapter GPU yang kompatibel".to_owned())
        })?;

        log_info!(
            "menggunakan adapter grafis: {} ({:?})",
            adapter.get_info().name,
            adapter.get_info().backend
        );

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("BubbleDesk Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .map_err(|e| RenderError::Other(format!("gagal membuat device GPU: {e}")))?;

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            // Fifo = vsync 60 FPS (spec §11)
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        // Shader WGSL
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("BubbleDesk WGSL Shaders"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/bubble.wgsl").into()),
        });

        // 1. Screen Uniform Buffer & Bind Group
        let initial_uniform = ScreenUniformRaw {
            screen_size: [width as f32, height as f32],
            background_dim: 0.35,
            time: 0.0,
        };
        let screen_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Screen Uniform Buffer"),
            size: std::mem::size_of::<ScreenUniformRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(
            &screen_uniform_buffer,
            0,
            bytemuck::bytes_of(&initial_uniform),
        );

        let screen_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Screen Uniform Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let screen_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Screen Uniform Bind Group"),
            layout: &screen_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen_uniform_buffer.as_entire_binding(),
            }],
        });

        // 2. Wallpaper Background Texture & Pipeline
        let background_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Wallpaper Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let background_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Background Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
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

        // Tekstur awal 1x1 hitam sebelum diisi wallpaper
        let background_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Background Wallpaper Texture"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &background_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[20, 24, 32, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );

        let background_view =
            background_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let background_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Background Bind Group"),
            layout: &background_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&background_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&background_sampler),
                },
            ],
        });

        let background_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Background Pipeline Layout"),
                bind_group_layouts: &[&screen_bind_group_layout, &background_bind_group_layout],
                push_constant_ranges: &[],
            });

        let background_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Background Render Pipeline"),
            layout: Some(&background_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_background"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_background"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_config.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // 3. Atlas Ikon & Bubble Instanced Pipeline
        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Icon Atlas Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let atlas_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Icon Atlas Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        // Tekstur dummy 1×1 transparan sebelum halaman atlas diisi (lazy allocation)
        let atlas_dummy_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Icon Atlas Dummy Texture"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &atlas_dummy_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[0, 0, 0, 0],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let dummy_view = atlas_dummy_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let atlas_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Icon Atlas Bind Group"),
            layout: &atlas_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&dummy_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&dummy_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&atlas_sampler),
                },
            ],
        });

        let bubble_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Bubble Pipeline Layout"),
                bind_group_layouts: &[&screen_bind_group_layout, &atlas_bind_group_layout],
                push_constant_ranges: &[],
            });

        let bubble_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Bubble Render Pipeline"),
            layout: Some(&bubble_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_bubble"),
                buffers: &[BubbleInstanceRaw::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_bubble"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let initial_capacity = 64;
        let bubble_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Bubble Instance Buffer"),
            size: (initial_capacity * std::mem::size_of::<BubbleInstanceRaw>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 4. Debug Overlay Pipeline & Texture
        let overlay_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Overlay Texture"),
            size: wgpu::Extent3d {
                width: OVERLAY_WIDTH,
                height: OVERLAY_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let overlay_view = overlay_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let overlay_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Overlay Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
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

        let overlay_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Overlay Bind Group"),
            layout: &overlay_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&overlay_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&background_sampler),
                },
            ],
        });

        let overlay_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Overlay Pipeline Layout"),
                bind_group_layouts: &[&screen_bind_group_layout, &overlay_bind_group_layout],
                push_constant_ranges: &[],
            });

        let overlay_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Overlay Render Pipeline"),
            layout: Some(&overlay_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_overlay"),
                buffers: &[OverlayInstanceRaw::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_overlay"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let overlay_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Overlay Instance Buffer"),
            size: std::mem::size_of::<OverlayInstanceRaw>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 5. Label Pill & Glyph Text Pipeline (R8 512×512)
        let label_bg_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Label Bg Pipeline Layout"),
                bind_group_layouts: &[&screen_bind_group_layout],
                push_constant_ranges: &[],
            });
        let label_bg_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Label Bg Render Pipeline"),
            layout: Some(&label_bg_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_label_bg"),
                buffers: &[label_bg_desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_label_bg"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let label_bg_capacity = 64;
        let label_bg_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Label Bg Instance Buffer"),
            size: (label_bg_capacity * std::mem::size_of::<LabelBgInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let glyph_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Glyph Atlas R8 Texture"),
            size: wgpu::Extent3d {
                width: GLYPH_ATLAS_SIZE,
                height: GLYPH_ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let glyph_view = glyph_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let glyph_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Glyph Atlas Bind Group"),
            layout: &overlay_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&glyph_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&background_sampler),
                },
            ],
        });
        let text_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Label Text Render Pipeline"),
            layout: Some(&overlay_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_text"),
                buffers: &[glyph_instance_desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_text"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let text_capacity = 512;
        let text_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Label Glyph Instance Buffer"),
            size: (text_capacity * std::mem::size_of::<GlyphInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Self {
            _window: window,
            surface,
            device,
            queue,
            surface_config,
            screen_uniform_buffer,
            screen_bind_group,
            background_texture,
            background_bind_group_layout,
            background_bind_group,
            background_sampler,
            background_pipeline,
            bubble_pipeline,
            bubble_instance_buffer,
            bubble_capacity: initial_capacity,
            atlas_dummy_texture,
            atlas_textures: [None, None],
            atlas_bind_group_layout,
            atlas_bind_group,
            atlas_sampler,
            label_bg_pipeline,
            label_bg_buffer,
            label_bg_capacity,
            glyph_texture,
            glyph_bind_group,
            text_pipeline,
            text_instance_buffer,
            text_capacity,
            overlay_texture,
            overlay_bind_group,
            overlay_pipeline,
            overlay_vertex_buffer,
            text_renderer: TextRenderer::new(),
            scratch_bubbles: Vec::with_capacity(initial_capacity),
            scratch_label_bgs: Vec::with_capacity(label_bg_capacity),
            scratch_glyphs: Vec::with_capacity(text_capacity),
            width,
            height,
        })
    }

    fn rebuild_atlas_bind_group(&mut self) {
        let view0 = self.atlas_textures[0]
            .as_ref()
            .unwrap_or(&self.atlas_dummy_texture)
            .create_view(&wgpu::TextureViewDescriptor::default());
        let view1 = self.atlas_textures[1]
            .as_ref()
            .unwrap_or(&self.atlas_dummy_texture)
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.atlas_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Icon Atlas Bind Group"),
            layout: &self.atlas_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view0),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view1),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.atlas_sampler),
                },
            ],
        });
    }
}

impl Renderer for WgpuRenderer {
    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.width = width;
        self.height = height;
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    fn set_background(&mut self, image: &WallpaperImage) {
        let width = image.width.max(1);
        let height = image.height.max(1);

        self.background_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Wallpaper Texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.background_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &image.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        let background_view = self
            .background_texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        self.background_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Background Bind Group"),
            layout: &self.background_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&background_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.background_sampler),
                },
            ],
        });
    }

    fn update_atlas(&mut self, page: usize, pixels: &[u8]) {
        if page >= self.atlas_textures.len() {
            log_warn!("indeks halaman atlas di luar batas: {page}");
            return;
        }
        let expected_len = (ATLAS_PAGE_SIZE * ATLAS_PAGE_SIZE * 4) as usize;
        if pixels.len() != expected_len {
            log_warn!(
                "ukuran buffer atlas tidak sesuai: {} != {expected_len}",
                pixels.len()
            );
            return;
        }
        if self.atlas_textures[page].is_none() {
            let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Icon Atlas Page Texture"),
                size: wgpu::Extent3d {
                    width: ATLAS_PAGE_SIZE,
                    height: ATLAS_PAGE_SIZE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.atlas_textures[page] = Some(tex);
            self.rebuild_atlas_bind_group();
        }
        if let Some(tex) = &self.atlas_textures[page] {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(ATLAS_PAGE_SIZE * 4),
                    rows_per_image: Some(ATLAS_PAGE_SIZE),
                },
                wgpu::Extent3d {
                    width: ATLAS_PAGE_SIZE,
                    height: ATLAS_PAGE_SIZE,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    fn draw(&mut self, frame: &FrameData) -> Result<(), RenderError> {
        if (frame.screen_width != self.width || frame.screen_height != self.height)
            && frame.screen_width > 0
            && frame.screen_height > 0
        {
            self.resize(frame.screen_width, frame.screen_height);
        }

        let output = match self.surface.get_current_texture() {
            Ok(texture) => texture,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.surface_config);
                return Err(RenderError::SurfaceLost);
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                return Err(RenderError::OutOfMemory("Swapchain out of memory".into()));
            }
            Err(wgpu::SurfaceError::Timeout) => {
                return Ok(());
            }
            Err(e) => {
                return Err(RenderError::Other(format!(
                    "surface get_current_texture error: {e}"
                )));
            }
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Update Screen Uniform
        let uniform = ScreenUniformRaw {
            screen_size: [self.width as f32, self.height as f32],
            background_dim: frame.background_dim,
            time: frame.time,
        };
        self.queue
            .write_buffer(&self.screen_uniform_buffer, 0, bytemuck::bytes_of(&uniform));

        // Upload Bubble Instance Buffer
        let num_bubbles = frame.bubbles.len();
        if num_bubbles > self.bubble_capacity {
            self.bubble_capacity = (num_bubbles * 2).max(64);
            self.bubble_instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Bubble Instance Buffer (Reallocated)"),
                size: (self.bubble_capacity * std::mem::size_of::<BubbleInstanceRaw>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }

        if num_bubbles > 0 {
            self.scratch_bubbles.clear();
            self.scratch_bubbles
                .extend(frame.bubbles.iter().map(|b| BubbleInstanceRaw {
                    center: b.center,
                    radius: b.radius,
                    visual_scale: b.visual_scale,
                    ring_color: b.ring_color,
                    ring_width: b.ring_width,
                    glow: b.glow,
                    flags: b.flags,
                    icon_page: b.icon_page,
                    icon_uv: b.icon_uv,
                }));
            self.queue.write_buffer(
                &self.bubble_instance_buffer,
                0,
                bytemuck::cast_slice(&self.scratch_bubbles),
            );
        }

        // Siapkan instance label pill, tooltip, & glyph teks
        if let Some(glyph_pixels) = self.text_renderer.prepare_labels_and_tooltip(
            frame.labels,
            frame.tooltip,
            self.width as f32,
            self.height as f32,
            &mut self.scratch_label_bgs,
            &mut self.scratch_glyphs,
        ) {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.glyph_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                glyph_pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(GLYPH_ATLAS_SIZE),
                    rows_per_image: Some(GLYPH_ATLAS_SIZE),
                },
                wgpu::Extent3d {
                    width: GLYPH_ATLAS_SIZE,
                    height: GLYPH_ATLAS_SIZE,
                    depth_or_array_layers: 1,
                },
            );
        }

        let num_label_bgs = self.scratch_label_bgs.len();
        if num_label_bgs > self.label_bg_capacity {
            self.label_bg_capacity = (num_label_bgs * 2).max(64);
            self.label_bg_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Label Bg Instance Buffer (Reallocated)"),
                size: (self.label_bg_capacity * std::mem::size_of::<LabelBgInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if num_label_bgs > 0 {
            self.queue.write_buffer(
                &self.label_bg_buffer,
                0,
                bytemuck::cast_slice(&self.scratch_label_bgs),
            );
        }

        let num_glyphs = self.scratch_glyphs.len();
        if num_glyphs > self.text_capacity {
            self.text_capacity = (num_glyphs * 2).max(512);
            self.text_instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Label Glyph Instance Buffer (Reallocated)"),
                size: (self.text_capacity * std::mem::size_of::<GlyphInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if num_glyphs > 0 {
            self.queue.write_buffer(
                &self.text_instance_buffer,
                0,
                bytemuck::cast_slice(&self.scratch_glyphs),
            );
        }

        // Render debug overlay jika diminta
        if frame.overlay.show {
            let overlay_bytes = self.text_renderer.render_overlay(
                frame.overlay.fps,
                frame.overlay.frame_time_ms,
                frame.overlay.working_set_bytes,
                frame.overlay.private_bytes,
                frame.overlay.body_count,
                frame.overlay.is_asleep,
            );
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.overlay_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &overlay_bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(OVERLAY_WIDTH * 4),
                    rows_per_image: Some(OVERLAY_HEIGHT),
                },
                wgpu::Extent3d {
                    width: OVERLAY_WIDTH,
                    height: OVERLAY_HEIGHT,
                    depth_or_array_layers: 1,
                },
            );

            // Letakkan panel overlay di pojok kiri atas (offset 16, 16)
            let overlay_inst = OverlayInstanceRaw {
                rect_pos: [16.0, 16.0],
                rect_size: [OVERLAY_WIDTH as f32, OVERLAY_HEIGHT as f32],
            };
            self.queue.write_buffer(
                &self.overlay_vertex_buffer,
                0,
                bytemuck::bytes_of(&overlay_inst),
            );
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("BubbleDesk Render Command Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("BubbleDesk Main Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.05,
                            g: 0.07,
                            b: 0.1,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // 1. Gambar latar belakang wallpaper
            render_pass.set_pipeline(&self.background_pipeline);
            render_pass.set_bind_group(0, &self.screen_bind_group, &[]);
            render_pass.set_bind_group(1, &self.background_bind_group, &[]);
            render_pass.draw(0..3, 0..1);

            // 2. Gambar bubble + ikon (instanced)
            if num_bubbles > 0 {
                render_pass.set_pipeline(&self.bubble_pipeline);
                render_pass.set_bind_group(0, &self.screen_bind_group, &[]);
                render_pass.set_bind_group(1, &self.atlas_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.bubble_instance_buffer.slice(..));
                render_pass.draw(0..6, 0..num_bubbles as u32);
            }

            // 3. Gambar latar pill label dan teks label bubble
            if num_label_bgs > 0 {
                render_pass.set_pipeline(&self.label_bg_pipeline);
                render_pass.set_bind_group(0, &self.screen_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.label_bg_buffer.slice(..));
                render_pass.draw(0..6, 0..num_label_bgs as u32);
            }
            if num_glyphs > 0 {
                render_pass.set_pipeline(&self.text_pipeline);
                render_pass.set_bind_group(0, &self.screen_bind_group, &[]);
                render_pass.set_bind_group(1, &self.glyph_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.text_instance_buffer.slice(..));
                render_pass.draw(0..6, 0..num_glyphs as u32);
            }

            // 4. Gambar debug overlay (jika aktif)
            if frame.overlay.show {
                render_pass.set_pipeline(&self.overlay_pipeline);
                render_pass.set_bind_group(0, &self.screen_bind_group, &[]);
                render_pass.set_bind_group(1, &self.overlay_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.overlay_vertex_buffer.slice(..));
                render_pass.draw(0..6, 0..1);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    fn shutdown(&mut self) {
        log_info!("WgpuRenderer dimatikan bersih");
    }
}
