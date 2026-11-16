// The scene every eye sees: a floor with a meter grid, three live panels around the viewer and
// a spinning globe. Each eye composes it again; its state lives in `SceneState`, so every eye
// shows the same panels.

use std::{f32::consts::PI, sync::Arc};

use cranpose_core::{key, mutableStateOf, remember, MutableState};
use cranpose_ui::{
    composable,
    space::{Mat4, Material, Mesh, MeshSurface, Vec3},
    text::TextUnit,
    Box, BoxSpec, Brush, Button, ButtonSpec, Color, Column, ColumnSpec, LinearArrangement,
    Modifier, Rect, Size, Text, TextStyle,
};

use super::eye_view::{cran, UNITS_PER_METER};
use crate::app::ticking::Ticking;

const PANEL_PIXELS: Size = Size {
    width: 640.0,
    height: 400.0,
};
const PANEL_METERS: (f32, f32) = (1.2, 0.75);
const FLOOR_METERS: f32 = 8.0;
const GLOBE_RADIUS: f32 = 0.22;
const SPIN_PER_SECOND: f32 = 0.6;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct SceneState {
    presses: MutableState<u32>,
    spin: MutableState<f32>,
}

#[derive(Clone)]
pub(super) struct Meshes {
    plane: Arc<Mesh>,
    sphere: Arc<Mesh>,
}

// Meshes are made once and shared: the same meshes are equal.
impl PartialEq for Meshes {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.plane, &other.plane) && Arc::ptr_eq(&self.sphere, &other.sphere)
    }
}

impl Meshes {
    pub(super) fn new() -> Self {
        Self {
            plane: Arc::new(Mesh::plane()),
            sphere: Arc::new(Mesh::sphere(40, 20)),
        }
    }
}

#[composable]
pub(super) fn rememberSceneState() -> SceneState {
    let state = remember(|| SceneState {
        presses: mutableStateOf(0),
        spin: mutableStateOf(0.0),
    })
    .with(|state| *state);
    let spin = state.spin;
    Ticking(true, move |seconds| {
        spin.update(|angle| *angle = (*angle + seconds * SPIN_PER_SECOND) % (2.0 * PI));
    });
    state
}

pub(super) fn text_style(size: f32, color: Color) -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.font_size = TextUnit::Sp(size);
    style.span_style.color = Some(color);
    style
}

// A panel at `center` (WebXR), upright and turned toward the viewer at the origin.
fn panel_transform(center: [f32; 3], width: f32, height: f32) -> Mat4 {
    let c = cran(center);
    let forward = Vec3::new(c.x, 0.0, c.z)
        .normalized()
        .unwrap_or(Vec3::new(0.0, 0.0, 1.0));
    let down = Vec3::new(0.0, 1.0, 0.0);
    let x = down.cross(forward) * width * UNITS_PER_METER;
    let y = down * height * UNITS_PER_METER;
    Mat4::from_axes(x, y, forward, c - x * 0.5 - y * 0.5)
}

#[composable]
pub(super) fn Scene(state: SceneState, meshes: Meshes) {
    let floor = Arc::clone(&meshes.plane);
    key("floor", move || Floor(Arc::clone(&floor)));
    let panels: [(PanelKind, [f32; 3]); 3] = [
        (PanelKind::Counter, [0.0, 1.5, -1.6]),
        (PanelKind::Notes, [-1.35, 1.45, -1.05]),
        (PanelKind::Spinner, [1.35, 1.45, -1.05]),
    ];
    for (kind, center) in panels {
        let mesh = Arc::clone(&meshes.plane);
        key(kind, move || Panel(kind, center, Arc::clone(&mesh), state));
    }
    let sphere = Arc::clone(&meshes.sphere);
    key("globe", move || Globe(Arc::clone(&sphere), state));
}

#[composable]
fn Floor(mesh: Arc<Mesh>) {
    let side = FLOOR_METERS * UNITS_PER_METER;
    Box(
        Modifier::empty()
            .required_size(Size {
                width: 1024.0,
                height: 1024.0,
            })
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&mesh),
                // Its x along x and its y toward the viewer, so it faces up.
                transform: Mat4::from_axes(
                    Vec3::new(side, 0.0, 0.0),
                    Vec3::new(0.0, 0.0, -side),
                    Vec3::new(0.0, 1.0, 0.0),
                    Vec3::new(-side * 0.5, 0.0, side * 0.5),
                ),
                material: Material::Unlit,
                casts_shadow: false,
                solid: false,
            })
            .draw_behind(|scope| {
                let size = scope.size();
                scope.draw_rect(Brush::solid(Color(0.10, 0.11, 0.14, 1.0)));
                let lines = (FLOOR_METERS * 2.0) as u32;
                for i in 0..=lines {
                    let t = i as f32 / lines as f32;
                    let (color, width) = if i % 2 == 0 {
                        (Color(0.3, 0.75, 0.5, 1.0), 3.0)
                    } else {
                        (Color(0.22, 0.25, 0.3, 1.0), 2.0)
                    };
                    let across = Rect {
                        x: t * size.width - width * 0.5,
                        y: 0.0,
                        width,
                        height: size.height,
                    };
                    let along = Rect {
                        x: 0.0,
                        y: t * size.height - width * 0.5,
                        width: size.width,
                        height: width,
                    };
                    scope.draw_rect_at(across, Brush::solid(color));
                    scope.draw_rect_at(along, Brush::solid(color));
                }
            }),
        BoxSpec::default(),
        || {},
    );
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum PanelKind {
    Counter,
    Notes,
    Spinner,
}

#[composable]
fn Panel(kind: PanelKind, center: [f32; 3], mesh: Arc<Mesh>, state: SceneState) {
    let transform = panel_transform(center, PANEL_METERS.0, PANEL_METERS.1);
    Box(
        Modifier::empty()
            .required_size(PANEL_PIXELS)
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&mesh),
                transform,
                material: Material::Unlit,
                casts_shadow: false,
                solid: true,
            })
            .rounded_corners(24.0)
            .background(Color(0.12, 0.14, 0.2, 1.0))
            .padding(28.0),
        BoxSpec::default(),
        move || match kind {
            PanelKind::Counter => CounterPanel(state),
            PanelKind::Notes => NotesPanel(),
            PanelKind::Spinner => SpinnerPanel(state),
        },
    );
}

#[composable]
fn CounterPanel(state: SceneState) {
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::SpacedBy(18.0)),
        move || {
            Text(
                "Cranpose in 3D",
                Modifier::empty(),
                text_style(40.0, Color::WHITE),
            );
            Text(
                "These panels are composables drawn on meshes. They stay live.",
                Modifier::empty(),
                text_style(22.0, Color(0.75, 0.8, 0.88, 1.0)),
            );
            let presses = state.presses.get();
            Button(
                Modifier::empty()
                    .rounded_corners(14.0)
                    .background(Color(0.15, 0.55, 0.35, 1.0))
                    .padding(18.0),
                ButtonSpec::default(),
                move || state.presses.update(|n| *n += 1),
                move || {
                    Text(
                        format!("Pressed {presses} times"),
                        Modifier::empty(),
                        text_style(26.0, Color::WHITE),
                    );
                },
            );
        },
    );
}

#[composable]
fn NotesPanel() {
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::SpacedBy(14.0)),
        || {
            Text(
                "Three ways to see it",
                Modifier::empty(),
                text_style(32.0, Color::WHITE),
            );
            for line in [
                "Flat: one camera. Drag to look around.",
                "Stereo: two eyes, each with its own uneven view.",
                "Enter VR: the headset's browser sets each eye's pose every frame.",
            ] {
                Text(
                    line,
                    Modifier::empty(),
                    text_style(20.0, Color(0.75, 0.8, 0.88, 1.0)),
                );
            }
        },
    );
}

#[composable]
fn SpinnerPanel(state: SceneState) {
    Box(
        Modifier::empty().fill_max_size().draw_behind(move |draw| {
            let size = draw.size();
            let angle = state.spin.get();
            let (cx, cy, r) = (size.width * 0.5, size.height * 0.55, size.height * 0.36);
            for i in 0..12 {
                let a = angle + i as f32 * PI / 6.0;
                let shade = 0.35 + 0.65 * (i as f32 / 12.0);
                let dot = Rect {
                    x: cx + a.cos() * r - 14.0,
                    y: cy + a.sin() * r - 14.0,
                    width: 28.0,
                    height: 28.0,
                };
                draw.draw_rect_at(dot, Brush::solid(Color(0.2, 0.5 * shade, shade, 1.0)));
            }
        }),
        BoxSpec::default(),
        || {
            Text(
                "Drawn every frame",
                Modifier::empty(),
                text_style(26.0, Color::WHITE),
            );
        },
    );
}

#[composable]
fn Globe(mesh: Arc<Mesh>, state: SceneState) {
    Box(
        Modifier::empty()
            .required_size(Size {
                width: 512.0,
                height: 256.0,
            })
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&mesh),
                transform: Mat4::scaling(
                    Vec3::new(1.0, 1.0, 1.0) * (GLOBE_RADIUS * UNITS_PER_METER),
                )
                .then(&Mat4::rotation_y(state.spin.get()))
                .then(&Mat4::translation(cran([0.75, 1.0, -1.2]))),
                material: Material::Unlit,
                casts_shadow: false,
                solid: true,
            })
            .draw_behind(|draw| {
                let size = draw.size();
                draw.draw_rect(Brush::solid(Color(0.1, 0.32, 0.62, 1.0)));
                for band in 0..8 {
                    let stripe = Rect {
                        x: size.width * band as f32 / 8.0,
                        y: 0.0,
                        width: size.width / 16.0,
                        height: size.height,
                    };
                    draw.draw_rect_at(stripe, Brush::solid(Color(0.14, 0.5, 0.36, 1.0)));
                }
            }),
        BoxSpec::default(),
        || {},
    );
}
