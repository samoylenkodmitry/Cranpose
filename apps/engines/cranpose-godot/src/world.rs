use demo_ui::Controls;
use godot::{
    classes::{
        ArrayMesh, Camera3D, DirectionalLight3D, Environment, INode3D, MeshInstance3D, MultiMesh,
        MultiMeshInstance3D, Node3D, ProceduralSkyMaterial, Shader, ShaderMaterial, Sky,
        StandardMaterial3D, WorldEnvironment,
        base_material_3d::{CullMode, Flags},
        environment::{AmbientSource, BgMode, ToneMapper},
        geometry_instance_3d::ShadowCastingSetting,
        light_3d::Param,
        mesh::{ArrayType, PrimitiveType},
        multi_mesh::TransformFormat,
    },
    prelude::*,
};
use nature::{MeshData, Stride, Terrain, Valley, Walker};

const SEED: u32 = 7;

const WATER_SHADER: &str = r#"
shader_type spatial;
render_mode blend_mix, cull_disabled, specular_schlick_ggx;
uniform vec4 shallow : source_color = vec4(0.16, 0.42, 0.46, 1.0);
uniform vec4 deep : source_color = vec4(0.04, 0.17, 0.22, 1.0);
varying vec3 world;
void vertex() {
    world = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
    VERTEX.y += sin(world.x * 0.35 + TIME * 1.2) * 0.03 + cos(world.z * 0.42 + TIME * 0.9) * 0.03;
}
void fragment() {
    vec2 p = world.xz;
    float dx = cos(p.x * 1.7 + TIME * 1.6) * 0.06 + cos((p.x + p.y) * 3.1 + TIME * 2.3) * 0.03;
    float dz = cos(p.y * 2.1 - TIME * 1.3) * 0.06 + cos((p.x - p.y) * 2.7 + TIME * 1.9) * 0.03;
    NORMAL = normalize((VIEW_MATRIX * vec4(normalize(vec3(-dx, 1.0, -dz)), 0.0)).xyz);
    float fresnel = pow(1.0 - clamp(dot(NORMAL, VIEW), 0.0, 1.0), 4.0);
    ALBEDO = mix(deep.rgb, shallow.rgb, 0.4);
    ROUGHNESS = 0.04;
    SPECULAR = 0.7;
    ALPHA = mix(0.8, 0.97, fresnel);
}
"#;

#[derive(GodotClass)]
#[class(base=Node3D, init)]
pub struct DemoWorld {
    base: Base<Node3D>,
    controls: Controls,
    terrain: Option<Terrain>,
    walker: Option<Walker>,
    respawn: u32,
    camera: Option<Gd<Camera3D>>,
    sun: Option<Gd<DirectionalLight3D>>,
    environment: Option<Gd<Environment>>,
}

#[godot_api]
impl INode3D for DemoWorld {
    fn ready(&mut self) {
        let valley = Valley::new(SEED);
        self.add_environment();
        let mut camera = Camera3D::new_alloc();
        camera.set_current(true);
        camera.set_far(900.0);
        self.base_mut().add_child(&camera);
        self.camera = Some(camera);
        let mut sun = DirectionalLight3D::new_alloc();
        sun.set_shadow(true);
        sun.set_param(Param::SHADOW_MAX_DISTANCE, 160.0);
        self.base_mut().add_child(&sun);
        self.sun = Some(sun);

        let terrain = valley.terrain;
        let ground = mesh_instance(&terrain.mesh(), plant_material(false, 0.95));
        self.base_mut().add_child(&ground);
        let mut water_material = ShaderMaterial::new_gd();
        let mut shader = Shader::new_gd();
        shader.set_code(WATER_SHADER);
        water_material.set_shader(&shader);
        let mut water = MeshInstance3D::new_alloc();
        water.set_mesh(&array_mesh(&terrain.water_mesh(96)));
        water.set_material_override(&water_material);
        water.set_cast_shadows_setting(ShadowCastingSetting::OFF);
        self.base_mut().add_child(&water);

        for group in &valley.groups {
            let mut multimesh = MultiMesh::new_gd();
            multimesh.set_transform_format(TransformFormat::TRANSFORM_3D);
            multimesh.set_mesh(&array_mesh(&group.mesh));
            multimesh.set_instance_count(group.placements.len() as i32);
            for (index, placement) in group.placements.iter().enumerate() {
                let basis = Basis::from_axis_angle(Vector3::UP, placement.yaw)
                    .scaled(Vector3::splat(placement.scale));
                let origin = Vector3::new(
                    placement.position[0],
                    placement.position[1],
                    placement.position[2],
                );
                multimesh.set_instance_transform(index as i32, Transform3D::new(basis, origin));
            }
            let mut instance = MultiMeshInstance3D::new_alloc();
            instance.set_multimesh(&multimesh);
            instance.set_material_override(&plant_material(group.double_sided, 0.85));
            if group.double_sided {
                instance.set_cast_shadows_setting(ShadowCastingSetting::OFF);
            }
            self.base_mut().add_child(&instance);
        }
        self.walker = Some(Walker::start(&terrain));
        self.terrain = Some(terrain);
        self.apply_controls();
    }

    fn process(&mut self, delta: f64) {
        let controls = self.controls;
        let (Some(terrain), Some(walker)) = (self.terrain, self.walker.as_mut()) else {
            return;
        };
        if self.respawn != controls.respawn {
            *walker = Walker::start(&terrain);
            self.respawn = controls.respawn;
        }
        let stride = Stride {
            forward: controls.walk.forward_axis(),
            right: controls.walk.right_axis(),
            run: controls.walk.run,
        };
        walker.step(&terrain, stride, controls.yaw, (delta as f32).min(0.1));
        let eye = walker.eye();
        let target = walker.look_at(controls.yaw, controls.pitch);
        if let Some(camera) = self.camera.as_mut() {
            camera.look_at_from_position(
                Vector3::new(eye[0], eye[1], eye[2]),
                Vector3::new(target[0], target[1], target[2]),
            );
        }
    }
}

impl DemoWorld {
    pub fn set_controls(&mut self, controls: Controls) {
        if self.controls == controls {
            return;
        }
        let sky_changed = (
            self.controls.time_of_day,
            self.controls.fog,
            self.controls.bloom,
        ) != (controls.time_of_day, controls.fog, controls.bloom);
        self.controls = controls;
        if sky_changed {
            self.apply_controls();
        }
    }

    pub fn altitude_and_stamina(&self) -> (f32, f32) {
        self.walker
            .map_or((0.0, 1.0), |walker| (walker.position[1], walker.stamina))
    }

    fn add_environment(&mut self) {
        let mut sky_material = ProceduralSkyMaterial::new_gd();
        sky_material.set_sky_top_color(Color::from_rgb(0.22, 0.42, 0.75));
        sky_material.set_sky_horizon_color(Color::from_rgb(0.7, 0.78, 0.86));
        sky_material.set_ground_horizon_color(Color::from_rgb(0.7, 0.78, 0.86));
        sky_material.set_ground_bottom_color(Color::from_rgb(0.25, 0.3, 0.25));
        let mut sky = Sky::new_gd();
        sky.set_material(&sky_material);
        let mut environment = Environment::new_gd();
        environment.set_background(BgMode::SKY);
        environment.set_sky(&sky);
        environment.set_ambient_source(AmbientSource::SKY);
        environment.set_tonemapper(ToneMapper::ACES);
        environment.set_glow_enabled(true);
        environment.set_glow_bloom(0.08);
        environment.set_ssao_enabled(true);
        environment.set_fog_enabled(true);
        environment.set_fog_sun_scatter(0.25);
        environment.set_fog_aerial_perspective(0.6);
        let mut world_environment = WorldEnvironment::new_alloc();
        world_environment.set_environment(&environment);
        self.base_mut().add_child(&world_environment);
        self.environment = Some(environment);
    }

    fn apply_controls(&mut self) {
        let controls = self.controls;
        let sun = nature::sun(controls.time_of_day);
        if let Some(light) = self.sun.as_mut() {
            let toward = Vector3::new(sun.toward[0], sun.toward[1], sun.toward[2]);
            light.look_at_from_position(Vector3::ZERO, -toward);
            light.set_color(Color::from_rgb(sun.color[0], sun.color[1], sun.color[2]));
            light.set_param(Param::ENERGY, 0.2 + sun.strength * 1.3);
        }
        if let Some(environment) = self.environment.as_mut() {
            environment.set_glow_intensity(controls.bloom * 2.0);
            environment.set_fog_density(0.0008 + controls.fog * 0.012);
            environment.set_fog_light_color(Color::from_rgb(0.62, 0.7, 0.8));
            environment.set_ambient_light_energy(0.3 + sun.strength * 0.6);
        }
    }
}

fn array_mesh(data: &MeshData) -> Gd<ArrayMesh> {
    let mut arrays = VarArray::new();
    arrays.resize(ArrayType::MAX.ord() as usize, &Variant::nil());
    let vectors = |values: &[[f32; 3]]| {
        values
            .iter()
            .map(|v| Vector3::new(v[0], v[1], v[2]))
            .collect::<PackedVector3Array>()
    };
    let colors: PackedColorArray = data
        .colors
        .iter()
        .map(|c| Color::from_rgba(c[0], c[1], c[2], c[3]))
        .collect();
    let indices: PackedInt32Array = data
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|[a, b, c]| [*a as i32, *c as i32, *b as i32])
        .collect();
    arrays.set(
        ArrayType::VERTEX.ord() as usize,
        &vectors(&data.positions).to_variant(),
    );
    arrays.set(
        ArrayType::NORMAL.ord() as usize,
        &vectors(&data.normals).to_variant(),
    );
    arrays.set(ArrayType::COLOR.ord() as usize, &colors.to_variant());
    arrays.set(ArrayType::INDEX.ord() as usize, &indices.to_variant());
    let mut mesh = ArrayMesh::new_gd();
    mesh.add_surface_from_arrays(PrimitiveType::TRIANGLES, &arrays);
    mesh
}

fn mesh_instance(data: &MeshData, material: Gd<StandardMaterial3D>) -> Gd<MeshInstance3D> {
    let mut instance = MeshInstance3D::new_alloc();
    instance.set_mesh(&array_mesh(data));
    instance.set_material_override(&material);
    instance
}

fn plant_material(double_sided: bool, roughness: f32) -> Gd<StandardMaterial3D> {
    let mut material = StandardMaterial3D::new_gd();
    material.set_flag(Flags::ALBEDO_FROM_VERTEX_COLOR, true);
    material.set_roughness(roughness);
    if double_sided {
        material.set_cull_mode(CullMode::DISABLED);
    }
    material
}
