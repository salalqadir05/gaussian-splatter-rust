use bevy::prelude::*;
use crate::scene::Scene;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use wgpu_types::Extent3d;
use bevy::render::render_resource::{TextureFormat, TextureUsages, TextureDimension};
use bevy::render::texture::Image;

#[derive(Component)]
pub struct SplatBuffer {
    pub data: Vec<u8>,
}

pub struct BevyPlugin;

impl Plugin for BevyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup)
            .add_systems(Update, (load_splats, render_splats).chain());
    }
}

#[derive(Component)]
pub struct GaussianSplat {
    pub splat_file: String,
    pub transform: Transform,
}

fn setup(mut commands: Commands) {
    commands.spawn((
        GaussianSplat {
            splat_file: "assets/splat_file.splat".to_string(),
            transform: Transform::default(),
        },
        GlobalTransform::default(),
        Visibility::default(),
        InheritedVisibility::default(),
        ViewVisibility::default(),
    ));
}

fn load_splats(mut scene: ResMut<Scene>, query: Query<&GaussianSplat>) {
    for splat in query.iter() {
        let splat_data = scene.load_splat_file(&splat.splat_file);
        if !splat_data.is_empty() {
            scene.splat_data = splat_data;
        }
    }
}

fn render_splats(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut scene_query: Query<&mut Scene>,
) {
    let mut scene = scene_query.single_mut();
    
    // Create a texture to render into
    let mut texture = Image::new_fill(
        Extent3d {
            width: 640,
            height: 480,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
    );
    texture.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
    
    // Render the scene
    scene.render(render_device, render_queue, &texture);
    
    // Add the texture to the asset system
    let texture_handle = images.add(texture);
    
    // Spawn a sprite with the rendered texture
    commands.spawn(SpriteBundle {
        texture: texture_handle,
        ..default()
    });
}
