const RUNTIME_SHADER_INLINE_UNIFORMS: usize = 16;

#[derive(Clone, Debug, PartialEq)]
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

    pub(crate) fn len(&self) -> usize {
        self.as_slice().len()
    }

    pub(crate) fn ensure_len(&mut self, min_len: usize) {
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

    pub(crate) fn set(&mut self, index: usize, value: f32) {
        if let Some(heap) = &mut self.heap {
            heap[index] = value;
        } else {
            self.inline[index] = value;
        }
    }

    #[cfg(test)]
    pub(crate) fn is_inline(&self) -> bool {
        self.heap.is_none()
    }
}
