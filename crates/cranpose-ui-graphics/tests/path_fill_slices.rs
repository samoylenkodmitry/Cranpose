//! A contour that each upright line crosses at most twice is cut into the
//! same slices whether it stands alone in its path or beside another
//! contour, which the sweep over all of a path's edges slices.

use cranpose_ui_graphics::{
    Brush, Color, DrawPrimitive, DrawScope, DrawScopeDefault, DrawStyle, Path, Point, Size,
    Trapezoid,
};

const FRAME: f32 = 128.0;

fn add_contour(path: &mut Path, points: &[(f32, f32)]) {
    for (index, &(x, y)) in points.iter().enumerate() {
        if index == 0 {
            path.move_to(Point::new(x, y));
        } else {
            path.line_to(Point::new(x, y));
        }
    }
    path.close();
}

fn slices_of(path: &Path) -> Vec<Trapezoid> {
    let mut scope = DrawScopeDefault::new(Size::new(FRAME, FRAME));
    scope.draw_path(path, Brush::solid(Color::WHITE), DrawStyle::Fill);
    scope
        .into_primitives()
        .into_iter()
        .filter_map(|primitive| match primitive {
            DrawPrimitive::Trapezoid { trapezoid, .. } => Some(trapezoid),
            _ => None,
        })
        .collect()
}

/// A chart's area under `values`, from x 4 to 96, over a baseline at y 80.
fn area(values: &[f32]) -> Vec<(f32, f32)> {
    let step = 92.0 / (values.len() - 1) as f32;
    std::iter::once((4.0, 80.0))
        .chain(
            values
                .iter()
                .enumerate()
                .map(|(index, &y)| (4.0 + index as f32 * step, y)),
        )
        .chain(std::iter::once((96.0, 80.0)))
        .collect()
}

/// A polygon on the ellipse around (50, 50) at `angles`, in turns.
fn on_ellipse(angles: &[f32]) -> Vec<(f32, f32)> {
    angles
        .iter()
        .map(|turns| {
            let angle = turns * std::f32::consts::TAU;
            (50.0 + 44.0 * angle.cos(), 50.0 + 30.0 * angle.sin())
        })
        .collect()
}

/// A generator of numbers in `0..1`, the same on every run.
struct Numbers(u64);

impl Numbers {
    fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

fn contours() -> Vec<(String, Vec<(f32, f32)>)> {
    let mut contours = vec![
        (
            "an area over its baseline".to_string(),
            area(&[44.0, 6.0, 40.0, 30.5, 52.0, 8.25, 20.0, 47.0]),
        ),
        (
            "an area that dips below its baseline".to_string(),
            area(&[60.0, 95.0, 70.0, 79.0, 81.0, 40.0, 100.0]),
        ),
        (
            "a flat area".to_string(),
            area(&[80.0, 80.0, 30.0, 80.0, 80.0]),
        ),
        (
            "a triangle".to_string(),
            vec![(10.0, 6.0), (10.4, 58.0), (54.0, 30.0)],
        ),
        (
            "a bow tie".to_string(),
            vec![(8.0, 10.0), (90.0, 70.0), (90.0, 10.0), (8.0, 70.0)],
        ),
        (
            "a disc".to_string(),
            on_ellipse(&(0..48).map(|index| index as f32 / 48.0).collect::<Vec<_>>()),
        ),
    ];
    let mut numbers = Numbers(7);
    for case in 0..40 {
        let values: Vec<f32> = (0..2 + case % 30)
            .map(|_| 4.0 + numbers.next() * 120.0)
            .collect();
        contours.push((format!("random area {case}"), area(&values)));
        let mut angles: Vec<f32> = (0..3 + case % 12).map(|_| numbers.next()).collect();
        angles.sort_by(f32::total_cmp);
        contours.push((format!("random convex polygon {case}"), on_ellipse(&angles)));
    }
    contours
}

#[test]
fn a_contour_slices_alike_alone_and_beside_another() {
    for (name, points) in contours() {
        let mut alone = Path::new();
        add_contour(&mut alone, &points);
        let mut beside = Path::new();
        add_contour(&mut beside, &points);
        add_contour(&mut beside, &[(110.0, 10.0), (126.0, 10.0), (118.0, 30.0)]);
        let right = points.iter().map(|&(x, _)| x).fold(f32::MIN, f32::max);
        let expected: Vec<Trapezoid> = slices_of(&beside)
            .into_iter()
            .filter(|slice| slice.right <= right)
            .collect();
        assert!(!expected.is_empty(), "{name}: the sweep slices the contour");
        assert_eq!(slices_of(&alone), expected, "{name}");
    }
}
