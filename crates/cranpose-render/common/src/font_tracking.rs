use std::sync::Arc;

use cranpose_ui::text::TextStyle;

#[derive(Clone, Default)]
pub(crate) struct FontTracking(Arc<[(f32, f32)]>);

impl FontTracking {
    pub(crate) fn from_face(face: &ttf_parser::Face<'_>) -> Self {
        face.tables()
            .trak
            .and_then(|table| Self::from_table(table, f32::from(face.units_per_em())))
            .unwrap_or_default()
    }

    fn from_table(table: ttf_parser::trak::Table<'_>, units_per_em: f32) -> Option<Self> {
        let data = table.horizontal;
        let normal = data.tracks.into_iter().find(|track| track.value == 0.0)?;
        let mut points = Vec::with_capacity(usize::from(data.sizes.len()));
        for index in 0..data.sizes.len() {
            let size = data.sizes.get(index)?.0;
            if size <= 0.0 || points.last().is_some_and(|&(previous, _)| size <= previous) {
                return None;
            }
            points.push((size, f32::from(normal.values.get(index)?) / units_per_em));
        }
        Some(Self(points.into()))
    }

    fn at_size(&self, size: f32) -> f32 {
        if !size.is_finite() || size <= 0.0 || self.0.is_empty() {
            return 0.0;
        }
        let index = self.0.partition_point(|&(point_size, _)| point_size < size);
        let em = if index == 0 {
            self.0[0].1
        } else if index == self.0.len() {
            self.0[index - 1].1
        } else {
            let (low_size, low) = self.0[index - 1];
            let (high_size, high) = self.0[index];
            low + (high - low) * (size - low_size) / (high_size - low_size)
        };
        em * size
    }

    pub(crate) fn resolve(&self, style: &TextStyle, size: f32) -> f32 {
        if style.span_style.letter_spacing.is_unspecified() {
            self.at_size(size)
        } else {
            style.resolve_letter_spacing(size)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &[u8] = include_bytes!("../tests/fixtures/default_tracking.trak");

    #[test]
    fn normal_tracking_interpolates_in_font_units_and_clamps_size_endpoints() {
        let table = ttf_parser::trak::Table::parse(TABLE).unwrap();
        let tracking = FontTracking::from_table(table, 2048.0).unwrap();
        for (size, units) in [
            (6.0, 38.0),
            (9.0, 38.0),
            (9.5, 31.0),
            (10.0, 24.0),
            (15.0, -11.0),
            (20.0, -46.0),
            (40.0, -46.0),
        ] {
            assert!((tracking.at_size(size) - units * size / 2048.0).abs() < 0.00001);
        }
        for size in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(tracking.at_size(size), 0.0);
        }
        assert_eq!(FontTracking::default().at_size(10.0), 0.0);
    }

    #[test]
    fn missing_normal_track_and_unordered_sizes_are_ignored() {
        for (offset, value) in [(20, 65536i32), (28, 0), (32, 8 * 65536)] {
            let mut bytes = TABLE.to_vec();
            bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
            let table = ttf_parser::trak::Table::parse(&bytes).unwrap();
            assert!(FontTracking::from_table(table, 2048.0).is_none());
        }
        for length in 0..TABLE.len() {
            let parsed = ttf_parser::trak::Table::parse(&TABLE[..length]);
            assert!(
                parsed
                    .and_then(|table| FontTracking::from_table(table, 2048.0))
                    .is_none()
            );
        }
    }
}
