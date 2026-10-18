use std::sync::Arc;

use super::{
    eye::EyePlace,
    math::{Mat4, Vec3},
    mesh::{Mesh, MeshHit},
};
use crate::{Color, Point, Rect, Size, transform::ProjectiveTransform};

/// How much nearer than its focal length a camera sees: points closer to the
/// eye than `focal_length * NEAR_PLANE` are cut away. It is the w at which
/// [`ProjectiveTransform::front_polygon`] cuts a perspective map.
pub const NEAR_PLANE: f32 = crate::transform::NEAR_W;

/// A pinhole camera that shows a 3D scene in a composable's bounds.
///
/// The camera looks along its own z axis with its x to the right and y down.
/// A point at `(x, y, z)` in the camera's frame shows at
/// `center + (x, y) * focal_length / z`, where `center` is the middle of the
/// bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Maps scene space into the camera's frame.
    pub view: Mat4,
    /// The distance from the eye to the image, in logical pixels: the
    /// distance at which one scene unit shows as one pixel.
    pub focal_length: f32,
}

impl Camera {
    /// A camera at `eye` that looks at `target`. `down` is the scene
    /// direction that shows as down; it need not be square to the view.
    pub fn look_at(eye: Vec3, target: Vec3, down: Vec3, focal_length: f32) -> Self {
        let forward = (target - eye)
            .normalized()
            .unwrap_or(Vec3::new(0.0, 0.0, 1.0));
        let right = down
            .cross(forward)
            .normalized()
            .unwrap_or(Vec3::new(1.0, 0.0, 0.0));
        let down = forward.cross(right);
        let row = |axis: Vec3| [axis.x, axis.y, axis.z, -axis.dot(eye)];
        Self {
            view: Mat4::from_rows([row(right), row(down), row(forward), [0.0, 0.0, 0.0, 1.0]]),
            focal_length,
        }
    }

    /// The focal length that shows `vertical_fov` radians across `height`
    /// pixels.
    pub fn focal_length_for(height: f32, vertical_fov: f32) -> f32 {
        height * 0.5 / (vertical_fov * 0.5).tan()
    }

    /// The eye's position in scene space.
    pub fn eye(&self) -> Vec3 {
        self.view
            .inverse()
            .map_or(Vec3::ZERO, |inverse| inverse.transform_point(Vec3::ZERO))
    }

    /// The camera-frame depth below which the camera sees nothing.
    pub fn near(&self) -> f32 {
        self.focal_length * NEAR_PLANE
    }

    /// Where `point` shows in bounds of `size`; `None` when it lies nearer
    /// than [`Self::near`] or behind the eye.
    pub fn project(&self, point: Vec3, size: Size) -> Option<Point> {
        let seen = self.view.transform_point(point);
        (seen.z >= self.near()).then(|| {
            let scale = self.focal_length / seen.z;
            Point::new(
                size.width * 0.5 + seen.x * scale,
                size.height * 0.5 + seen.y * scale,
            )
        })
    }

    /// The map from the scene this camera sees into the frame in which
    /// `window`, a rect on the frame's plane, is a doorway into the scene:
    /// the camera stands in the doorway's middle, looking along the frame's
    /// z, with its x and y along the frame's, a scene unit to a frame unit.
    /// What the camera sees lies behind the doorway.
    pub fn doorway(&self, window: Rect) -> Mat4 {
        self.view.then(&Mat4::translation(Vec3::new(
            window.x + window.width * 0.5,
            window.y + window.height * 0.5,
            0.0,
        )))
    }

    /// The ray from the eye through `point` of bounds of `size`, in scene
    /// space, with a direction that reaches depth one in the camera's frame
    /// at distance one.
    pub fn ray(&self, point: Point, size: Size) -> Option<Ray> {
        let inverse = self.view.inverse()?;
        let through = Vec3::new(
            (point.x - size.width * 0.5) / self.focal_length,
            (point.y - size.height * 0.5) / self.focal_length,
            1.0,
        );
        Some(Ray {
            origin: inverse.transform_point(Vec3::ZERO),
            direction: inverse.transform_vector(through),
        })
    }
}

/// A half line in scene space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    /// Where the ray starts.
    pub origin: Vec3,
    /// Which way the ray goes; distances along the ray count in its length.
    pub direction: Vec3,
}

/// A light from one direction, with an even part that reaches every side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    /// The direction the light travels, in scene space.
    pub direction: Vec3,
    /// How bright a surface the light does not reach is, from 0 to 1: the
    /// side of a lit surface turned away from the light, and any surface in
    /// shadow.
    pub ambient: f32,
    /// Whether the surfaces that cast shadows cast them on the scene's
    /// surfaces.
    pub shadows: bool,
    /// The angle the light spans as the scene sees it, in radians: 0 for a
    /// point far away, whose shadows have hard edges. A light of some size
    /// casts shadows that are sharp where a caster meets a surface and soft
    /// farther from it, as the sun's 0.0093 does and a wide lamp's more.
    pub size: f32,
}

impl Light {
    /// How bright a surface is, from `ambient` to 1, where its side that
    /// faces the viewer has `normal`: the light reaches only a side that
    /// faces it.
    pub fn brightness(&self, normal: Vec3) -> f32 {
        let facing = match (self.direction.normalized(), normal.normalized()) {
            (Some(direction), Some(normal)) => (-normal.dot(direction)).max(0.0),
            _ => 0.0,
        };
        self.ambient + (1.0 - self.ambient) * facing
    }

    /// The map from scene space to a shadow map of this light that holds
    /// `casters`: x and y from -1 to 1 across the map, and depth from 0 at
    /// the caster nearest the light to 1 at the farthest. A point farther
    /// along the light than a caster's depth there lies in its shadow.
    /// `None` without casters, or for a light with no direction.
    pub fn shadow_projection<'a>(
        &self,
        casters: impl IntoIterator<Item = &'a MeshSurface>,
    ) -> Option<Mat4> {
        const MARGIN: f32 = 0.02;
        let forward = self.direction.normalized()?;
        let hint = if forward.y.abs() < 0.9 {
            Vec3::new(0.0, 1.0, 0.0)
        } else {
            Vec3::new(1.0, 0.0, 0.0)
        };
        let right = hint.cross(forward).normalized()?;
        let up = forward.cross(right);
        let view = Mat4::from_rows([
            [right.x, right.y, right.z, 0.0],
            [up.x, up.y, up.z, 0.0],
            [forward.x, forward.y, forward.z, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]);
        let mut low = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let mut high = -low;
        for caster in casters {
            let (caster_low, caster_high) = caster.mesh.bounds_in(&caster.transform.then(&view));
            (low, high) = (low.min(caster_low), high.max(caster_high));
        }
        let extent = high - low;
        if !(extent.x.is_finite() && extent.y.is_finite() && extent.z.is_finite()) {
            return None;
        }
        let pad = |extent: f32| (extent * MARGIN).max(1.0);
        let (low, high) = (
            low - Vec3::new(pad(extent.x), pad(extent.y), pad(extent.z)),
            high + Vec3::new(pad(extent.x), pad(extent.y), pad(extent.z)),
        );
        let extent = high - low;
        let fit = Mat4::from_rows([
            [2.0 / extent.x, 0.0, 0.0, -1.0 - 2.0 * low.x / extent.x],
            [0.0, 2.0 / extent.y, 0.0, -1.0 - 2.0 * low.y / extent.y],
            [0.0, 0.0, 1.0 / extent.z, -low.z / extent.z],
            [0.0, 0.0, 0.0, 1.0],
        ]);
        Some(view.then(&fit))
    }
}

/// A scene's camera and light, which a `scene3d` modifier shows its
/// children's mesh surfaces through.
///
/// A scene inside another scene's space is a doorway into a space of its
/// own: the rect its node takes in its frame opens onto that space, which
/// lies behind it, placed so the doorway stands where the inner scene's
/// camera stands (see [`Camera::doorway`]). The window's eye sees through
/// doorways from the space it stands in, as deep as they show on screen,
/// and walks through them (see [`EyePlace`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Scene3d {
    /// The camera the scene is seen through. For a scene behind a doorway,
    /// the one that stands in the doorway; for the window's scene with an
    /// [`eye`](Self::eye), the eye's, in the space the eye stands in.
    pub camera: Camera,
    /// The light on the scene's lit surfaces, or `None` for none.
    pub light: Option<Light>,
    /// For the window's scene, where its eye stands among the spaces behind
    /// the scene's doorways and where the renderer leaves what the eye can
    /// bump into; `None` for a camera in the scene's own space that walks
    /// through nothing. A scene behind a doorway leaves it out.
    pub eye: Option<EyePlace>,
}

/// A composable's content drawn on a mesh placed in a scene.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshSurface {
    /// The mesh the content is drawn on.
    pub mesh: Arc<Mesh>,
    /// Places the mesh in the scene.
    pub transform: Mat4,
    /// How the content meets the scene's light and what lies behind it.
    pub material: Material,
    /// Whether the surface casts a shadow from the scene's light. Every lit
    /// and unlit surface takes the shadows that fall on it.
    pub casts_shadow: bool,
    /// Whether an eye that walks into the surface (see
    /// [`EyeWorld::walk`](super::EyeWorld::walk)) stops there. A doorway in
    /// the surface's content lets it through.
    pub solid: bool,
}

/// How a surface's content meets the scene's light and what lies behind
/// it.
#[derive(Clone, Debug, PartialEq)]
pub enum Material {
    /// Shaded by the angle the light falls at, and darkened where a shadow
    /// falls on it.
    Lit,
    /// The content as it is, as a screen shows it, darkened only where a
    /// shadow falls on it.
    Unlit,
    /// The content at its full colour, as fire or a lamp shows: neither the
    /// light nor a shadow changes it.
    Emissive,
    /// Adds the content's colour to what lies behind it, as light does: a
    /// halo, a flame or a spark. It hides nothing, and surfaces nearer the
    /// eye hide it.
    Glow,
    /// The scene behind shows through, bent by the surface's curve, with the
    /// light's highlight on it and the content drawn over it. Glass shows
    /// the surfaces that are not glass behind it.
    Glass(Glass),
    /// A shape an app's shader finds inside the mesh, ray by ray, as the
    /// camera sees it, with the depth of what each ray meets: the mesh only
    /// bounds it. A volume casts no shadow.
    Volume(Volume),
}

/// An app's shader that draws inside a mesh: each pixel the mesh covers
/// asks it what the camera's ray through the pixel meets.
///
/// `shader` is WGSL that defines
///
/// ```text
/// fn volume(eye: vec3<f32>, direction: vec3<f32>, entry: f32, exit: f32, footprint: f32) -> VolumeHit
/// ```
///
/// in the mesh's own coordinates: the eye; the ray's direction, of length
/// one; how far along the ray it enters the mesh's bounds, zero for an eye
/// inside them, and leaves the mesh; and how wide a pixel is, in radians.
/// It returns a `VolumeHit`: the colour, premultiplied, nothing at an alpha
/// of zero; and the point the ray met, which sets the pixel's depth.
/// `volume_param(index)` reads `params[index]`, `volume_content(uv)` the
/// surface's content and `volume_light()` the scene's light, in the mesh's
/// coordinates. The renderer puts
/// [`MESH_VOLUME_PRELUDE_WGSL`](crate::framework_shaders::MESH_VOLUME_PRELUDE_WGSL),
/// which declares them, before it. The mesh's normals must point out of it.
#[derive(Clone, Debug)]
pub struct Volume {
    /// The app's WGSL. Keep one `Arc` per shader: the renderer builds a
    /// pipeline for each.
    pub shader: Arc<str>,
    /// The values the shader reads.
    pub params: [f32; Volume::PARAMS],
}

impl Volume {
    /// How many values a volume's shader reads.
    pub const PARAMS: usize = 32;
}

impl PartialEq for Volume {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shader, &other.shader) && self.params == other.params
    }
}

/// How glass bends and tints the scene behind it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glass {
    /// The refractive index: 1 bends nothing, window glass is 1.5 and
    /// diamond 2.4.
    pub index: f32,
    /// How far a ray travels inside the glass, in scene units: a ball's
    /// diameter for a solid ball. A ray bends entering the glass and again
    /// leaving it, as through a solid ball, so a thick ball shows the scene
    /// behind it upside down.
    pub thickness: f32,
    /// The colour the glass tints what shows through it; white for clear
    /// glass.
    pub tint: Color,
}

impl Glass {
    /// Clear window glass a ray crosses for `thickness` scene units.
    pub const fn clear(thickness: f32) -> Self {
        Self {
            index: 1.5,
            thickness,
            tint: Color(1.0, 1.0, 1.0, 1.0),
        }
    }
}

impl MeshSurface {
    /// The content of a composable of `size` on the mesh: the content point
    /// where the ray meets the mesh, the triangle it meets, and how far along
    /// the ray.
    pub fn cast(&self, ray: Ray, size: Size) -> Option<SurfaceHit> {
        let inverse = self.transform.inverse()?;
        let hit = self.mesh.cast(
            inverse.transform_point(ray.origin),
            inverse.transform_vector(ray.direction),
        )?;
        Some(SurfaceHit::of(hit, size))
    }

    /// The content point of a composable of `size` where the ray meets the
    /// plane of `triangle`: a drag that leaves the mesh's edge goes on along
    /// it.
    pub fn cast_plane(&self, ray: Ray, triangle: u32, size: Size) -> Option<Point> {
        let inverse = self.transform.inverse()?;
        let uv = self.mesh.cast_plane(
            inverse.transform_point(ray.origin),
            inverse.transform_vector(ray.direction),
            triangle,
        )?;
        Some(content_point(uv, size))
    }

    /// The projective map that carries the content of a composable of
    /// `size` from the mesh to the bounds of `viewport` that `camera` shows.
    /// It is exact for a flat mesh; for a curved one it follows the plane
    /// that touches the mesh at the content's middle. It sends the part
    /// nearer than the camera's near plane to w below [`NEAR_PLANE`], which
    /// [`ProjectiveTransform::front_polygon`] cuts away. `None` for content
    /// of no size, or for a mesh with no texture plane.
    pub fn content_transform(
        &self,
        camera: &Camera,
        viewport: Size,
        size: Size,
    ) -> Option<ProjectiveTransform> {
        if !(size.width > 0.0 && size.height > 0.0) {
            return None;
        }
        let plane = self.mesh.texture_plane()?;
        let to_camera = self.transform.then(&camera.view);
        let origin = to_camera.transform_point(plane.origin);
        let right = to_camera.transform_vector(plane.u_axis) * (1.0 / size.width);
        let down = to_camera.transform_vector(plane.v_axis) * (1.0 / size.height);
        let (center_x, center_y) = (viewport.width * 0.5, viewport.height * 0.5);
        let focal_length = camera.focal_length;
        let column = |v: Vec3| {
            [
                v.x + center_x * v.z / focal_length,
                v.y + center_y * v.z / focal_length,
                v.z / focal_length,
            ]
        };
        let [a, b, c] = [column(right), column(down), column(origin)];
        Some(ProjectiveTransform::from_rows([
            [a[0], b[0], c[0]],
            [a[1], b[1], c[1]],
            [a[2], b[2], c[2]],
        ]))
    }

    /// The map from the content space of a composable of `size` on the mesh
    /// into the scene: x and y in the content's pixels from its top left, z
    /// into the surface, away from the side it shows its content to, a unit
    /// as long as a content pixel. A mesh surface inside this content is
    /// placed in that space. It is exact for a flat mesh; for a curved one it
    /// follows the plane that touches the mesh at the content's middle.
    /// `None` for content of no size, or for a mesh with no texture plane.
    pub fn content_to_scene(&self, size: Size) -> Option<Mat4> {
        if !(size.width > 0.0 && size.height > 0.0) {
            return None;
        }
        let plane = self.mesh.texture_plane()?;
        let right = self.transform.transform_vector(plane.u_axis) * (1.0 / size.width);
        let down = self.transform.transform_vector(plane.v_axis) * (1.0 / size.height);
        let into = right.cross(down).normalized()? * (right.length() * down.length()).sqrt();
        Some(Mat4::from_axes(
            right,
            down,
            into,
            self.transform.transform_point(plane.origin),
        ))
    }

    /// This surface with its transform followed by `to_scene`: a surface
    /// inside another's content, placed in the scene through the other's
    /// [`content_to_scene`](Self::content_to_scene). Glass keeps its
    /// thickness in the units of the space it is placed from.
    pub fn placed_by(&self, to_scene: &Mat4) -> Self {
        let material = match &self.material {
            Material::Glass(glass) => Material::Glass(Glass {
                thickness: glass.thickness * to_scene.length_scale(),
                ..*glass
            }),
            material => material.clone(),
        };
        Self {
            mesh: Arc::clone(&self.mesh),
            transform: self.transform.then(to_scene),
            material,
            casts_shadow: self.casts_shadow,
            solid: self.solid,
        }
    }

    /// Where the content point `point` of a composable of `size` lies in the
    /// scene.
    pub fn point_at(&self, point: Point, size: Size) -> Option<Vec3> {
        let uv = [
            point.x / size.width.max(f32::MIN_POSITIVE),
            point.y / size.height.max(f32::MIN_POSITIVE),
        ];
        self.mesh
            .point_at(uv)
            .map(|position| self.transform.transform_point(position))
    }
}

/// Where a ray meets a [`MeshSurface`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceHit {
    /// How far along the ray, in units of its direction.
    pub distance: f32,
    /// The mesh triangle the ray meets.
    pub triangle: u32,
    /// The content point at the hit.
    pub point: Point,
}

impl SurfaceHit {
    fn of(hit: MeshHit, size: Size) -> Self {
        Self {
            distance: hit.distance,
            triangle: hit.triangle,
            point: content_point(hit.uv, size),
        }
    }
}

fn content_point([u, v]: [f32; 2], size: Size) -> Point {
    Point::new(u * size.width, v * size.height)
}

/// Where a layer sits in 3D, as its graphics layer carries it.
///
/// Inside a scene, every layer takes a frame from its parent's: its place
/// in its parent's layout, turned into 3D on the parent's plane, with z
/// away from the side the content shows to. A layer that carries a space
/// changes the frame its descendants take.
#[derive(Clone, Debug, PartialEq)]
pub enum LayerSpace {
    /// The layer shows a scene: its children's frames are the scene's
    /// space. Inside another scene, the layer's rect is a doorway into it.
    Scene(Scene3d),
    /// The layer's content is drawn on a mesh placed in the layer's frame,
    /// and its children's frames lie in that content.
    Surface(MeshSurface),
    /// Moves the frames of the layer's descendants in its own frame, as a
    /// group of 3D objects moves together. The layer's own draws stay where
    /// they would be without it.
    Transform(Mat4),
}
