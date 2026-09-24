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
