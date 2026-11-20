//! Reads a glTF 2.0 file into a [`GltfModel`].
//!
//! The reader takes the node tree of the file's scene, one mesh for each primitive, one picture
//! for each material and the keyframes of each animation. It leaves out what the volume does not
//! draw: skins and morph targets, cameras, lights, and every material property but the base color.

use std::{path::Path, sync::Arc};

use cranpose::space::{Mesh, MeshError, Vec3};

use crate::{
    gltf_buffers::Buffers,
    gltf_clip::{Channel, Clip, Interpolation, Track, Trs, normalized_rotation},
    gltf_error::GltfError,
    gltf_material::{Alpha, Texture, bake},
    gltf_model::{Draw, GltfModel, MaterialLayer},
    spatial_gltf_model_source::SpatialGltfModelSource,
};

// Reads the model that `source` holds.
pub(crate) fn import(source: &SpatialGltfModelSource) -> Result<GltfModel, GltfError> {
    let (bytes, base) = source.read()?;
    parse(&bytes, base)
}

// Reads the model in `bytes`. `base` is the directory that relative URIs name files in.
pub(crate) fn parse(bytes: &[u8], base: Option<&Path>) -> Result<GltfModel, GltfError> {
    let gltf::Gltf { document, blob } =
        gltf::Gltf::from_slice(bytes).map_err(|error| GltfError::Invalid(error.to_string()))?;
    let buffers = Buffers::load(&document, blob, base)?;
    let nodes: Vec<gltf::Node<'_>> = document.nodes().collect();
    let scene = Scene::find(&document, &nodes);
    let mut layers = Layers::new(&document);
    let mut draws = Vec::new();
    let mut meshes = Meshes::new(&document);
    for &node in &scene.order {
        let Some(mesh) = nodes[node].mesh() else {
            continue;
        };
        for primitive in mesh.primitives() {
            let material = primitive.material();
            let tex_coord = material
                .pbr_metallic_roughness()
                .base_color_texture()
                .map_or(0, |texture| texture.tex_coord());
            let Some(mesh) = meshes.of(mesh.index(), &primitive, &buffers, tex_coord)? else {
                continue;
            };
            draws.push(Draw {
                node,
                mesh,
                layer: layers.of(&material, &buffers, base)?,
            });
        }
    }
    let clips = clips(&document, &buffers)?;
    let rest = nodes
        .iter()
        .map(|node| {
            let (translation, rotation, scale) = node.transform().decomposed();
            Trs {
                translation,
                rotation: normalized_rotation(rotation),
                scale,
            }
        })
        .collect();
    let names = nodes
        .iter()
        .map(|node| node.name().map(Arc::from))
        .collect();
    let mut model = GltfModel {
        rest,
        parents: scene.parents,
        names,
        order: scene.order,
        draws,
        layers: layers.baked,
        clips,
        bounds: ([0.0; 3], [0.0; 3]),
    };
    model.bounds = bounds_at_rest(&model)?;
    Ok(model)
}

// The nodes that the model's scene reaches.
struct Scene {
    parents: Vec<Option<usize>>,
    // Each node of the scene after its parent
    order: Vec<usize>,
}

impl Scene {
    // The default scene, or the first one, or the nodes that no node holds when the file has no
    // scene.
    fn find(document: &gltf::Document, nodes: &[gltf::Node<'_>]) -> Self {
        let mut parents = vec![None; nodes.len()];
        for node in nodes {
            for child in node.children() {
                parents[child.index()] = Some(node.index());
            }
        }
        let roots: Vec<usize> = match document
            .default_scene()
            .or_else(|| document.scenes().next())
        {
            Some(scene) => scene.nodes().map(|node| node.index()).collect(),
            None => (0..parents.len())
                .filter(|&node| parents[node].is_none())
                .collect(),
        };
        let mut order = Vec::with_capacity(parents.len());
        let mut seen = vec![false; parents.len()];
        let mut stack: Vec<usize> = roots.into_iter().rev().collect();
        let mut children = Vec::new();
        while let Some(node) = stack.pop() {
            // A file whose nodes loop reaches a node twice
            if std::mem::replace(&mut seen[node], true) {
                continue;
            }
            order.push(node);
            // The stack pops the last child first, so the children go on in reverse
            children.clear();
            children.extend(nodes[node].children().map(|child| child.index()));
            stack.extend(children.drain(..).rev());
        }
        Self { parents, order }
    }
}

// What the loader knows of one primitive's mesh.
#[derive(Clone)]
enum Made {
    Not,
    // A primitive of points or lines, or with no triangle, which draws nothing
    Nothing,
    Mesh(Arc<Mesh>),
}

// The meshes made so far, so that nodes that share a primitive share its mesh.
struct Meshes(Vec<Vec<Made>>);

impl Meshes {
    fn new(document: &gltf::Document) -> Self {
        Self(
            document
                .meshes()
                .map(|mesh| vec![Made::Not; mesh.primitives().count()])
                .collect(),
        )
    }

    // The mesh of `primitive`, made on first use, or `None` for a primitive that draws nothing.
    fn of(
        &mut self,
        mesh: usize,
        primitive: &gltf::Primitive<'_>,
        buffers: &Buffers,
        tex_coord: u32,
    ) -> Result<Option<Arc<Mesh>>, GltfError> {
        let slot = &mut self.0[mesh][primitive.index()];
        if matches!(slot, Made::Not) {
            let made = primitive_mesh(primitive, buffers, tex_coord).map_err(|reason| {
                GltfError::Mesh {
                    mesh,
                    primitive: primitive.index(),
                    reason,
                }
            })?;
            *slot = made.map_or(Made::Nothing, |mesh| Made::Mesh(Arc::new(mesh)));
        }
        Ok(match slot {
            Made::Mesh(mesh) => Some(Arc::clone(mesh)),
            Made::Not | Made::Nothing => None,
        })
    }
}

// The mesh of a primitive that draws triangles. A primitive of points or lines draws nothing, and
// so does a primitive with no triangle.
fn primitive_mesh(
    primitive: &gltf::Primitive<'_>,
    buffers: &Buffers,
    tex_coord: u32,
) -> Result<Option<Mesh>, String> {
    use gltf::mesh::Mode;
    let mode = primitive.mode();
    if !matches!(
        mode,
        Mode::Triangles | Mode::TriangleStrip | Mode::TriangleFan
    ) {
        return Ok(None);
    }
    let reader = primitive.reader(|buffer| buffers.data(&buffer));
    let positions: Vec<[f32; 3]> = reader
        .read_positions()
        .ok_or("it has no positions, or they lie outside their buffer")?
        .collect();
    let normals: Option<Vec<[f32; 3]>> = reader.read_normals().map(Iterator::collect);
    let uvs = match reader.read_tex_coords(tex_coord) {
        Some(coords) => coords.into_f32().collect(),
        None => vec![[0.0; 2]; positions.len()],
    };
    let listed: Vec<u32> = match reader.read_indices() {
        Some(indices) => indices.into_u32().collect(),
        None => {
            let count = u32::try_from(positions.len())
                .map_err(|_| "it has more vertices than a mesh can index")?;
            (0..count).collect()
        }
    };
    let indices = match mode {
        Mode::TriangleStrip => strip_triangles(&listed),
        Mode::TriangleFan => fan_triangles(&listed),
        _ => listed,
    };
    match Mesh::new(positions, uvs, normals, indices) {
        Ok(mesh) => Ok(Some(mesh)),
        Err(MeshError::Empty) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

// The triangles of a strip: each index after the second makes one with the two before it, and
// every other triangle turns its first two corners over to keep its winding.
fn strip_triangles(strip: &[u32]) -> Vec<u32> {
    let mut triangles = Vec::with_capacity(strip.len().saturating_sub(2) * 3);
    for (at, corners) in strip.windows(3).enumerate() {
        if at % 2 == 0 {
            triangles.extend([corners[0], corners[1], corners[2]]);
        } else {
            triangles.extend([corners[1], corners[0], corners[2]]);
        }
    }
    triangles
}

// The triangles of a fan: each pair of neighbors after the first index makes one with it.
fn fan_triangles(fan: &[u32]) -> Vec<u32> {
    let Some((&center, rim)) = fan.split_first() else {
        return Vec::new();
    };
    rim.windows(2)
        .flat_map(|pair| [center, pair[0], pair[1]])
        .collect()
}

// The pictures of the materials that primitives use, baked on first use.
struct Layers {
    // The layer of each material, and of the default material after them
    of_material: Vec<Option<usize>>,
    baked: Vec<MaterialLayer>,
}

impl Layers {
    fn new(document: &gltf::Document) -> Self {
        Self {
            of_material: vec![None; document.materials().count() + 1],
            baked: Vec::new(),
        }
    }

    // The index of the layer that shows `material`.
    fn of(
        &mut self,
        material: &gltf::Material<'_>,
        buffers: &Buffers,
        base: Option<&Path>,
    ) -> Result<usize, GltfError> {
        let slot = material.index().unwrap_or(self.of_material.len() - 1);
        if let Some(layer) = self.of_material[slot] {
            return Ok(layer);
        }
        let pbr = material.pbr_metallic_roughness();
        let source = pbr.base_color_texture().map(|info| info.texture().source());
        let texture = match &source {
            Some(image) => Some(decode(image, buffers, base)?),
            None => None,
        };
        let alpha = match material.alpha_mode() {
            gltf::material::AlphaMode::Opaque => Alpha::Opaque,
            gltf::material::AlphaMode::Mask => Alpha::Mask(material.alpha_cutoff().unwrap_or(0.5)),
            gltf::material::AlphaMode::Blend => Alpha::Blend,
        };
        let layer =
            bake(pbr.base_color_factor(), alpha, texture).map_err(|reason| GltfError::Image {
                index: material.index().unwrap_or(0),
                reason,
            })?;
        self.baked.push(layer);
        self.of_material[slot] = Some(self.baked.len() - 1);
        Ok(self.baked.len() - 1)
    }
}

// The pixels of `image`.
fn decode(
    image: &gltf::Image<'_>,
    buffers: &Buffers,
    base: Option<&Path>,
) -> Result<Texture, GltfError> {
    let bytes = buffers.image(image, base)?;
    let decoded = image::load_from_memory(&bytes).map_err(|error| GltfError::Image {
        index: image.index(),
        reason: error.to_string(),
    })?;
    let rgba = decoded.into_rgba8();
    Ok(Texture {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

// The animations of the document.
fn clips(document: &gltf::Document, buffers: &Buffers) -> Result<Vec<Clip>, GltfError> {
    document
        .animations()
        .map(|animation| {
            let index = animation.index();
            let failed = |reason: String| GltfError::Animation { index, reason };
            let mut channels = Vec::new();
            for channel in animation.channels() {
                let reader = channel.reader(|buffer| buffers.data(&buffer));
                let times: Vec<f32> = reader
                    .read_inputs()
                    .ok_or_else(|| failed("a sampler's times lie outside their buffer".into()))?
                    .collect();
                let outputs = reader
                    .read_outputs()
                    .ok_or_else(|| failed("a sampler's values lie outside their buffer".into()))?;
                let interpolation = match channel.sampler().interpolation() {
                    gltf::animation::Interpolation::Linear => Interpolation::Linear,
                    gltf::animation::Interpolation::Step => Interpolation::Step,
                    gltf::animation::Interpolation::CubicSpline => Interpolation::CubicSpline,
                };
                use gltf::animation::util::ReadOutputs;
                let keyed = match outputs {
                    ReadOutputs::Translations(values) => {
                        Track::new(times, values.collect(), interpolation).map(Channel::Translation)
                    }
                    ReadOutputs::Rotations(values) => {
                        Track::new(times, values.into_f32().collect(), interpolation)
                            .map(Channel::Rotation)
                    }
                    ReadOutputs::Scales(values) => {
                        Track::new(times, values.collect(), interpolation).map(Channel::Scale)
                    }
                    // The model has no morph targets to weigh
                    ReadOutputs::MorphTargetWeights(_) => continue,
                };
                channels.push((channel.target().node().index(), keyed.map_err(failed)?));
            }
            Ok(Clip::new(animation.name().map(str::to_owned), channels))
        })
        .collect()
}

// The box around the meshes in the nodes' rest pose, or an error when there is nothing to draw.
fn bounds_at_rest(model: &GltfModel) -> Result<([f32; 3], [f32; 3]), GltfError> {
    let (mut locals, mut worlds) = (Vec::new(), Vec::new());
    model.world_transforms([], &mut locals, &mut worlds);
    let mut low = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut high = -low;
    for draw in &model.draws {
        for &position in draw.mesh.positions() {
            let point = worlds[draw.node].transform_point(Vec3::from_array(position));
            (low, high) = (low.min(point), high.max(point));
        }
    }
    if model.draws.is_empty() || !low.to_array().iter().all(|value| value.is_finite()) {
        return Err(GltfError::Empty);
    }
    Ok((low.to_array(), high.to_array()))
}

#[cfg(test)]
#[path = "tests/gltf_import_tests.rs"]
mod tests;
