use alloc::{sync::Arc, vec::Vec};
use core::ffi::CStr;

use super::super::{Command, CommandBuffer, CommandEncoder, PrivateCapabilities};
use super::{marker, text};
use crate::CommandEncoder as _;

fn record(label: &str, markers: &[&str]) -> CommandBuffer {
    let mut encoder = CommandEncoder {
        cmd_buffer: CommandBuffer::default(),
        state: Default::default(),
        private_caps: PrivateCapabilities::empty(),
        counters: Arc::default(),
    };
    unsafe {
        encoder.begin_encoding(Some(label)).unwrap();
        for (index, &text) in markers.iter().enumerate() {
            if index % 2 == 0 {
                encoder.begin_debug_marker(text);
            } else {
                encoder.insert_debug_marker(text);
            }
        }
        encoder.end_encoding().unwrap()
    }
}

// Each string as the queue hands it to the GL debug functions, with the bytes that follow it up to
// and with the NUL
fn handed_to_gl(buffer: &CommandBuffer) -> Vec<&CStr> {
    let markers = buffer.commands.iter().filter_map(|command| match command {
        Command::PushDebugGroup(range) | Command::InsertDebugMarker(range) => {
            Some(marker(&buffer.data_bytes, range))
        }
        _ => None,
    });
    buffer.label.as_deref().into_iter().chain(markers).collect()
}

// The labels of the Quest 3's crashes: 16 bytes fill a heap block, so the byte after them belongs
// to the next block or to a page that cannot be read. Two markers side by side must not run on
// into each other.
#[test]
fn a_nul_follows_each_string_for_the_gl_debug_functions() {
    let buffer = record(
        "Scene Shadow Map",
        &["Scene Surfaces", "Clear Pass", "Scene Shadow Map"],
    );
    let strings = handed_to_gl(&buffer);
    assert_eq!(
        strings,
        [
            c"Scene Shadow Map",
            c"Scene Surfaces",
            c"Clear Pass",
            c"Scene Shadow Map"
        ]
    );
    for string in strings {
        // The text that glow passes with its length starts at the string and stops right before
        // its NUL
        let handed = text(string);
        assert_eq!(handed.as_ptr(), string.as_ptr().cast::<u8>());
        assert_eq!(handed.len(), string.count_bytes());
    }
}

// A driver stops at the first NUL of a string, so the recorded text stops there too. An empty
// string goes to GL as `<empty>`, also with a NUL after it.
#[test]
fn a_string_ends_at_its_first_nul_and_an_empty_one_reads_empty() {
    let buffer = record("before\0after", &["", "inside\0marker"]);
    assert_eq!(handed_to_gl(&buffer), [c"before", c"", c"inside"]);
    let empty = text(c"");
    assert_eq!(empty, "<empty>");
    assert_eq!(empty.as_ptr(), c"<empty>".as_ptr().cast::<u8>());
}
