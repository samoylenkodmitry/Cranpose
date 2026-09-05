use cranpose_ui_graphics::{ShapeRecordBody, ShapeRecordCurve};

const BODY_ATTRIBUTES: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
    0 => Float32x4, 2 => Float32x4, 3 => Float32, 4 => Uint32,
    5 => Uint32, 6 => Uint32, 7 => Float32x4
];
const CURVE_ATTRIBUTES: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
    1 => Float32x4, 8 => Float32x4
];

pub(crate) fn record_vertex_layouts() -> [wgpu::VertexBufferLayout<'static>; 2] {
    [
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ShapeRecordBody>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &BODY_ATTRIBUTES,
        },
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ShapeRecordCurve>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &CURVE_ATTRIBUTES,
        },
    ]
}
