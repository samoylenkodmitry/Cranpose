use cranpose_render_common::debug_toggles::DebugToggle;
use cranpose_ui_graphics::{GLASS_DISPERSION_OFF_FLAG, GLASS_PHYSICAL_REFRACTION_OFF_FLAG};

static ABLATE: DebugToggle = DebugToggle::new("CRANPOSE_ABLATE");

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
pub(crate) struct ShapeAblation {
    pub(crate) material: bool,
    pub(crate) fill: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
pub(crate) struct GlassAblation {
    pub(crate) dispersion: bool,
    pub(crate) refraction: bool,
}

impl GlassAblation {
    pub(crate) fn forced_flags(self) -> impl Iterator<Item = &'static str> {
        [
            (self.dispersion, GLASS_DISPERSION_OFF_FLAG),
            (self.refraction, GLASS_PHYSICAL_REFRACTION_OFF_FLAG),
        ]
        .into_iter()
        .filter_map(|(on, flag)| on.then_some(flag))
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) struct Ablation {
    pub(crate) stages: bool,
    pub(crate) glass: bool,
    pub(crate) blur: bool,
    pub(crate) substrates: bool,
    pub(crate) text: bool,
    pub(crate) shape: ShapeAblation,
    pub(crate) glass_flags: GlassAblation,
}

impl Ablation {
    pub(crate) fn current() -> Self {
        ABLATE.with(|value| value.map_or(Self::default(), Self::parse))
    }

    fn parse(list: &str) -> Self {
        let mut ablation = Self::default();
        for name in list.split(',').map(str::trim) {
            match name {
                "stages" => ablation.stages = true,
                "glass" => ablation.glass = true,
                "blur" => ablation.blur = true,
                "substrates" => ablation.substrates = true,
                "text" => ablation.text = true,
                "shape" => ablation.shape.material = true,
                "shape_fill" => ablation.shape.fill = true,
                "glass_dispersion" => ablation.glass_flags.dispersion = true,
                "glass_refraction" => ablation.glass_flags.refraction = true,
                _ => {}
            }
        }
        ablation
    }
}

#[cfg(test)]
mod tests {
    use super::{Ablation, GlassAblation, ShapeAblation};

    #[test]
    fn names_switch_their_work_off_and_unknown_names_change_nothing() {
        assert_eq!(Ablation::parse(""), Ablation::default());
        assert_eq!(
            Ablation::parse("glass, text,unknown"),
            Ablation {
                glass: true,
                text: true,
                ..Ablation::default()
            }
        );
        assert_eq!(
            Ablation::parse("shape_fill, shape"),
            Ablation {
                shape: ShapeAblation {
                    material: true,
                    fill: true,
                },
                ..Ablation::default()
            }
        );
        let glass = Ablation::parse("glass_refraction,glass_dispersion");
        assert_eq!(
            glass,
            Ablation {
                glass_flags: GlassAblation {
                    dispersion: true,
                    refraction: true,
                },
                ..Ablation::default()
            }
        );
        assert_eq!(
            glass.glass_flags.forced_flags().collect::<Vec<_>>(),
            ["GLASS_DISPERSION_OFF", "GLASS_PHYSICAL_REFRACTION_OFF"]
        );
        assert_eq!(Ablation::default().glass_flags.forced_flags().count(), 0);
        assert_eq!(
            Ablation::parse("stages,blur,substrates"),
            Ablation {
                stages: true,
                blur: true,
                substrates: true,
                ..Ablation::default()
            }
        );
    }

    #[test]
    fn the_toggle_reads_the_current_list() {
        cranpose_render_common::debug_toggles::set_debug_toggle("CRANPOSE_ABLATE", Some("blur"));
        assert!(Ablation::current().blur);
        cranpose_render_common::debug_toggles::set_debug_toggle("CRANPOSE_ABLATE", None);
        assert_eq!(Ablation::current(), Ablation::default());
    }
}
