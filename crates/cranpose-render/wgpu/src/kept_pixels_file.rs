// The file that keeps the outputs of shader layers between launches, and
// the thread that writes it.
//
// The file holds a header (MAGIC, the file key, the launch that wrote it)
// and then entries up to its end. Each entry holds its key, format, width,
// height, the last launch that used it, its length and its pixels. A later
// entry under a key replaces an earlier one.

use std::{
    fs::{File, OpenOptions},
    hash::{Hash, Hasher},
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex, PoisonError,
        mpsc::{self, RecvTimeoutError},
    },
    thread::JoinHandle,
    time::Duration,
};

use cranpose_core::collections::map::{HashMap, HashSet};
use cranpose_ui_graphics::FxHasher;

const MAGIC: &[u8; 4] = b"CKP2";
const HEADER_BYTES: u64 = 4 + 8 + 4;
const HEAD_BYTES: u64 = 8 + 8 + 4 + 4 + 4 + 4;
// How many launches an output stays on disk after the last that used it.
const KEPT_LAUNCHES: u32 = 4;
// The most bytes of pixels the file keeps.
const MOST_BYTES: u64 = 32 << 20;
// How long drawn outputs must pause before the file takes them: the fills
// of a screen's first frames reach the file in one write.
const QUIET: Duration = Duration::from_secs(1);

// Only one writer reads and writes a file at a time.
static WRITING: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy)]
pub(crate) struct Head {
    pub(crate) key: u64,
    pub(crate) format: u64,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) last: u32,
    pub(crate) len: u32,
}

// An entry of the file and where its pixels start.
#[derive(Clone, Copy)]
pub(crate) struct Indexed {
    pub(crate) head: Head,
    pub(crate) offset: u64,
}

pub(crate) struct Drawn {
    pub(crate) head: Head,
    pub(crate) bytes: Vec<u8>,
}

struct Batch {
    drawn: Vec<Drawn>,
    // every key the launch used so far
    used: HashSet<u64>,
}

// The thread that writes what a launch draws into its file.
pub(crate) struct Writer {
    batches: Option<mpsc::Sender<Batch>>,
    thread: Option<JoinHandle<()>>,
}

// The file's key: its layout, the framework's shaders and this crate's
// version, as the pipeline cache keys its file.
fn file_key() -> u64 {
    let mut hasher = FxHasher::default();
    MAGIC.hash(&mut hasher);
    hasher.write_u64(cranpose_ui_graphics::framework_shaders::SOURCES_KEY);
    hasher.write(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.finish()
}

// The launch that wrote `file` and where the last entry under each key
// lies, or none when its layout or version differ.
pub(crate) fn read_index(file: File) -> Option<(u32, File, HashMap<u64, Indexed>)> {
    let end = file.metadata().ok()?.len();
    let mut reader = BufReader::new(file);
    let launch = read_header(&mut reader)?;
    let mut index = HashMap::default();
    let mut offset = HEADER_BYTES;
    while let Some(head) = read_head(&mut reader) {
        offset += HEAD_BYTES;
        let len = u64::from(head.len);
        if offset + len > end || reader.seek_relative(i64::from(head.len)).is_err() {
            break;
        }
        index.insert(head.key, Indexed { head, offset });
        offset += len;
    }
    Some((launch, reader.into_inner(), index))
}

fn read_header(reader: &mut impl Read) -> Option<u32> {
    let mut header = [0; HEADER_BYTES as usize];
    reader.read_exact(&mut header).ok()?;
    let (magic, rest) = header.split_first_chunk::<4>()?;
    let (key, launch) = rest.split_first_chunk::<8>()?;
    if magic != MAGIC || u64::from_le_bytes(*key) != file_key() {
        return None;
    }
    Some(u32::from_le_bytes(launch.try_into().ok()?))
}

fn read_head(reader: &mut impl Read) -> Option<Head> {
    let mut bytes = [0; HEAD_BYTES as usize];
    reader.read_exact(&mut bytes).ok()?;
    let word =
        |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let long = |at: usize| u64::from(word(at)) | u64::from(word(at + 4)) << 32;
    Some(Head {
        key: long(0),
        format: long(8),
        width: word(16),
        height: word(20),
        last: word(24),
        len: word(28),
    })
}

fn write_entry(writer: &mut impl Write, head: Head, bytes: &[u8]) -> std::io::Result<()> {
    writer.write_all(&head.key.to_le_bytes())?;
    writer.write_all(&head.format.to_le_bytes())?;
    writer.write_all(&head.width.to_le_bytes())?;
    writer.write_all(&head.height.to_le_bytes())?;
    writer.write_all(&head.last.to_le_bytes())?;
    writer.write_all(&head.len.to_le_bytes())?;
    writer.write_all(bytes)
}

impl Writer {
    // A thread that writes `launch`'s outputs into the file at `path`.
    pub(crate) fn spawn(path: PathBuf, launch: u32) -> Option<Self> {
        let (batches, received) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("kept-pixels".into())
            .spawn(move || run(&path, launch, &received));
        match thread {
            Ok(thread) => Some(Self {
                batches: Some(batches),
                thread: Some(thread),
            }),
            Err(error) => {
                log::warn!("[kept-pixels] writer: {error}");
                None
            }
        }
    }

    // Hands `drawn` to the file, with every key the launch `used` so far.
    pub(crate) fn send(&self, drawn: Vec<Drawn>, used: HashSet<u64>) {
        if let Some(batches) = &self.batches {
            let _ = batches.send(Batch { drawn, used });
        }
    }
}

impl Drop for Writer {
    // Writes what is left at once and waits for it, so the next launch
    // finds it.
    fn drop(&mut self) {
        self.batches = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

// Writes each burst of drawn outputs once it pauses: the launch's first
// write replaces the file with its outputs and the kept ones; a later
// write adds its outputs at the file's end.
fn run(path: &Path, launch: u32, batches: &mpsc::Receiver<Batch>) {
    // the bytes of pixels in the file, once this launch replaced it
    let mut total = None;
    while let Ok(Batch {
        mut drawn,
        mut used,
    }) = batches.recv()
    {
        let closed = loop {
            match batches.recv_timeout(QUIET) {
                Ok(batch) => {
                    drawn.extend(batch.drawn);
                    used = batch.used;
                }
                Err(RecvTimeoutError::Timeout) => break false,
                Err(RecvTimeoutError::Disconnected) => break true,
            }
        };
        let _writing = WRITING.lock().unwrap_or_else(PoisonError::into_inner);
        let count = drawn.len();
        let written = match total {
            None => replace(path, launch, drawn, &used),
            Some(total) => append(path, total, drawn),
        };
        total = match written {
            Ok(written) => {
                log::info!("[kept-pixels] wrote {count} outputs, {written} B of pixels");
                Some(written)
            }
            Err(error) => {
                log::warn!("[kept-pixels] write {path:?}: {error}");
                None
            }
        };
        if closed {
            return;
        }
    }
}

// Replaces the file with `drawn` and with the outputs it kept that this
// launch `used` or that a recent launch used; returns the bytes of pixels
// written.
fn replace(
    path: &Path,
    launch: u32,
    drawn: Vec<Drawn>,
    used: &HashSet<u64>,
) -> std::io::Result<u64> {
    let tmp = path.with_extension("pixels.tmp");
    let mut writer = BufWriter::new(File::create(&tmp)?);
    writer.write_all(MAGIC)?;
    writer.write_all(&file_key().to_le_bytes())?;
    writer.write_all(&launch.to_le_bytes())?;
    let fresh: HashSet<u64> = drawn.iter().map(|drawn| drawn.head.key).collect();
    let mut total = write_drawn(&mut writer, 0, drawn)?;
    if let Some((_, mut earlier, index)) = File::open(path).ok().and_then(read_index) {
        let mut kept: Vec<Indexed> = index
            .into_values()
            .filter(|indexed| !fresh.contains(&indexed.head.key))
            .collect();
        kept.sort_unstable_by_key(|indexed| indexed.offset);
        let mut bytes = Vec::new();
        for Indexed { mut head, offset } in kept {
            if used.contains(&head.key) {
                head.last = launch;
            }
            let len = u64::from(head.len);
            if launch.wrapping_sub(head.last) >= KEPT_LAUNCHES || total + len > MOST_BYTES {
                continue;
            }
            bytes.resize(head.len as usize, 0);
            earlier.seek(SeekFrom::Start(offset))?;
            earlier.read_exact(&mut bytes)?;
            write_entry(&mut writer, head, &bytes)?;
            total += len;
        }
    }
    writer.into_inner()?.sync_all()?;
    std::fs::rename(&tmp, path)?;
    Ok(total)
}

// Adds `drawn` at the end of the file, which holds `total` bytes of pixels;
// returns the bytes of pixels it then holds.
fn append(path: &Path, total: u64, drawn: Vec<Drawn>) -> std::io::Result<u64> {
    let mut writer = BufWriter::new(OpenOptions::new().append(true).open(path)?);
    let total = write_drawn(&mut writer, total, drawn)?;
    writer.into_inner()?.sync_all()?;
    Ok(total)
}

fn write_drawn(writer: &mut impl Write, mut total: u64, drawn: Vec<Drawn>) -> std::io::Result<u64> {
    for drawn in drawn {
        let len = u64::from(drawn.head.len);
        if total + len <= MOST_BYTES {
            write_entry(writer, drawn.head, &drawn.bytes)?;
            total += len;
        }
    }
    Ok(total)
}
