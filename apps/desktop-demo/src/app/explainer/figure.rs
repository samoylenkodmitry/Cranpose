// The figure of the "How it works" scene: the camera, the drawing board
// that holds the flat copy and the photographic print that holds the bent
// copy, posed for a point of the scroll walk.
//
// World units are logical pixels. The origin is the middle of the flat
// sheet, x to the right, y up, z toward the viewer. The camera only pitches
// down and backs away, and the board leans back by that pitch, so it always
// stands parallel to the picture plane: the flat copy is only ever scaled
// and moved on screen. The print is a vertical ruled surface over a curve in
// the ground plane, measured by arc length, so it is the paragraph bent
// without stretching.

use std::f32::consts::TAU;

use cranpose_ui::{
    space::{Camera, Mat4, Vec3},
    Point, Rect, Size,
};

use super::{
    walk::{ease_softly, unit},
    whole,
};

// Straight pieces in the print's curve.
pub(super) const PROFILE_SEGMENTS: usize = 112;
const PROFILE_POINTS: usize = PROFILE_SEGMENTS + 1;

// When things happen along the walk.
pub(super) mod timeline {
    // Step 2: the camera pitches down and backs away.
    pub(in super::super) const PULL_BACK: (f32, f32) = (0.262, 0.44);
    // The print fades in.
    pub(in super::super) const PRINT_IN: (f32, f32) = (0.25, 0.275);
    // Step 3: the print curls.
    pub(in super::super) const BEND: (f32, f32) = (0.5, 0.72);
    // Step 4: the cord comes.
    pub(in super::super) const CORD: (f32, f32) = (0.765, 0.81);
}

// Where everything stands once the camera has pulled back, worked out
// backwards from the picture wanted: the print at a chosen size low in the
// free part of the stage, the board as far back as it goes while it still
// fits under that part's top edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Plan {
    pub u: f32,
    // The sheet on the stage as it lies flat, before the walk.
    pub sheet: Rect,
    // The wood round the sheet at the sides and top, and at the bottom.
    pub margin: f32,
    pub margin_bottom: f32,
    // The stage's right edge of the caption.
    pub caption_right: f32,
    cam_dist: f32,
    pitch: f32,
    shift_y: f32,
    shift_z: f32,
    board_back: f32,
    board_x: f32,
    print_x: f32,
    print_yaw: f32,
}

impl Plan {
    pub(super) fn new(stage: Size) -> Self {
        let u = unit(stage);
        let (w, h) = (734.0 * u, 500.0 * u);
        let sheet = Rect {
            x: stage.width * 0.5 - 104.0 * u,
            y: stage.height * 0.5 - 306.0 * u,
            width: w,
            height: h,
        };
        let (margin, margin_bottom) = (24.0 * u, 54.0 * u);
        let caption_right = (stage.width * 0.5 - 644.0 * u).max(16.0) + 448.0 * u;
        let (x0, x1) = (
            caption_right + 40.0 * u,
            (stage.width - 24.0 * u).min(caption_right + 900.0 * u),
        );
        let (y0, y1) = (30.0 * u, stage.height - 62.0 * u);
        let (region_w, region_h) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
        let centre = (sheet.x + w * 0.5, sheet.y + h * 0.5);
        let (outer_w, outer_h) = (w + 2.0 * margin, h + margin + margin_bottom);
        let pitch = 0.46f32;
        let cam_dist = 4300.0 * u;
        let (cos, sin) = (pitch.cos(), pitch.sin());
        let outer_mid_y = (margin - margin_bottom) * 0.5;
        let table_y = -(h * 0.5 + margin_bottom);

        let print_scale = 0.8f32
            .min(region_w * 0.735 / outer_w)
            .min(region_h * 0.515 / (outer_h * cos));
        let lift = 34.0 * u;
        let print_mid_y = y1 - outer_h * print_scale * cos * 0.5 - lift * print_scale;
        let print_mid_x = x0 + region_w * 0.615;
        let shift_z = cam_dist * (1.0 - 1.0 / print_scale) - outer_mid_y * sin;
        let shift_y = -(print_mid_y - centre.1) / print_scale - outer_mid_y * cos;

        // The board's foot goes back along the table until its top would
        // leave the free part of the stage.
        let scale_at = |back: f32| cam_dist / (cam_dist - (table_y * sin - back * cos + shift_z));
        let (mut low, mut high) = (0.0f32, 5000.0 * u);
        for _ in 0..40 {
            let back = (low + high) * 0.5;
            let scale = scale_at(back);
            let foot = centre.1 - (table_y * cos + back * sin + shift_y) * scale;
            if foot - outer_h * scale > y0 {
                low = back;
            } else {
                high = back;
            }
        }
        let board_scale = scale_at(low);
        let board_mid_x = x0 + outer_w * board_scale * 0.5 + region_w * 0.03;
        Self {
            u,
            sheet,
            margin,
            margin_bottom,
            caption_right,
            cam_dist,
            pitch,
            shift_y,
            shift_z,
            board_back: low,
            board_x: (board_mid_x - centre.0) / board_scale,
            print_x: (print_mid_x - centre.0) / print_scale,
            print_yaw: -0.06,
        }
    }

    // The table's height.
    pub(super) fn table_y(&self) -> f32 {
        -(self.sheet.height * 0.5 + self.margin_bottom)
    }

    // The print's (and the board's) height from the table up.
    pub(super) fn outer_height(&self) -> f32 {
        self.sheet.height + self.margin + self.margin_bottom
    }

    // The print's width, its white border included.
    pub(super) fn outer_width(&self) -> f32 {
        self.sheet.width + 2.0 * self.margin
    }

    // The middle of the window's pool of light on the table, x and z, once
    // the camera has pulled back `pull` of the way: it follows the figure.
    pub(super) fn light_pool(&self, pull: f32) -> (f32, f32) {
        let x = 60.0 * self.u + (self.board_x * 0.35 + self.print_x * 0.65 - 60.0 * self.u) * pull;
        (x, -self.board_back * 0.30 * pull)
    }

    // How far back the table runs before it curves up into the wall.
    pub(super) fn wall_curve(&self) -> f32 {
        1500.0 * self.u + self.board_back * 0.25
    }

    // The camera's numbers once it has pulled back `pull` of the way: the
    // cosine and sine of its pitch, the world's shift in view space, y and
    // z, and the eye's distance from the picture plane.
    pub(super) fn camera_numbers(&self, pull: f32) -> [f32; 5] {
        let pitch = self.pitch * pull;
        [
            pitch.cos(),
            pitch.sin(),
            self.shift_y * pull,
            self.shift_z * pull,
            self.cam_dist,
        ]
    }
}

// The figure at a point of the walk.
#[derive(Clone, Debug)]
pub(super) struct Figure {
    pub plan: Plan,
    // How far the camera has pulled back, from 0 to 1.
    pub pull: f32,
    // How far the print has curled, from 0 to 1.
    pub bend: f32,
    pub cos: f32,
    pub sin: f32,
    pub cam_dist: f32,
    // The eye, in world space.
    pub eye: Vec3,
    // The board's foot: its middle's x and how far back along the table.
    pub board_x: f32,
    pub board_back: f32,
    // The middle of the flat sheet, in world space.
    pub board_middle: Vec3,
    // The flat copy's scale on the stage.
    pub board_scale: f32,
    // The print's curve: each point's x, z and normal x, z.
    profile: [[f32; 4]; PROFILE_POINTS],
}

impl Figure {
    // The figure `progress` along the walk, its print swayed `sway`
    // seconds.
    pub(super) fn at(plan: Plan, progress: f32, sway: f32) -> Self {
        let pull = ease_softly(timeline::PULL_BACK.0, timeline::PULL_BACK.1, progress);
        let bend = ease_softly(timeline::BEND.0, timeline::BEND.1, progress);
        let pitch = plan.pitch * pull;
        let (cos, sin) = (pitch.cos(), pitch.sin());
        let (shift_y, shift_z) = (plan.shift_y * pull, plan.shift_z * pull);
        let mut figure = Self {
            plan,
            pull,
            bend,
            cos,
            sin,
            cam_dist: plan.cam_dist,
            eye: Vec3::ZERO,
            board_x: plan.board_x * pull,
            board_back: plan.board_back * pull,
            board_middle: Vec3::ZERO,
            board_scale: 1.0,
            profile: [[0.0; 4]; PROFILE_POINTS],
        };
        figure.eye = figure.view_to_world(Vec3::new(0.0, -shift_y, plan.cam_dist - shift_z));
        // The board leans back by the pitch: in view space it is a plane of
        // one depth, and the flat copy shows at one scale.
        let table_y = plan.table_y();
        let back = figure.board_back;
        let view_z = table_y * sin - back * cos + shift_z;
        let view_y =
            table_y * cos + back * sin + shift_y + plan.margin_bottom + plan.sheet.height * 0.5;
        figure.board_scale = plan.cam_dist / (plan.cam_dist - view_z);
        figure.board_middle = figure.view_to_world(Vec3::new(
            figure.board_x,
            view_y - shift_y,
            view_z - shift_z,
        ));
        figure.bend_profile(sway);
        figure
    }

    // A direction or offset in view space, turned into world space.
    fn view_to_world(&self, v: Vec3) -> Vec3 {
        Vec3::new(
            v.x,
            v.y * self.cos + v.z * self.sin,
            -v.y * self.sin + v.z * self.cos,
        )
    }

    // The board's axes in world space: right, up along its face, and out of
    // its face toward the eye.
    pub(super) fn board_axes(&self) -> (Vec3, Vec3, Vec3) {
        (
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, self.cos, -self.sin),
            Vec3::new(0.0, self.sin, self.cos),
        )
    }

    // Where the sheet point `point` (from its top left corner, y down) lies
    // on the flat copy, `off` in front of the board.
    pub(super) fn on_board(&self, point: Point, off: f32) -> Vec3 {
        let (right, up, out) = self.board_axes();
        let sheet = self.plan.sheet;
        self.board_middle
            + right * (point.x - sheet.width * 0.5)
            + up * (sheet.height * 0.5 - point.y)
            + out * off
    }

    // The print's curve seen from above: one period of a sine sets the
    // angle of the paper along it, swaying slowly, turned so that, end to
    // end, it faces the plan's yaw, its middle at the plan's x.
    fn bend_profile(&mut self, sway: f32) {
        let plan = self.plan;
        let n = PROFILE_SEGMENTS;
        let piece = plan.outer_width() / n as f32;
        let mut angle = [0.0f32; PROFILE_POINTS];
        if self.bend > 0.0 {
            let depth = 0.86 + 0.05 * (sway * 0.9 + 1.0).sin();
            for (index, angle) in angle.iter_mut().enumerate() {
                let t = index as f32 / n as f32;
                let phase = t * 1.04 + 0.10 + 0.035 * (sway * 0.55).sin();
                *angle =
                    self.bend * (depth * (TAU * phase).sin() + 0.07 * (sway * 0.7 + t * 3.4).sin());
            }
        }
        let (mut sum_cos, mut sum_sin) = (0.0f32, 0.0f32);
        for pair in angle.windows(2) {
            let middle = (pair[0] + pair[1]) * 0.5;
            sum_cos += middle.cos();
            sum_sin += middle.sin();
        }
        let turn = plan.print_yaw * self.pull - sum_sin.atan2(sum_cos);
        for angle in &mut angle {
            *angle += turn;
        }
        let (mut x, mut z, mut mean_x, mut mean_z) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for index in 0..=n {
            self.profile[index] = [x, z, 0.0, 0.0];
            mean_x += x / (n + 1) as f32;
            mean_z += z / (n + 1) as f32;
            if index < n {
                let middle = (angle[index] + angle[index + 1]) * 0.5;
                x += middle.cos() * piece;
                z += middle.sin() * piece;
            }
        }
        let centre_x = plan.print_x * self.pull;
        for (point, angle) in self.profile.iter_mut().zip(angle) {
            *point = [
                point[0] - mean_x + centre_x,
                point[1] - mean_z,
                -angle.sin(),
                angle.cos(),
            ];
        }
    }

    // Where the sheet point `point` lies on the print, `off` in front of
    // the paper, and the paper's normal there.
    pub(super) fn on_print(&self, point: Point, off: f32) -> (Vec3, Vec3) {
        let plan = self.plan;
        let along = ((point.x + plan.margin) / plan.outer_width() * PROFILE_SEGMENTS as f32)
            .clamp(0.0, PROFILE_SEGMENTS as f32 - 1.0e-4);
        let index = whole::count(along.floor());
        let share = along - index as f32;
        let (a, b) = (self.profile[index], self.profile[index + 1]);
        let mix = |from: f32, to: f32| from + (to - from) * share;
        let normal = Vec3::new(mix(a[2], b[2]), 0.0, mix(a[3], b[3]))
            .normalized()
            .unwrap_or(Vec3::new(0.0, 0.0, 1.0));
        let on_paper = Vec3::new(
            mix(a[0], b[0]),
            plan.sheet.height * 0.5 - point.y,
            mix(a[1], b[1]),
        );
        (on_paper + normal * off, normal)
    }

    // The camera, for a scene node whose middle shows the world's origin.
    pub(super) fn camera(&self, node: Size) -> Camera {
        let to_scene = world_to_scene(node);
        let eye = to_scene.transform_point(self.eye);
        let forward = to_scene.transform_vector(self.view_to_world(Vec3::new(0.0, 0.0, -1.0)));
        let down = to_scene.transform_vector(self.view_to_world(Vec3::new(0.0, -1.0, 0.0)));
        Camera::look_at(eye, eye + forward, down, self.cam_dist)
    }
}

// The map from world space into a scene node of `node`'s size whose middle
// shows the world's origin: the scene's y runs down and its z away.
pub(super) fn world_to_scene(node: Size) -> Mat4 {
    Mat4::scaling(Vec3::new(1.0, -1.0, -1.0)).then(&Mat4::translation(Vec3::new(
        node.width * 0.5,
        node.height * 0.5,
        0.0,
    )))
}
