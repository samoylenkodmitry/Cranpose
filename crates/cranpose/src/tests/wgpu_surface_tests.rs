use super::{PresentStep, PresentSurface, surface_present_step};

// A surface that owes a frame or not, and shows the scene or not.
struct Surface {
    owes: bool,
    shows: bool,
}

impl PresentSurface for Surface {
    fn needs_redraw(&self) -> bool {
        self.owes
    }

    fn shows_scene(&mut self) -> bool {
        self.shows
    }
}

fn surface(owes: bool, shows: bool) -> Surface {
    Surface { owes, shows }
}

fn presents(surface_dirty: bool, update_visual_changed: bool, surface: &mut Surface) -> bool {
    surface_present_step(surface_dirty, update_visual_changed, surface) == PresentStep::Present
}

#[test]
fn present_is_required_until_surface_is_clean() {
    assert!(presents(true, false, &mut surface(false, false)));
    assert!(presents(false, true, &mut surface(false, false)));
    assert!(presents(false, false, &mut surface(true, false)));
    assert!(!presents(false, false, &mut surface(false, false)));
}

#[test]
fn a_surface_that_shows_the_scene_takes_no_image_unless_it_is_dirty() {
    assert!(!presents(false, true, &mut surface(true, true)));
    assert!(presents(true, true, &mut surface(true, true)));
}

// A frame skipped because the surface already shows what was asked for is
// told apart from one nothing asked for: the desktop robot counts the first
// as presented, or its wait for the next present never ends.
#[test]
fn a_skipped_frame_says_whether_it_was_asked_for() {
    let step = |dirty, changed, owes, shows| surface_present_step(dirty, changed, &mut surface(owes, shows));
    assert_eq!(step(false, true, false, true), PresentStep::Shown);
    assert_eq!(step(false, false, true, true), PresentStep::Shown);
    assert_eq!(step(false, false, false, true), PresentStep::Unasked);
    assert_eq!(step(false, false, false, false), PresentStep::Unasked);
    assert_eq!(step(true, false, false, true), PresentStep::Present);
}
