//! The GL state a command buffer's commands have set, so that a command
//! setting what the context already holds makes no GL call.
//!
//! On WebGL every GL call crosses from WebAssembly into JavaScript and is
//! validated and serialized for the browser's GPU process, which decodes and
//! validates it again. The encoder re-specifies every vertex attribute of a
//! pipeline at each draw whose buffers or first instance moved, disables them
//! all at each pipeline switch, and binds textures, uniform buffers and the
//! scissor again for every batch; most of those calls set what is already set.
//!
//! The state lives for one command buffer: the queue resets the context's
//! state before each one, and the device's own calls (buffer and texture
//! uploads) run between them. Any command the tracker does not know forgets
//! everything it holds, after it has made the context agree with the commands
//! it deferred.

use core::ops::Range;

use glow::HasContext;

use super::{
    AttributeDesc, Command as C, DepthState, StencilOps, VertexAttribKind, VertexBufferDesc,
    MAX_TEXTURE_SLOTS, MAX_VERTEX_ATTRIBUTES,
};

/// Uniform buffer bindings tracked by slot; a slot above them is bound
/// every time.
const TRACKED_UNIFORM_SLOTS: usize = 16;

/// Where one attribute reads its data, as `glVertexAttrib[I]Pointer` set it.
#[derive(Clone, Copy, PartialEq)]
struct AttributePointer {
    buffer: glow::Buffer,
    size: i32,
    format: u32,
    integer: bool,
    stride: i32,
    offset: i32,
}

#[derive(Clone, Copy, Default)]
struct Attribute {
    enabled: Option<bool>,
    divisor: Option<u32>,
    pointer: Option<AttributePointer>,
}

#[derive(Clone, PartialEq)]
struct BoundTexture {
    target: u32,
    texture: glow::Texture,
    aspects: crate::FormatAspects,
    mip_levels: Range<u32>,
}

#[derive(Clone, Copy, PartialEq)]
struct StencilFunc {
    function: u32,
    reference: u32,
    read_mask: u32,
}

#[derive(Clone, PartialEq)]
struct StencilWrite {
    write_mask: u32,
    ops: StencilOps,
}

/// The tracked state; `None` is a value the tracker does not know.
#[derive(Default)]
pub(super) struct GlState {
    array_buffer: Option<glow::Buffer>,
    attributes: [Attribute; MAX_VERTEX_ATTRIBUTES],
    /// Attributes an `UnsetVertexAttribute` turned off, which stay enabled
    /// until a draw or an untracked command needs the context to agree:
    /// the next pipeline usually enables most of them again.
    pending_disables: u32,
    program: Option<glow::Program>,
    scissor: Option<(i32, i32, i32, i32)>,
    scissor_test: Option<bool>,
    depth_test: Option<bool>,
    stencil_test: Option<bool>,
    depth: Option<(u32, bool)>,
    /// Front and back faces.
    stencil_funcs: [Option<StencilFunc>; 2],
    stencil_writes: [Option<StencilWrite>; 2],
    uniform_buffers: [Option<(glow::Buffer, i32, i32)>; TRACKED_UNIFORM_SLOTS],
    active_texture: Option<u32>,
    textures: [Option<BoundTexture>; MAX_TEXTURE_SLOTS],
    samplers: [Option<Option<glow::Sampler>>; MAX_TEXTURE_SLOTS],
}

impl GlState {
    /// Runs `command` when the tracker owns it and returns `true`; otherwise
    /// makes the context agree with what the tracker deferred, forgets what
    /// the command may change, and returns `false` for the queue to run it.
    pub(super) unsafe fn apply(&mut self, gl: &glow::Context, command: &C) -> bool {
        match *command {
            C::SetVertexAttribute {
                buffer: Some(buffer),
                ref buffer_desc,
                ref attribute_desc,
            } => unsafe { self.set_attribute(gl, buffer, buffer_desc, attribute_desc) },
            C::UnsetVertexAttribute(location) => {
                self.pending_disables |= 1 << location;
            }
            C::SetProgram(program) => {
                if self.program != Some(program) {
                    unsafe { gl.use_program(Some(program)) };
                    self.program = Some(program);
                }
            }
            C::SetScissor(ref rect) => {
                let scissor = (rect.x, rect.y, rect.w, rect.h);
                if self.scissor != Some(scissor) {
                    unsafe { gl.scissor(rect.x, rect.y, rect.w, rect.h) };
                    self.scissor = Some(scissor);
                }
                unsafe { Self::toggle(gl, &mut self.scissor_test, glow::SCISSOR_TEST, true) };
            }
            C::ConfigureDepthStencil(aspects) => unsafe {
                let depth = aspects.contains(crate::FormatAspects::DEPTH);
                let stencil = aspects.contains(crate::FormatAspects::STENCIL);
                Self::toggle(gl, &mut self.depth_test, glow::DEPTH_TEST, depth);
                Self::toggle(gl, &mut self.stencil_test, glow::STENCIL_TEST, stencil);
            },
            C::SetDepth(DepthState { function, mask }) => {
                if self.depth != Some((function, mask)) {
                    unsafe { gl.depth_func(function) };
                    unsafe { gl.depth_mask(mask) };
                    self.depth = Some((function, mask));
                }
            }
            C::SetStencilFunc {
                face,
                function,
                reference,
                read_mask,
            } => {
                let func = StencilFunc {
                    function,
                    reference,
                    read_mask,
                };
                if Self::stencil_faces(face).any(|side| self.stencil_funcs[side] != Some(func)) {
                    unsafe {
                        gl.stencil_func_separate(face, function, reference as i32, read_mask)
                    };
                    for side in Self::stencil_faces(face) {
                        self.stencil_funcs[side] = Some(func);
                    }
                }
            }
            C::SetStencilOps {
                face,
                write_mask,
                ref ops,
            } => {
                let write = StencilWrite {
                    write_mask,
                    ops: ops.clone(),
                };
                if Self::stencil_faces(face)
                    .any(|side| self.stencil_writes[side].as_ref() != Some(&write))
                {
                    unsafe { gl.stencil_mask_separate(face, write_mask) };
                    unsafe { gl.stencil_op_separate(face, ops.fail, ops.depth_fail, ops.pass) };
                    for side in Self::stencil_faces(face) {
                        self.stencil_writes[side] = Some(write.clone());
                    }
                }
            }
            C::BindBuffer {
                target: glow::UNIFORM_BUFFER,
                slot,
                buffer,
                offset,
                size,
            } if (slot as usize) < TRACKED_UNIFORM_SLOTS => {
                let binding = Some((buffer, offset, size));
                if self.uniform_buffers[slot as usize] != binding {
                    unsafe {
                        gl.bind_buffer_range(glow::UNIFORM_BUFFER, slot, Some(buffer), offset, size)
                    };
                    self.uniform_buffers[slot as usize] = binding;
                }
            }
            C::BindSampler(unit, sampler) => {
                if self.samplers[unit as usize] != Some(sampler) {
                    unsafe { gl.bind_sampler(unit, sampler) };
                    self.samplers[unit as usize] = Some(sampler);
                }
            }
            C::BindTexture {
                slot,
                texture,
                target,
                aspects,
                ref mip_levels,
            } => unsafe {
                self.bind_texture(
                    gl,
                    slot,
                    BoundTexture {
                        target,
                        texture,
                        aspects,
                        mip_levels: mip_levels.clone(),
                    },
                )
            },
            // Bindings of other targets and slots, and image units.
            C::BindBuffer { .. } | C::BindImage { .. } => return false,
            // Draws read the attributes: the deferred disables land first.
            C::Draw { .. }
            | C::DrawIndexed { .. }
            | C::DrawIndirect { .. }
            | C::DrawIndexedIndirect { .. } => {
                unsafe { self.disable_pending(gl) };
                return false;
            }
            // State the tracker holds none of.
            C::SetIndexBuffer(_)
            | C::SetViewport { .. }
            | C::SetDepthBias(_)
            | C::SetAlphaToCoverage(_)
            | C::SetPrimitive(_)
            | C::SetBlendConstant(_)
            | C::SetColorTarget { .. }
            | C::SetImmediates { .. }
            | C::SetClipDistances { .. }
            | C::InsertDebugMarker(_)
            | C::PushDebugGroup(_)
            | C::PopDebugGroup => return false,
            _ => {
                unsafe { self.forget(gl) };
                return false;
            }
        }
        true
    }

    /// Makes the context agree with the deferred disables at the end of a
    /// command buffer, which leaves the attributes as the encoder did.
    pub(super) unsafe fn finish(&mut self, gl: &glow::Context) {
        unsafe { self.disable_pending(gl) };
    }

    unsafe fn forget(&mut self, gl: &glow::Context) {
        unsafe { self.disable_pending(gl) };
        *self = Self::default();
    }

    unsafe fn disable_pending(&mut self, gl: &glow::Context) {
        let mut pending = core::mem::take(&mut self.pending_disables);
        while pending != 0 {
            let location = pending.trailing_zeros();
            pending &= pending - 1;
            let attribute = &mut self.attributes[location as usize];
            if attribute.enabled != Some(false) {
                unsafe { gl.disable_vertex_attrib_array(location) };
                attribute.enabled = Some(false);
            }
        }
    }

    unsafe fn set_attribute(
        &mut self,
        gl: &glow::Context,
        buffer: glow::Buffer,
        buffer_desc: &VertexBufferDesc,
        attribute_desc: &AttributeDesc,
    ) {
        let location = attribute_desc.location;
        self.pending_disables &= !(1 << location);
        let format = &attribute_desc.format_desc;
        let pointer = AttributePointer {
            buffer,
            size: format.element_count,
            format: format.element_format,
            integer: matches!(format.attrib_kind, VertexAttribKind::Integer),
            stride: buffer_desc.stride as i32,
            offset: attribute_desc.offset as i32,
        };
        let divisor = buffer_desc.step as u32;
        let attribute = &mut self.attributes[location as usize];
        if attribute.enabled != Some(true) {
            unsafe { gl.enable_vertex_attrib_array(location) };
            attribute.enabled = Some(true);
        }
        if attribute.pointer != Some(pointer) {
            if self.array_buffer != Some(buffer) {
                unsafe { gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer)) };
                self.array_buffer = Some(buffer);
            }
            if pointer.integer {
                unsafe {
                    gl.vertex_attrib_pointer_i32(
                        location,
                        pointer.size,
                        pointer.format,
                        pointer.stride,
                        pointer.offset,
                    )
                };
            } else {
                unsafe {
                    gl.vertex_attrib_pointer_f32(
                        location,
                        pointer.size,
                        pointer.format,
                        true, // always normalized
                        pointer.stride,
                        pointer.offset,
                    )
                };
            }
            attribute.pointer = Some(pointer);
        }
        if attribute.divisor != Some(divisor) {
            unsafe { gl.vertex_attrib_divisor(location, divisor) };
            attribute.divisor = Some(divisor);
        }
    }

    unsafe fn bind_texture(&mut self, gl: &glow::Context, slot: u32, bound: BoundTexture) {
        if self.textures[slot as usize].as_ref() == Some(&bound) {
            return;
        }
        if self.active_texture != Some(slot) {
            unsafe { gl.active_texture(glow::TEXTURE0 + slot) };
            self.active_texture = Some(slot);
        }
        let BoundTexture {
            target,
            texture,
            aspects,
            ref mip_levels,
        } = bound;
        unsafe { gl.bind_texture(target, Some(texture)) };
        unsafe { gl.tex_parameter_i32(target, glow::TEXTURE_BASE_LEVEL, mip_levels.start as i32) };
        unsafe {
            gl.tex_parameter_i32(target, glow::TEXTURE_MAX_LEVEL, (mip_levels.end - 1) as i32)
        };

        let version = gl.version();
        let is_min_es_3_1 = version.is_embedded && (version.major, version.minor) >= (3, 1);
        let is_min_4_3 = !version.is_embedded && (version.major, version.minor) >= (4, 3);
        if is_min_es_3_1 || is_min_4_3 {
            let mode = match aspects {
                crate::FormatAspects::DEPTH => Some(glow::DEPTH_COMPONENT),
                crate::FormatAspects::STENCIL => Some(glow::STENCIL_INDEX),
                _ => None,
            };
            if let Some(mode) = mode {
                unsafe { gl.tex_parameter_i32(target, glow::DEPTH_STENCIL_TEXTURE_MODE, mode as _) };
            }
        }
        // The levels and mode just set belong to the texture: another unit
        // that holds it may now hold it with other ones.
        for other in self.textures.iter_mut() {
            if other.as_ref().is_some_and(|other| other.texture == texture) {
                *other = None;
            }
        }
        self.textures[slot as usize] = Some(bound);
    }

    unsafe fn toggle(gl: &glow::Context, known: &mut Option<bool>, capability: u32, on: bool) {
        if *known != Some(on) {
            if on {
                unsafe { gl.enable(capability) };
            } else {
                unsafe { gl.disable(capability) };
            }
            *known = Some(on);
        }
    }

    /// The faces, front (0) and back (1), a stencil call for `face` sets.
    fn stencil_faces(face: u32) -> impl Iterator<Item = usize> {
        let (front, back) = match face {
            glow::FRONT => (true, false),
            glow::BACK => (false, true),
            _ => (true, true),
        };
        [front.then_some(0), back.then_some(1)].into_iter().flatten()
    }
}
