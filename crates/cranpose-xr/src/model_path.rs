//! Which of the runtime's render model paths holds a hand's controller.

use crate::Hand;

// The path among `paths` of the model of `hand`'s controller, or `None` when the runtime lists
// none. The runtime names its models with a path such as `/model_fb/controller/left`: a segment
// `controller` and then the side. The first path that has both names the model. A path of another
// device, such as a keyboard's, does not.
pub(crate) fn controller_path(paths: &[String], hand: Hand) -> Option<usize> {
    let side = match hand {
        Hand::Left => "left",
        Hand::Right => "right",
    };
    paths.iter().position(|path| {
        let mut segments = path.split('/');
        segments.any(|segment| segment == "controller") && segments.any(|segment| segment == side)
    })
}

#[cfg(test)]
#[path = "tests/model_path_tests.rs"]
mod tests;
