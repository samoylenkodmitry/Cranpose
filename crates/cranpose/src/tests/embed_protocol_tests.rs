use super::*;

fn framed(body: &[u8]) -> Vec<u8> {
    let mut bytes = (body.len() as u32).to_le_bytes().to_vec();
    bytes.extend_from_slice(body);
    bytes
}

fn read_all(bytes: &[u8]) -> Vec<HostEvent> {
    let mut reader = bytes;
    let mut events = Vec::new();
    while let Some(event) = read_host_event(&mut reader).expect("stream decodes") {
        events.push(event);
    }
    events
}

#[test]
fn hello_is_encoded_with_version_and_token() {
    let mut bytes = Vec::new();
    write_app_event(&mut bytes, &AppEvent::Hello { token: "ab" }).expect("encodes");

    assert_eq!(
        bytes,
        vec![
            0x0B, 0, 0, 0,    //
            0x01, //
            0x01, 0, 0, 0, //
            0x02, 0, 0, 0, b'a', b'b',
        ]
    );
}

#[test]
fn a_frame_carries_only_the_rows_of_its_rectangle() {
    let pixels: Vec<u8> = (0..3u8 * 2 * 4).collect();
    let mut bytes = Vec::new();
    write_app_event(
        &mut bytes,
        &AppEvent::Frame(FrameUpdate {
            frame_id: 9,
            buffer_width: 3,
            buffer_height: 2,
            rect: PixelRect {
                x: 1,
                y: 1,
                width: 2,
                height: 1,
            },
            pixels: &pixels,
        }),
    )
    .expect("encodes");

    let mut expected = vec![0x02];
    for value in [9u32, 3, 2, 1, 1, 2, 1] {
        expected.extend_from_slice(&value.to_le_bytes());
    }
    expected.extend_from_slice(&pixels[16..24]);
    assert_eq!(bytes, framed(&expected));
}

#[test]
fn a_frame_rectangle_outside_the_buffer_is_rejected() {
    let pixels = vec![0u8; 16];
    let mut bytes = Vec::new();
    let result = write_app_event(
        &mut bytes,
        &AppEvent::Frame(FrameUpdate {
            frame_id: 1,
            buffer_width: 2,
            buffer_height: 2,
            rect: PixelRect {
                x: 1,
                y: 0,
                width: 2,
                height: 1,
            },
            pixels: &pixels,
        }),
    );

    assert_eq!(
        result.map_err(|error| error.kind()),
        Err(io::ErrorKind::InvalidInput)
    );
    assert!(bytes.is_empty(), "nothing is written for a rejected frame");
}

#[test]
fn cursor_and_message_are_encoded_as_strings() {
    let mut bytes = Vec::new();
    write_app_event(&mut bytes, &AppEvent::Cursor("text")).expect("encodes");
    write_app_event(
        &mut bytes,
        &AppEvent::Message {
            channel: "c",
            payload: "{}",
        },
    )
    .expect("encodes");

    let mut expected = framed(&[0x03, 4, 0, 0, 0, b't', b'e', b'x', b't']);
    expected.extend(framed(&[0x04, 1, 0, 0, 0, b'c', 2, 0, 0, 0, b'{', b'}']));
    assert_eq!(bytes, expected);
}

#[test]
fn host_events_decode_from_their_wire_form() {
    let mut stream = Vec::new();
    let mut resize = vec![0x01];
    resize.extend_from_slice(&640u32.to_le_bytes());
    resize.extend_from_slice(&480u32.to_le_bytes());
    resize.extend_from_slice(&2.0f32.to_le_bytes());
    resize.extend_from_slice(&120.0f32.to_le_bytes());
    stream.extend(framed(&resize));

    let mut down = vec![0x03];
    down.extend_from_slice(&10.5f32.to_le_bytes());
    down.extend_from_slice(&20.0f32.to_le_bytes());
    stream.extend(framed(&down));

    let mut scroll = vec![0x06];
    for value in [1.0f32, 2.0, 0.0, -40.0] {
        scroll.extend_from_slice(&value.to_le_bytes());
    }
    scroll.push(0b0101);
    stream.extend(framed(&scroll));

    stream.extend(framed(&[
        0x07, 1, 0b1010, 4, 0, 0, 0, b'K', b'e', b'y', b'A',
    ]));
    stream.extend(framed(&[0x08, 2, 0, 0, 0, 0xC3, 0xA9]));
    stream.extend(framed(&[0x09, 0]));
    stream.extend(framed(&[0x0A, 1, 0, 0, 0, b'c', 1, 0, 0, 0, b'p']));
    stream.extend(framed(&[0x0B, 7, 0, 0, 0]));
    stream.extend(framed(&[0x05]));
    stream.extend(framed(&[0x0D, 0]));
    stream.extend(framed(&[0x0C]));

    assert_eq!(
        read_all(&stream),
        vec![
            HostEvent::Resize {
                width: 640,
                height: 480,
                scale: 2.0,
                refresh_hz: 120.0,
            },
            HostEvent::PointerDown { x: 10.5, y: 20.0 },
            HostEvent::Scroll {
                x: 1.0,
                y: 2.0,
                delta_x: 0.0,
                delta_y: -40.0,
                modifiers: Modifiers {
                    shift: true,
                    ctrl: false,
                    alt: true,
                    meta: false,
                },
            },
            HostEvent::Key {
                down: true,
                modifiers: Modifiers {
                    shift: false,
                    ctrl: true,
                    alt: false,
                    meta: true,
                },
                code: "KeyA".to_string(),
            },
            HostEvent::Text("é".to_string()),
            HostEvent::Theme { dark: false },
            HostEvent::Message {
                channel: "c".to_string(),
                payload: "p".to_string(),
            },
            HostEvent::FrameAck(7),
            HostEvent::PointerLeave,
            HostEvent::Visibility(false),
            HostEvent::Close,
        ]
    );
}

#[test]
fn pointer_move_and_up_decode() {
    let mut stream = Vec::new();
    for kind in [0x02u8, 0x04] {
        let mut body = vec![kind];
        body.extend_from_slice(&3.0f32.to_le_bytes());
        body.extend_from_slice(&4.0f32.to_le_bytes());
        stream.extend(framed(&body));
    }

    assert_eq!(
        read_all(&stream),
        vec![
            HostEvent::PointerMove { x: 3.0, y: 4.0 },
            HostEvent::PointerUp { x: 3.0, y: 4.0 },
        ]
    );
}

#[test]
fn unknown_host_messages_are_skipped() {
    let mut stream = framed(&[0x7F, 1, 2, 3]);
    stream.extend(framed(&[0x09, 1]));

    assert_eq!(read_all(&stream), vec![HostEvent::Theme { dark: true }]);
}

#[test]
fn a_truncated_message_is_an_error_and_a_clean_end_is_none() {
    let mut empty: &[u8] = &[];
    assert!(matches!(read_host_event(&mut empty), Ok(None)));

    let truncated = [4u8, 0, 0, 0, 0x0B, 1];
    let mut reader: &[u8] = &truncated;
    assert!(read_host_event(&mut reader).is_err());

    let short_field = framed(&[0x0B, 1]);
    let mut reader: &[u8] = &short_field;
    assert_eq!(
        read_host_event(&mut reader).map_err(|error| error.kind()),
        Err(io::ErrorKind::UnexpectedEof)
    );
}

#[test]
fn oversized_and_empty_host_messages_are_rejected() {
    let mut reader: &[u8] = &[0, 0, 0, 0];
    assert_eq!(
        read_host_event(&mut reader).map_err(|error| error.kind()),
        Err(io::ErrorKind::InvalidData)
    );

    let huge = (MAX_HOST_MESSAGE_BYTES as u32 + 1).to_le_bytes();
    let mut reader: &[u8] = &huge;
    assert_eq!(
        read_host_event(&mut reader).map_err(|error| error.kind()),
        Err(io::ErrorKind::InvalidData)
    );
}

#[test]
fn invalid_utf8_is_rejected() {
    let body = framed(&[0x08, 1, 0, 0, 0, 0xFF]);
    let mut reader: &[u8] = &body;
    assert_eq!(
        read_host_event(&mut reader).map_err(|error| error.kind()),
        Err(io::ErrorKind::InvalidData)
    );
}

#[test]
fn modifier_bits_map_to_modifiers() {
    assert_eq!(modifiers_from_bits(0), Modifiers::NONE);
    assert_eq!(
        modifiers_from_bits(0b1111),
        Modifiers {
            shift: true,
            ctrl: true,
            alt: true,
            meta: true,
        }
    );
}

#[test]
fn a_full_rectangle_spans_the_buffer() {
    assert_eq!(
        PixelRect::full(4, 3),
        PixelRect {
            x: 0,
            y: 0,
            width: 4,
            height: 3,
        }
    );
}
