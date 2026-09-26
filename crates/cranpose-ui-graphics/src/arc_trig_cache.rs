use crate::{
    ArcGeometry, TAU,
    float::{at_least, within},
};

#[derive(Clone, Debug)]
struct AngleTrig<const N: usize> {
    keys: [u32; N],
    values: [(f32, f32); N],
    len: usize,
    next: usize,
}

impl<const N: usize> Default for AngleTrig<N> {
    fn default() -> Self {
        Self {
            keys: [0; N],
            values: [(0.0, 0.0); N],
            len: 0,
            next: 0,
        }
    }
}

impl<const N: usize> AngleTrig<N> {
    fn resolve(&mut self, angle: f32) -> (f32, f32) {
        let key = angle.to_bits();
        if let Some(index) = self.keys[..self.len].iter().position(|held| *held == key) {
            return self.values[index];
        }
        let result = angle.sin_cos();
        self.keys[self.next] = key;
        self.values[self.next] = result;
        self.len = (self.len + 1).min(N);
        self.next = (self.next + 1) % N;
        result
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ArcTrigCache {
    mid: AngleTrig<1>,
    half: AngleTrig<8>,
}

impl ArcTrigCache {
    pub(crate) fn resolve(&mut self, geometry: &ArcGeometry) -> [f32; 4] {
        if geometry.sweep_angle >= TAU && geometry.start_angle == 0.0 {
            return [0.0, -1.0, 0.0, -1.0];
        }
        let half_sweep = within(geometry.sweep_angle, 0.0, TAU) * 0.5;
        let (mid_sin, mid_cos) = self.mid.resolve(geometry.start_angle + half_sweep);
        let (half_sin, half_cos) = self.half.resolve(half_sweep);
        [mid_sin, mid_cos, at_least(half_sin, 0.0), half_cos]
    }
}

#[cfg(test)]
#[path = "tests/arc_trig_cache_tests.rs"]
mod tests;
