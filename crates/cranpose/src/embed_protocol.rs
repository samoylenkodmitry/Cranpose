use std::io::{self, Read, Write};

use cranpose_app_shell::Modifiers;

use crate::native_window::WindowResizeDirection;

pub(crate) const PROTOCOL_VERSION: u32 = 2;

pub(crate) type SurfaceId = u32;

pub(crate) const PRIMARY_SURFACE: SurfaceId = 0;

const MAX_HOST_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

const HOST_RESIZE: u8 = 0x01;
const HOST_POINTER_MOVE: u8 = 0x02;
const HOST_POINTER_DOWN: u8 = 0x03;
const HOST_POINTER_UP: u8 = 0x04;
const HOST_POINTER_LEAVE: u8 = 0x05;
const HOST_SCROLL: u8 = 0x06;
const HOST_KEY: u8 = 0x07;
const HOST_TEXT: u8 = 0x08;
const HOST_THEME: u8 = 0x09;
const HOST_MESSAGE: u8 = 0x0A;
const HOST_FRAME_ACK: u8 = 0x0B;
const HOST_CLOSE: u8 = 0x0C;
const HOST_VISIBILITY: u8 = 0x0D;
const HOST_MOVED: u8 = 0x0E;
const HOST_CLOSE_REQUESTED: u8 = 0x0F;

const APP_HELLO: u8 = 0x01;
const APP_FRAME: u8 = 0x02;
const APP_CURSOR: u8 = 0x03;
const APP_MESSAGE: u8 = 0x04;
const APP_OPEN_WINDOW: u8 = 0x05;
const APP_OPEN_OVERLAY: u8 = 0x06;
const APP_CLOSE_SURFACE: u8 = 0x07;
const APP_BEGIN_MOVE: u8 = 0x08;
const APP_BEGIN_RESIZE: u8 = 0x09;

const MODIFIER_SHIFT: u8 = 1;
const MODIFIER_CTRL: u8 = 1 << 1;
const MODIFIER_ALT: u8 = 1 << 2;
const MODIFIER_META: u8 = 1 << 3;

pub(crate) const WINDOW_DECORATED: u8 = 1;
pub(crate) const WINDOW_TRANSPARENT: u8 = 1 << 1;
pub(crate) const WINDOW_RESIZABLE: u8 = 1 << 2;
pub(crate) const WINDOW_ALWAYS_ON_TOP: u8 = 1 << 3;
pub(crate) const WINDOW_SHADOW: u8 = 1 << 4;
pub(crate) const WINDOW_TAKES_FOCUS: u8 = 1 << 5;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum HostEvent {
    Surface {
        surface: SurfaceId,
        event: SurfaceEvent,
    },
    Theme {
        dark: bool,
    },
    Message {
        channel: String,
        payload: String,
    },
    Close,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SurfaceEvent {
    Resize {
        width: u32,
        height: u32,
        scale: f32,
        refresh_hz: f32,
    },
    PointerMove {
        x: f32,
        y: f32,
    },
    PointerDown {
        x: f32,
        y: f32,
    },
    PointerUp {
        x: f32,
        y: f32,
    },
    PointerLeave,
    Scroll {
        x: f32,
        y: f32,
        delta_x: f32,
        delta_y: f32,
        modifiers: Modifiers,
    },
    Key {
        down: bool,
        modifiers: Modifiers,
        code: String,
    },
    Text(String),
    FrameAck(u32),
    Visibility(bool),
    Moved {
        x: f32,
        y: f32,
    },
    CloseRequested,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PixelRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl PixelRect {
    pub(crate) fn full(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }
}

pub(crate) struct FrameUpdate<'a> {
    pub(crate) surface: SurfaceId,
    pub(crate) frame_id: u32,
    pub(crate) buffer_width: u32,
    pub(crate) buffer_height: u32,
    pub(crate) rect: PixelRect,
    pub(crate) pixels: &'a [u8],
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WindowSpec {
    pub(crate) surface: SurfaceId,
    pub(crate) title: String,
    pub(crate) position: Option<(f32, f32)>,
    pub(crate) relative_to_host: bool,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) flags: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SurfaceCommand {
    OpenWindow(WindowSpec),
    OpenOverlay { surface: SurfaceId, anchor: String },
    Close(SurfaceId),
    BeginMove(SurfaceId),
    BeginResize(SurfaceId, WindowResizeDirection),
}

pub(crate) enum AppEvent<'a> {
    Hello { token: &'a str },
    Frame(FrameUpdate<'a>),
    Cursor { surface: SurfaceId, name: &'a str },
    Message { channel: &'a str, payload: &'a str },
    Command(&'a SurfaceCommand),
}

pub(crate) fn modifiers_from_bits(bits: u8) -> Modifiers {
    Modifiers {
        shift: bits & MODIFIER_SHIFT != 0,
        ctrl: bits & MODIFIER_CTRL != 0,
        alt: bits & MODIFIER_ALT != 0,
        meta: bits & MODIFIER_META != 0,
    }
}

pub(crate) fn resize_direction_code(direction: WindowResizeDirection) -> u8 {
    match direction {
        WindowResizeDirection::East => 0,
        WindowResizeDirection::North => 1,
        WindowResizeDirection::NorthEast => 2,
        WindowResizeDirection::NorthWest => 3,
        WindowResizeDirection::South => 4,
        WindowResizeDirection::SouthEast => 5,
        WindowResizeDirection::SouthWest => 6,
        WindowResizeDirection::West => 7,
    }
}

pub(crate) fn write_app_event(writer: &mut impl Write, event: &AppEvent<'_>) -> io::Result<()> {
    match event {
        AppEvent::Hello { token } => {
            let mut body = Body::new(APP_HELLO);
            body.u32(PROTOCOL_VERSION);
            body.string(token);
            body.write_to(writer)
        }
        AppEvent::Frame(frame) => write_frame(writer, frame),
        AppEvent::Cursor { surface, name } => {
            let mut body = Body::new(APP_CURSOR);
            body.u32(*surface);
            body.string(name);
            body.write_to(writer)
        }
        AppEvent::Message { channel, payload } => {
            let mut body = Body::new(APP_MESSAGE);
            body.string(channel);
            body.string(payload);
            body.write_to(writer)
        }
        AppEvent::Command(command) => command_body(command).write_to(writer),
    }
}

fn command_body(command: &SurfaceCommand) -> Body {
    match command {
        SurfaceCommand::OpenWindow(spec) => {
            let mut body = Body::new(APP_OPEN_WINDOW);
            body.u32(spec.surface);
            body.string(&spec.title);
            let (x, y) = spec.position.unwrap_or((f32::NAN, f32::NAN));
            body.f32(x);
            body.f32(y);
            body.f32(spec.width);
            body.f32(spec.height);
            body.u8(u8::from(spec.relative_to_host));
            body.u8(spec.flags);
            body
        }
        SurfaceCommand::OpenOverlay { surface, anchor } => {
            let mut body = Body::new(APP_OPEN_OVERLAY);
            body.u32(*surface);
            body.string(anchor);
            body
        }
        SurfaceCommand::Close(surface) => surface_body(APP_CLOSE_SURFACE, *surface),
        SurfaceCommand::BeginMove(surface) => surface_body(APP_BEGIN_MOVE, *surface),
        SurfaceCommand::BeginResize(surface, direction) => {
            let mut body = surface_body(APP_BEGIN_RESIZE, *surface);
            body.u8(resize_direction_code(*direction));
            body
        }
    }
}

fn surface_body(kind: u8, surface: SurfaceId) -> Body {
    let mut body = Body::new(kind);
    body.u32(surface);
    body
}

fn write_frame(writer: &mut impl Write, frame: &FrameUpdate<'_>) -> io::Result<()> {
    let rect = frame.rect;
    let row_bytes = rect.width as usize * 4;
    let stride = frame.buffer_width as usize * 4;
    let rect_end_row = rect.y as usize + rect.height as usize;
    if rect.x as usize + rect.width as usize > frame.buffer_width as usize
        || rect_end_row > frame.buffer_height as usize
        || frame.pixels.len() < stride * frame.buffer_height as usize
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "frame rectangle lies outside the frame buffer",
        ));
    }
    let mut header = Body::new(APP_FRAME);
    header.u32(frame.surface);
    header.u32(frame.frame_id);
    header.u32(frame.buffer_width);
    header.u32(frame.buffer_height);
    header.u32(rect.x);
    header.u32(rect.y);
    header.u32(rect.width);
    header.u32(rect.height);
    let body_len = header.bytes.len() + row_bytes * rect.height as usize;
    writer.write_all(&length_prefix(body_len)?)?;
    writer.write_all(&header.bytes)?;
    for row in rect.y as usize..rect_end_row {
        let start = row * stride + rect.x as usize * 4;
        writer.write_all(&frame.pixels[start..start + row_bytes])?;
    }
    Ok(())
}

pub(crate) fn read_host_event(reader: &mut impl Read) -> io::Result<Option<HostEvent>> {
    loop {
        let mut length = [0u8; 4];
        if !read_exact_or_eof(reader, &mut length)? {
            return Ok(None);
        }
        let length = u32::from_le_bytes(length) as usize;
        if length == 0 || length > MAX_HOST_MESSAGE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("host message of {length} bytes is out of range"),
            ));
        }
        let mut body = vec![0u8; length];
        reader.read_exact(&mut body)?;
        if let Some(event) = decode_host_event(&body)? {
            return Ok(Some(event));
        }
    }
}

pub(crate) fn decode_host_event(body: &[u8]) -> io::Result<Option<HostEvent>> {
    let mut fields = Fields::new(body);
    let event = match fields.u8()? {
        HOST_THEME => fields.flag().map(|dark| HostEvent::Theme { dark }),
        HOST_MESSAGE => fields.message(),
        HOST_CLOSE => Ok(HostEvent::Close),
        kind if SURFACE_KINDS.contains(&kind) => fields.surface_event(kind),
        kind => {
            log::debug!("embed: skipping host message kind {kind:#04x}");
            return Ok(None);
        }
    };
    event.map(Some)
}

const SURFACE_KINDS: [u8; 12] = [
    HOST_RESIZE,
    HOST_POINTER_MOVE,
    HOST_POINTER_DOWN,
    HOST_POINTER_UP,
    HOST_POINTER_LEAVE,
    HOST_SCROLL,
    HOST_KEY,
    HOST_TEXT,
    HOST_FRAME_ACK,
    HOST_VISIBILITY,
    HOST_MOVED,
    HOST_CLOSE_REQUESTED,
];

fn read_exact_or_eof(reader: &mut impl Read, buffer: &mut [u8]) -> io::Result<bool> {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..]) {
            Ok(0) if filled == 0 => return Ok(false),
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

fn length_prefix(body_len: usize) -> io::Result<[u8; 4]> {
    u32::try_from(body_len)
        .map(u32::to_le_bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "message exceeds 4 GiB"))
}

struct Body {
    bytes: Vec<u8>,
}

impl Body {
    fn new(kind: u8) -> Self {
        Self { bytes: vec![kind] }
    }

    fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn f32(&mut self, value: f32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn string(&mut self, value: &str) {
        self.u32(value.len() as u32);
        self.bytes.extend_from_slice(value.as_bytes());
    }

    fn write_to(&self, writer: &mut impl Write) -> io::Result<()> {
        writer.write_all(&length_prefix(self.bytes.len())?)?;
        writer.write_all(&self.bytes)
    }
}

struct Fields<'a> {
    bytes: &'a [u8],
}

impl<'a> Fields<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    fn take<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        let (head, rest) = self
            .bytes
            .split_first_chunk::<N>()
            .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))?;
        self.bytes = rest;
        Ok(*head)
    }

    fn u8(&mut self) -> io::Result<u8> {
        self.take::<1>().map(|[byte]| byte)
    }

    fn u32(&mut self) -> io::Result<u32> {
        self.take::<4>().map(u32::from_le_bytes)
    }

    fn f32(&mut self) -> io::Result<f32> {
        self.take::<4>().map(f32::from_le_bytes)
    }

    fn flag(&mut self) -> io::Result<bool> {
        self.u8().map(|byte| byte != 0)
    }

    fn point(&mut self) -> io::Result<(f32, f32)> {
        Ok((self.f32()?, self.f32()?))
    }

    fn surface_event(&mut self, kind: u8) -> io::Result<HostEvent> {
        let surface = self.u32()?;
        let event = match kind {
            HOST_RESIZE => self.resize(),
            HOST_POINTER_MOVE => self
                .point()
                .map(|(x, y)| SurfaceEvent::PointerMove { x, y }),
            HOST_POINTER_DOWN => self
                .point()
                .map(|(x, y)| SurfaceEvent::PointerDown { x, y }),
            HOST_POINTER_UP => self.point().map(|(x, y)| SurfaceEvent::PointerUp { x, y }),
            HOST_POINTER_LEAVE => Ok(SurfaceEvent::PointerLeave),
            HOST_SCROLL => self.scroll(),
            HOST_KEY => self.key(),
            HOST_TEXT => self.string().map(SurfaceEvent::Text),
            HOST_FRAME_ACK => self.u32().map(SurfaceEvent::FrameAck),
            HOST_VISIBILITY => self.flag().map(SurfaceEvent::Visibility),
            HOST_MOVED => self.point().map(|(x, y)| SurfaceEvent::Moved { x, y }),
            HOST_CLOSE_REQUESTED => Ok(SurfaceEvent::CloseRequested),
            kind => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("host message kind {kind:#04x} names no surface event"),
            )),
        };
        event.map(|event| HostEvent::Surface { surface, event })
    }

    fn resize(&mut self) -> io::Result<SurfaceEvent> {
        Ok(SurfaceEvent::Resize {
            width: self.u32()?,
            height: self.u32()?,
            scale: self.f32()?,
            refresh_hz: self.f32()?,
        })
    }

    fn scroll(&mut self) -> io::Result<SurfaceEvent> {
        let (x, y) = self.point()?;
        let (delta_x, delta_y) = self.point()?;
        Ok(SurfaceEvent::Scroll {
            x,
            y,
            delta_x,
            delta_y,
            modifiers: modifiers_from_bits(self.u8()?),
        })
    }

    fn key(&mut self) -> io::Result<SurfaceEvent> {
        Ok(SurfaceEvent::Key {
            down: self.flag()?,
            modifiers: modifiers_from_bits(self.u8()?),
            code: self.string()?,
        })
    }

    fn message(&mut self) -> io::Result<HostEvent> {
        Ok(HostEvent::Message {
            channel: self.string()?,
            payload: self.string()?,
        })
    }

    fn string(&mut self) -> io::Result<String> {
        let length = self.u32()? as usize;
        if length > self.bytes.len() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        let (text, rest) = self.bytes.split_at(length);
        self.bytes = rest;
        String::from_utf8(text.to_vec())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }
}

#[cfg(test)]
#[path = "tests/embed_protocol_tests.rs"]
mod tests;
