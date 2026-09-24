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
#[path = "tests/font_tracking_tests.rs"]
mod tests;
