use crate::render_effect::{RuntimeShader, RuntimeShaderUniformError};

const RUNTIME_SHADER_INLINE_UNIFORMS: usize = 16;

// Compared through `as_slice`: the inline slots keep stale values once the
// uniforms move to the heap.
#[derive(Clone, Debug)]
pub(crate) struct RuntimeShaderUniforms {
    len: usize,
    inline: [f32; RUNTIME_SHADER_INLINE_UNIFORMS],
    heap: Option<Vec<f32>>,
}

impl RuntimeShaderUniforms {
    pub(crate) fn new() -> Self {
        Self {
            len: 0,
            inline: [0.0; RUNTIME_SHADER_INLINE_UNIFORMS],
            heap: None,
        }
    }

    pub(crate) fn as_slice(&self) -> &[f32] {
        if let Some(heap) = &self.heap {
            heap.as_slice()
        } else {
            &self.inline[..self.len]
        }
    }

    fn ensure_len(&mut self, min_len: usize) {
        if let Some(heap) = &mut self.heap {
            if heap.len() < min_len {
                heap.resize(min_len, 0.0);
            }
            return;
        }

        if min_len <= RUNTIME_SHADER_INLINE_UNIFORMS {
            self.len = self.len.max(min_len);
            return;
        }

        let mut heap = Vec::with_capacity(min_len);
        heap.extend_from_slice(&self.inline[..self.len]);
        heap.resize(min_len, 0.0);
        self.heap = Some(heap);
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [f32] {
        match &mut self.heap {
            Some(heap) => heap.as_mut_slice(),
            None => &mut self.inline[..self.len],
        }
    }

    // Glass sets about 90 uniforms an effect on each frame it moves, all inside
    // the stored slots, which grow only through the range check below.
    pub(crate) fn try_write<const N: usize>(
        &mut self,
        index: usize,
        values: [f32; N],
    ) -> Result<(), RuntimeShaderUniformError> {
        loop {
            let stored = self.as_mut_slice();
            if let Some(slots) = stored.get_mut(index..).and_then(<[f32]>::first_chunk_mut) {
                *slots = values;
                return Ok(());
            }
            self.try_grow(index, N)?;
        }
    }

    #[cold]
    #[inline(never)]
    fn try_grow(&mut self, index: usize, width: usize) -> Result<(), RuntimeShaderUniformError> {
        let min_len = index
            .checked_add(width)
            .filter(|&min_len| min_len <= RuntimeShader::MAX_USER_UNIFORMS)
            .ok_or(RuntimeShaderUniformError::OutOfUserRange {
                index,
                width,
                max_user_uniforms: RuntimeShader::MAX_USER_UNIFORMS,
                reserved_start: RuntimeShader::RESERVED_UNIFORM_START,
                max_uniforms: RuntimeShader::MAX_UNIFORMS,
            })?;
        self.ensure_len(min_len);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn is_inline(&self) -> bool {
        self.heap.is_none()
    }
}
