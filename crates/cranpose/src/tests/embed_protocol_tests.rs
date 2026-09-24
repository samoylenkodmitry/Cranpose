use super::*;

fn framed(body: &[u8]) -> Vec<u8> {
    let mut bytes = (body.len() as u32).to_le_bytes().to_vec();
    bytes.extend_from_slice(body);
    bytes
}

fn le(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn floats(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn read_all(bytes: &[u8]) -> Vec<HostEvent> {
    let mut reader = bytes;
    let mut events = Vec::new();
    while let Some(event) = read_host_event(&mut reader).expect("stream decodes") {
        events.push(event);
    }
    events
}

fn encode(event: &AppEvent<'_>) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_app_event(&mut bytes, event).expect("encodes");
    bytes
}

fn surface_event(surface: SurfaceId, event: SurfaceEvent) -> HostEvent {
    HostEvent::Surface { surface, event }
}

#[test]
fn hello_is_encoded_with_version_and_token() {
    assert_eq!(
        encode(&AppEvent::Hello { token: "ab" }),
        vec![
            0x0B, 0, 0, 0,    //
            0x01, //
            0x02, 0, 0, 0, //
            0x02, 0, 0, 0, b'a', b'b',
        ]
    );
}

#[test]
fn a_frame_carries_its_surface_and_only_the_rows_of_its_rectangle() {
    let pixels: Vec<u8> = (0..3u8 * 2 * 4).collect();
    let bytes = encode(&AppEvent::Frame(FrameUpdate {
        surface: 5,
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
    }));

    let mut expected = vec![0x02];
    expected.extend(le(&[5, 9, 3, 2, 1, 1, 2, 1]));
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
            surface: PRIMARY_SURFACE,
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
    let mut bytes = encode(&AppEvent::Cursor {
        surface: 2,
        name: "text",
    });
    bytes.extend(encode(&AppEvent::Message {
        channel: "c",
        payload: "{}",
    }));

    let mut cursor = vec![0x03];
    cursor.extend(le(&[2, 4]));
    cursor.extend_from_slice(b"text");
    let mut expected = framed(&cursor);
    expected.extend(framed(&[0x04, 1, 0, 0, 0, b'c', 2, 0, 0, 0, b'{', b'}']));
    assert_eq!(bytes, expected);
}

#[test]
fn window_and_overlay_commands_are_encoded() {
    let window = SurfaceCommand::OpenWindow(WindowSpec {
        surface: 3,
        title: "w".to_string(),
        position: Some((10.0, 20.0)),
        relative_to_host: true,
        width: 40.0,
        height: 30.0,
        flags: WINDOW_TRANSPARENT | WINDOW_ALWAYS_ON_TOP,
    });
    let mut expected = vec![0x05];
    expected.extend(le(&[3, 1]));
    expected.push(b'w');
    expected.extend(floats(&[10.0, 20.0, 40.0, 30.0]));
    expected.extend([1, 0b1010]);
    assert_eq!(encode(&AppEvent::Command(&window)), framed(&expected));

    let overlay = SurfaceCommand::OpenOverlay {
        surface: 4,
        anchor: "editor".to_string(),
    };
    let mut expected = vec![0x06];
    expected.extend(le(&[4, 6]));
    expected.extend_from_slice(b"editor");
    assert_eq!(encode(&AppEvent::Command(&overlay)), framed(&expected));
}

#[test]
fn an_unplaced_window_sends_nan_for_its_position() {
    let bytes = encode(&AppEvent::Command(&SurfaceCommand::OpenWindow(
        WindowSpec {
            surface: 1,
            title: String::new(),
            position: None,
            relative_to_host: false,
            width: 1.0,
            height: 1.0,
            flags: 0,
        },
    )));
    let x = f32::from_le_bytes([bytes[13], bytes[14], bytes[15], bytes[16]]);
    let y = f32::from_le_bytes([bytes[17], bytes[18], bytes[19], bytes[20]]);
    assert!(x.is_nan() && y.is_nan());
}

#[test]
fn close_move_and_resize_commands_name_their_surface() {
    let mut close = vec![0x07];
    close.extend(le(&[6]));
    assert_eq!(
        encode(&AppEvent::Command(&SurfaceCommand::Close(6))),
        framed(&close)
    );

    let mut begin_move = vec![0x08];
    begin_move.extend(le(&[6]));
    assert_eq!(
        encode(&AppEvent::Command(&SurfaceCommand::BeginMove(6))),
        framed(&begin_move)
    );

    let mut resize = vec![0x09];
    resize.extend(le(&[6]));
    resize.push(5);
    assert_eq!(
        encode(&AppEvent::Command(&SurfaceCommand::BeginResize(
            6,
            WindowResizeDirection::SouthEast
        ))),
        framed(&resize)
    );
}

#[test]
fn resize_directions_have_stable_codes() {
    let codes: Vec<u8> = [
        WindowResizeDirection::East,
        WindowResizeDirection::North,
        WindowResizeDirection::NorthEast,
        WindowResizeDirection::NorthWest,
        WindowResizeDirection::South,
        WindowResizeDirection::SouthEast,
        WindowResizeDirection::SouthWest,
        WindowResizeDirection::West,
    ]
    .into_iter()
    .map(resize_direction_code)
    .collect();
    assert_eq!(codes, (0..8).collect::<Vec<u8>>());
}

#[test]
fn host_events_decode_from_their_wire_form() {
    let mut stream = Vec::new();
    let mut resize = vec![0x01];
    resize.extend(le(&[0, 640, 480]));
    resize.extend(floats(&[2.0, 120.0]));
    stream.extend(framed(&resize));

    let mut down = vec![0x03];
    down.extend(le(&[1]));
    down.extend(floats(&[10.5, 20.0]));
    stream.extend(framed(&down));

    let mut scroll = vec![0x06];
    scroll.extend(le(&[0]));
    scroll.extend(floats(&[1.0, 2.0, 0.0, -40.0]));
    scroll.push(0b0101);
    stream.extend(framed(&scroll));

    let mut key = vec![0x07];
    key.extend(le(&[0]));
    key.extend([1, 0b1010, 4, 0, 0, 0]);
    key.extend_from_slice(b"KeyA");
    stream.extend(framed(&key));

    let mut text = vec![0x08];
    text.extend(le(&[0, 2]));
    text.extend([0xC3, 0xA9]);
    stream.extend(framed(&text));

    stream.extend(framed(&[0x09, 0]));
    stream.extend(framed(&[0x0A, 1, 0, 0, 0, b'c', 1, 0, 0, 0, b'p']));

    let mut ack = vec![0x0B];
    ack.extend(le(&[2, 7]));
    stream.extend(framed(&ack));

    let mut leave = vec![0x05];
    leave.extend(le(&[0]));
    stream.extend(framed(&leave));

    let mut visibility = vec![0x0D];
    visibility.extend(le(&[0]));
    visibility.push(0);
    stream.extend(framed(&visibility));

    let mut moved = vec![0x0E];
    moved.extend(le(&[3]));
    moved.extend(floats(&[100.0, 50.0]));
    stream.extend(framed(&moved));

    let mut close_requested = vec![0x0F];
    close_requested.extend(le(&[3]));
    stream.extend(framed(&close_requested));

    stream.extend(framed(&[0x0C]));

    assert_eq!(
        read_all(&stream),
        vec![
            surface_event(
                0,
                SurfaceEvent::Resize {
                    width: 640,
                    height: 480,
                    scale: 2.0,
                    refresh_hz: 120.0,
                }
            ),
            surface_event(1, SurfaceEvent::PointerDown { x: 10.5, y: 20.0 }),
            surface_event(
                0,
                SurfaceEvent::Scroll {
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
                }
            ),
            surface_event(
                0,
                SurfaceEvent::Key {
                    down: true,
                    modifiers: Modifiers {
                        shift: false,
                        ctrl: true,
                        alt: false,
                        meta: true,
                    },
                    code: "KeyA".to_string(),
                }
            ),
            surface_event(0, SurfaceEvent::Text("é".to_string())),
            HostEvent::Theme { dark: false },
            HostEvent::Message {
                channel: "c".to_string(),
                payload: "p".to_string(),
            },
            surface_event(2, SurfaceEvent::FrameAck(7)),
            surface_event(0, SurfaceEvent::PointerLeave),
            surface_event(0, SurfaceEvent::Visibility(false)),
            surface_event(3, SurfaceEvent::Moved { x: 100.0, y: 50.0 }),
            surface_event(3, SurfaceEvent::CloseRequested),
            HostEvent::Close,
        ]
    );
}

#[test]
fn pointer_move_and_up_decode() {
    let mut stream = Vec::new();
    for kind in [0x02u8, 0x04] {
        let mut body = vec![kind];
        body.extend(le(&[0]));
        body.extend(floats(&[3.0, 4.0]));
        stream.extend(framed(&body));
    }

    assert_eq!(
        read_all(&stream),
        vec![
            surface_event(0, SurfaceEvent::PointerMove { x: 3.0, y: 4.0 }),
            surface_event(0, SurfaceEvent::PointerUp { x: 3.0, y: 4.0 }),
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

    let short_field = framed(&[0x0B, 1, 0, 0, 0, 1]);
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
    let mut text = vec![0x08];
    text.extend(le(&[0, 1]));
    text.push(0xFF);
    let body = framed(&text);
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
