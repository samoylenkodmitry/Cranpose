//! Reads a glTF 2.0 file into a [`GltfModel`].
//!
//! The reader takes the node tree of the file's scene, one mesh for each primitive, one picture
//! for each material and the keyframes of each animation. A skinned primitive whose every vertex
//! follows one joint becomes one mesh for each joint (see `gltf_skin`). It leaves out what the
//! volume does not draw: other skins and morph targets, cameras, lights, and every material
//! property but the base color.

use std::{path::Path, sync::Arc};

use cranpose::space::{Mesh, MeshError, Vec3};

use crate::{
    controller_parts::ControllerParts,
    gltf_buffers::Buffers,
    gltf_clip::{Channel, Clip, Interpolation, Track, Trs, normalized_rotation},
    gltf_error::GltfError,
    gltf_ktx2,
    gltf_material::{Alpha, Texture, bake},
    gltf_model::{Draw, GltfModel, MaterialLayer},
    gltf_skin::{Pieces, rigid_pieces},
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
            let placed = meshes.placed(
                &nodes[node],
                &primitive,
                &buffers,
                tex_coord,
                &scene.reached,
            )?;
            if placed.is_empty() {
                continue;
            }
            let layer = layers.of(&document, &material, &buffers, base)?;
            draws.extend(
                placed
                    .into_iter()
                    .map(|(node, mesh)| Draw { node, mesh, layer }),
            );
        }
    }
    let clips = clips(&document, &buffers)?;
    let rest: Vec<Trs> = nodes
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
    let names: Vec<Option<Arc<str>>> = nodes
        .iter()
        .map(|node| node.name().map(Arc::from))
        .collect();
    let parts = ControllerParts::find(&names, &rest, &clips);
    let mut model = GltfModel {
        rest,
        parents: scene.parents,
        names,
        order: scene.order,
        draws,
        layers: layers.baked,
        clips,
        parts,
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
    // Whether the scene reaches each node
    reached: Vec<bool>,
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
        Self {
            parents,
            order,
            reached: seen,
        }
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

    // The meshes that draw `primitive` of `node`, each with the node that places it: the pieces of
    // a rigid skin on their joints, or else the primitive's one mesh on `node`. Empty for a
    // primitive that draws nothing.
    fn placed(
        &mut self,
        node: &gltf::Node<'_>,
        primitive: &gltf::Primitive<'_>,
        buffers: &Buffers,
        tex_coord: u32,
        reached: &[bool],
    ) -> Result<Pieces, GltfError> {
        let Some(mesh) = node.mesh() else {
            return Ok(Pieces::new());
        };
        if let Some(skin) = node.skin() {
            let pieces =
                rigid_pieces(&skin, primitive, buffers, tex_coord, reached).map_err(|reason| {
                    GltfError::Mesh {
                        mesh: mesh.index(),
                        primitive: primitive.index(),
                        reason,
                    }
                })?;
            if let Some(pieces) = pieces {
                return Ok(pieces);
            }
        }
        let made = self.of(mesh.index(), primitive, buffers, tex_coord)?;
        Ok(made.map(|made| (node.index(), made)).into_iter().collect())
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

// The vertices and the triangles of a primitive, three corners to a triangle.
pub(crate) struct Triangles {
    pub(crate) positions: Vec<[f32; 3]>,
    pub(crate) uvs: Vec<[f32; 2]>,
    pub(crate) normals: Option<Vec<[f32; 3]>>,
    pub(crate) indices: Vec<u32>,
}

impl Triangles {
    // The mesh of the triangles, or `None` when there is no triangle.
    pub(crate) fn into_mesh(self) -> Result<Option<Mesh>, String> {
        match Mesh::new(self.positions, self.uvs, self.normals, self.indices) {
            Ok(mesh) => Ok(Some(mesh)),
            Err(MeshError::Empty) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }
}

// The mesh of a primitive that draws triangles. A primitive of points or lines draws nothing, and
// so does a primitive with no triangle.
fn primitive_mesh(
    primitive: &gltf::Primitive<'_>,
    buffers: &Buffers,
    tex_coord: u32,
) -> Result<Option<Mesh>, String> {
    match primitive_triangles(primitive, buffers, tex_coord)? {
        Some(triangles) => triangles.into_mesh(),
        None => Ok(None),
    }
}

// The triangles of a primitive, or `None` for a primitive of points or lines.
pub(crate) fn primitive_triangles(
    primitive: &gltf::Primitive<'_>,
    buffers: &Buffers,
    tex_coord: u32,
) -> Result<Option<Triangles>, String> {
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
    Ok(Some(Triangles {
        positions,
        uvs,
        normals,
        indices,
    }))
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
        document: &gltf::Document,
        material: &gltf::Material<'_>,
        buffers: &Buffers,
        base: Option<&Path>,
    ) -> Result<usize, GltfError> {
        let slot = material.index().unwrap_or(self.of_material.len() - 1);
        if let Some(layer) = self.of_material[slot] {
            return Ok(layer);
        }
        let pbr = material.pbr_metallic_roughness();
        let texture = match pbr.base_color_texture() {
            Some(info) => Some(decode(document, &info.texture(), buffers, base)?),
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

// The image that `texture` shows: the one that its `KHR_texture_basisu` extension names, which the
// extension allows in place of the texture's own source, or else the texture's source.
fn image_of<'a>(
    document: &'a gltf::Document,
    texture: &gltf::Texture<'a>,
) -> Result<gltf::Image<'a>, GltfError> {
    let named = texture
        .extension_value("KHR_texture_basisu")
        .map(|extension| extension.get("source").and_then(gltf::json::Value::as_u64));
    let image = match named {
        Some(Some(index)) => usize::try_from(index)
            .ok()
            .and_then(|index| document.images().nth(index)),
        Some(None) => None,
        None => texture.source(),
    };
    image.ok_or_else(|| {
        GltfError::Invalid(format!(
            "texture {} names no image that the file holds",
            texture.index()
        ))
    })
}

// The pixels of the image of `texture`.
fn decode(
    document: &gltf::Document,
    texture: &gltf::Texture<'_>,
    buffers: &Buffers,
    base: Option<&Path>,
) -> Result<Texture, GltfError> {
    let image = image_of(document, texture)?;
    let bytes = buffers.image(&image, base)?;
    let failed = |reason: String| GltfError::Image {
        index: image.index(),
        reason,
    };
    if gltf_ktx2::is_ktx2(&bytes) {
        return gltf_ktx2::decode(&bytes).map_err(failed);
    }
    let decoded = image::load_from_memory(&bytes).map_err(|error| failed(error.to_string()))?;
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
