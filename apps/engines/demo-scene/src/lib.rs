use std::f32::consts::TAU;

use bevy::{
    asset::RenderAssetUsages,
    camera::{Exposure, Hdr, RenderTarget},
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    light::{
        Atmosphere, AtmosphereEnvironmentMapLight, CascadeShadowConfigBuilder, NotShadowCaster,
        atmosphere::ScatteringMedium, light_consts::lux,
    },
    math::Affine2,
    mesh::{Indices, PrimitiveTopology},
    pbr::{AtmosphereSettings, DistanceFog, FogFalloff},
    post_process::bloom::Bloom,
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        view::Tonemapping,
    },
};
use demo_ui::Controls;
use nature::{MeshData, Stride, Terrain, Valley, Walker};

const SEED: u32 = 7;
const WATER_TILE: f32 = 9.0;

#[derive(Resource, Clone, Copy, Default, PartialEq, Deref, DerefMut)]
pub struct SceneControls(pub Controls);

#[derive(Resource, Clone, Copy, Default, PartialEq)]
pub struct SceneStats {
    pub altitude: f32,
    pub stamina: f32,
}

#[derive(Resource)]
struct Explorer {
    terrain: Terrain,
    walker: Walker,
    respawn: u32,
}

#[derive(Resource)]
struct WaterMaterial(Handle<StandardMaterial>);

pub fn build(app: &mut App, target: RenderTarget) {
    let valley = Valley::new(SEED);
    app.insert_resource(SceneControls::default())
        .init_resource::<SceneStats>()
        .insert_resource(Explorer {
            terrain: valley.terrain,
            walker: Walker::start(&valley.terrain),
            respawn: 0,
        })
        .add_systems(
            Startup,
            move |commands: Commands,
                  meshes: ResMut<Assets<Mesh>>,
                  materials: ResMut<Assets<StandardMaterial>>,
                  images: ResMut<Assets<Image>>,
                  media: ResMut<Assets<ScatteringMedium>>| {
                spawn(
                    commands,
                    meshes,
                    materials,
                    images,
                    media,
                    &valley,
                    target.clone(),
                )
            },
        )
        .add_systems(Update, (walk, sky, water));
}

fn mesh(data: &MeshData) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, data.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, data.normals.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, data.colors.clone())
    .with_inserted_indices(Indices::U32(data.indices.clone()))
}

fn water_normals() -> Image {
    const SIZE: u32 = 256;
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    let waves = [
        (3.0, 1.0, 0.0, 0.5),
        (5.0, -2.0, 1.3, 0.3),
        (-4.0, 7.0, 2.1, 0.2),
        (11.0, 3.0, 0.7, 0.12),
    ];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (u, v) = (x as f32 / SIZE as f32 * TAU, y as f32 / SIZE as f32 * TAU);
            let (mut dx, mut dy) = (0.0, 0.0);
            for (kx, ky, phase, amplitude) in waves {
                let slope = (kx * u + ky * v + phase).cos() * amplitude;
                dx += slope * kx * 0.06;
                dy += slope * ky * 0.06;
            }
            let normal = Vec3::new(-dx, -dy, 1.0).normalize();
            pixels.extend(
                [normal.x, normal.y, normal.z].map(|c| ((c * 0.5 + 0.5) * 255.0).round() as u8),
            );
            pixels.push(255);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

fn spawn(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut media: ResMut<Assets<ScatteringMedium>>,
    valley: &Valley,
    target: RenderTarget,
) {
    commands.spawn(Atmosphere::earth(
        media.add(ScatteringMedium::earth(256, 256)),
    ));
    commands.spawn((
        Camera3d::default(),
        Hdr,
        target,
        AtmosphereSettings::default(),
        AtmosphereEnvironmentMapLight::default(),
        Exposure::SUNLIGHT,
        Tonemapping::AcesFitted,
        Bloom::NATURAL,
        DistanceFog::default(),
        Transform::default(),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: lux::RAW_SUNLIGHT,
            shadow_maps_enabled: true,
            ..default()
        },
        CascadeShadowConfigBuilder {
            first_cascade_far_bound: 12.0,
            maximum_distance: 160.0,
            ..default()
        }
        .build(),
        Transform::default(),
    ));
    let terrain = &valley.terrain;
    commands.spawn((
        Mesh3d(meshes.add(mesh(&terrain.mesh()))),
        MeshMaterial3d(materials.add(StandardMaterial {
            perceptual_roughness: 0.95,
            ..default()
        })),
    ));
    let water = terrain.water_mesh(1);
    let uvs: Vec<[f32; 2]> = water
        .positions
        .iter()
        .map(|p| [p[0] / WATER_TILE, p[2] / WATER_TILE])
        .collect();
    let mut water_mesh = mesh(&water).with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    if let Err(error) = water_mesh.generate_tangents() {
        warn!("water tangents: {error}");
    }
    let water_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.09, 0.27, 0.32, 0.8),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.05,
        reflectance: 0.6,
        normal_map_texture: Some(images.add(water_normals())),
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(water_mesh)),
        MeshMaterial3d(water_material.clone()),
        NotShadowCaster,
    ));
    commands.insert_resource(WaterMaterial(water_material));
    let solid = materials.add(StandardMaterial {
        perceptual_roughness: 0.85,
        ..default()
    });
    let thin = materials.add(StandardMaterial {
        perceptual_roughness: 0.8,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    for group in &valley.groups {
        let shape = meshes.add(mesh(&group.mesh));
        let material = if group.double_sided {
            thin.clone()
        } else {
            solid.clone()
        };
        for placement in &group.placements {
            let transform = Transform {
                translation: Vec3::from(placement.position),
                rotation: Quat::from_rotation_y(placement.yaw),
                scale: Vec3::splat(placement.scale),
            };
            let mut entity = commands.spawn((
                Mesh3d(shape.clone()),
                MeshMaterial3d(material.clone()),
                transform,
            ));
            if group.double_sided {
                entity.insert(NotShadowCaster);
            }
        }
    }
}

fn walk(
    time: Res<Time>,
    controls: Res<SceneControls>,
    mut explorer: ResMut<Explorer>,
    mut stats: ResMut<SceneStats>,
    mut cameras: Query<&mut Transform, With<Camera3d>>,
) {
    let Explorer {
        terrain,
        walker,
        respawn,
    } = &mut *explorer;
    if *respawn != controls.respawn {
        *walker = Walker::start(terrain);
        *respawn = controls.respawn;
    }
    let stride = Stride {
        forward: controls.walk.forward_axis(),
        right: controls.walk.right_axis(),
        run: controls.walk.run,
    };
    walker.step(terrain, stride, controls.yaw, time.delta_secs().min(0.1));
    let eye = Vec3::from(walker.eye());
    let target = Vec3::from(walker.look_at(controls.yaw, controls.pitch));
    for mut transform in &mut cameras {
        *transform = Transform::from_translation(eye).looking_at(target, Vec3::Y);
    }
    let next = SceneStats {
        altitude: walker.position[1],
        stamina: walker.stamina,
    };
    if *stats != next {
        *stats = next;
    }
}

fn sky(
    controls: Res<SceneControls>,
    mut lights: Query<&mut Transform, With<DirectionalLight>>,
    mut fogs: Query<&mut DistanceFog>,
    mut blooms: Query<&mut Bloom>,
) {
    if !controls.is_changed() {
        return;
    }
    let sun = nature::sun(controls.time_of_day);
    let toward = Vec3::from(sun.toward);
    for mut transform in &mut lights {
        *transform = Transform::default().looking_to(-toward, Vec3::Y);
    }
    let visibility = 700.0 - controls.fog * 620.0;
    for mut fog in &mut fogs {
        fog.color = Color::srgba(0.62, 0.7, 0.8, 1.0);
        fog.directional_light_color =
            Color::linear_rgba(sun.color[0], sun.color[1], sun.color[2], 0.5);
        fog.directional_light_exponent = 24.0;
        fog.falloff = FogFalloff::from_visibility_colors(
            visibility,
            Color::srgb(0.35, 0.4, 0.48),
            Color::srgb(0.75, 0.8, 0.86),
        );
    }
    for mut bloom in &mut blooms {
        bloom.intensity = controls.bloom * 0.5;
    }
}

fn water(
    time: Res<Time>,
    water: Res<WaterMaterial>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(mut material) = materials.get_mut(&water.0) else {
        return;
    };
    let t = time.elapsed_secs();
    material.uv_transform = Affine2::from_translation(Vec2::new(t * 0.02, t * 0.013));
}
