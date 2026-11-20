//! The identifier that a runtime gives an anchor it keeps.

use std::{error::Error, fmt, str::FromStr};

/// A 128-bit identifier of an anchor that the runtime keeps between two starts of the app. Jetpack
/// XR's `Anchor.persist` returns Java's `UUID`. `Anchors::persisted_uuids`, `Anchors::load` and
/// `Anchor::uuid` use this type.
///
/// It prints and reads as 32 hexadecimal digits in groups of 8, 4, 4, 4 and 12, such as
/// `123e4567-e89b-12d3-a456-426614174000`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Uuid([u8; 16]);

impl Uuid {
    /// The identifier with these bytes, most significant first.
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// The bytes of the identifier, most significant first.
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

// The places of the dashes in the text form, counted in bytes
const DASHES: [usize; 4] = [4, 6, 8, 10];

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, byte) in self.0.iter().enumerate() {
            if DASHES.contains(&index) {
                f.write_str("-")?;
            }
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// The text is not an identifier in the form that [`Uuid`] prints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UuidParseError;

impl fmt::Display for UuidParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a UUID has 32 hexadecimal digits in groups of 8, 4, 4, 4 and 12")
    }
}

impl Error for UuidParseError {}

impl FromStr for Uuid {
    type Err = UuidParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut bytes = [0_u8; 16];
        let mut at = 0;
        let mut index = 0;
        let text = text.as_bytes();
        while index < 16 {
            if DASHES.contains(&index) {
                if text.get(at) != Some(&b'-') {
                    return Err(UuidParseError);
                }
                at += 1;
            }
            let pair = text.get(at..at + 2).ok_or(UuidParseError)?;
            bytes[index] = (digit(pair[0])? << 4) | digit(pair[1])?;
            at += 2;
            index += 1;
        }
        if at == text.len() {
            Ok(Self(bytes))
        } else {
            Err(UuidParseError)
        }
    }
}

fn digit(byte: u8) -> Result<u8, UuidParseError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(UuidParseError),
    }
}

#[cfg(test)]
#[path = "tests/uuid_tests.rs"]
mod tests;
