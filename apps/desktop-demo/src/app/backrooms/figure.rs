// A tall dark figure that stands in a room behind a doorway and turns to
// face the eye. It is gone by the time the eye walks into its room.

use std::sync::Arc;

use cranpose_ui::{
    composable,
    space::{Mesh, MeshSurface, Vec3},
    Box, BoxSpec, Brush, Color, CornerRadii, DrawScope, Point, Rect, Size,
};

use super::{
    finish::{Finish, Lamps, Sight},
    place::{spot, upright},
};

const FIGURE: Size = Size {
    width: 80.0,
    height: 215.0,
};
const SHADOW: Color = Color(0.015, 0.012, 0.01, 1.0);

#[composable(no_skip)]
pub(super) fn Figure(sight: Sight, plane: Arc<Mesh>, lamps: Lamps, foot: Vec3) {
    Box(
        spot(FIGURE)
            .mesh_surface(move || {
                let transform = upright(foot, sight.eye_in_room(), FIGURE);
                MeshSurface {
                    mesh: Arc::clone(&plane),
                    transform,
                    material: sight.material(Finish::Content, &transform, lamps),
                    casts_shadow: false,
                    solid: false,
                }
            })
            .draw_behind(silhouette),
        BoxSpec::default(),
        || {},
    );
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

// A head too small for its long body, arms that reach past its knees, and
// two pale points where its eyes would be.
fn silhouette(scope: &mut dyn DrawScope) {
    let size = scope.size();
    let middle = size.width * 0.5;
    let shadow = Brush::solid(SHADOW);
    scope.draw_circle(shadow.clone(), Point::new(middle, 14.0), 12.0);
    let parts = [
        // The neck, the body and the legs.
        (rect(middle - 4.0, 22.0, 8.0, 12.0), 3.0),
        (rect(middle - 17.0, 32.0, 34.0, 92.0), 12.0),
        (rect(middle - 15.0, 116.0, 12.0, size.height - 116.0), 5.0),
        (rect(middle + 3.0, 116.0, 12.0, size.height - 116.0), 5.0),
        // The arms.
        (rect(middle - 27.0, 36.0, 9.0, 130.0), 4.5),
        (rect(middle + 18.0, 36.0, 9.0, 130.0), 4.5),
    ];
    for (rect, radius) in parts {
        scope.draw_round_rect_at(rect, shadow.clone(), CornerRadii::uniform(radius));
    }
    let glint = Brush::solid(Color(0.85, 0.82, 0.7, 0.9));
    scope.draw_circle(glint.clone(), Point::new(middle - 4.5, 13.0), 1.6);
    scope.draw_circle(glint, Point::new(middle + 4.5, 13.0), 1.6);
}
