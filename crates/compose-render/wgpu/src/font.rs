use std::sync::Arc;

use glyphon::{fontdb, Attrs, Family, FontSystem, Weight};

#[derive(Clone, Copy)]
struct StaticFontData(&'static [u8]);

impl AsRef<[u8]> for StaticFontData {
    fn as_ref(&self) -> &[u8] {
        self.0
    }
}

pub(crate) const DEFAULT_FONT_SIZE: f32 = 24.0;
pub(crate) const DEFAULT_LINE_HEIGHT: f32 = DEFAULT_FONT_SIZE * 1.4;

pub(crate) fn create_font_system() -> FontSystem {
    let font_sources = [fontdb::Source::Binary(Arc::new(StaticFontData(
        include_bytes!("../../../../apps/desktop-demo/assets/Roboto-Light.ttf"),
    )))];
    FontSystem::new_with_fonts(font_sources)
}

#[derive(Clone, Debug)]
pub(crate) struct PreferredFont {
    pub family: String,
    pub weight: Weight,
}

pub(crate) fn detect_preferred_font(font_system: &FontSystem) -> Option<PreferredFont> {
    let mut fallback = None;

    for face in font_system.db().faces() {
        let Some((primary_family, _)) = face.families.first() else {
            continue;
        };
        let matches_family = primary_family.eq_ignore_ascii_case("Roboto")
            || face
                .post_script_name
                .to_ascii_lowercase()
                .contains("roboto");
        if !matches_family {
            continue;
        }

        let candidate = PreferredFont {
            family: primary_family.clone(),
            weight: face.weight,
        };

        if face.weight == Weight::LIGHT {
            return Some(candidate);
        }

        if fallback.is_none() {
            fallback = Some(candidate);
        }
    }

    fallback
}

pub(crate) fn primary_text_attrs(preferred: Option<&PreferredFont>) -> Attrs<'_> {
    match preferred {
        Some(font) => Attrs::new()
            .family(Family::Name(&font.family))
            .weight(font.weight),
        None => Attrs::new().family(Family::SansSerif),
    }
}

pub(crate) fn fallback_text_attrs(preferred: Option<&PreferredFont>) -> Option<Attrs<'_>> {
    preferred.map(|_| Attrs::new().family(Family::SansSerif))
}
