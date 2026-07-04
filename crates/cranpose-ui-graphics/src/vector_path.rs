//! SVG path-data (`d` attribute) parsing and CPU fill rasterization.
//!
//! [`VectorPath`] parses the SVG path mini-language
//! (`M/m L/l H/h V/v C/c S/s Q/q T/t A/a Z/z`) into subpaths flattened to
//! polylines: curves are subdivided adaptively, arcs are converted via the
//! W3C endpoint-to-center parameterization and sampled. Fills are rendered
//! with an anti-aliased scanline rasterizer into a coverage mask, which the
//! draw pipeline turns into an [`crate::ImageBitmap`] primitive — so every
//! render backend gets vector shapes without new renderer primitives.
//!
//! Parse once (`VectorPath::parse`), draw per frame
//! (`DrawScope::draw_vector_path`); the one-shot
//! `DrawScope::draw_svg_path(d, brush)` convenience re-parses each call.

use crate::geometry::{Point, Rect};
use thiserror::Error;

/// Maximum recursion depth for adaptive curve flattening.
const MAX_FLATTEN_DEPTH: u32 = 12;
/// Curve flattening tolerance in path units.
const FLATTEN_TOLERANCE: f32 = 0.05;
/// Arc sampling: maximum angle step per segment.
const ARC_MAX_ANGLE_STEP: f32 = std::f32::consts::PI / 16.0;
/// Anti-aliasing sub-scanlines per pixel row.
const SUBSAMPLES: usize = 4;

/// Errors produced while parsing SVG path data.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SvgPathError {
    #[error("unexpected byte {byte:?} at offset {offset}")]
    UnexpectedByte { byte: char, offset: usize },
    #[error("expected a number at offset {offset}")]
    ExpectedNumber { offset: usize },
    #[error("expected an arc flag (0 or 1) at offset {offset}")]
    ExpectedFlag { offset: usize },
    #[error("path data must start with a moveto (M/m) command")]
    MissingMoveTo,
}

/// Fill rule for [`VectorPath`] rasterization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PathFillRule {
    /// Fill where the winding number is non-zero (SVG default).
    #[default]
    NonZero,
    /// Fill where a ray crosses an odd number of edges.
    EvenOdd,
}

/// A parsed SVG path: subpaths flattened to polylines, ready to fill.
#[derive(Debug, Clone)]
pub struct VectorPath {
    /// Flattened subpaths. Fill treats every subpath as closed.
    subpaths: Vec<Vec<Point>>,
    fill_rule: PathFillRule,
    bounds: Rect,
}

impl VectorPath {
    /// Parses SVG path data (the `d` attribute syntax).
    pub fn parse(d: &str) -> Result<Self, SvgPathError> {
        let subpaths = parse_path_data(d)?;
        Ok(Self::from_subpaths(subpaths, PathFillRule::NonZero))
    }

    /// Parses SVG path data with an explicit fill rule.
    pub fn parse_with_fill_rule(d: &str, fill_rule: PathFillRule) -> Result<Self, SvgPathError> {
        let subpaths = parse_path_data(d)?;
        Ok(Self::from_subpaths(subpaths, fill_rule))
    }

    fn from_subpaths(subpaths: Vec<Vec<Point>>, fill_rule: PathFillRule) -> Self {
        let mut min = Point::new(f32::INFINITY, f32::INFINITY);
        let mut max = Point::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
        for point in subpaths.iter().flatten() {
            min.x = min.x.min(point.x);
            min.y = min.y.min(point.y);
            max.x = max.x.max(point.x);
            max.y = max.y.max(point.y);
        }
        let bounds = if min.x.is_finite() {
            Rect {
                x: min.x,
                y: min.y,
                width: (max.x - min.x).max(0.0),
                height: (max.y - min.y).max(0.0),
            }
        } else {
            Rect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            }
        };
        Self {
            subpaths,
            fill_rule,
            bounds,
        }
    }

    /// Returns a copy using the given fill rule.
    pub fn with_fill_rule(mut self, fill_rule: PathFillRule) -> Self {
        self.fill_rule = fill_rule;
        self
    }

    /// The fill rule used by [`coverage_mask`](Self::coverage_mask).
    pub fn fill_rule(&self) -> PathFillRule {
        self.fill_rule
    }

    /// Tight bounding box of the flattened path, in path units.
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// Whether the path contains no fillable geometry.
    pub fn is_empty(&self) -> bool {
        !self.subpaths.iter().any(|subpath| subpath.len() >= 3)
    }

    /// Flattened subpaths (each is filled as a closed polygon).
    pub fn subpaths(&self) -> &[Vec<Point>] {
        &self.subpaths
    }

    /// Rasterizes the fill into an anti-aliased 8-bit coverage mask of
    /// `width x height` pixels. A path point `p` maps to the pixel-space
    /// position `(p - origin) * scale`.
    pub fn coverage_mask(&self, width: usize, height: usize, origin: Point, scale: f32) -> Vec<u8> {
        let mut mask = vec![0u8; width * height];
        if width == 0 || height == 0 || scale <= 0.0 {
            return mask;
        }

        // Collect pixel-space edges from all subpaths (implicitly closed).
        struct Edge {
            top: Point,
            bottom: Point,
            /// +1 when the original edge points downward (top -> bottom),
            /// -1 when it points upward.
            winding: i32,
        }
        let mut edges = Vec::new();
        for subpath in &self.subpaths {
            if subpath.len() < 3 {
                continue;
            }
            let map = |p: &Point| Point::new((p.x - origin.x) * scale, (p.y - origin.y) * scale);
            for i in 0..subpath.len() {
                let a = map(&subpath[i]);
                let b = map(&subpath[(i + 1) % subpath.len()]);
                if a.y == b.y {
                    continue;
                }
                if a.y < b.y {
                    edges.push(Edge {
                        top: a,
                        bottom: b,
                        winding: 1,
                    });
                } else {
                    edges.push(Edge {
                        top: b,
                        bottom: a,
                        winding: -1,
                    });
                }
            }
        }
        if edges.is_empty() {
            return mask;
        }

        let mut crossings: Vec<(f32, i32)> = Vec::new();
        let mut row_coverage = vec![0.0f32; width];
        let subsample_weight = 1.0 / SUBSAMPLES as f32;

        for row in 0..height {
            row_coverage.fill(0.0);
            let mut row_touched = false;

            for sub in 0..SUBSAMPLES {
                let sample_y = row as f32 + (sub as f32 + 0.5) * subsample_weight;

                crossings.clear();
                for edge in &edges {
                    if edge.top.y <= sample_y && sample_y < edge.bottom.y {
                        let t = (sample_y - edge.top.y) / (edge.bottom.y - edge.top.y);
                        let x = edge.top.x + t * (edge.bottom.x - edge.top.x);
                        crossings.push((x, edge.winding));
                    }
                }
                if crossings.len() < 2 {
                    continue;
                }
                crossings.sort_by(|a, b| a.0.total_cmp(&b.0));

                // Walk crossings, accumulating spans per fill rule.
                let mut winding = 0i32;
                let mut span_start = 0.0f32;
                for &(x, direction) in crossings.iter() {
                    let was_inside = match self.fill_rule {
                        PathFillRule::NonZero => winding != 0,
                        PathFillRule::EvenOdd => winding % 2 != 0,
                    };
                    winding += match self.fill_rule {
                        PathFillRule::NonZero => direction,
                        PathFillRule::EvenOdd => 1,
                    };
                    let is_inside = match self.fill_rule {
                        PathFillRule::NonZero => winding != 0,
                        PathFillRule::EvenOdd => winding % 2 != 0,
                    };
                    if !was_inside && is_inside {
                        span_start = x;
                    } else if was_inside && !is_inside {
                        row_touched |= accumulate_span(
                            &mut row_coverage,
                            span_start,
                            x,
                            subsample_weight,
                            width,
                        );
                    }
                }
            }

            if row_touched {
                let mask_row = &mut mask[row * width..(row + 1) * width];
                for (dst, coverage) in mask_row.iter_mut().zip(row_coverage.iter()) {
                    let existing = *dst as f32 / 255.0;
                    let combined = (existing + coverage).min(1.0);
                    *dst = (combined * 255.0 + 0.5) as u8;
                }
            }
        }

        mask
    }
}

/// Adds one horizontal span `[x0, x1)` of one sub-scanline into the row
/// coverage accumulator, handling fractional span ends. Returns whether any
/// pixel was touched.
fn accumulate_span(row_coverage: &mut [f32], x0: f32, x1: f32, weight: f32, width: usize) -> bool {
    let x0 = x0.max(0.0);
    let x1 = x1.min(width as f32);
    if x1 <= x0 {
        return false;
    }

    let first = x0.floor() as usize;
    let last = (x1.ceil() as usize).min(width);
    for (pixel, coverage) in row_coverage.iter_mut().enumerate().take(last).skip(first) {
        let pixel_start = pixel as f32;
        let pixel_end = pixel_start + 1.0;
        let covered = (x1.min(pixel_end) - x0.max(pixel_start)).max(0.0);
        *coverage += covered * weight;
    }
    true
}

// ============================================================================
// Path data parsing
// ============================================================================

struct PathLexer<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> PathLexer<'a> {
    fn new(d: &'a str) -> Self {
        Self {
            bytes: d.as_bytes(),
            pos: 0,
        }
    }

    fn skip_separators(&mut self) {
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b' ' | b'\t' | b'\r' | b'\n' | b',' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_separators();
        self.bytes.get(self.pos).copied()
    }

    /// Whether the next token can start a number.
    fn at_number(&mut self) -> bool {
        matches!(self.peek(), Some(b'0'..=b'9' | b'.' | b'-' | b'+'))
    }

    fn next_command(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        if byte.is_ascii_alphabetic() {
            self.pos += 1;
            Some(byte)
        } else {
            None
        }
    }

    /// Parses one SVG number: `[+-]? (digits [. digits?]? | . digits) exponent?`.
    /// A second `.` terminates the number, so `1.5.5` lexes as `1.5`, `.5`.
    fn next_number(&mut self) -> Result<f32, SvgPathError> {
        self.skip_separators();
        let start = self.pos;
        let bytes = self.bytes;
        let mut pos = self.pos;

        if pos < bytes.len() && (bytes[pos] == b'+' || bytes[pos] == b'-') {
            pos += 1;
        }
        let int_digits = Self::eat_digits(bytes, &mut pos);
        let mut frac_digits = 0;
        if pos < bytes.len() && bytes[pos] == b'.' {
            pos += 1;
            frac_digits = Self::eat_digits(bytes, &mut pos);
        }
        if int_digits == 0 && frac_digits == 0 {
            return Err(SvgPathError::ExpectedNumber { offset: start });
        }
        if pos < bytes.len() && (bytes[pos] == b'e' || bytes[pos] == b'E') {
            let mut exp_pos = pos + 1;
            if exp_pos < bytes.len() && (bytes[exp_pos] == b'+' || bytes[exp_pos] == b'-') {
                exp_pos += 1;
            }
            if Self::eat_digits(bytes, &mut exp_pos) > 0 {
                pos = exp_pos;
            }
        }

        let text = std::str::from_utf8(&bytes[start..pos])
            .map_err(|_| SvgPathError::ExpectedNumber { offset: start })?;
        let value = text
            .parse::<f32>()
            .map_err(|_| SvgPathError::ExpectedNumber { offset: start })?;
        self.pos = pos;
        Ok(value)
    }

    fn eat_digits(bytes: &[u8], pos: &mut usize) -> usize {
        let start = *pos;
        while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
            *pos += 1;
        }
        *pos - start
    }

    /// Arc flags are single characters and may be packed (`110 10` etc).
    fn next_flag(&mut self) -> Result<bool, SvgPathError> {
        self.skip_separators();
        match self.bytes.get(self.pos) {
            Some(b'0') => {
                self.pos += 1;
                Ok(false)
            }
            Some(b'1') => {
                self.pos += 1;
                Ok(true)
            }
            _ => Err(SvgPathError::ExpectedFlag { offset: self.pos }),
        }
    }

    fn at_end(&mut self) -> bool {
        self.peek().is_none()
    }
}

struct PathBuilder {
    subpaths: Vec<Vec<Point>>,
    current: Vec<Point>,
    position: Point,
    subpath_start: Point,
    /// Reflection anchors for smooth curves (S/T).
    last_cubic_control: Option<Point>,
    last_quad_control: Option<Point>,
}

impl PathBuilder {
    fn new() -> Self {
        Self {
            subpaths: Vec::new(),
            current: Vec::new(),
            position: Point::ZERO,
            subpath_start: Point::ZERO,
            last_cubic_control: None,
            last_quad_control: None,
        }
    }

    fn flush_subpath(&mut self) {
        if self.current.len() >= 2 {
            self.subpaths.push(std::mem::take(&mut self.current));
        } else {
            self.current.clear();
        }
    }

    fn move_to(&mut self, point: Point) {
        self.flush_subpath();
        self.position = point;
        self.subpath_start = point;
        self.current.push(point);
    }

    fn line_to(&mut self, point: Point) {
        if self.current.is_empty() {
            self.current.push(self.position);
        }
        self.current.push(point);
        self.position = point;
    }

    fn close(&mut self) {
        self.position = self.subpath_start;
        self.flush_subpath();
        // Commands after Z continue from the subpath start.
        self.current.push(self.subpath_start);
    }

    fn finish(mut self) -> Vec<Vec<Point>> {
        self.flush_subpath();
        self.subpaths
    }
}

fn parse_path_data(d: &str) -> Result<Vec<Vec<Point>>, SvgPathError> {
    let mut lexer = PathLexer::new(d);
    let mut builder = PathBuilder::new();
    let mut command: Option<u8> = None;
    let mut seen_moveto = false;

    loop {
        if lexer.at_end() {
            break;
        }

        if let Some(next) = lexer.next_command() {
            command = Some(next);
        } else if command.is_none() || !lexer.at_number() {
            let offset = lexer.pos;
            let byte = lexer.bytes.get(offset).copied().unwrap_or(b'?') as char;
            return Err(SvgPathError::UnexpectedByte { byte, offset });
        }

        let Some(cmd) = command else {
            return Err(SvgPathError::MissingMoveTo);
        };
        if !seen_moveto && !matches!(cmd, b'M' | b'm') {
            return Err(SvgPathError::MissingMoveTo);
        }
        let relative = cmd.is_ascii_lowercase();
        let pos = builder.position;
        let rel = |value: Point| {
            if relative {
                Point::new(pos.x + value.x, pos.y + value.y)
            } else {
                value
            }
        };

        match cmd.to_ascii_uppercase() {
            b'M' => {
                let point = rel(read_point(&mut lexer)?);
                builder.move_to(point);
                seen_moveto = true;
                builder.last_cubic_control = None;
                builder.last_quad_control = None;
                // Extra coordinate pairs are implicit linetos.
                command = Some(if relative { b'l' } else { b'L' });
            }
            b'L' => {
                let point = rel(read_point(&mut lexer)?);
                builder.line_to(point);
                builder.last_cubic_control = None;
                builder.last_quad_control = None;
            }
            b'H' => {
                let x = lexer.next_number()?;
                let x = if relative { pos.x + x } else { x };
                builder.line_to(Point::new(x, pos.y));
                builder.last_cubic_control = None;
                builder.last_quad_control = None;
            }
            b'V' => {
                let y = lexer.next_number()?;
                let y = if relative { pos.y + y } else { y };
                builder.line_to(Point::new(pos.x, y));
                builder.last_cubic_control = None;
                builder.last_quad_control = None;
            }
            b'C' => {
                let c1 = rel(read_point(&mut lexer)?);
                let c2 = rel(read_point(&mut lexer)?);
                let end = rel(read_point(&mut lexer)?);
                emit_cubic(&mut builder, c1, c2, end);
            }
            b'S' => {
                let c1 = match builder.last_cubic_control {
                    Some(control) => reflect(pos, control),
                    None => pos,
                };
                let c2 = rel(read_point(&mut lexer)?);
                let end = rel(read_point(&mut lexer)?);
                emit_cubic(&mut builder, c1, c2, end);
            }
            b'Q' => {
                let control = rel(read_point(&mut lexer)?);
                let end = rel(read_point(&mut lexer)?);
                emit_quad(&mut builder, control, end);
            }
            b'T' => {
                let control = match builder.last_quad_control {
                    Some(control) => reflect(pos, control),
                    None => pos,
                };
                let end = rel(read_point(&mut lexer)?);
                emit_quad(&mut builder, control, end);
            }
            b'A' => {
                let rx = lexer.next_number()?;
                let ry = lexer.next_number()?;
                let x_rotation_deg = lexer.next_number()?;
                let large_arc = lexer.next_flag()?;
                let sweep = lexer.next_flag()?;
                let end = rel(read_point(&mut lexer)?);
                emit_arc(&mut builder, rx, ry, x_rotation_deg, large_arc, sweep, end);
                builder.last_cubic_control = None;
                builder.last_quad_control = None;
            }
            b'Z' => {
                builder.close();
                builder.last_cubic_control = None;
                builder.last_quad_control = None;
                // Z takes no arguments; require an explicit next command.
                command = None;
            }
            other => {
                return Err(SvgPathError::UnexpectedByte {
                    byte: other as char,
                    offset: lexer.pos.saturating_sub(1),
                });
            }
        }
    }

    if !seen_moveto {
        return Err(SvgPathError::MissingMoveTo);
    }
    Ok(builder.finish())
}

fn read_point(lexer: &mut PathLexer<'_>) -> Result<Point, SvgPathError> {
    let x = lexer.next_number()?;
    let y = lexer.next_number()?;
    Ok(Point::new(x, y))
}

fn reflect(origin: Point, point: Point) -> Point {
    Point::new(2.0 * origin.x - point.x, 2.0 * origin.y - point.y)
}

fn emit_cubic(builder: &mut PathBuilder, c1: Point, c2: Point, end: Point) {
    let start = builder.position;
    flatten_cubic(builder, start, c1, c2, end, 0);
    builder.position = end;
    builder.last_cubic_control = Some(c2);
    builder.last_quad_control = None;
}

fn emit_quad(builder: &mut PathBuilder, control: Point, end: Point) {
    // Elevate the quadratic to a cubic and reuse the cubic flattener.
    let start = builder.position;
    let c1 = Point::new(
        start.x + 2.0 / 3.0 * (control.x - start.x),
        start.y + 2.0 / 3.0 * (control.y - start.y),
    );
    let c2 = Point::new(
        end.x + 2.0 / 3.0 * (control.x - end.x),
        end.y + 2.0 / 3.0 * (control.y - end.y),
    );
    flatten_cubic(builder, start, c1, c2, end, 0);
    builder.position = end;
    builder.last_quad_control = Some(control);
    builder.last_cubic_control = None;
}

fn flatten_cubic(
    builder: &mut PathBuilder,
    p0: Point,
    p1: Point,
    p2: Point,
    p3: Point,
    depth: u32,
) {
    if depth >= MAX_FLATTEN_DEPTH || cubic_is_flat(p0, p1, p2, p3) {
        builder.line_to(p3);
        return;
    }

    let mid = |a: Point, b: Point| Point::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
    let p01 = mid(p0, p1);
    let p12 = mid(p1, p2);
    let p23 = mid(p2, p3);
    let p012 = mid(p01, p12);
    let p123 = mid(p12, p23);
    let p0123 = mid(p012, p123);

    flatten_cubic(builder, p0, p01, p012, p0123, depth + 1);
    flatten_cubic(builder, p0123, p123, p23, p3, depth + 1);
}

/// Flatness test: both control points close enough to the chord.
fn cubic_is_flat(p0: Point, p1: Point, p2: Point, p3: Point) -> bool {
    let d1 = point_to_chord_distance_squared(p1, p0, p3);
    let d2 = point_to_chord_distance_squared(p2, p0, p3);
    let tolerance = FLATTEN_TOLERANCE * FLATTEN_TOLERANCE;
    d1 <= tolerance && d2 <= tolerance
}

fn point_to_chord_distance_squared(point: Point, a: Point, b: Point) -> f32 {
    let ab = Point::new(b.x - a.x, b.y - a.y);
    let ap = Point::new(point.x - a.x, point.y - a.y);
    let ab_len_sq = ab.x * ab.x + ab.y * ab.y;
    if ab_len_sq <= f32::EPSILON {
        return ap.x * ap.x + ap.y * ap.y;
    }
    let cross = ab.x * ap.y - ab.y * ap.x;
    cross * cross / ab_len_sq
}

/// Converts an SVG endpoint-parameterized arc to line segments
/// (W3C SVG 2 appendix B.2.4).
fn emit_arc(
    builder: &mut PathBuilder,
    rx: f32,
    ry: f32,
    x_rotation_deg: f32,
    large_arc: bool,
    sweep: bool,
    end: Point,
) {
    let start = builder.position;
    if (start.x - end.x).abs() <= f32::EPSILON && (start.y - end.y).abs() <= f32::EPSILON {
        return;
    }
    let mut rx = rx.abs();
    let mut ry = ry.abs();
    if rx <= f32::EPSILON || ry <= f32::EPSILON {
        builder.line_to(end);
        return;
    }

    let phi = x_rotation_deg.to_radians();
    let (sin_phi, cos_phi) = phi.sin_cos();

    // Step 1: half the vector between endpoints, in the rotated frame.
    let dx2 = (start.x - end.x) * 0.5;
    let dy2 = (start.y - end.y) * 0.5;
    let x1p = cos_phi * dx2 + sin_phi * dy2;
    let y1p = -sin_phi * dx2 + cos_phi * dy2;

    // Correct out-of-range radii.
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        let scale = lambda.sqrt();
        rx *= scale;
        ry *= scale;
    }

    // Step 2: center in the rotated frame.
    let rx_sq = rx * rx;
    let ry_sq = ry * ry;
    let numerator = (rx_sq * ry_sq - rx_sq * y1p * y1p - ry_sq * x1p * x1p).max(0.0);
    let denominator = rx_sq * y1p * y1p + ry_sq * x1p * x1p;
    let mut coefficient = if denominator <= f32::EPSILON {
        0.0
    } else {
        (numerator / denominator).sqrt()
    };
    if large_arc == sweep {
        coefficient = -coefficient;
    }
    let cxp = coefficient * rx * y1p / ry;
    let cyp = -coefficient * ry * x1p / rx;

    // Step 3: center in the original frame.
    let cx = cos_phi * cxp - sin_phi * cyp + (start.x + end.x) * 0.5;
    let cy = sin_phi * cxp + cos_phi * cyp + (start.y + end.y) * 0.5;

    // Step 4: start angle and sweep extent.
    let angle_of = |x: f32, y: f32| y.atan2(x);
    let theta1 = angle_of((x1p - cxp) / rx, (y1p - cyp) / ry);
    let theta2 = angle_of((-x1p - cxp) / rx, (-y1p - cyp) / ry);
    let two_pi = std::f32::consts::TAU;
    let mut delta = theta2 - theta1;
    if sweep {
        if delta < 0.0 {
            delta += two_pi;
        }
    } else if delta > 0.0 {
        delta -= two_pi;
    }

    let segments = ((delta.abs() / ARC_MAX_ANGLE_STEP).ceil() as usize).max(2);
    for i in 1..=segments {
        let theta = theta1 + delta * (i as f32 / segments as f32);
        let (sin_theta, cos_theta) = theta.sin_cos();
        let x = cos_phi * rx * cos_theta - sin_phi * ry * sin_theta + cx;
        let y = sin_phi * rx * cos_theta + cos_phi * ry * sin_theta + cy;
        builder.line_to(Point::new(x, y));
    }
    // Land exactly on the endpoint despite floating-point sampling error.
    builder.line_to(end);
    builder.position = end;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask_at(mask: &[u8], width: usize, x: usize, y: usize) -> u8 {
        mask[y * width + x]
    }

    // ── parser ──────────────────────────────────────────────────────────

    #[test]
    fn parses_absolute_triangle() {
        let path = VectorPath::parse("M 0 0 L 10 0 L 10 10 Z").expect("valid path");
        assert_eq!(path.subpaths().len(), 1);
        assert_eq!(
            path.subpaths()[0],
            vec![
                Point::new(0.0, 0.0),
                Point::new(10.0, 0.0),
                Point::new(10.0, 10.0)
            ]
        );
        let bounds = path.bounds();
        assert_eq!((bounds.x, bounds.y), (0.0, 0.0));
        assert_eq!((bounds.width, bounds.height), (10.0, 10.0));
    }

    #[test]
    fn parses_relative_commands_and_h_v() {
        let path = VectorPath::parse("m 5 5 l 10 0 v 10 h -10 z").expect("valid path");
        assert_eq!(
            path.subpaths()[0],
            vec![
                Point::new(5.0, 5.0),
                Point::new(15.0, 5.0),
                Point::new(15.0, 15.0),
                Point::new(5.0, 15.0)
            ]
        );
    }

    #[test]
    fn parses_packed_numbers_and_negative_shorthand() {
        // "10-5" is two numbers; ".5.5" is (0.5, 0.5).
        let path = VectorPath::parse("M10-5L.5.5Z").expect("valid path");
        assert_eq!(
            path.subpaths()[0],
            vec![Point::new(10.0, -5.0), Point::new(0.5, 0.5)]
        );
    }

    #[test]
    fn implicit_lineto_after_moveto() {
        let path = VectorPath::parse("M 0 0 10 0 10 10").expect("valid path");
        assert_eq!(path.subpaths()[0].len(), 3);
        assert_eq!(path.subpaths()[0][2], Point::new(10.0, 10.0));
    }

    #[test]
    fn cubic_flattening_hits_endpoints() {
        let path = VectorPath::parse("M 0 0 C 0 10 10 10 10 0").expect("valid path");
        let points = &path.subpaths()[0];
        assert_eq!(points[0], Point::new(0.0, 0.0));
        assert_eq!(*points.last().unwrap(), Point::new(10.0, 0.0));
        assert!(points.len() > 4, "curve must be subdivided");
        // The curve midpoint of this symmetric cubic is (5, 7.5).
        let mid = points
            .iter()
            .min_by(|a, b| (a.x - 5.0).abs().total_cmp(&(b.x - 5.0).abs()))
            .unwrap();
        assert!(
            (mid.y - 7.5).abs() < 0.2,
            "flattened curve must pass near the true midpoint, got {mid:?}"
        );
    }

    #[test]
    fn smooth_cubic_reflects_control_point() {
        // S after C reflects the previous control point; the joined curves
        // are C1-continuous, so the polyline has no kink at the join (5,5).
        let path = VectorPath::parse("M 0 0 C 0 5 2 5 5 5 S 10 5 10 10").expect("valid path");
        let points = &path.subpaths()[0];
        assert_eq!(*points.last().unwrap(), Point::new(10.0, 10.0));
        assert!(points
            .iter()
            .any(|p| (p.x - 5.0).abs() < 0.1 && (p.y - 5.0).abs() < 0.1));
    }

    #[test]
    fn quadratic_and_smooth_quadratic() {
        let path = VectorPath::parse("M 0 0 Q 5 10 10 0 T 20 0").expect("valid path");
        let points = &path.subpaths()[0];
        assert_eq!(*points.last().unwrap(), Point::new(20.0, 0.0));
        // Quadratic apex at t=0.5 is (5, 5).
        assert!(points
            .iter()
            .any(|p| (p.x - 5.0).abs() < 0.3 && (p.y - 5.0).abs() < 0.3));
        // T mirrors the control: the second hump dips to (15, -5).
        assert!(points
            .iter()
            .any(|p| (p.x - 15.0).abs() < 0.3 && (p.y + 5.0).abs() < 0.3));
    }

    #[test]
    fn arc_travels_through_expected_quadrant() {
        // Half circle of radius 5 from (0,0) to (10,0), sweeping below.
        let path = VectorPath::parse("M 0 0 A 5 5 0 0 1 10 0").expect("valid path");
        let points = &path.subpaths()[0];
        assert_eq!(*points.last().unwrap(), Point::new(10.0, 0.0));
        let lowest = points.iter().fold(0.0f32, |acc, p| acc.min(p.y));
        assert!(
            (lowest + 5.0).abs() < 0.1,
            "sweep=1 arc must pass through (5,-5), lowest y = {lowest}"
        );

        let path = VectorPath::parse("M 0 0 A 5 5 0 0 0 10 0").expect("valid path");
        let highest = path.subpaths()[0]
            .iter()
            .fold(0.0f32, |acc, p| acc.max(p.y));
        assert!(
            (highest - 5.0).abs() < 0.1,
            "sweep=0 arc must pass through (5,5), highest y = {highest}"
        );
    }

    #[test]
    fn arc_flags_may_be_packed() {
        let spaced = VectorPath::parse("M 0 0 A 5 5 0 0 1 10 0").expect("valid path");
        let packed = VectorPath::parse("M0 0A5 5 0 0110 0").expect("valid path");
        assert_eq!(
            spaced.subpaths()[0].len(),
            packed.subpaths()[0].len(),
            "packed arc flags must parse identically"
        );
    }

    #[test]
    fn multiple_subpaths() {
        let path =
            VectorPath::parse("M 0 0 h 4 v 4 h -4 Z M 10 10 h 4 v 4 h -4 Z").expect("valid path");
        assert_eq!(path.subpaths().len(), 2);
    }

    #[test]
    fn rejects_garbage() {
        assert!(VectorPath::parse("this is not a path").is_err());
        assert!(
            VectorPath::parse("L 10 10").is_err(),
            "must start with moveto"
        );
        assert!(VectorPath::parse("M 10").is_err(), "missing y coordinate");
        assert!(
            VectorPath::parse("M 0 0 A 5 5 0 2 1 10 0").is_err(),
            "bad flag"
        );
        assert_eq!(
            VectorPath::parse("").unwrap_err(),
            SvgPathError::MissingMoveTo
        );
    }

    // ── rasterizer ──────────────────────────────────────────────────────

    #[test]
    fn fills_axis_aligned_rectangle() {
        let path = VectorPath::parse("M 2 2 H 8 V 8 H 2 Z").expect("valid path");
        let mask = path.coverage_mask(10, 10, Point::ZERO, 1.0);

        assert_eq!(mask_at(&mask, 10, 5, 5), 255, "interior must be opaque");
        assert_eq!(mask_at(&mask, 10, 4, 2), 255, "top edge row is inside");
        assert_eq!(mask_at(&mask, 10, 0, 0), 0, "outside must stay empty");
        assert_eq!(mask_at(&mask, 10, 9, 9), 0, "outside must stay empty");
    }

    #[test]
    fn triangle_edge_is_antialiased() {
        let path = VectorPath::parse("M 0 0 L 8 0 L 0 8 Z").expect("valid path");
        let mask = path.coverage_mask(8, 8, Point::ZERO, 1.0);

        assert_eq!(mask_at(&mask, 8, 1, 1), 255, "deep interior is opaque");
        assert_eq!(mask_at(&mask, 8, 7, 7), 0, "far corner is empty");
        // Pixels straddling the diagonal must have partial coverage.
        let diagonal = mask_at(&mask, 8, 4, 3);
        assert!(
            diagonal > 30 && diagonal < 225,
            "diagonal pixel should be partially covered, got {diagonal}"
        );
    }

    #[test]
    fn even_odd_ring_has_a_hole() {
        // Outer square with an inner square drawn in the SAME winding
        // direction: even-odd punches the hole, non-zero fills it solid.
        let d = "M 0 0 H 12 V 12 H 0 Z M 4 4 H 8 V 8 H 4 Z";
        let even_odd =
            VectorPath::parse_with_fill_rule(d, PathFillRule::EvenOdd).expect("valid path");
        let non_zero = VectorPath::parse(d).expect("valid path");

        let even_odd_mask = even_odd.coverage_mask(12, 12, Point::ZERO, 1.0);
        let non_zero_mask = non_zero.coverage_mask(12, 12, Point::ZERO, 1.0);

        assert_eq!(mask_at(&even_odd_mask, 12, 6, 6), 0, "even-odd hole");
        assert_eq!(mask_at(&even_odd_mask, 12, 2, 6), 255, "even-odd ring");
        assert_eq!(mask_at(&non_zero_mask, 12, 6, 6), 255, "non-zero solid");
    }

    #[test]
    fn non_zero_ring_with_reversed_inner_winding_has_a_hole() {
        // Inner square wound the opposite way: non-zero also punches it.
        let d = "M 0 0 H 12 V 12 H 0 Z M 4 4 V 8 H 8 V 4 Z";
        let path = VectorPath::parse(d).expect("valid path");
        let mask = path.coverage_mask(12, 12, Point::ZERO, 1.0);
        assert_eq!(mask_at(&mask, 12, 6, 6), 0, "reversed winding hole");
        assert_eq!(mask_at(&mask, 12, 2, 6), 255, "ring stays filled");
    }

    #[test]
    fn circle_from_arcs_fills_center_and_respects_radius() {
        // Full circle of radius 8 centered at (8, 8) from two arcs.
        let path =
            VectorPath::parse("M 0 8 A 8 8 0 1 1 16 8 A 8 8 0 1 1 0 8 Z").expect("valid path");
        let mask = path.coverage_mask(16, 16, Point::ZERO, 1.0);

        assert_eq!(mask_at(&mask, 16, 8, 8), 255, "circle center is opaque");
        assert_eq!(mask_at(&mask, 16, 0, 0), 0, "circle corner is empty");
        assert_eq!(mask_at(&mask, 16, 15, 0), 0, "circle corner is empty");
        // Roughly correct area: sum of coverage ~ pi * r^2.
        let area: f32 = mask.iter().map(|&value| value as f32 / 255.0).sum();
        let expected = std::f32::consts::PI * 8.0 * 8.0;
        assert!(
            (area - expected).abs() / expected < 0.05,
            "filled area {area} should be close to {expected}"
        );
    }

    #[test]
    fn scale_and_origin_map_path_units_to_pixels() {
        let path = VectorPath::parse("M 10 10 H 14 V 14 H 10 Z").expect("valid path");
        // Rasterize the 4x4 square at 2x with the mask origin at (10, 10).
        let mask = path.coverage_mask(8, 8, Point::new(10.0, 10.0), 2.0);
        assert_eq!(mask_at(&mask, 8, 4, 4), 255, "scaled interior");
        let full: usize = mask.iter().filter(|&&value| value == 255).count();
        assert_eq!(full, 64, "the 8x8 pixel mask must be fully covered");
    }

    #[test]
    fn empty_and_degenerate_paths_produce_empty_masks() {
        let path = VectorPath::parse("M 5 5 L 6 6").expect("valid path");
        assert!(path.is_empty());
        let mask = path.coverage_mask(8, 8, Point::ZERO, 1.0);
        assert!(mask.iter().all(|&value| value == 0));
    }
}
