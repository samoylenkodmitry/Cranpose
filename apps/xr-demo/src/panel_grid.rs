// A build that shows nothing but a grid of small panels of distinct colors ahead of the head, for
// a check of the compositor's layer budget. A Quest 3 takes 16 layers in a frame, so a grid of 19
// panels needs more. The projection layer then draws the panels of the lowest rank, and a
// screenshot tells by the colors whether every panel shows in both eyes.

use cranpose::prelude::*;
use cranpose_ui::text::FontWeight;
use cranpose_xr::{FollowMode, FollowTarget, FollowingSubspace, SpatialPanel, SubspaceModifier};
use player_demo::sized;

// `XR_DEMO_PANELS=<count>` builds the grid of `count` panels.
const PANELS_BUILD: Option<&str> = option_env!("XR_DEMO_PANELS");

// The side of a panel and the step from one panel to the next, in dp, which are millimeters.
const SIDE: f32 = 130.0;
const STEP: f32 = 160.0;
const COLUMNS: usize = 5;
// How much farther each panel stands than the one before it, in dp, so that the order by distance
// is fixed: the last panels stand farthest.
const DEPTH_STEP: f32 = 10.0;

// The number of panels of a grid build, or `None` for another build.
pub(crate) fn grid_build() -> Option<usize> {
    PANELS_BUILD.and_then(|count| count.parse().ok())
}

// The color of panel `index` of `count`: hues evenly around the color wheel at full saturation.
fn grid_color(index: usize, count: usize) -> Color {
    let hue = 6.0 * index as f32 / count as f32;
    let channel = |offset: f32| {
        let distance = (hue - offset).rem_euclid(6.0);
        let distance = distance.min(6.0 - distance);
        (2.0 - distance).clamp(0.0, 1.0)
    };
    Color(channel(0.0), channel(2.0), channel(4.0), 1.0)
}

// The grid, a meter ahead of the head where the build started. The farthest panel asks for a
// higher layer priority, so it keeps a layer of its own and the four panels before it go into the
// projection layer.
#[composable]
pub(crate) fn PanelGrid(count: usize) {
    FollowingSubspace(FollowTarget::View(FollowMode::Snap), move || {
        let rows = count.div_ceil(COLUMNS);
        for index in 0..count {
            let (column, row) = (index % COLUMNS, index / COLUMNS);
            let x = (column as f32 - (COLUMNS - 1) as f32 / 2.0) * STEP;
            let y = ((rows - 1) as f32 / 2.0 - row as f32) * STEP;
            let priority = i32::from(index + 1 == count);
            SpatialPanel(
                SubspaceModifier::new()
                    .size(SIDE, SIDE)
                    .offset(x, y, -DEPTH_STEP * index as f32)
                    .layer_priority(priority),
                move || {
                    Box(
                        Modifier::empty()
                            .fill_max_size()
                            .background(grid_color(index, count)),
                        BoxSpec::default().content_alignment(Alignment::CENTER),
                        move || {
                            Text(
                                format!("{}", index + 1),
                                Modifier::empty(),
                                sized(40.0, Some(FontWeight::BOLD)),
                            );
                        },
                    );
                },
            );
        }
    });
}
