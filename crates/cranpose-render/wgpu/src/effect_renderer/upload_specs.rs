use cranpose_ui_graphics::RuntimeShader;

use super::{
    BlitUniforms, BlurUniforms, OffsetUniforms, ProjectiveBlitUniforms, ProjectiveBlitVertex,
};
use crate::frame_graph::{UploadAllocatorId, UploadAllocatorSpec};

pub(super) fn blur_uniform_spec(pass: UploadAllocatorId) -> UploadAllocatorSpec {
    let (buffer, bind_group) = match pass {
        UploadAllocatorId::BlurDownsample => (
            "Blur Downsample Uniform Buffer",
            "Blur Downsample Uniform Bind Group",
        ),
        UploadAllocatorId::BlurHorizontal => (
            "Blur Horizontal Uniform Buffer",
            "Blur Horizontal Uniform Bind Group",
        ),
        _ => (
            "Blur Vertical Uniform Buffer",
            "Blur Vertical Uniform Bind Group",
        ),
    };
    UploadAllocatorSpec::uniform(buffer, bind_group, size_of::<BlurUniforms>() as u64)
}

pub(super) fn offset_uniform_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::uniform(
        "Offset Uniform Buffer",
        "Offset Uniform Bind Group",
        size_of::<OffsetUniforms>() as u64,
    )
}

pub(super) fn blit_uniform_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::uniform(
        "Blit Uniform Buffer",
        "Blit Uniform Bind Group",
        size_of::<BlitUniforms>() as u64,
    )
}

pub(super) fn projective_blit_uniform_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::uniform(
        "Projective Blit Uniform Buffer",
        "Projective Blit Uniform Bind Group",
        size_of::<ProjectiveBlitUniforms>() as u64,
    )
}

pub(super) fn projective_blit_vertex_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::vertex(
        "Projective Blit Vertex Buffer",
        (size_of::<ProjectiveBlitVertex>() * 5) as u64,
    )
}

pub(super) fn effect_uniform_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::uniform(
        "Effect Uniform Buffer",
        "Effect Uniform Bind Group",
        (RuntimeShader::MAX_UNIFORMS * size_of::<f32>()) as u64,
    )
}
