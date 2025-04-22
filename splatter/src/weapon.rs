use bevy::prelude::*;

pub struct WeaponPlugin;

impl Plugin for WeaponPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, weapon_controls);
    }
}

#[derive(Component)]
pub struct Weapon {
    pub fire_rate: f32,
    pub last_shot: f32,
    pub ammo: i32,
    pub max_ammo: i32,
}

impl Default for Weapon {
    fn default() -> Self {
        Self {
            fire_rate: 0.1,
            last_shot: 0.0,
            ammo: 30,
            max_ammo: 30,
        }
    }
}

#[derive(Component)]
pub struct Bullet {
    pub speed: f32,
    pub damage: f32,
}

#[derive(Component)]
pub struct ReloadTimer {
    pub weapon: Entity,
    pub duration: Timer,
}

fn weapon_controls(
    mut commands: Commands,
    time: Res<Time>,
    mouse: Res<Input<MouseButton>>,
    keyboard: Res<Input<KeyCode>>,
    mut weapons: Query<(Entity, &mut Weapon)>,
) {
    for (entity, mut weapon) in weapons.iter_mut() {
        // Handle reloading
        if keyboard.just_pressed(KeyCode::R) && weapon.ammo < weapon.max_ammo {
            commands.spawn((
                ReloadTimer {
                    weapon: entity,
                    duration: Timer::from_seconds(2.0, TimerMode::Once),
                },
            ));
        }

        // Handle shooting
        if mouse.pressed(MouseButton::Left) 
            && weapon.ammo > 0 
            && time.elapsed_seconds() - weapon.last_shot >= weapon.fire_rate 
        {
            weapon.ammo -= 1;
            weapon.last_shot = time.elapsed_seconds();

            commands.spawn((
                Bullet {
                    speed: 20.0,
                    damage: 10.0,
                },
                TransformBundle::from_transform(Transform::from_xyz(0.0, 0.0, 0.0)),
            ));
        }
    }
} 