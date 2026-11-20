// What a hand shows for each runtime and each choice of the app, and which model it loads.

use super::*;

// The runtime's model, as a Quest hands it out.
fn quest() -> Offer {
    Offer::Bytes(Arc::from(&b"glTF"[..]))
}

#[test]
fn a_runtime_with_a_model_shows_it_once_it_has_loaded() {
    let offer = quest();
    let runtime = ControllerModel::Runtime;
    assert_eq!(look(&runtime, &offer, None), Look::Nothing);
    assert_eq!(look(&runtime, &offer, Some(Load::Loading)), Look::Nothing);
    assert_eq!(look(&runtime, &offer, Some(Load::Ready)), Look::Model);
}

#[test]
fn a_runtime_model_that_fails_to_load_ends_at_the_plain_model() {
    assert_eq!(
        look(&ControllerModel::Runtime, &quest(), Some(Load::Failed)),
        Look::Plain
    );
}

#[test]
fn a_runtime_with_no_model_shows_the_plain_one_with_no_wait() {
    // Android XR, a desktop runtime, an app that did not declare `Use::controller_models()` and a
    // build with no glTF loader all give the backend nothing to load
    assert_eq!(
        look(&ControllerModel::Runtime, &Offer::Absent, None),
        Look::Plain
    );
}

#[test]
fn a_hand_waits_for_the_backends_question_instead_of_showing_a_model_that_it_may_replace() {
    assert_eq!(
        look(&ControllerModel::Runtime, &Offer::Pending, None),
        Look::Nothing
    );
}

#[test]
fn the_plain_choice_shows_the_plain_model_whatever_the_runtime_offers() {
    for offer in [Offer::Pending, Offer::Absent, quest()] {
        for load in [
            None,
            Some(Load::Loading),
            Some(Load::Ready),
            Some(Load::Failed),
        ] {
            assert_eq!(look(&ControllerModel::Plain, &offer, load), Look::Plain);
        }
    }
}

#[test]
fn two_offers_are_the_same_only_when_they_hold_the_same_bytes() {
    let offer = quest();
    assert_eq!(offer.clone(), offer);
    assert_ne!(offer, quest());
    assert_ne!(Offer::Pending, Offer::Absent);
}

#[cfg(feature = "gltf")]
mod gltf {
    use super::*;
    use crate::SpatialGltfModelSource;

    fn app_model() -> ControllerModel {
        ControllerModel::Gltf(SpatialGltfModelSource::from_path("model.glb"))
    }

    #[test]
    fn the_apps_model_loads_whatever_the_runtime_offers_and_shows_when_ready() {
        for offer in [Offer::Pending, Offer::Absent, quest()] {
            assert_eq!(
                look(&app_model(), &offer, Some(Load::Loading)),
                Look::Nothing
            );
            assert_eq!(look(&app_model(), &offer, Some(Load::Ready)), Look::Model);
            assert_eq!(look(&app_model(), &offer, Some(Load::Failed)), Look::Plain);
        }
    }

    #[test]
    fn a_hand_loads_the_runtimes_bytes_the_apps_file_or_nothing() {
        let bytes = quest();
        assert!(source(&ControllerModel::Runtime, &bytes).is_some());
        assert!(source(&ControllerModel::Runtime, &Offer::Absent).is_none());
        assert!(source(&ControllerModel::Runtime, &Offer::Pending).is_none());
        assert!(source(&ControllerModel::Plain, &bytes).is_none());
        let own = app_model();
        let ControllerModel::Gltf(expected) = &own else {
            unreachable!("the model is a glTF one");
        };
        assert_eq!(source(&own, &Offer::Absent).as_ref(), Some(expected));
    }

    #[test]
    fn the_runtimes_bytes_make_an_equal_source_each_time_so_the_loaded_state_stays() {
        let bytes = quest();
        assert_eq!(
            source(&ControllerModel::Runtime, &bytes),
            source(&ControllerModel::Runtime, &bytes)
        );
        assert_ne!(
            source(&ControllerModel::Runtime, &bytes),
            source(&ControllerModel::Runtime, &quest())
        );
    }
}
