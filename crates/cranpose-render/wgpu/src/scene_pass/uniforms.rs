// The values that a scene's shaders read for each surface, and where a surface lands on its page.

use cranpose_ui_graphics::{
    Rect,
    space::{Light, Mat4, Material, Mesh, MeshSurface, Shaded, TextureEdges, Vec3, Volume},
};

use super::{
    SHADOW_DEPTH_BIAS, SHADOW_MAP_SIZE, ScenePage, SceneShadedUniforms, SceneSurfaceUniforms,
    SceneVolumeUniforms, SpaceShading, SurfaceShading,
};
use crate::{collect::SceneDraw, frame_graph::UploadAllocatorSpec};

pub(crate) fn scene_surface_uniform_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::uniform(
        "Scene Surface Uniform Buffer",
        "Scene Surface Uniform Bind Group",
        size_of::<SceneSurfaceUniforms>() as u64,
    )
}

pub(crate) fn scene_volume_uniform_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::uniform(
        "Scene Volume Uniform Buffer",
        "Scene Volume Uniform Bind Group",
        size_of::<SceneVolumeUniforms>() as u64,
    )
}

pub(crate) fn scene_shaded_uniform_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::uniform(
        "Scene Shaded Uniform Buffer",
        "Scene Shaded Uniform Bind Group",
        size_of::<SceneShadedUniforms>() as u64,
    )
}

/// A space's light for a shader: the direction it travels, of length one,
/// through `into`, and its ambient share; an ambient share below zero for a
/// space without light.
fn light_uniform(light: Option<Light>, into: impl Fn(Vec3) -> Vec3) -> [f32; 4] {
    match light {
        Some(light) => {
            let direction = into(light.direction).normalized().unwrap_or(Vec3::ZERO);
            [
                direction.x,
                direction.y,
                direction.z,
                light.ambient.clamp(0.0, 1.0),
            ]
        }
        None => [0.0, 0.0, 0.0, -1.0],
    }
}

/// The uniforms of `volume` on `mesh`, placed by `transform` in `scene` and
/// drawn on `page` at `alpha`, whose eye is at `eye`; `None` when its
/// transform flattens space.
pub(crate) fn volume_uniforms(
    scene: &SceneDraw,
    (transform, mesh, volume): (&Mat4, &Mesh, &Volume),
    (alpha, source_region): (f32, [f32; 4]),
    (page, eye, space): (ScenePage, Vec3, SpaceShading),
) -> Option<SceneVolumeUniforms> {
    let model_from_scene = transform.inverse()?;
    let point = |point: Vec3| [point.x, point.y, point.z, 1.0];
    let (low, high) = mesh.bounds();
    Some(SceneVolumeUniforms {
        clip_from_model: transform
            .then(&clip_from_scene(scene, page))
            .transpose()
            .rows(),
        eye: point(model_from_scene.transform_point(eye)),
        light: light_uniform(space.light, |direction| {
            model_from_scene.transform_vector(direction)
        }),
        footprint: [
            1.0 / (scene.camera.focal_length * page.scale).max(f32::MIN_POSITIVE),
            alpha.clamp(0.0, 1.0),
            0.0,
            0.0,
        ],
        low: point(low),
        high: point(high),
        params: bytemuck::cast(volume.params),
        clip: plane_in_model(space.clip, transform),
        source_region,
    })
}

/// The values the shader of `shaded` reads on a surface `transform` places
/// in the scene, seen from `eye` in the light of `space`; `None` when its
/// transform flattens space.
pub(crate) fn shaded_uniforms(
    (transform, shaded): (&Mat4, &Shaded),
    (eye, space): (Vec3, SpaceShading),
) -> Option<SceneShadedUniforms> {
    let model_from_scene = transform.inverse()?;
    let eye = model_from_scene.transform_point(eye);
    Some(SceneShadedUniforms {
        eye: [eye.x, eye.y, eye.z, 1.0],
        light: light_uniform(space.light, |direction| {
            model_from_scene.transform_vector(direction)
        }),
        params: bytemuck::cast(shaded.params),
    })
}

/// The plane `plane` of the scene, in the coordinates of a mesh `transform`
/// places in the scene.
fn plane_in_model(plane: [f32; 4], transform: &Mat4) -> [f32; 4] {
    let rows = transform.rows();
    std::array::from_fn(|column| (0..4).map(|row| plane[row] * rows[row][column]).sum())
}

/// The uniforms of one surface of `scene` drawn on `page`, whose eye is at
/// `eye`; `None` when its transform flattens space.
pub(crate) fn surface_uniforms(
    scene: &SceneDraw,
    surface: &SurfaceShading,
    (page, eye): (ScenePage, Vec3),
) -> Option<SceneSurfaceUniforms> {
    let inverse = surface.transform.inverse()?.rows();
    let normal_row = |row: usize| [inverse[0][row], inverse[1][row], inverse[2][row], 0.0];
    let light = light_uniform(surface.space.light, |direction| direction);
    let columns = |matrix: Mat4| matrix.transpose().rows();
    let clip_from_scene = clip_from_scene(scene, page);
    let (mode, glass) = match surface.material {
        Material::Unlit => (0.0, None),
        // A shaded surface is lit until its shader builds.
        Material::Lit | Material::Shaded(_) => (1.0, None),
        Material::Emissive | Material::Glow | Material::ShadedGlow(_) => (2.0, None),
        Material::Glass(glass) => (0.0, Some(*glass)),
        // A volume takes its own uniforms.
        Material::Volume(_) => (0.0, None),
    };
    let glass_tint = glass.map_or([1.0; 4], |glass| {
        [glass.tint.0, glass.tint.1, glass.tint.2, 1.0]
    });
    Some(SceneSurfaceUniforms {
        clip_from_model: columns(surface.transform.then(&clip_from_scene)),
        scene_from_model: columns(surface.transform),
        light_clip_from_model: columns(
            surface
                .light_projection
                .map_or(Mat4::IDENTITY, |projection| {
                    surface.transform.then(&projection)
                }),
        ),
        clip_from_scene: columns(clip_from_scene),
        normal_rows: [normal_row(0), normal_row(1), normal_row(2)],
        eye: [eye.x, eye.y, eye.z, 1.0],
        light,
        source_region: surface.source_region,
        params: [
            surface.alpha.clamp(0.0, 1.0),
            mode,
            f32::from(u8::from(surface.edges == TextureEdges::Outline)),
            0.0,
        ],
        shadow: [1.0 / SHADOW_MAP_SIZE as f32, SHADOW_DEPTH_BIAS, 0.0, 0.0],
        shadow_soft: penumbra_scale(scene, surface.light_projection),
        glass: [
            glass.map_or(1.0, |glass| 1.0 / glass.index.max(1.0)),
            glass.map_or(0.0, |glass| glass.thickness.max(0.0)),
            page.size.0 as f32,
            page.size.1 as f32,
        ],
        glass_tint,
        backdrop_region: surface.backdrop_region,
        clip: surface.space.clip,
    })
}

/// How wide a penumbra of the scene's light opens, in shadow map
/// coordinates along each axis, a unit of shadow map depth behind its
/// caster: the light's spread over the map's depth range, measured in the
/// map's width and height. Zero for a light of no size.
fn penumbra_scale(scene: &SceneDraw, light_projection: Option<Mat4>) -> [f32; 4] {
    let (Some(light), Some(projection)) = (scene.light, light_projection) else {
        return [0.0; 4];
    };
    if light.size <= 0.0 {
        return [0.0; 4];
    }
    // The projection's rows scale x and y by 2 over the map's width and
    // height, and depth by 1 over its depth range.
    let rows = projection.rows();
    let length = |row: [f32; 4]| Vec3::new(row[0], row[1], row[2]).length();
    let depth = length(rows[2]).max(f32::MIN_POSITIVE);
    let spread = light.size.tan();
    [
        spread * length(rows[0]) * 0.5 / depth,
        spread * length(rows[1]) * 0.5 / depth,
        1.0,
        0.0,
    ]
}

/// The map from `scene` to the clip space of `page`, through the scene's
/// camera: what lies nearer than the camera's near plane falls outside it,
/// and depth grows with distance.
fn clip_from_scene(scene: &SceneDraw, page: ScenePage) -> Mat4 {
    let camera = scene.camera;
    let (scale, focal_length) = (page.scale, camera.focal_length);
    let (width, height) = (page.size.0 as f32, page.size.1 as f32);
    let center_x = scale * (scene.rect.x + scene.rect.width * 0.5) - page.offset[0];
    let center_y = scale * (scene.rect.y + scene.rect.height * 0.5) - page.offset[1];
    let projection = Mat4::from_rows([
        [
            2.0 * scale * focal_length / width,
            0.0,
            2.0 * center_x / width - 1.0,
            0.0,
        ],
        [
            0.0,
            -2.0 * scale * focal_length / height,
            1.0 - 2.0 * center_y / height,
            0.0,
        ],
        [0.0, 0.0, 1.0, -camera.near()],
        [0.0, 0.0, 1.0, 0.0],
    ]);
    camera.view.then(&projection)
}

/// The pixels of `page` a mesh placed by `transform` in `scene` covers,
/// from the corners of its bounds: left, top, right and bottom. `None` when
/// a corner lies behind the eye.
pub(crate) fn surface_page_bounds(
    scene: &SceneDraw,
    transform: &Mat4,
    mesh: &Mesh,
    page: ScenePage,
) -> Option<[f32; 4]> {
    let rows = transform.then(&clip_from_scene(scene, page)).rows();
    let (low, high) = mesh.bounds();
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for corner in 0..8 {
        let pick = |bit: usize, low: f32, high: f32| if corner & bit == 0 { low } else { high };
        let point = [
            pick(1, low.x, high.x),
            pick(2, low.y, high.y),
            pick(4, low.z, high.z),
            1.0,
        ];
        let [x, y, _, w] = rows.map(|row| row.iter().zip(point).map(|(a, b)| a * b).sum::<f32>());
        if w <= 0.0 {
            return None;
        }
        let pixel = [
            (x / w * 0.5 + 0.5) * page.size.0 as f32,
            (0.5 - y / w * 0.5) * page.size.1 as f32,
        ];
        bounds = [
            bounds[0].min(pixel[0]),
            bounds[1].min(pixel[1]),
            bounds[2].max(pixel[0]),
            bounds[3].max(pixel[1]),
        ];
    }
    Some(bounds)
}

// How far `surface`'s bounds reach along the view: drawn far to near, writing follows its wall.
pub(crate) fn view_depth(scene: &SceneDraw, surface: &MeshSurface) -> f32 {
    let (low, high) = surface.mesh.bounds();
    let row = surface.transform.then(&scene.camera.view).rows()[2];
    let far = |at: f32, low: f32, high: f32| (at * low).max(at * high);
    row[3] + far(row[0], low.x, high.x) + far(row[1], low.y, high.y) + far(row[2], low.z, high.z)
}

/// The pixels of `page` the scene and the clip above a surface leave, as a
/// scissor; `None` when they leave none.
pub(crate) fn scene_scissor(
    scene: &SceneDraw,
    clip: Option<Rect>,
    page: ScenePage,
) -> Option<(u32, u32, u32, u32)> {
    let mut rect = scene.rect;
    for cut in [scene.clip, clip].into_iter().flatten() {
        rect = rect.intersect(cut)?;
    }
    let left = (rect.x * page.scale - page.offset[0]).floor().max(0.0);
    let top = (rect.y * page.scale - page.offset[1]).floor().max(0.0);
    let right = ((rect.x + rect.width) * page.scale - page.offset[0])
        .ceil()
        .min(page.size.0 as f32);
    let bottom = ((rect.y + rect.height) * page.scale - page.offset[1])
        .ceil()
        .min(page.size.1 as f32);
    (right > left && bottom > top).then_some((
        left as u32,
        top as u32,
        (right - left) as u32,
        (bottom - top) as u32,
    ))
}
