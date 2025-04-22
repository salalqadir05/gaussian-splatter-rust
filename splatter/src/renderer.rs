use std::convert::TryInto;
use crate::{
    scene::{Scene, Splat},
    utils::{mat4_multiplication, mat4_transform, motor3d_to_mat4, perspective_projection, transmute_slice},
};
use geometric_algebra::{
    ppga3d::{Motor, Point},
    Inverse,
};
use wgpu::util::DeviceExt;
use bevy::prelude::*;
use bevy::render::render_resource::*;
use bevy::render::renderer::RenderDevice;
use wgpu::Queue;
/// Selects how splats are sorted by their distance to the camera
pub enum DepthSorting {
    /// No sorting at all
    None,
    /// Sorting takes place on the CPU and is copied over to the GPU
    Cpu,
    /// Sorting takes place internally on the GPU
    Gpu,
    /// Like [DepthSorting::Gpu] and additionally skips rendering frustum culled splats by stream compaction
    GpuIndirectDraw,
}

/// Rendering configuration
pub struct Configuration {
    /// Format of the frame buffer texture
    pub surface_configuration: wgpu::SurfaceConfiguration,
    /// Selects how splats are sorted by their distance to the camera
    pub depth_sorting: DepthSorting,
    /// Uses the parallel projected covariance for decomposition of semi axes
    pub use_covariance_for_scale: bool,
    /// Decomposes the conic sections and renders them as rotated rectangles
    pub use_unaligned_rectangles: bool,
    /// How many spherical harmonics coefficients to use, possible values are 0..=3
    pub spherical_harmonics_order: usize,
    /// Maximum number of splats to allocate memory for
    pub max_splat_count: usize,
    /// How many bits of the key to bin in a single pass. Should be 8
    pub radix_bits_per_digit: usize,
    /// Factor by which the center of a splat can be outside the frustum without being called. Should be > 1.0
    pub frustum_culling_tolerance: f32,
    /// Factor by which the raserized rectangle reaches beyond the ellipse inside. Should be 2.0
    pub ellipse_margin: f32,
    /// Factor to scale splat ellipsoids with. Should be 1.0
    pub splat_scale: f32,
}

#[repr(C)]
pub(crate) struct Uniforms {
    camera_matrix: [Point; 4],
    view_matrix: [Point; 4],
    view_projection_matrix: [Point; 4],
    view_size: [f32; 2],
    image_size: [u32; 2],
    frustum_culling_tolerance: f32,
    ellipse_size_bias: f32,
    ellipse_margin: f32,
    splat_scale: f32,
    padding: [f32; 0],
}

/// Splats forward renderer
pub struct Renderer {
    pipeline: RenderPipeline,
    bind_group_layout: BindGroupLayout,
    vertex_buffer: Buffer,
}

impl Renderer {
    /// Constructs a new [Renderer]
    pub fn new(device: &RenderDevice) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Splat Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/splat.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Splat Bind Group Layout"),
            entries: &[],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Splat Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Splat Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: TextureFormat::Bgra8UnormSrgb,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
        });

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Splat Vertex Buffer"),
            size: 1024,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            bind_group_layout,
            vertex_buffer,
        }
    }

    /// Renders the given `scene` into `frame_view`
    pub fn render_frame(
        &self,
        device: &RenderDevice,
        queue: &Queue,
        frame_view: &TextureView,
        viewport_size: Extent3d,
        camera_motor: Motor,
        scene: &Scene,
    ) {
        let camera_matrix = motor3d_to_mat4(&camera_motor);
        let view_matrix = motor3d_to_mat4(&camera_motor.inverse());
        let field_of_view_y = std::f32::consts::PI * 0.5;
        let view_height = (field_of_view_y * 0.5).tan();
        let view_width = (viewport_size.width as f32 / viewport_size.height as f32) / view_height;
        let projection_matrix = perspective_projection(view_width, view_height, 1.0, 1000.0);
        let view_projection_matrix = mat4_multiplication(&projection_matrix, &view_matrix);
        let mut splat_count = scene.splat_count;
        if matches!(self.config.depth_sorting, DepthSorting::Cpu) {
            let mut entries: Vec<(u32, u32)> = (0..scene.splat_count)
                .filter_map(|splat_index| {
                    let world_position = Point::new(
                        scene.splat_positions[splat_index * 3 + 0],
                        scene.splat_positions[splat_index * 3 + 1],
                        scene.splat_positions[splat_index * 3 + 2],
                        1.0,
                    );
                    let homogenous_position = mat4_transform(&view_projection_matrix, &world_position);
                    let clip_space_position = homogenous_position * (1.0 / homogenous_position[3]);
                    if clip_space_position[0].abs() < self.config.frustum_culling_tolerance
                        && clip_space_position[1].abs() < self.config.frustum_culling_tolerance
                        && (clip_space_position[2] - 0.5).abs() < 0.5
                    {
                        Some((unsafe { std::mem::transmute::<f32, u32>(clip_space_position[2]) }, splat_index as u32))
                    } else {
                        None
                    }
                })
                .collect();
            splat_count = entries.len();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            queue.write_buffer(&self.entry_buffer_a, 0, transmute_slice::<_, u8>(&entries));
        }
        let uniform_data = &[Uniforms {
            camera_matrix,
            view_matrix,
            view_projection_matrix,
            view_size: [view_width, view_height],
            image_size: [viewport_size.width, viewport_size.height],
            frustum_culling_tolerance: self.config.frustum_culling_tolerance,
            ellipse_size_bias: 0.2 * view_width / viewport_size.width as f32,
            ellipse_margin: self.config.ellipse_margin,
            splat_scale: self.config.splat_scale,
            padding: [0.0; 0],
        }];
        queue.write_buffer(&self.uniform_buffer, 0, transmute_slice::<_, u8>(uniform_data));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        if matches!(self.config.depth_sorting, DepthSorting::Gpu | DepthSorting::GpuIndirectDraw) {
            encoder.clear_buffer(&self.sorting_buffer, 0, None);
            {
                let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None });
                compute_pass.set_bind_group(0, &scene.compute_bind_groups[1], &[]);
                compute_pass.set_pipeline(&self.radix_sort_a_pipeline);
                compute_pass.dispatch_workgroups(((splat_count + self.workgroup_entries_a - 1) / self.workgroup_entries_a) as u32, 1, 1);
                compute_pass.set_pipeline(&self.radix_sort_b_pipeline);
                compute_pass.dispatch_workgroups(1, self.radix_digit_places as u32, 1);
            }
            for pass_index in 0..self.radix_digit_places {
                if pass_index > 0 {
                    encoder.clear_buffer(
                        &self.sorting_buffer,
                        0,
                        Some(std::num::NonZeroU64::new((self.radix_base * self.max_tile_count_c * std::mem::size_of::<u32>()) as u64).unwrap()),
                    );
                }
                let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None });
                compute_pass.set_pipeline(&self.radix_sort_c_pipeline);
                compute_pass.set_bind_group(0, &scene.compute_bind_groups[pass_index], &[]);
                compute_pass.dispatch_workgroups(1, ((splat_count + self.workgroup_entries_c - 1) / self.workgroup_entries_c) as u32, 1);
            }
        }
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: frame_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: true,
                    },
                })],
                depth_stencil_attachment: None,
            });
            render_pass.set_pipeline(&self.pipeline);
            if let Some(bind_group) = &scene.render_bind_group {
                render_pass.set_bind_group(0, bind_group, &[]);
            }
            if matches!(self.config.depth_sorting, DepthSorting::GpuIndirectDraw) {
                render_pass.draw_indirect(&self.sorting_buffer, (self.sorting_buffer_size - std::mem::size_of::<u32>() * 5) as u64);
            } else {
                render_pass.draw(0..4, 0..splat_count as u32);
            }
        }
        queue.submit(Some(encoder.finish()));
    }
}


