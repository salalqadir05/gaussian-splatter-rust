use bevy::prelude::*;

#[derive(Component)]
pub struct GaussianSplat {
    pub splat_file: String,
}

#[derive(Bundle)]
pub struct GaussianSplatBundle {
    pub splat: GaussianSplat,
    pub transform: Transform,
    pub global_transform: GlobalTransform,
    pub visibility: Visibility,
}
