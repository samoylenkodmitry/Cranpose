// The key colours the words of an effect's content draw in: the effect
// reads each colour as a part of the picture, the headline, the paragraph
// and a selection, and draws that part in its own way.

use cranpose_ui::Color;

pub(super) const HEAD_INK: Color = Color(1.0, 0.0, 0.0, 1.0);
pub(super) const COPY_INK: Color = Color(0.0, 0.0, 1.0, 1.0);
pub(super) const SELECTION_KEY: Color = Color(0.0, 1.0, 0.0, 1.0);
