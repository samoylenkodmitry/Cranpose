// The index lists of the meshes whose points lie in a grid, row by row.

// A point's place in a mesh, as the `u32` the mesh holds.
pub(super) fn vertex(index: usize) -> u32 {
    u32::try_from(index).expect("a mesh has fewer vertices than a u32 counts")
}

// The triangles of a grid of `columns` by `rows` points. Each cell takes
// the top left, bottom left and top right corners, then the top right,
// bottom left and bottom right corners.
pub(super) fn quads(columns: usize, rows: usize) -> Vec<u32> {
    let stride = vertex(columns);
    let mut indices = Vec::with_capacity((columns - 1) * (rows - 1) * 6);
    for row in 0..vertex(rows - 1) {
        for column in 0..vertex(columns - 1) {
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
