use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::{
    camera::{Hdr, RenderTarget},
    light::{CascadeShadowConfigBuilder, GlobalAmbientLight},
    post_process::bloom::Bloom,
    prelude::*,
    render::view::Tonemapping,
};
use demo_ui::Controls;

#[derive(Resource, Clone, Copy, Default, PartialEq, Deref, DerefMut)]
pub struct SceneControls(pub Controls);

#[derive(Component)]
pub struct Spin(f32);

#[derive(Component)]
struct Falling {
    speed: f32,
    rest: f32,
}

#[derive(Resource)]
pub struct DropAssets {
    mesh: Handle<Mesh>,
    materials: Vec<Handle<StandardMaterial>>,
}

#[derive(Component)]
struct Bob {
    phase: f32,
    base: f32,
}

pub fn build(app: &mut App, target: RenderTarget) {
    app.insert_resource(SceneControls::default())
        .insert_resource(ClearColor(Color::srgb(0.015, 0.02, 0.045)))
        .insert_resource(GlobalAmbientLight {
            brightness: 160.0,
            ..default()
        })
        .add_systems(
            Startup,
            move |commands: Commands,
                  meshes: ResMut<Assets<Mesh>>,
                  materials: ResMut<Assets<StandardMaterial>>| {
                spawn(commands, meshes, materials, target.clone())
            },
        )
        .init_resource::<SceneStats>()
        .add_systems(Update, (orbit, sun, spin, bob, bloom, fall, count));
}

fn spawn(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    target: RenderTarget,
) {
    commands.spawn((
        Camera3d::default(),
        Hdr,
        Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
        target,
        Transform::from_xyz(8.0, 6.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        CascadeShadowConfigBuilder {
            first_cascade_far_bound: 6.0,
            maximum_distance: 40.0,
            ..default()
        }
        .build(),
        Transform::default(),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(40.0, 40.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.16, 0.17, 0.2),
            perceptual_roughness: 0.85,
            ..default()
        })),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(0.45, 5.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.85, 0.82, 0.76),
            metallic: 0.2,
            perceptual_roughness: 0.4,
            ..default()
        })),
        Transform::from_xyz(0.0, 2.5, 0.0),
    ));
    let shapes = [
        meshes.add(Cuboid::new(1.3, 1.3, 1.3)),
        meshes.add(Sphere::new(0.8).mesh().uv(48, 24)),
        meshes.add(Torus::new(0.45, 0.95)),
    ];
    let palette = [
        Color::srgb(0.95, 0.33, 0.36),
        Color::srgb(0.25, 0.6, 0.98),
        Color::srgb(0.98, 0.78, 0.25),
        Color::srgb(0.35, 0.86, 0.6),
        Color::srgb(0.72, 0.45, 0.98),
        Color::srgb(0.98, 0.55, 0.22),
        Color::srgb(0.3, 0.86, 0.92),
        Color::srgb(0.95, 0.4, 0.75),
    ];
    for (index, color) in palette.into_iter().enumerate() {
        let angle = index as f32 / palette.len() as f32 * TAU;
        let position = Vec3::new(angle.cos() * 5.0, 1.2, angle.sin() * 5.0);
        commands.spawn((
            Mesh3d(shapes[index % shapes.len()].clone()),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                metallic: if index % 2 == 0 { 0.7 } else { 0.05 },
                perceptual_roughness: if index % 2 == 0 { 0.25 } else { 0.55 },
                ..default()
            })),
            Transform::from_translation(position),
            Spin(0.6 + index as f32 * 0.15),
            Bob {
                phase: angle,
                base: position.y,
            },
        ));
    }
    commands.insert_resource(DropAssets {
        mesh: shapes[0].clone(),
        materials: palette
            .iter()
            .map(|color| {
                materials.add(StandardMaterial {
                    base_color: *color,
                    metallic: 0.5,
                    perceptual_roughness: 0.35,
                    ..default()
                })
            })
            .collect(),
    });
    let glow = meshes.add(Sphere::new(0.32).mesh().uv(32, 16));
    for (index, emissive) in [
        LinearRgba::rgb(14.0, 4.0, 1.2),
        LinearRgba::rgb(1.5, 6.0, 16.0),
        LinearRgba::rgb(4.0, 14.0, 6.0),
    ]
    .into_iter()
    .enumerate()
    {
        let angle = index as f32 / 3.0 * TAU + 0.5;
        let position = Vec3::new(angle.cos() * 2.4, 3.6, angle.sin() * 2.4);
        commands.spawn((
            Mesh3d(glow.clone()),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::BLACK,
                emissive,
                ..default()
            })),
            Transform::from_translation(position),
            Bob {
                phase: angle * 2.0,
                base: position.y,
            },
            children![PointLight {
                color: Color::linear_rgb(emissive.red, emissive.green, emissive.blue),
                intensity: 60_000.0,
                range: 9.0,
                ..default()
            }],
        ));
    }
}

fn orbit(controls: Res<SceneControls>, mut cameras: Query<&mut Transform, With<Camera3d>>) {
    let pitch = controls.pitch.clamp(0.05, FRAC_PI_2 - 0.05);
    let eye = Vec3::new(
        controls.yaw.cos() * pitch.cos(),
        pitch.sin(),
        controls.yaw.sin() * pitch.cos(),
    ) * controls.distance;
    for mut transform in &mut cameras {
        *transform = Transform::from_translation(eye + Vec3::Y * 1.2)
            .looking_at(Vec3::new(0.0, 1.4, 0.0), Vec3::Y);
    }
}

fn sun(controls: Res<SceneControls>, mut lights: Query<&mut Transform, With<DirectionalLight>>) {
    for mut transform in &mut lights {
        *transform = Transform::from_rotation(Quat::from_euler(
            EulerRot::YXZ,
            controls.sun * TAU,
            -0.75,
            0.0,
        ));
    }
}

fn spin(controls: Res<SceneControls>, time: Res<Time>, mut shapes: Query<(&mut Transform, &Spin)>) {
    if !controls.spin {
        return;
    }
    for (mut transform, spin) in &mut shapes {
        transform.rotate_y(spin.0 * time.delta_secs());
        transform.rotate_x(spin.0 * 0.5 * time.delta_secs());
    }
}

fn bob(controls: Res<SceneControls>, time: Res<Time>, mut shapes: Query<(&mut Transform, &Bob)>) {
    if !controls.spin {
        return;
    }
    let now = time.elapsed_secs();
    for (mut transform, bob) in &mut shapes {
        transform.translation.y = bob.base + (now * 1.6 + bob.phase).sin() * 0.35;
    }
}

fn bloom(controls: Res<SceneControls>, mut blooms: Query<&mut Bloom>) {
    for mut bloom in &mut blooms {
        bloom.intensity = controls.bloom;
    }
}

pub fn drop_shape(mut commands: Commands, assets: Res<DropAssets>, shapes: Query<(), With<Spin>>) {
    let index = shapes.iter().count();
    let angle = index as f32 * 2.399_963;
    let radius = 2.0 + (index % 5) as f32 * 1.4;
    let material = assets.materials[index % assets.materials.len()].clone();
    commands.spawn((
        Mesh3d(assets.mesh.clone()),
        MeshMaterial3d(material),
        Transform::from_xyz(angle.cos() * radius, 9.0, angle.sin() * radius)
            .with_scale(Vec3::splat(0.6)),
        Spin(0.4 + (index % 7) as f32 * 0.2),
        Falling {
            speed: 0.0,
            rest: 0.4,
        },
    ));
}

#[derive(Resource, Clone, Copy, Default, PartialEq)]
pub struct SceneStats {
    pub shapes: usize,
}

fn count(shapes: Query<(), With<Spin>>, mut stats: ResMut<SceneStats>) {
    let shapes = shapes.iter().count();
    if stats.shapes != shapes {
        stats.shapes = shapes;
    }
}

fn fall(
    mut commands: Commands,
    time: Res<Time>,
    mut shapes: Query<(Entity, &mut Transform, &mut Falling)>,
) {
    for (entity, mut transform, mut falling) in &mut shapes {
        falling.speed += 18.0 * time.delta_secs();
        transform.translation.y -= falling.speed * time.delta_secs();
        if transform.translation.y <= falling.rest {
            transform.translation.y = falling.rest;
            commands.entity(entity).remove::<Falling>();
        }
    }
}
