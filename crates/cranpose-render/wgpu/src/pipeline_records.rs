//! The pipelines recent launches drew with, as the pipeline cache file keeps
//! them: the next launch builds the last first screen's ahead of its first
//! frame, and after an update every recent one once that frame is drawn.

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

/// A recorded pipeline: how many launches since one drew with it, and
/// whether the last launch drew its first screen with it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Recorded<T> {
    pub(crate) entry: T,
    pub(crate) age: u8,
    pub(crate) first_screen: bool,
}

impl<T> Recorded<T> {
    pub(crate) fn as_ref(&self) -> Recorded<&T> {
        Recorded {
            entry: &self.entry,
            age: self.age,
            first_screen: self.first_screen,
        }
    }

    pub(crate) fn map<U>(self, f: impl FnOnce(T) -> U) -> Recorded<U> {
        Recorded {
            entry: f(self.entry),
            age: self.age,
            first_screen: self.first_screen,
        }
    }
}

/// The recorded pipelines besides the last first screen's shapes, which the
/// file keeps ahead of this section.
#[derive(Default)]
pub(crate) struct PipelineRecords {
    /// Shape pipelines, by their keys' bits.
    pub(crate) shapes: Vec<Recorded<u64>>,
    pub(crate) shaders: Vec<Recorded<ShaderPipelineRecord>>,
    /// Fixed pipelines, by label.
    pub(crate) fixed: Vec<Recorded<String>>,
}

/// Opens the records' section of the cache file.
const SECTION: &[u8; 4] = b"RSP3";

/// Appends a section holding the records to `bytes`, or `None` when one of
/// them does not fit the layout.
pub(crate) fn encode<'a>(
    shapes: impl Iterator<Item = Recorded<u64>>,
    shaders: impl Iterator<Item = Recorded<&'a ShaderPipelineRecord>>,
    fixed: impl Iterator<Item = Recorded<&'a str>>,
    bytes: &mut Vec<u8>,
) -> Option<()> {
    bytes.extend_from_slice(SECTION);
    put_list(bytes, shapes, |bytes, key| {
        bytes.extend_from_slice(&key.to_le_bytes());
        Some(())
    })?;
    put_list(bytes, shaders, |bytes, record| {
        bytes.extend_from_slice(&record.source.to_le_bytes());
        bytes.extend_from_slice(&record.overrides.to_le_bytes());
        bytes.extend_from_slice(&[record.mode, record.variant]);
        put_name(bytes, record.split.as_deref().unwrap_or_default())?;
        bytes.extend_from_slice(&u16::try_from(record.constants.len()).ok()?.to_le_bytes());
        for (name, value) in &record.constants {
            put_name(bytes, name)?;
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        Some(())
    })?;
    put_list(bytes, fixed, put_name)
}

/// Appends the count of `records`, then each entry `put` writes followed by
/// its age and first-screen flag in one byte.
fn put_list<T>(
    bytes: &mut Vec<u8>,
    records: impl Iterator<Item = Recorded<T>>,
    mut put: impl FnMut(&mut Vec<u8>, T) -> Option<()>,
) -> Option<()> {
    let count_at = bytes.len();
    bytes.extend_from_slice(&[0; 4]);
    let mut count = 0_u32;
    for record in records {
        put(bytes, record.entry)?;
        let age = (record.age < 0x80).then_some(record.age)?;
        bytes.push(age << 1 | u8::from(record.first_screen));
        count = count.checked_add(1)?;
    }
    bytes
        .get_mut(count_at..count_at + 4)?
        .copy_from_slice(&count.to_le_bytes());
    Some(())
}

fn put_name(bytes: &mut Vec<u8>, name: &str) -> Option<()> {
    bytes.push(u8::try_from(name.len()).ok()?);
    bytes.extend_from_slice(name.as_bytes());
    Some(())
}

/// The records the section at the start of `bytes` holds and the bytes
/// after it, or `None` when `bytes` opens no section.
pub(crate) fn decode(bytes: &[u8]) -> Option<(PipelineRecords, &[u8])> {
    let mut reader = Reader(bytes.strip_prefix(SECTION.as_slice())?);
    let shapes = reader.list(Reader::u64)?;
    let shaders = reader.list(|reader| {
        let source = reader.u64()?;
        let overrides = reader.u64()?;
        let [mode, variant] = reader.take()?;
        let split = Some(reader.name()?).filter(|split| !split.is_empty());
        let constants = (0..reader.u16()?)
            .map(|_| Some((reader.name()?, f64::from_le_bytes(reader.take()?))))
            .collect::<Option<Vec<_>>>()?;
        Some(ShaderPipelineRecord {
            source,
            overrides,
            mode,
            variant,
            split,
            constants,
        })
    })?;
    let fixed = reader.list(Reader::name)?;
    Some((
        PipelineRecords {
            shapes,
            shaders,
            fixed,
        },
        reader.0,
    ))
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

    /// A count, then that many entries each followed by its mark.
    fn list<T>(
        &mut self,
        mut entry: impl FnMut(&mut Self) -> Option<T>,
    ) -> Option<Vec<Recorded<T>>> {
        (0..self.u32()?)
            .map(|_| {
                let entry = entry(self)?;
                let [mark] = self.take()?;
                Some(Recorded {
                    entry,
                    age: mark >> 1,
                    first_screen: mark & 1 == 1,
                })
            })
            .collect()
    }

    fn name(&mut self) -> Option<String> {
        let [len] = self.take()?;
        let (name, rest) = self.0.split_at_checked(usize::from(len))?;
        self.0 = rest;
        String::from_utf8(name.to_vec()).ok()
    }
}
