use crate::{ArcGeometry, TAU};

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
        let half_sweep = geometry.sweep_angle.clamp(0.0, TAU) * 0.5;
        let (mid_sin, mid_cos) = self.mid.resolve(geometry.start_angle + half_sweep);
        let (half_sin, half_cos) = self.half.resolve(half_sweep);
        [mid_sin, mid_cos, half_sin.max(0.0), half_cos]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Point, StrokeCap};

    #[test]
    fn reuse_preserves_bits_when_either_angle_changes() {
        let mut cache = ArcTrigCache::default();
        for (start, sweep) in [
            (0.3, 0.1),
            (0.3, 0.1),
            (0.4, 0.1),
            (0.4, 0.2),
            (0.3, 0.1),
            (0.0, TAU),
            (0.4, 0.2),
            (0.0, -0.0),
            (-0.0, -0.0),
            (f32::INFINITY, 0.1),
            (0.3, f32::NAN),
            (0.3, 0.1),
        ] {
            let geometry = ArcGeometry {
                center: Point::ZERO,
                inner_radius: 10.0,
                outer_radius: 12.0,
                start_angle: start,
                sweep_angle: sweep,
                cap: StrokeCap::Butt,
            };
            let expected = if sweep >= TAU && start == 0.0 {
                [0.0, -1.0, 0.0, -1.0]
            } else {
                let half = sweep.clamp(0.0, TAU) * 0.5;
                let (mid_sin, mid_cos) = (start + half).sin_cos();
                let (half_sin, half_cos) = half.sin_cos();
                [mid_sin, mid_cos, half_sin.max(0.0), half_cos]
            };
            assert_eq!(
                cache.resolve(&geometry).map(f32::to_bits),
                expected.map(f32::to_bits)
            );
        }
    }

    #[test]
    fn interleaved_sweeps_preserve_bits_through_reuse_and_eviction() {
        let mut cache = ArcTrigCache::default();
        let sweeps = [0.13, 0.29, 0.53, 0.79, 1.01, 1.37, 1.73, 2.11, 2.57, 3.19];
        for frame in 0..7 {
            for index in [0, 3, 1, 5, 7, 2, 6, 4, 0, 7, 3, 9, 8, 1, 6, 9, 0] {
                let geometry = ArcGeometry {
                    center: Point::ZERO,
                    inner_radius: 10.0,
                    outer_radius: 12.0,
                    start_angle: frame as f32 * 0.031 + index as f32 * 0.17,
                    sweep_angle: sweeps[index],
                    cap: StrokeCap::Butt,
                };
                let half = geometry.sweep_angle * 0.5;
                let (mid_sin, mid_cos) = (geometry.start_angle + half).sin_cos();
                let (half_sin, half_cos) = half.sin_cos();
                assert_eq!(
                    cache.resolve(&geometry).map(f32::to_bits),
                    [mid_sin, mid_cos, half_sin.max(0.0), half_cos].map(f32::to_bits),
                    "frame {frame}, sweep {index}"
                );
            }
        }
    }
}
