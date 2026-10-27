use super::{PresentSurface, surface_present_required};

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

fn required(dirty: bool, changed: bool, owes: bool, shows: bool) -> bool {
    surface_present_required(dirty, changed, &mut Surface { owes, shows })
}

#[test]
fn present_is_required_until_surface_is_clean() {
    assert!(required(true, false, false, false));
    assert!(required(false, true, false, false));
    assert!(required(false, false, true, false));
    assert!(!required(false, false, false, false));
}

#[test]
fn a_surface_that_shows_the_scene_takes_no_image_unless_it_is_dirty() {
    assert!(!required(false, true, true, true));
    assert!(required(true, true, true, true));
}
