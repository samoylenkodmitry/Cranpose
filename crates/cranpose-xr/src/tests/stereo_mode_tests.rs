use super::*;

fn views(mode: StereoMode, size: (u32, u32)) -> Vec<EyeView> {
    mode.views(size).collect()
}

fn view(eye: Eye, offset: [u32; 2], extent: [u32; 2]) -> EyeView {
    EyeView {
        eye,
        offset,
        extent,
    }
}

#[test]
fn mono_shows_the_whole_picture_to_both_eyes() {
    assert_eq!(
        views(StereoMode::Mono, (1280, 720)),
        [view(Eye::Both, [0, 0], [1280, 720])]
    );
}

#[test]
fn side_by_side_gives_the_left_half_to_the_left_eye() {
    assert_eq!(
        views(StereoMode::SideBySide, (1281, 720)),
        [
            view(Eye::Left, [0, 0], [640, 720]),
            view(Eye::Right, [640, 0], [640, 720]),
        ]
    );
}

#[test]
fn top_bottom_gives_the_top_half_to_the_left_eye() {
    assert_eq!(
        views(StereoMode::TopBottom, (1280, 720)),
        [
            view(Eye::Left, [0, 0], [1280, 360]),
            view(Eye::Right, [0, 360], [1280, 360]),
        ]
    );
}
