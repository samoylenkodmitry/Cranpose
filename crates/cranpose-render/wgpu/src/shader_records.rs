//! The runtime shader pipelines a launch drew its first screen with, as the
//! pipeline cache file keeps them for the next launch to build ahead of its
//! first frame.

/// One runtime shader pipeline as its draw named it: the shader's source and
/// override set, its blend mode, the draw variant, and the overrides it
/// compiled with.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShaderPipelineRecord {
    pub(crate) source: u64,
    pub(crate) overrides: u64,
    pub(crate) mode: u8,
    pub(crate) variant: u8,
    pub(crate) split: Option<String>,
    pub(crate) constants: Vec<(String, f64)>,
}

/// Opens the records' section of the cache file.
const SECTION: &[u8; 4] = b"RSP1";

/// Appends a section holding `records` to `bytes`, or `None` when one of
/// them does not fit the layout.
pub(crate) fn encode(records: &[ShaderPipelineRecord], bytes: &mut Vec<u8>) -> Option<()> {
    bytes.extend_from_slice(SECTION);
    bytes.extend_from_slice(&u32::try_from(records.len()).ok()?.to_le_bytes());
    for record in records {
        bytes.extend_from_slice(&record.source.to_le_bytes());
        bytes.extend_from_slice(&record.overrides.to_le_bytes());
        bytes.extend_from_slice(&[record.mode, record.variant]);
        put_name(bytes, record.split.as_deref().unwrap_or_default())?;
        bytes.extend_from_slice(&u16::try_from(record.constants.len()).ok()?.to_le_bytes());
        for (name, value) in &record.constants {
            put_name(bytes, name)?;
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    Some(())
}

fn put_name(bytes: &mut Vec<u8>, name: &str) -> Option<()> {
    bytes.push(u8::try_from(name.len()).ok()?);
    bytes.extend_from_slice(name.as_bytes());
    Some(())
}

/// The records the section at the start of `bytes` holds and the bytes
/// after it, or `None` when `bytes` opens no section.
pub(crate) fn decode(bytes: &[u8]) -> Option<(Vec<ShaderPipelineRecord>, &[u8])> {
    let mut reader = Reader(bytes.strip_prefix(SECTION.as_slice())?);
    let count = reader.u32()?;
    let mut records = Vec::new();
    for _ in 0..count {
        let source = reader.u64()?;
        let overrides = reader.u64()?;
        let [mode, variant] = reader.take()?;
        let split = Some(reader.name()?).filter(|split| !split.is_empty());
        let constants = (0..reader.u16()?)
            .map(|_| Some((reader.name()?, f64::from_le_bytes(reader.take()?))))
            .collect::<Option<Vec<_>>>()?;
        records.push(ShaderPipelineRecord {
            source,
            overrides,
            mode,
            variant,
            split,
            constants,
        });
    }
    Some((records, reader.0))
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, rest) = self.0.split_first_chunk::<N>()?;
        self.0 = rest;
        Some(*head)
    }

    fn u16(&mut self) -> Option<u16> {
        self.take().map(u16::from_le_bytes)
    }

    fn u32(&mut self) -> Option<u32> {
        self.take().map(u32::from_le_bytes)
    }

    fn u64(&mut self) -> Option<u64> {
        self.take().map(u64::from_le_bytes)
    }

    fn name(&mut self) -> Option<String> {
        let [len] = self.take()?;
        let (name, rest) = self.0.split_at_checked(usize::from(len))?;
        self.0 = rest;
        String::from_utf8(name.to_vec()).ok()
    }
}
