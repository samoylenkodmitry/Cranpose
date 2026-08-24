use tiny_skia::{
    FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Rect as SkiaRect, Stroke,
    Transform,
};

use super::SvgPainterError;
use crate::modifier::Size;

#[derive(Debug)]
pub(super) struct SvgDocument {
    intrinsic_size: Size,
    view_box: SvgViewBox,
    elements: Vec<SvgElement>,
}

#[derive(Clone, Copy, Debug)]
struct SvgViewBox {
    min_x: f32,
    min_y: f32,
    width: f32,
    height: f32,
}

#[derive(Clone, Debug)]
enum SvgElement {
    Rect(SvgRect),
    Path(SvgPath),
}

#[derive(Clone, Debug)]
struct SvgRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    fill: Option<SvgColor>,
    stroke: Option<SvgStroke>,
}

#[derive(Clone, Debug)]
struct SvgPath {
    path: Path,
    fill: Option<SvgColor>,
    stroke: Option<SvgStroke>,
}

#[derive(Clone, Copy, Debug)]
struct SvgColor {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

#[derive(Clone, Debug)]
struct SvgStroke {
    color: SvgColor,
    width: f32,
    line_cap: LineCap,
    line_join: LineJoin,
}

#[derive(Clone, Debug)]
struct SvgTag {
    name: String,
    attributes: Vec<(String, String)>,
}

impl SvgDocument {
    pub(super) fn intrinsic_size(&self) -> Size {
        self.intrinsic_size
    }
}

pub(super) fn parse_svg_document(bytes: &[u8]) -> Result<SvgDocument, SvgPainterError> {
    let source =
        std::str::from_utf8(bytes).map_err(|error| SvgPainterError::Parse(error.to_string()))?;
    let tags = parse_tags(source)?;
    let svg_tag = tags
        .iter()
        .find(|tag| tag.name == "svg")
        .ok_or_else(|| SvgPainterError::Parse("missing <svg> root element".to_string()))?;
    let intrinsic_size = parse_intrinsic_size(svg_tag)?;
    let view_box = parse_view_box(svg_tag, intrinsic_size)?;
    let mut elements = Vec::new();

    for tag in tags.iter().filter(|tag| tag.name != "svg") {
        match tag.name.as_str() {
            "rect" => {
                if let Some(rect) = parse_rect(tag)? {
                    elements.push(SvgElement::Rect(rect));
                }
            }
            "path" => {
                if let Some(path) = parse_path_element(tag)? {
                    elements.push(SvgElement::Path(path));
                }
            }
            "g" | "defs" | "title" | "desc" | "metadata" => {}
            name => {
                return Err(SvgPainterError::Parse(format!(
                    "unsupported SVG element <{name}>"
                )));
            }
        }
    }

    Ok(SvgDocument {
        intrinsic_size,
        view_box,
        elements,
    })
}

pub(super) fn rasterize_svg_document(document: &SvgDocument, pixmap: &mut Pixmap) {
    let transform = document.raster_transform(pixmap.width(), pixmap.height());
    let mut pixmap = pixmap.as_mut();

    for element in &document.elements {
        match element {
            SvgElement::Rect(rect) => draw_rect(&mut pixmap, rect, transform),
            SvgElement::Path(path) => draw_path(&mut pixmap, path, transform),
        }
    }
}

pub(super) fn demultiplied_rgba_pixels(pixmap: &Pixmap) -> Vec<u8> {
    pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect()
}

impl SvgDocument {
    fn raster_transform(&self, width: u32, height: u32) -> Transform {
        let sx = width as f32 / self.view_box.width;
        let sy = height as f32 / self.view_box.height;
        Transform::from_row(
            sx,
            0.0,
            0.0,
            sy,
            -self.view_box.min_x * sx,
            -self.view_box.min_y * sy,
        )
    }
}

fn draw_rect(pixmap: &mut tiny_skia::PixmapMut<'_>, rect: &SvgRect, transform: Transform) {
    if let Some(fill) = rect.fill {
        if let Some(skia_rect) = SkiaRect::from_xywh(rect.x, rect.y, rect.width, rect.height) {
            let mut paint = Paint::default();
            fill.apply_to(&mut paint);
            pixmap.fill_rect(skia_rect, &paint, transform, None);
        }
    }

    if let Some(stroke) = &rect.stroke {
        if let Some(path) = rect_path(rect) {
            stroke_path(pixmap, &path, stroke, transform);
        }
    }
}

fn draw_path(pixmap: &mut tiny_skia::PixmapMut<'_>, path: &SvgPath, transform: Transform) {
    if let Some(fill) = path.fill {
        let mut paint = Paint::default();
        fill.apply_to(&mut paint);
        pixmap.fill_path(&path.path, &paint, FillRule::Winding, transform, None);
    }

    if let Some(stroke) = &path.stroke {
        stroke_path(pixmap, &path.path, stroke, transform);
    }
}

fn stroke_path(
    pixmap: &mut tiny_skia::PixmapMut<'_>,
    path: &Path,
    svg_stroke: &SvgStroke,
    transform: Transform,
) {
    let mut paint = Paint::default();
    svg_stroke.color.apply_to(&mut paint);
    let stroke = Stroke {
        width: svg_stroke.width,
        line_cap: svg_stroke.line_cap,
        line_join: svg_stroke.line_join,
        ..Stroke::default()
    };
    pixmap.stroke_path(path, &paint, &stroke, transform, None);
}

fn rect_path(rect: &SvgRect) -> Option<Path> {
    let mut builder = PathBuilder::new();
    builder.move_to(rect.x, rect.y);
    builder.line_to(rect.x + rect.width, rect.y);
    builder.line_to(rect.x + rect.width, rect.y + rect.height);
    builder.line_to(rect.x, rect.y + rect.height);
    builder.close();
    builder.finish()
}

impl SvgColor {
    fn apply_to(self, paint: &mut Paint<'_>) {
        paint.set_color_rgba8(self.r, self.g, self.b, self.a);
    }
}

fn parse_tags(source: &str) -> Result<Vec<SvgTag>, SvgPainterError> {
    let mut tags = Vec::new();
    let mut remainder = source;
    while let Some(start) = remainder.find('<') {
        remainder = &remainder[start + 1..];
        let Some(end) = remainder.find('>') else {
            return Err(SvgPainterError::Parse("unterminated SVG tag".to_string()));
        };
        let mut body = remainder[..end].trim();
        remainder = &remainder[end + 1..];

        if body.is_empty()
            || body.starts_with('/')
            || body.starts_with('?')
            || body.starts_with("!--")
            || body.starts_with("!DOCTYPE")
        {
            continue;
        }
        if let Some(stripped) = body.strip_suffix('/') {
            body = stripped.trim_end();
        }

        let name_end = body
            .find(|ch: char| ch.is_ascii_whitespace())
            .unwrap_or(body.len());
        let name = body[..name_end].to_ascii_lowercase();
        let attributes = parse_attributes(&body[name_end..])?;
        tags.push(SvgTag { name, attributes });
    }

    Ok(tags)
}

fn parse_attributes(source: &str) -> Result<Vec<(String, String)>, SvgPainterError> {
    let bytes = source.as_bytes();
    let mut index = 0;
    let mut attributes = Vec::new();

    while index < bytes.len() {
        while index < bytes.len() && (bytes[index].is_ascii_whitespace() || bytes[index] == b'/') {
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }

        let key_start = index;
        while index < bytes.len()
            && !bytes[index].is_ascii_whitespace()
            && bytes[index] != b'='
            && bytes[index] != b'/'
        {
            index += 1;
        }
        let key = source[key_start..index].to_ascii_lowercase();

        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != b'=' {
            attributes.push((key, String::new()));
            continue;
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() {
            return Err(SvgPainterError::Parse(
                "attribute value is missing".to_string(),
            ));
        }

        let value = if bytes[index] == b'"' || bytes[index] == b'\'' {
            let quote = bytes[index];
            index += 1;
            let value_start = index;
            while index < bytes.len() && bytes[index] != quote {
                index += 1;
            }
            if index >= bytes.len() {
                return Err(SvgPainterError::Parse(
                    "unterminated attribute value".to_string(),
                ));
            }
            let value = source[value_start..index].to_string();
            index += 1;
            value
        } else {
            let value_start = index;
            while index < bytes.len() && !bytes[index].is_ascii_whitespace() && bytes[index] != b'/'
            {
                index += 1;
            }
            source[value_start..index].to_string()
        };
        attributes.push((key, value));
    }

    Ok(attributes)
}

fn parse_intrinsic_size(tag: &SvgTag) -> Result<Size, SvgPainterError> {
    if let (Some(width), Some(height)) = (number_attr(tag, "width")?, number_attr(tag, "height")?) {
        if width > 0.0 && height > 0.0 {
            return Ok(Size::new(width, height));
        }
    }

    let view_box = attr(tag, "viewbox")
        .ok_or_else(|| SvgPainterError::Parse("SVG must declare width/height or viewBox".into()))?;
    let values = parse_number_list(view_box)?;
    if values.len() != 4 || values[2] <= 0.0 || values[3] <= 0.0 {
        return Err(SvgPainterError::Parse("invalid SVG viewBox".to_string()));
    }
    Ok(Size::new(values[2], values[3]))
}

fn parse_view_box(tag: &SvgTag, intrinsic_size: Size) -> Result<SvgViewBox, SvgPainterError> {
    if let Some(view_box) = attr(tag, "viewbox") {
        let values = parse_number_list(view_box)?;
        if values.len() != 4 || values[2] <= 0.0 || values[3] <= 0.0 {
            return Err(SvgPainterError::Parse("invalid SVG viewBox".to_string()));
        }
        return Ok(SvgViewBox {
            min_x: values[0],
            min_y: values[1],
            width: values[2],
            height: values[3],
        });
    }

    Ok(SvgViewBox {
        min_x: 0.0,
        min_y: 0.0,
        width: intrinsic_size.width,
        height: intrinsic_size.height,
    })
}

fn parse_rect(tag: &SvgTag) -> Result<Option<SvgRect>, SvgPainterError> {
    let x = number_attr(tag, "x")?.unwrap_or(0.0);
    let y = number_attr(tag, "y")?.unwrap_or(0.0);
    let width = number_attr(tag, "width")?
        .ok_or_else(|| SvgPainterError::Parse("<rect> is missing width".to_string()))?;
    let height = number_attr(tag, "height")?
        .ok_or_else(|| SvgPainterError::Parse("<rect> is missing height".to_string()))?;
    if width <= 0.0 || height <= 0.0 {
        return Ok(None);
    }

    let fill = parse_fill(tag)?;
    let stroke = parse_stroke(tag)?;
    if fill.is_none() && stroke.is_none() {
        return Ok(None);
    }

    Ok(Some(SvgRect {
        x,
        y,
        width,
        height,
        fill,
        stroke,
    }))
}

fn parse_path_element(tag: &SvgTag) -> Result<Option<SvgPath>, SvgPainterError> {
    let data =
        attr(tag, "d").ok_or_else(|| SvgPainterError::Parse("<path> is missing d".to_string()))?;
    let path = parse_path_data(data)?;
    let fill = parse_fill(tag)?;
    let stroke = parse_stroke(tag)?;
    if fill.is_none() && stroke.is_none() {
        return Ok(None);
    }
    Ok(Some(SvgPath { path, fill, stroke }))
}

fn parse_fill(tag: &SvgTag) -> Result<Option<SvgColor>, SvgPainterError> {
    let opacity = number_attr_or_style(tag, "opacity")?.unwrap_or(1.0);
    let fill_opacity = number_attr_or_style(tag, "fill-opacity")?.unwrap_or(1.0);
    paint_attr(tag, "fill", Some(SvgColor::BLACK), opacity * fill_opacity)
}

fn parse_stroke(tag: &SvgTag) -> Result<Option<SvgStroke>, SvgPainterError> {
    let opacity = number_attr_or_style(tag, "opacity")?.unwrap_or(1.0);
    let stroke_opacity = number_attr_or_style(tag, "stroke-opacity")?.unwrap_or(1.0);
    let Some(color) = paint_attr(tag, "stroke", None, opacity * stroke_opacity)? else {
        return Ok(None);
    };
    let width = number_attr_or_style(tag, "stroke-width")?.unwrap_or(1.0);
    if width < 0.0 {
        return Err(SvgPainterError::Parse(
            "stroke-width must be non-negative".to_string(),
        ));
    }
    Ok(Some(SvgStroke {
        color,
        width,
        line_cap: parse_line_cap(attr_or_style(tag, "stroke-linecap")),
        line_join: parse_line_join(attr_or_style(tag, "stroke-linejoin")),
    }))
}

fn paint_attr(
    tag: &SvgTag,
    name: &str,
    default: Option<SvgColor>,
    opacity: f32,
) -> Result<Option<SvgColor>, SvgPainterError> {
    match attr_or_style(tag, name) {
        Some(value) if value.trim().eq_ignore_ascii_case("none") => Ok(None),
        Some(value) => parse_color(value, opacity).map(Some),
        None => Ok(default.map(|color| color.with_opacity(opacity))),
    }
}

fn parse_line_cap(value: Option<&str>) -> LineCap {
    match value
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("round") => LineCap::Round,
        Some("square") => LineCap::Square,
        _ => LineCap::Butt,
    }
}

fn parse_line_join(value: Option<&str>) -> LineJoin {
    match value
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("round") => LineJoin::Round,
        Some("bevel") => LineJoin::Bevel,
        _ => LineJoin::Miter,
    }
}

impl SvgColor {
    const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };

    fn with_opacity(self, opacity: f32) -> Self {
        let opacity = opacity.clamp(0.0, 1.0);
        Self {
            a: ((self.a as f32 * opacity).round()).clamp(0.0, 255.0) as u8,
            ..self
        }
    }
}

fn parse_color(value: &str, opacity: f32) -> Result<SvgColor, SvgPainterError> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        return parse_hex_color(hex, opacity);
    }

    let color = match value.to_ascii_lowercase().as_str() {
        "black" => SvgColor::BLACK,
        "white" => SvgColor {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
        "red" => SvgColor {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        },
        "green" => SvgColor {
            r: 0,
            g: 128,
            b: 0,
            a: 255,
        },
        "blue" => SvgColor {
            r: 0,
            g: 0,
            b: 255,
            a: 255,
        },
        "transparent" => SvgColor {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        },
        other if other.starts_with("rgb(") && other.ends_with(')') => {
            return parse_rgb_function(other, opacity);
        }
        _ => {
            return Err(SvgPainterError::Parse(format!(
                "unsupported SVG color `{value}`"
            )));
        }
    };
    Ok(color.with_opacity(opacity))
}

fn parse_hex_color(hex: &str, opacity: f32) -> Result<SvgColor, SvgPainterError> {
    let parse_pair = |value: &str| {
        u8::from_str_radix(value, 16)
            .map_err(|_| SvgPainterError::Parse(format!("invalid SVG color `#{hex}`")))
    };
    let parse_nibble = |value: &str| {
        u8::from_str_radix(value, 16)
            .map(|nibble| nibble * 17)
            .map_err(|_| SvgPainterError::Parse(format!("invalid SVG color `#{hex}`")))
    };

    let color = match hex.len() {
        3 => SvgColor {
            r: parse_nibble(&hex[0..1])?,
            g: parse_nibble(&hex[1..2])?,
            b: parse_nibble(&hex[2..3])?,
            a: 255,
        },
        4 => SvgColor {
            r: parse_nibble(&hex[0..1])?,
            g: parse_nibble(&hex[1..2])?,
            b: parse_nibble(&hex[2..3])?,
            a: parse_nibble(&hex[3..4])?,
        },
        6 => SvgColor {
            r: parse_pair(&hex[0..2])?,
            g: parse_pair(&hex[2..4])?,
            b: parse_pair(&hex[4..6])?,
            a: 255,
        },
        8 => SvgColor {
            r: parse_pair(&hex[0..2])?,
            g: parse_pair(&hex[2..4])?,
            b: parse_pair(&hex[4..6])?,
            a: parse_pair(&hex[6..8])?,
        },
        _ => {
            return Err(SvgPainterError::Parse(format!(
                "invalid SVG color `#{hex}`"
            )));
        }
    };
    Ok(color.with_opacity(opacity))
}

fn parse_rgb_function(value: &str, opacity: f32) -> Result<SvgColor, SvgPainterError> {
    let inner = value
        .strip_prefix("rgb(")
        .and_then(|value| value.strip_suffix(')'))
        .ok_or_else(|| SvgPainterError::Parse(format!("invalid SVG color `{value}`")))?;
    let parts: Vec<_> = inner.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        return Err(SvgPainterError::Parse(format!(
            "invalid SVG color `{value}`"
        )));
    }
    Ok(SvgColor {
        r: parse_color_component(parts[0])?,
        g: parse_color_component(parts[1])?,
        b: parse_color_component(parts[2])?,
        a: 255,
    }
    .with_opacity(opacity))
}

fn parse_color_component(value: &str) -> Result<u8, SvgPainterError> {
    let value = value.trim();
    if let Some(percent) = value.strip_suffix('%') {
        let number: f32 = percent
            .parse()
            .map_err(|_| SvgPainterError::Parse(format!("invalid color component `{value}`")))?;
        Ok(((number.clamp(0.0, 100.0) / 100.0) * 255.0).round() as u8)
    } else {
        let number: f32 = value
            .parse()
            .map_err(|_| SvgPainterError::Parse(format!("invalid color component `{value}`")))?;
        Ok(number.round().clamp(0.0, 255.0) as u8)
    }
}

fn parse_path_data(data: &str) -> Result<Path, SvgPainterError> {
    PathDataParser::new(data).parse()
}

struct PathDataParser<'a> {
    data: &'a str,
    index: usize,
    command: Option<char>,
    current: (f32, f32),
    subpath_start: (f32, f32),
    builder: PathBuilder,
}

impl<'a> PathDataParser<'a> {
    fn new(data: &'a str) -> Self {
        Self {
            data,
            index: 0,
            command: None,
            current: (0.0, 0.0),
            subpath_start: (0.0, 0.0),
            builder: PathBuilder::new(),
        }
    }

    fn parse(mut self) -> Result<Path, SvgPainterError> {
        while self.skip_separators() {
            if let Some(command) = self.peek_command() {
                self.index += command.len_utf8();
                self.command = Some(command);
            }
            let command = self
                .command
                .ok_or_else(|| SvgPainterError::Parse("SVG path is missing a command".into()))?;
            self.parse_command(command)?;
        }

        self.builder
            .finish()
            .ok_or_else(|| SvgPainterError::Parse("SVG path is empty".to_string()))
    }

    fn parse_command(&mut self, command: char) -> Result<(), SvgPainterError> {
        match command {
            'M' | 'm' => self.parse_move(command == 'm'),
            'L' | 'l' => self.parse_line(command == 'l'),
            'H' | 'h' => self.parse_horizontal(command == 'h'),
            'V' | 'v' => self.parse_vertical(command == 'v'),
            'C' | 'c' => self.parse_cubic(command == 'c'),
            'Q' | 'q' => self.parse_quad(command == 'q'),
            'Z' | 'z' => {
                self.builder.close();
                self.current = self.subpath_start;
                self.command = None;
                Ok(())
            }
            'A' | 'a' | 'S' | 's' | 'T' | 't' => Err(SvgPainterError::Parse(format!(
                "unsupported SVG path command `{command}`"
            ))),
            _ => Err(SvgPainterError::Parse(format!(
                "invalid SVG path command `{command}`"
            ))),
        }
    }

    fn parse_move(&mut self, relative: bool) -> Result<(), SvgPainterError> {
        let mut first = true;
        while self.next_starts_number() {
            let point = self.read_point(relative)?;
            if first {
                self.builder.move_to(point.0, point.1);
                self.subpath_start = point;
                first = false;
            } else {
                self.builder.line_to(point.0, point.1);
            }
            self.current = point;
        }
        self.command = Some(if relative { 'l' } else { 'L' });
        Ok(())
    }

    fn parse_line(&mut self, relative: bool) -> Result<(), SvgPainterError> {
        while self.next_starts_number() {
            let point = self.read_point(relative)?;
            self.builder.line_to(point.0, point.1);
            self.current = point;
        }
        Ok(())
    }

    fn parse_horizontal(&mut self, relative: bool) -> Result<(), SvgPainterError> {
        while self.next_starts_number() {
            let x = self.read_number()?;
            let point = if relative {
                (self.current.0 + x, self.current.1)
            } else {
                (x, self.current.1)
            };
            self.builder.line_to(point.0, point.1);
            self.current = point;
        }
        Ok(())
    }

    fn parse_vertical(&mut self, relative: bool) -> Result<(), SvgPainterError> {
        while self.next_starts_number() {
            let y = self.read_number()?;
            let point = if relative {
                (self.current.0, self.current.1 + y)
            } else {
                (self.current.0, y)
            };
            self.builder.line_to(point.0, point.1);
            self.current = point;
        }
        Ok(())
    }

    fn parse_cubic(&mut self, relative: bool) -> Result<(), SvgPainterError> {
        while self.next_starts_number() {
            let c1 = self.read_point(relative)?;
            let c2 = self.read_point(relative)?;
            let point = self.read_point(relative)?;
            self.builder
                .cubic_to(c1.0, c1.1, c2.0, c2.1, point.0, point.1);
            self.current = point;
        }
        Ok(())
    }

    fn parse_quad(&mut self, relative: bool) -> Result<(), SvgPainterError> {
        while self.next_starts_number() {
            let c = self.read_point(relative)?;
            let point = self.read_point(relative)?;
            self.builder.quad_to(c.0, c.1, point.0, point.1);
            self.current = point;
        }
        Ok(())
    }

    fn read_point(&mut self, relative: bool) -> Result<(f32, f32), SvgPainterError> {
        let x = self.read_number()?;
        let y = self.read_number()?;
        if relative {
            Ok((self.current.0 + x, self.current.1 + y))
        } else {
            Ok((x, y))
        }
    }

    fn read_number(&mut self) -> Result<f32, SvgPainterError> {
        self.skip_separators();
        let start = self.index;
        let bytes = self.data.as_bytes();
        if self.index < bytes.len() && matches!(bytes[self.index], b'+' | b'-') {
            self.index += 1;
        }
        while self.index < bytes.len() && bytes[self.index].is_ascii_digit() {
            self.index += 1;
        }
        if self.index < bytes.len() && bytes[self.index] == b'.' {
            self.index += 1;
            while self.index < bytes.len() && bytes[self.index].is_ascii_digit() {
                self.index += 1;
            }
        }
        if self.index < bytes.len() && matches!(bytes[self.index], b'e' | b'E') {
            let exponent = self.index;
            self.index += 1;
            if self.index < bytes.len() && matches!(bytes[self.index], b'+' | b'-') {
                self.index += 1;
            }
            let digit_start = self.index;
            while self.index < bytes.len() && bytes[self.index].is_ascii_digit() {
                self.index += 1;
            }
            if digit_start == self.index {
                self.index = exponent;
            }
        }
        if start == self.index {
            return Err(SvgPainterError::Parse("expected SVG path number".into()));
        }
        self.data[start..self.index]
            .parse()
            .map_err(|_| SvgPainterError::Parse("invalid SVG path number".to_string()))
    }

    fn skip_separators(&mut self) -> bool {
        let bytes = self.data.as_bytes();
        while self.index < bytes.len()
            && (bytes[self.index].is_ascii_whitespace() || bytes[self.index] == b',')
        {
            self.index += 1;
        }
        self.index < bytes.len()
    }

    fn next_starts_number(&mut self) -> bool {
        if !self.skip_separators() {
            return false;
        }
        let byte = self.data.as_bytes()[self.index];
        byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.')
    }

    fn peek_command(&self) -> Option<char> {
        self.data[self.index..]
            .chars()
            .next()
            .filter(|ch| ch.is_ascii_alphabetic())
    }
}

fn number_attr(tag: &SvgTag, name: &str) -> Result<Option<f32>, SvgPainterError> {
    attr(tag, name).map(parse_number).transpose()
}

fn number_attr_or_style(tag: &SvgTag, name: &str) -> Result<Option<f32>, SvgPainterError> {
    attr_or_style(tag, name).map(parse_number).transpose()
}

fn parse_number(value: &str) -> Result<f32, SvgPainterError> {
    let value = value.trim();
    if value.ends_with('%') {
        return Err(SvgPainterError::Parse(format!(
            "percentage SVG length `{value}` is unsupported"
        )));
    }
    let number = value
        .strip_suffix("px")
        .unwrap_or(value)
        .parse()
        .map_err(|_| SvgPainterError::Parse(format!("invalid SVG number `{value}`")))?;
    Ok(number)
}

fn parse_number_list(value: &str) -> Result<Vec<f32>, SvgPainterError> {
    value
        .split(|ch: char| ch.is_ascii_whitespace() || ch == ',')
        .filter(|part| !part.is_empty())
        .map(parse_number)
        .collect()
}

fn attr<'a>(tag: &'a SvgTag, name: &str) -> Option<&'a str> {
    tag.attributes
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn attr_or_style<'a>(tag: &'a SvgTag, name: &str) -> Option<&'a str> {
    attr(tag, name).or_else(|| style_attr(tag, name))
}

fn style_attr<'a>(tag: &'a SvgTag, name: &str) -> Option<&'a str> {
    attr(tag, "style").and_then(|style| {
        style.split(';').find_map(|entry| {
            let (key, value) = entry.split_once(':')?;
            if key.trim().eq_ignore_ascii_case(name) {
                Some(value.trim())
            } else {
                None
            }
        })
    })
}
