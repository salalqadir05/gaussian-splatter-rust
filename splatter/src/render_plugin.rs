use std::borrow::Cow;
use bevy::render::{
    render_resource::*,
    renderer::{RenderDevice, RenderQueue},
    render_phase::{PhaseItem, DrawFunctionId},
    view::ViewTarget,
    RenderApp,
    ExtractSchedule,
};
use bevy::prelude::*;
use crate::renderer::Configuration;
use bevy::asset::Handle;
use wgpu::Color;
use std::default::Default;
use bevy::utils::nonmax::NonMaxU32;
use crate::scene::Scene;

#[derive(Resource)]
pub struct Renderer {
    pub config: Configuration,
    pub pipeline: Option<RenderPipeline>,
}

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
struct ExtractSplatSet;

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
pub struct GaussianSplatRenderPlugin;

impl Plugin for GaussianSplatRenderPlugin {
    fn build(&self, app: &mut App) {
        let render_app = app.sub_app_mut(RenderApp);
        render_app
            .init_resource::<Renderer>()
            .add_systems(ExtractSchedule, extract_splats.in_set(ExtractSplatSet));
    }
}

pub struct PipelineResource {
    pub pipeline: RenderPipeline,
}

pub struct GaussianSplatPhase {
    pub entity: Entity,
    pub pipeline: PipelineResource,
    pub distance: f32,
    pub draw_function: DrawFunctionId,
    pub instance_index: usize,
}

impl Clone for GaussianSplatPhase {
    fn clone(&self) -> Self {
        Self {
            entity: self.entity,
            pipeline: PipelineResource {
                pipeline: self.pipeline.pipeline.clone(),
            },
            distance: self.distance,
            draw_function: self.draw_function,
            instance_index: self.instance_index,
        }
    }
}

impl PhaseItem for GaussianSplatPhase {
    type SortKey = u64;

    fn entity(&self) -> Entity {
        self.entity
    }

    fn sort_key(&self) -> Self::SortKey {
        0
    }

    fn draw_function(&self) -> DrawFunctionId {
        self.draw_function
    }

    fn sort(items: &mut [Self]) {
        items.sort_by_key(|item| item.sort_key());
    }

    fn batch_range(&self) -> &std::ops::Range<u32> {
        static RANGE: std::ops::Range<u32> = 0..0;
        &RANGE
    }

    fn batch_range_mut(&mut self) -> &mut std::ops::Range<u32> {
        panic!("batch_range_mut() should not be called directly!")
    }

    fn dynamic_offset(&self) -> Option<NonMaxU32> {
        None
    }

    fn dynamic_offset_mut(&mut self) -> &mut Option<NonMaxU32> {
        panic!("dynamic_offset_mut() should not be called directly!")
    }
}

impl FromWorld for Renderer {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();
        Renderer::new(render_device, Configuration {
            surface_configuration: wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: wgpu::TextureFormat::Bgra8UnormSrgb,
                width: 800,
                height: 600,
                present_mode: wgpu::PresentMode::Fifo,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![],
            },
            depth_sorting: crate::renderer::DepthSorting::Gpu,
            use_covariance_for_scale: false,
            use_unaligned_rectangles: false,
            spherical_harmonics_order: 1,
            max_splat_count: 1,
            radix_bits_per_digit: 1,
            frustum_culling_tolerance: 0.0,
            ellipse_margin: 0.0,
            splat_scale: 0.0,
        })
    }
}

impl Renderer {
    pub fn new(_render_device: &RenderDevice, config: Configuration) -> Self {
        Self {
            config,
            pipeline: None,
        }
    }

    pub fn render_scene(
        &mut self,
        render_device: &RenderDevice,
        render_queue: &RenderQueue,
        view_target: &ViewTarget,
        shaders: Handle<Shader>,
    ) {
        let shader_handle = Handle::<Shader>::weak_from_u128(123456789);
        if !shaders.ge(&shader_handle) {
            return;
        }

        let _shader = shader_handle.to_owned();

        if self.pipeline.is_none() {
            let bind_group_layout = render_device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("Gaussian Splat Bind Group Layout"),
                entries: &[],
            });

            let pipeline_layout = render_device.create_pipeline_layout(&PipelineLayoutDescriptor {
                label: Some("Gaussian Splat Pipeline Layout"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });

            let vertex_shader_module = render_device.create_shader_module(ShaderModuleDescriptor {
                label: Some("Vertex Shader"),
                source: ShaderSource::Wgsl(Cow::Borrowed(include_str!("shaders.wgsl"))),
            });

            let fragment_shader_module = render_device.create_shader_module(ShaderModuleDescriptor {
                label: Some("Fragment Shader"),
                source: ShaderSource::Wgsl(Cow::Borrowed(include_str!("shaders.wgsl"))),
            });

            let render_pipeline_descriptor = wgpu::RenderPipelineDescriptor {
                label: Some("Gaussian Splat Pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &vertex_shader_module,
                    entry_point: "main",
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &fragment_shader_module,
                    entry_point: "main",
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Bgra8UnormSrgb,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::PointList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState {
                    count: 1,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
            };

            let render_pipeline = render_device.create_render_pipeline(&render_pipeline_descriptor);
            self.pipeline = Some(render_pipeline);
        }

        let texture = view_target.main_texture();
        let texture_view = texture.create_view(&TextureViewDescriptor::default());
        if let Some(pipeline) = &self.pipeline {
            let mut encoder = render_device.create_command_encoder(&CommandEncoderDescriptor {
                label: Some("Gaussian Splat Encoder"),
            });

            {
                let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                    label: Some("Splat Render Pass"),
                    color_attachments: &[Some(RenderPassColorAttachment {
                        view: &texture_view,
                        resolve_target: None,
                        ops: Operations {
                            load: LoadOp::Clear(Color::BLACK),
                            store: true,
                        },
                    })],
                    depth_stencil_attachment: None,
                });

                render_pass.set_pipeline(pipeline);
                render_pass.draw(0..1, 0..1);
            }

            render_queue.submit(std::iter::once(encoder.finish()));
        }
    }
}

fn extract_splats(_commands: Commands, _scene: Res<Scene>) {
    // Extract splats from the scene
    // This is a placeholder for the actual extraction logic
}