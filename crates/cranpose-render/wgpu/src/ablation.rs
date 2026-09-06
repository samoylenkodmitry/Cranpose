use cranpose_render_common::debug_toggles::DebugToggle;

static ABLATE: DebugToggle = DebugToggle::new("CRANPOSE_ABLATE");

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) struct Ablation {
    pub(crate) stages: bool,
    pub(crate) glass: bool,
    pub(crate) blur: bool,
    pub(crate) substrates: bool,
    pub(crate) text: bool,
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
                _ => {}
            }
        }
        ablation
    }
}

#[cfg(test)]
mod tests {
    use super::Ablation;

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
