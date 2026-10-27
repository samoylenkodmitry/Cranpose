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

fn surface(owes: bool, shows: bool) -> Surface {
    Surface { owes, shows }
}

#[test]
fn present_is_required_until_surface_is_clean() {
    assert!(surface_present_required(true, false, &mut surface(false, false)));
    assert!(surface_present_required(false, true, &mut surface(false, false)));
    assert!(surface_present_required(false, false, &mut surface(true, false)));
    assert!(!surface_present_required(false, false, &mut surface(false, false)));
}

#[test]
fn a_surface_that_shows_the_scene_takes_no_image_unless_it_is_dirty() {
    assert!(!surface_present_required(false, true, &mut surface(true, true)));
    assert!(surface_present_required(true, true, &mut surface(true, true)));
}
