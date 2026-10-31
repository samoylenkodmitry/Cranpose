use cranpose_ui_graphics::{ShapeRecordBody, ShapeRecordCurve};

// The stroke width, flags, brush and placement are one `Uint32x4`, which
// the shader splits again (`ShapeRecordInput`).
const BODY_ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Float32x4, 2 => Float32x4, 3 => Uint32x4, 4 => Float32x4
];
const CURVE_ATTRIBUTES: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
    1 => Float32x4, 5 => Float32x4
];

pub(crate) fn record_vertex_layouts() -> [wgpu::VertexBufferLayout<'static>; 2] {
    [
        wgpu::VertexBufferLayout {
            array_stride: size_of::<ShapeRecordBody>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &BODY_ATTRIBUTES,
        },
        wgpu::VertexBufferLayout {
            array_stride: size_of::<ShapeRecordCurve>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &CURVE_ATTRIBUTES,
        },
    ]
}
