// A silk curtain on a brass rod. The rod, its two finials, its bracket,
// each clip and the cloth are composables of their own, each drawn on a
// mesh in the stage's scene. The cloth's content stays live: its words
// select and its buttons press where they show on the folds.
//
// The cloth hangs and moves at one size; a mount shows it anywhere and at
// any size, so a curtain can grow while it hangs without starting again.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{Arc, OnceLock},
};

use cranpose_core::{key, remember, rememberMutableStateOf};
use cranpose_ui::{
    composable,
    space::{Mat4, Material, Mesh, MeshSurface, Shaded, Vec3},
    Box, BoxSpec, Color, Modifier, Size,
};

use super::cloth::{Air, Cloth, ClothHang};
use crate::app::{room::TORUS_OBJ, ticking::Ticking};

// The breeze that keeps a hanging curtain alive, in pixels a second
// squared.
const BREEZE: f32 = 260.0;
// The push of a full gust, in pixels a second squared.
const GUST: f32 = 15000.0;
// How long a gust blows, in seconds.
const GUST_SECONDS: f32 = 1.6;
const BRASS: Color = Color(0.86, 0.66, 0.30, 1.0);
const SILK_RED: Color = Color(0.74, 0.05, 0.07, 1.0);
// The gold that the content on the silk marks gold leaf with: the leaf
// takes it as its metal.
pub(super) const LEAF_GOLD: Color = Color(0.96, 0.78, 0.32, 1.0);
// The silk's sheen, how far the light wraps round its folds and how bright
// a fold the light misses is.
const SILK_LIGHT: [f32; 3] = [1.0, 0.25, 0.28];

// Where a curtain shows: its cloth, moved so its first clip stands at `at`
// and scaled by `scale` across and down, with a rod of `rod_radius`
// reaching `overhang` past the clips at each end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Mount {
    pub at: Vec3,
    pub scale: (f32, f32),
    pub rod_radius: f32,
    pub overhang: f32,
}

impl Mount {
    // The mount `share` of the way from `self` to `other`.
    pub(super) fn lerp(self, other: Self, share: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * share;
        Self {
            at: self.at.lerp(other.at, share),
            scale: (
                mix(self.scale.0, other.scale.0),
                mix(self.scale.1, other.scale.1),
            ),
            rod_radius: mix(self.rod_radius, other.rod_radius),
            overhang: mix(self.overhang, other.overhang),
        }
    }

    // Places the cloth, hung at `hang`, on this mount.
    fn cloth_transform(&self, hang: &ClothHang) -> Mat4 {
        Mat4::translation(-hang.top_left)
            .then(&Mat4::scaling(Vec3::new(self.scale.0, self.scale.1, 1.0)))
            .then(&Mat4::translation(self.at))
    }

    // How far the clips span on this mount.
    fn span(&self, hang: &ClothHang) -> f32 {
        hang.span * self.scale.0
    }

    fn ring_radius(&self) -> f32 {
        self.rod_radius * 1.7
    }

    // The rod's axis: its height and depth.
    fn rod_axis(&self) -> (f32, f32) {
        (self.at.y - self.ring_radius() * 2.2, self.at.z)
    }

    // Where clip `index` of `hang`'s holds the cloth's top edge.
    fn clip(&self, hang: &ClothHang, index: usize) -> Vec3 {
        let share = index as f32 / (hang.clips - 1).max(1) as f32;
        self.at + Vec3::new(self.span(hang) * share, 0.0, 0.0)
    }
}

// Where a curtain shows now, read each time its parts are drawn: a curtain
// that grows as the page scrolls moves without composing again.
pub(super) type MountNow = Rc<dyn Fn() -> Mount>;

// How far the print has soaked into the cloth now, from 0 to 1, read each
// time the cloth is drawn.
pub(super) type SoakNow = Rc<dyn Fn() -> f32>;

// Blows the curtain: a gust that dies down over `GUST_SECONDS`.
#[derive(Clone, Default)]
pub(super) struct Gust(Rc<Cell<f32>>);

impl Gust {
    pub(super) fn blow(&self) {
        self.0.set(GUST_SECONDS);
    }

    // The gust's push for a frame `seconds` long, and the time it has left
    // after it.
    fn take(&self, seconds: f32) -> f32 {
        let left = self.0.get();
        if left <= 0.0 {
            return 0.0;
        }
        self.0.set((left - seconds).max(0.0));
        let share = left / GUST_SECONDS;
        // Rises fast and dies down slowly.
        GUST * (share * (1.0 - share) * 4.0).min(1.0).max(share * share)
    }
}

// A curtain of cloth hung at `hang`, shown where `mount` says, with
// `cloth` printed on it as far as `soak` says.
#[composable(no_skip)]
pub(super) fn Curtain(
    hang: ClothHang,
    (mount, soak): (MountNow, SoakNow),
    (moving, gust): (bool, Gust),
    cloth: impl FnMut() + 'static,
) {
    let shapes = remember(BrassShapes::new).with(Clone::clone);
    let rod = Rc::clone(&mount);
    BrassPart(Arc::clone(&shapes.tube), move || {
        let mount = rod();
        let (y, z) = mount.rod_axis();
        let length = mount.span(&hang) + mount.overhang * 2.0;
        let middle = Vec3::new(mount.at.x + mount.span(&hang) * 0.5, y, z);
        Mat4::scaling(Vec3::new(mount.rod_radius, length, mount.rod_radius))
            .then(&Mat4::rotation_z(std::f32::consts::FRAC_PI_2))
            .then(&Mat4::translation(middle))
    });
    for end in [0.0f32, 1.0] {
        let (ball, mount) = (Arc::clone(&shapes.ball), Rc::clone(&mount));
        key(("finial", end.to_bits()), move || {
            BrassPart(ball, move || {
                let mount = mount();
                let (y, z) = mount.rod_axis();
                let x =
                    mount.at.x - mount.overhang + (mount.span(&hang) + mount.overhang * 2.0) * end;
                let radius = mount.rod_radius * 2.4;
                Mat4::scaling(Vec3::new(radius, radius, radius))
                    .then(&Mat4::translation(Vec3::new(x, y, z)))
            });
        });
    }
    let (block, bracket) = (Arc::clone(&shapes.block), Rc::clone(&mount));
    key("bracket", move || {
        // The arm that holds the rod's far end off the wall.
        BrassPart(block, move || {
            let mount = bracket();
            let (y, z) = mount.rod_axis();
            let depth = hang.wall - z;
            let side = mount.rod_radius * 1.4;
            let x = mount.at.x + mount.span(&hang) + mount.overhang - mount.rod_radius * 5.0;
            Mat4::scaling(Vec3::new(side, side, depth)).then(&Mat4::translation(Vec3::new(
                x,
                y,
                z + depth * 0.5,
            )))
        });
    });
    for index in 0..hang.clips {
        let (shapes, mount) = (shapes.clone(), Rc::clone(&mount));
        key(("clip", index), move || Clip(hang, index, mount, shapes));
    }
    // A cloth hung anew starts again, with its own state.
    key(hang.bits(), move || {
        Silk(hang, (mount, soak), (moving, gust), cloth);
    });
}

// The shapes the curtain's brass is made of, made once.
#[derive(Clone)]
struct BrassShapes {
    tube: Arc<Mesh>,
    ball: Arc<Mesh>,
    block: Arc<Mesh>,
    ring: Option<Arc<Mesh>>,
}

impl BrassShapes {
    fn new() -> Self {
        Self {
            tube: Arc::new(Mesh::cylinder(24)),
            ball: Arc::new(Mesh::sphere(20, 12)),
            block: Arc::new(Mesh::cuboid()),
            ring: Mesh::from_obj(TORUS_OBJ).ok().map(Arc::new),
        }
    }
}

// One piece of polished brass: `mesh` placed by `place`, read each time it
// is drawn.
#[composable(no_skip)]
fn BrassPart(mesh: Arc<Mesh>, place: impl Fn() -> Mat4 + 'static) {
    Box(
        Modifier::empty()
            .required_size(Size::new(16.0, 16.0))
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&mesh),
                transform: place(),
                material: brass(),
                casts_shadow: true,
                solid: true,
            })
            .background(BRASS),
        BoxSpec::default(),
        || {},
    );
}

// A ring round the rod and the bell-shaped clip below it that bites the
// cloth's top edge where clip `index` holds it.
#[composable(no_skip)]
fn Clip(hang: ClothHang, index: usize, mount: MountNow, shapes: BrassShapes) {
    if let Some(torus) = shapes.ring {
        let mount = Rc::clone(&mount);
        key("ring", move || {
            BrassPart(torus, move || {
                let mount = mount();
                let (rod_y, rod_z) = mount.rod_axis();
                let at = mount.clip(&hang, index);
                // The torus lies round the y axis; turned, it rings the rod.
                let scale = mount.ring_radius() / 1.4;
                Mat4::scaling(Vec3::new(scale, scale * 0.7, scale))
                    .then(&Mat4::rotation_z(std::f32::consts::FRAC_PI_2))
                    .then(&Mat4::translation(Vec3::new(at.x, rod_y, rod_z)))
            });
        });
    }
    BrassPart(shapes.ball, move || {
        let mount = mount();
        let (rod_y, _) = mount.rod_axis();
        let at = mount.clip(&hang, index);
        let top = rod_y + mount.ring_radius();
        let height = (at.y - top + mount.rod_radius * 1.5).max(mount.rod_radius);
        let width = mount.rod_radius * 1.6;
        Mat4::scaling(Vec3::new(width, height * 0.5, width * 0.7)).then(&Mat4::translation(
            Vec3::new(at.x, top + height * 0.5, at.z),
        ))
    });
}

// The cloth, simulated while `moving`, with `content` printed on it.
#[composable(no_skip)]
fn Silk(
    hang: ClothHang,
    (mount, soak): (MountNow, SoakNow),
    (moving, gust): (bool, Gust),
    content: impl FnMut() + 'static,
) {
    let cloth = remember(move || Rc::new(RefCell::new(Cloth::new(hang)))).with(Rc::clone);
    let first = Rc::clone(&cloth);
    let mesh = rememberMutableStateOf(move || Arc::new(first.borrow().mesh()));
    Ticking(moving, move |seconds| {
        let air = Air {
            breeze: BREEZE,
            gust: gust.take(seconds),
        };
        let mut cloth = cloth.borrow_mut();
        cloth.advance(seconds, air);
        mesh.set(Arc::new(cloth.mesh()));
    });
    Box(
        Modifier::empty()
            .required_size(hang.content)
            .mesh_surface(move || MeshSurface {
                mesh: mesh.get(),
                transform: mount().cloth_transform(&hang),
                material: silk(soak(), hang.content),
                casts_shadow: true,
                solid: true,
            })
            .background(SILK_RED),
        BoxSpec::default(),
        content,
    );
}

fn brass() -> Material {
    static MATERIAL: OnceLock<Material> = OnceLock::new();
    MATERIAL
        .get_or_init(|| {
            Material::Shaded(Shaded {
                shader: Arc::from(cranpose_ui::include_wgsl!("studio.wgsl", "brass.wgsl")),
                params: [0.0; Shaded::PARAMS],
            })
        })
        .clone()
}

// Red silk with gold leaf, its `content` printed on it soaked in as far as
// `soak` says.
fn silk(soak: f32, content: Size) -> Material {
    static SHADER: OnceLock<Arc<str>> = OnceLock::new();
    let mut params = [0.0; Shaded::PARAMS];
    params[..SILK_LIGHT.len()].copy_from_slice(&SILK_LIGHT);
    params[3] = soak;
    params[12..14].copy_from_slice(&[content.width, content.height]);
    for (first, color) in [(4, LEAF_GOLD), (8, SILK_RED)] {
        params[first..first + 3].copy_from_slice(&[color.0, color.1, color.2]);
    }
    let shader = SHADER.get_or_init(|| {
        Arc::from(cranpose_ui::include_wgsl!(
            "studio.wgsl",
            "noise.wgsl",
            "silk.wgsl"
        ))
    });
    Material::Shaded(Shaded {
        shader: Arc::clone(shader),
        params,
    })
}
