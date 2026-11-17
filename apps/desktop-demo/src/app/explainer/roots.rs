// Where drips can start on a block of laid-out text: the low points along
// the bottom edges of its letters. The glyphs' outlines are filled on a
// grid once per layout; the bottom edge of every column is joined into arcs,
// and the lowest stretch of each arc is a place where a drop gathers.

use cranpose_text::{GlyphOutline, OutlineSegment, OutlineTransform, TextLayout};
use cranpose_ui::Point;

const CURVE_STEPS: usize = 8;
const LONGEST_ARC: usize = 400;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Root {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub thickness: f32,
}

pub(super) struct Ink {
    columns: usize,
    rows: usize,
    cell: f32,
    filled: Vec<bool>,
}

impl Ink {
    pub(super) fn new(width: f32, height: f32, cell: f32) -> Self {
        let columns = (width / cell).ceil().max(1.0) as usize;
        let rows = (height / cell).ceil().max(1.0) as usize;
        Self {
            columns,
            rows,
            cell,
            filled: vec![false; columns * rows],
        }
    }

    pub(super) fn fill(&mut self, layout: &TextLayout, origin: Point) {
        let mut edges = Vec::new();
        let mut crossings = Vec::new();
        for line in 0..layout.line_count() {
            for run in layout.glyph_runs(line) {
                let transform = OutlineTransform {
                    size: run.size,
                    ..OutlineTransform::default()
                };
                for glyph in run.positions() {
                    let Some(outline) = GlyphOutline::of(run.typeface, glyph.id, transform) else {
                        continue;
                    };
                    let corner = Point::new(
                        origin.x + glyph.x + outline.left() as f32,
                        origin.y + glyph.y + outline.top() as f32,
                    );
                    edges.clear();
                    flatten(&outline, corner, &mut edges);
                    self.fill_edges(&edges, &mut crossings);
                }
            }
        }
    }

    fn fill_edges(&mut self, edges: &[[Point; 2]], crossings: &mut Vec<(f32, i32)>) {
        let (top, bottom) = edges
            .iter()
            .fold((f32::MAX, f32::MIN), |(top, bottom), [a, b]| {
                (top.min(a.y.min(b.y)), bottom.max(a.y.max(b.y)))
            });
        if top > bottom {
            return;
        }
        let first = ((top / self.cell - 0.5).ceil().max(0.0)) as usize;
        let last = ((bottom / self.cell - 0.5).floor()).min(self.rows as f32 - 1.0);
        if last < 0.0 {
            return;
        }
        for row in first..=last as usize {
            let y = (row as f32 + 0.5) * self.cell;
            crossings.clear();
            for [a, b] in edges {
                if (a.y <= y) == (b.y <= y) {
                    continue;
                }
                let x = a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y);
                crossings.push((x, if b.y > a.y { 1 } else { -1 }));
            }
            crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut winding = 0;
            for pair in crossings.windows(2) {
                winding += pair[0].1;
                if winding == 0 {
                    continue;
                }
                let from = (pair[0].0 / self.cell - 0.5).ceil().max(0.0) as usize;
                let to = (pair[1].0 / self.cell - 0.5).floor();
                if to < 0.0 {
                    continue;
                }
                let to = (to as usize).min(self.columns - 1);
                for column in from..=to {
                    self.filled[row * self.columns + column] = true;
                }
            }
        }
    }

    fn ink(&self, column: isize, row: isize) -> bool {
        column >= 0
            && row >= 0
            && (column as usize) < self.columns
            && (row as usize) < self.rows
            && self.filled[row as usize * self.columns + column as usize]
    }

    fn bottom_edge(&self, column: isize, row: isize) -> bool {
        self.ink(column, row) && !self.ink(column, row + 1)
    }

    pub(super) fn roots(&self) -> Vec<Root> {
        let mut taken = vec![false; self.columns * self.rows];
        let mut roots = Vec::new();
        let mut arc = Vec::new();
        for row in 0..self.rows.saturating_sub(1) {
            for column in 0..self.columns {
                let index = row * self.columns + column;
                if taken[index] || !self.bottom_edge(column as isize, row as isize) {
                    continue;
                }
                taken[index] = true;
                arc.clear();
                self.follow((column as isize, row as isize), -1, &mut taken, &mut arc);
                arc.reverse();
                arc.push((column as isize, row as isize));
                self.follow((column as isize, row as isize), 1, &mut taken, &mut arc);
                if arc.len() >= 2 {
                    self.low_points(&arc, &mut roots);
                }
            }
        }
        roots
    }

    fn follow(
        &self,
        start: (isize, isize),
        step: isize,
        taken: &mut [bool],
        arc: &mut Vec<(isize, isize)>,
    ) {
        let (mut column, mut row) = start;
        for _ in 0..LONGEST_ARC {
            let next = column + step;
            let Some(found) = [0, 1, -1, 2, -2].into_iter().map(|dy| row + dy).find(|&y| {
                self.bottom_edge(next, y) && !taken[y as usize * self.columns + next as usize]
            }) else {
                break;
            };
            column = next;
            row = found;
            taken[row as usize * self.columns + column as usize] = true;
            arc.push((column, row));
        }
    }

    fn low_points(&self, arc: &[(isize, isize)], roots: &mut Vec<Root>) {
        let mut start = 0;
        while start < arc.len() {
            let row = arc[start].1;
            let mut end = start;
            while end + 1 < arc.len() && arc[end + 1].1 == row {
                end += 1;
            }
            let before = start.checked_sub(1).map_or(-1, |index| arc[index].1);
            let after = arc.get(end + 1).map_or(-1, |cell| cell.1);
            if before < row && after < row {
                let (mut from, mut to) = (start, end);
                while from > 0 && row - arc[from - 1].1 <= 2 {
                    from -= 1;
                }
                while to + 1 < arc.len() && row - arc[to + 1].1 <= 2 {
                    to += 1;
                }
                let (left, right) = (arc[from].0, arc[to].0);
                let middle = (left + right) / 2;
                let thickness = (0..60).take_while(|&up| self.ink(middle, row - up)).count();
                roots.push(Root {
                    x: (middle as f32 + 0.5) * self.cell,
                    y: (row + 1) as f32 * self.cell,
                    width: (right - left + 1) as f32 * self.cell,
                    thickness: thickness as f32 * self.cell,
                });
            }
            start = end + 1;
        }
    }

    pub(super) fn clear_below(&self, x: f32, y: f32) -> Option<f32> {
        let column = (x / self.cell).round() as isize;
        let first = (y / self.cell).round() as isize + 3;
        (first..self.rows as isize)
            .find(|&row| self.ink(column, row))
            .map(|row| row as f32 * self.cell - y)
    }
}

fn flatten(outline: &GlyphOutline, corner: Point, edges: &mut Vec<[Point; 2]>) {
    let at = |point: Point| Point::new(corner.x + point.x, corner.y + point.y);
    for segment in outline.segments() {
        let mut curve = |point: &dyn Fn(f32) -> Point| {
            let mut last = point(0.0);
            for step in 1..=CURVE_STEPS {
                let next = point(step as f32 / CURVE_STEPS as f32);
                edges.push([last, next]);
                last = next;
            }
        };
        match segment {
            OutlineSegment::Line(a, b) => edges.push([at(a), at(b)]),
            OutlineSegment::Quad(a, b, c) => curve(&|t| {
                let (a, b, c) = (at(a), at(b), at(c));
                lerp(lerp(a, b, t), lerp(b, c, t), t)
            }),
            OutlineSegment::Cubic(a, b, c, d) => curve(&|t| {
                let (a, b, c, d) = (at(a), at(b), at(c), at(d));
                let (ab, bc, cd) = (lerp(a, b, t), lerp(b, c, t), lerp(c, d, t));
                lerp(lerp(ab, bc, t), lerp(bc, cd, t), t)
            }),
        }
    }
}

fn lerp(a: Point, b: Point, t: f32) -> Point {
    Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}
