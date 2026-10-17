use std::{collections::HashMap, fmt};

use super::mesh::{Mesh, MeshError, smooth_normals};

/// Why the text given to [`Mesh::from_obj`] does not make a mesh.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjError {
    /// The line the problem is on, counted from one; zero when it is about
    /// the file as a whole.
    pub line: usize,
    /// What is wrong.
    pub kind: ObjErrorKind,
}

/// What is wrong in an OBJ file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjErrorKind {
    /// A number does not parse.
    BadNumber,
    /// A statement has too few numbers, or a face too few corners.
    TooFewValues,
    /// A face names a position, texture coordinate or normal that the file
    /// has not given before it.
    IndexOutOfRange,
    /// The parts do not make a mesh.
    Mesh(MeshError),
}

impl fmt::Display for ObjError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match &self.kind {
            ObjErrorKind::BadNumber => "a number does not parse",
            ObjErrorKind::TooFewValues => "too few values",
            ObjErrorKind::IndexOutOfRange => "a face names a vertex that is not given before it",
            ObjErrorKind::Mesh(error) => return write!(f, "OBJ file: {error}"),
        };
        write!(f, "OBJ line {}: {what}", self.line)
    }
}

impl std::error::Error for ObjError {}

#[derive(Default)]
struct Builder {
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    normals: Vec<[f32; 3]>,
    corners: HashMap<(usize, Option<usize>, Option<usize>), u32>,
    out_positions: Vec<[f32; 3]>,
    out_uvs: Vec<[f32; 2]>,
    out_normals: Vec<Option<[f32; 3]>>,
    indices: Vec<u32>,
}

impl Mesh {
    /// A mesh read from the text of a Wavefront OBJ file.
    ///
    /// It reads the positions (`v`), texture coordinates (`vt`), normals
    /// (`vn`) and faces (`f`, split into triangles) of every object in the
    /// file, and skips the other statements, such as materials and groups.
    /// OBJ counts the texture's v from the bottom; the mesh flips it, so a
    /// model textured for an image shows a composable upright. A face
    /// without texture coordinates takes `(0, 0)`, and one without normals
    /// takes smooth ones.
    pub fn from_obj(source: &str) -> Result<Self, ObjError> {
        let mut builder = Builder::default();
        for (number, line) in source.lines().enumerate() {
            let line = line.split('#').next().unwrap_or_default();
            let mut words = line.split_whitespace();
            let fail = |kind| ObjError {
                line: number + 1,
                kind,
            };
            match words.next() {
                Some("v") => {
                    let [x, y, z] = numbers(&mut words).map_err(fail)?;
                    builder.positions.push([x, y, z]);
                }
                Some("vt") => {
                    let u = number_of(words.next()).map_err(fail)?;
                    let v = words
                        .next()
                        .map_or(Ok(0.0), |word| number_of(Some(word)))
                        .map_err(fail)?;
                    builder.uvs.push([u, 1.0 - v]);
                }
                Some("vn") => {
                    let [x, y, z] = numbers(&mut words).map_err(fail)?;
                    builder.normals.push([x, y, z]);
                }
                Some("f") => builder.face(words).map_err(fail)?,
                _ => {}
            }
        }
        builder.finish()
    }
}

impl Builder {
    /// Splits a face into a fan of triangles around its first corner.
    fn face<'a>(&mut self, words: impl Iterator<Item = &'a str>) -> Result<(), ObjErrorKind> {
        let mut corners = 0usize;
        let (mut first, mut previous) = (0, 0);
        for word in words {
            let corner = self.corner(word)?;
            match corners {
                0 => first = corner,
                1 => {}
                _ => self.indices.extend([first, previous, corner]),
            }
            previous = corner;
            corners += 1;
        }
        if corners < 3 {
            return Err(ObjErrorKind::TooFewValues);
        }
        Ok(())
    }

    fn corner(&mut self, word: &str) -> Result<u32, ObjErrorKind> {
        let mut parts = word.split('/');
        let position =
            resolve(parts.next(), self.positions.len())?.ok_or(ObjErrorKind::TooFewValues)?;
        let uv = resolve(parts.next(), self.uvs.len())?;
        let normal = resolve(parts.next(), self.normals.len())?;
        if let Some(&index) = self.corners.get(&(position, uv, normal)) {
            return Ok(index);
        }
        let index = self.out_positions.len() as u32;
        self.out_positions.push(self.positions[position]);
        self.out_uvs.push(uv.map_or([0.0, 0.0], |uv| self.uvs[uv]));
        self.out_normals
            .push(normal.map(|normal| self.normals[normal]));
        self.corners.insert((position, uv, normal), index);
        Ok(index)
    }

    fn finish(self) -> Result<Mesh, ObjError> {
        let fail = |error| ObjError {
            line: 0,
            kind: ObjErrorKind::Mesh(error),
        };
        if self.indices.is_empty() {
            return Err(fail(MeshError::Empty));
        }
        let smooth = self
            .out_normals
            .iter()
            .any(Option::is_none)
            .then(|| smooth_normals(&self.out_positions, &self.indices));
        let normals = self
            .out_normals
            .iter()
            .enumerate()
            .map(|(index, normal)| match (normal, &smooth) {
                (Some(normal), _) => unit(*normal),
                (None, Some(smooth)) => smooth[index],
                (None, None) => [0.0, 0.0, -1.0],
            })
            .collect();
        Mesh::new(
            self.out_positions,
            self.out_uvs,
            Some(normals),
            self.indices,
        )
        .map_err(fail)
    }
}

fn unit(normal: [f32; 3]) -> [f32; 3] {
    super::math::Vec3::from_array(normal)
        .normalized()
        .map_or([0.0, 0.0, -1.0], super::math::Vec3::to_array)
}

/// The zero-based index an OBJ index names among `count` given so far:
/// counted from one, or back from the last when negative. `None` for an
/// empty part, as in `1//3`.
fn resolve(part: Option<&str>, count: usize) -> Result<Option<usize>, ObjErrorKind> {
    let Some(part) = part.filter(|part| !part.is_empty()) else {
        return Ok(None);
    };
    let index: i64 = part.parse().map_err(|_| ObjErrorKind::BadNumber)?;
    let resolved = match index {
        1.. => index - 1,
        ..0 => count as i64 + index,
        0 => return Err(ObjErrorKind::IndexOutOfRange),
    };
    usize::try_from(resolved)
        .ok()
        .filter(|&resolved| resolved < count)
        .map(Some)
        .ok_or(ObjErrorKind::IndexOutOfRange)
}

fn number_of(word: Option<&str>) -> Result<f32, ObjErrorKind> {
    word.ok_or(ObjErrorKind::TooFewValues)?
        .parse()
        .map_err(|_| ObjErrorKind::BadNumber)
}

fn numbers<'a>(words: &mut impl Iterator<Item = &'a str>) -> Result<[f32; 3], ObjErrorKind> {
    Ok([
        number_of(words.next())?,
        number_of(words.next())?,
        number_of(words.next())?,
    ])
}
