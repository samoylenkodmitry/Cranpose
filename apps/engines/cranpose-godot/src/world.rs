use std::f32::consts::{FRAC_PI_2, TAU};

use demo_ui::Controls;
use godot::{
    classes::{
        BoxMesh, Camera3D, CylinderMesh, DirectionalLight3D, Environment, INode3D, Light3D, Mesh,
        MeshInstance3D, Node3D, OmniLight3D, PlaneMesh, SphereMesh, StandardMaterial3D, TorusMesh,
        WorldEnvironment,
        base_material_3d::Feature,
        environment::{AmbientSource, BgMode, ToneMapper},
        light_3d::Param,
    },
    prelude::*,
};

const PALETTE: [(f32, f32, f32); 8] = [
    (0.95, 0.33, 0.36),
    (0.25, 0.6, 0.98),
    (0.98, 0.78, 0.25),
    (0.35, 0.86, 0.6),
    (0.72, 0.45, 0.98),
    (0.98, 0.55, 0.22),
    (0.3, 0.86, 0.92),
    (0.95, 0.4, 0.75),
];

struct Shape {
    node: Gd<MeshInstance3D>,
    spin: f32,
    phase: f32,
    rest: f32,
    falling: Option<f32>,
}

#[derive(GodotClass)]
#[class(base=Node3D, init)]
pub struct DemoWorld {
    base: Base<Node3D>,
    controls: Controls,
    camera: Option<Gd<Camera3D>>,
    sun: Option<Gd<DirectionalLight3D>>,
    environment: Option<Gd<Environment>>,
    shapes: Vec<Shape>,
    time: f32,
}

#[godot_api]
impl INode3D for DemoWorld {
    fn ready(&mut self) {
        let mut environment = Environment::new_gd();
        environment.set_background(BgMode::COLOR);
        environment.set_bg_color(Color::from_rgb(0.015, 0.02, 0.045));
        environment.set_ambient_source(AmbientSource::COLOR);
        environment.set_ambient_light_color(Color::from_rgb(0.55, 0.6, 0.7));
        environment.set_ambient_light_energy(0.4);
        environment.set_tonemapper(ToneMapper::ACES);
        environment.set_glow_enabled(true);
        environment.set_glow_bloom(0.15);
        let mut world_environment = WorldEnvironment::new_alloc();
        world_environment.set_environment(&environment);
        self.base_mut().add_child(&world_environment);
        self.environment = Some(environment);

        let mut camera = Camera3D::new_alloc();
        camera.set_current(true);
        self.base_mut().add_child(&camera);
        self.camera = Some(camera);

        let mut sun = DirectionalLight3D::new_alloc();
        sun.set_shadow(true);
        sun.set_param(Param::ENERGY, 1.3);
        self.base_mut().add_child(&sun);
        self.sun = Some(sun);

        let mut ground = PlaneMesh::new_gd();
        ground.set_size(Vector2::new(40.0, 40.0));
        self.add_mesh(
            ground.upcast(),
            material((0.16, 0.17, 0.2), 0.0, 0.85),
            Vector3::ZERO,
        );
        let mut pillar = CylinderMesh::new_gd();
        pillar.set_top_radius(0.45);
        pillar.set_bottom_radius(0.45);
        pillar.set_height(5.0);
        self.add_mesh(
            pillar.upcast(),
            material((0.85, 0.82, 0.76), 0.2, 0.4),
            Vector3::new(0.0, 2.5, 0.0),
        );

        for (index, color) in PALETTE.into_iter().enumerate() {
            let angle = index as f32 / PALETTE.len() as f32 * TAU;
            let position = Vector3::new(angle.cos() * 5.0, 1.2, angle.sin() * 5.0);
            let metallic = if index % 2 == 0 { 0.7 } else { 0.05 };
            let roughness = if index % 2 == 0 { 0.25 } else { 0.55 };
            let node = self.add_mesh(
                shape_mesh(index),
                material(color, metallic, roughness),
                position,
            );
            self.shapes.push(Shape {
                node,
                spin: 0.6 + index as f32 * 0.15,
                phase: angle,
                rest: position.y,
                falling: None,
            });
        }

        for (index, color) in [(1.0, 0.3, 0.08), (0.1, 0.4, 1.0), (0.3, 1.0, 0.4)]
            .into_iter()
            .enumerate()
        {
            let angle = index as f32 / 3.0 * TAU + 0.5;
            let position = Vector3::new(angle.cos() * 2.4, 3.6, angle.sin() * 2.4);
            let mut sphere = SphereMesh::new_gd();
            sphere.set_radius(0.32);
            sphere.set_height(0.64);
            let mut glow = StandardMaterial3D::new_gd();
            glow.set_albedo(Color::BLACK);
            glow.set_feature(Feature::EMISSION, true);
            glow.set_emission(Color::from_rgb(color.0, color.1, color.2));
            glow.set_emission_energy_multiplier(9.0);
            let mut node = self.add_mesh(sphere.upcast(), glow, position);
            let mut light = OmniLight3D::new_alloc();
            light.set_color(Color::from_rgb(color.0, color.1, color.2));
            light.set_param(Param::ENERGY, 2.5);
            light.set_param(Param::RANGE, 9.0);
            node.add_child(&light.upcast::<Light3D>());
            self.shapes.push(Shape {
                node,
                spin: 0.0,
                phase: angle * 2.0,
                rest: position.y,
                falling: None,
            });
        }
        self.apply_controls();
    }

    fn process(&mut self, delta: f64) {
        let delta = delta as f32;
        self.time += delta;
        let time = self.time;
        let animate = self.controls.spin;
        for shape in &mut self.shapes {
            let mut position = shape.node.get_position();
            if let Some(speed) = shape.falling.as_mut() {
                *speed += 18.0 * delta;
                position.y = (position.y - *speed * delta).max(shape.rest);
                if position.y <= shape.rest {
                    shape.falling = None;
                }
            } else if animate {
                position.y = shape.rest + (time * 1.6 + shape.phase).sin() * 0.35;
            }
            shape.node.set_position(position);
            if animate && shape.spin > 0.0 {
                shape.node.rotate_y(shape.spin * delta);
                shape.node.rotate_x(shape.spin * 0.5 * delta);
            }
        }
    }
}

impl DemoWorld {
    pub fn set_controls(&mut self, controls: Controls) {
        if self.controls != controls {
            self.controls = controls;
            self.apply_controls();
        }
    }

    pub fn shape_count(&self) -> usize {
        self.shapes.iter().filter(|shape| shape.spin > 0.0).count()
    }

    pub fn drop_shape(&mut self) {
        let index = self.shape_count();
        let angle = index as f32 * 2.399_963;
        let radius = 2.0 + (index % 5) as f32 * 1.4;
        let mut mesh = BoxMesh::new_gd();
        mesh.set_size(Vector3::splat(0.8));
        let node = self.add_mesh(
            mesh.upcast(),
            material(PALETTE[index % PALETTE.len()], 0.5, 0.35),
            Vector3::new(angle.cos() * radius, 9.0, angle.sin() * radius),
        );
        self.shapes.push(Shape {
            node,
            spin: 0.4 + (index % 7) as f32 * 0.2,
            phase: angle,
            rest: 0.4,
            falling: Some(0.0),
        });
    }

    fn add_mesh(
        &mut self,
        mesh: Gd<Mesh>,
        material: Gd<StandardMaterial3D>,
        position: Vector3,
    ) -> Gd<MeshInstance3D> {
        let mut node = MeshInstance3D::new_alloc();
        node.set_mesh(&mesh);
        node.set_material_override(&material);
        node.set_position(position);
        self.base_mut().add_child(&node);
        node
    }

    fn apply_controls(&mut self) {
        let controls = self.controls;
        let pitch = controls.pitch.clamp(0.05, FRAC_PI_2 - 0.05);
        let eye = Vector3::new(
            controls.yaw.cos() * pitch.cos(),
            pitch.sin(),
            controls.yaw.sin() * pitch.cos(),
        ) * controls.distance
            + Vector3::new(0.0, 1.2, 0.0);
        if let Some(camera) = self.camera.as_mut() {
            camera.look_at_from_position(eye, Vector3::new(0.0, 1.4, 0.0));
        }
        if let Some(sun) = self.sun.as_mut() {
            sun.set_rotation(Vector3::new(-0.75, controls.sun * TAU, 0.0));
        }
        if let Some(environment) = self.environment.as_mut() {
            environment.set_glow_intensity(controls.bloom * 4.0);
        }
    }
}

fn shape_mesh(index: usize) -> Gd<Mesh> {
    match index % 3 {
        0 => {
            let mut mesh = BoxMesh::new_gd();
            mesh.set_size(Vector3::splat(1.3));
            mesh.upcast()
        }
        1 => {
            let mut mesh = SphereMesh::new_gd();
            mesh.set_radius(0.8);
            mesh.set_height(1.6);
            mesh.upcast()
        }
        _ => {
            let mut mesh = TorusMesh::new_gd();
            mesh.set_inner_radius(0.5);
            mesh.set_outer_radius(1.0);
            mesh.upcast()
        }
    }
}

fn material(color: (f32, f32, f32), metallic: f32, roughness: f32) -> Gd<StandardMaterial3D> {
    let mut material = StandardMaterial3D::new_gd();
    material.set_albedo(Color::from_rgb(color.0, color.1, color.2));
    material.set_metallic(metallic);
    material.set_roughness(roughness);
    material
}
