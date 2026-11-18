// The index lists of the meshes whose points lie in a grid, row by row.

use super::whole;

// The triangles of a grid of `columns` by `rows` points. Each cell takes
// the top left, bottom left and top right corners, then the top right,
// bottom left and bottom right corners.
pub(super) fn quads(columns: usize, rows: usize) -> Vec<u32> {
    let stride = whole::narrow(columns);
    let mut indices = Vec::with_capacity((columns - 1) * (rows - 1) * 6);
    for row in 0..whole::narrow(rows - 1) {
        for column in 0..stride - 1 {
            let at = row * stride + column;
            indices.extend_from_slice(&[
                at,
                at + stride,
                at + 1,
                at + 1,
                at + stride,
                at + stride + 1,
            ]);
        }
    }
    indices
}
