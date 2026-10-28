use std::{
    any::{Any, TypeId},
    hash::{Hash, Hasher},
    rc::Rc,
    sync::OnceLock,
};

use crate::{Key, thread_state};

#[cfg(any(test, debug_assertions))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LocationKeyDebugInfo {
    pub(crate) file: String,
    pub(crate) line: u32,
    pub(crate) column: u32,
}

#[cfg(any(test, debug_assertions))]
fn register_location_key_debug_info(key: Key, file: &str, line: u32, column: u32) {
    let info = || LocationKeyDebugInfo {
        file: file.to_owned(),
        line,
        column,
    };
    let collision = thread_state::runtimes(|state| {
        let mut registry = state.location_keys.borrow_mut();
        match registry.entry(key) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                thread_state::watch_thread_exit();
                entry.insert(info());
                None
            }
            std::collections::hash_map::Entry::Occupied(entry) => {
                let existing = entry.get();
                (existing.file != file || existing.line != line || existing.column != column)
                    .then(|| (existing.clone(), info()))
            }
        }
    });
    if let Some((existing, incoming)) = collision {
        thread_state::cells(|cells| {
            let count = &cells.location_key_collisions;
            count.set(count.get().saturating_add(1));
        });
        log::error!("location key collision: key={key} first={existing:?} second={incoming:?}");
    }
}

#[cfg(all(debug_assertions, not(test)))]
fn location_key_diagnostics_enabled() -> bool {
    crate::DIAGNOSTICS.location_key_diagnostics.is_set()
}

#[cfg(test)]
pub(crate) fn register_location_key_debug_info_for_test(
    key: Key,
    file: &str,
    line: u32,
    column: u32,
) {
    register_location_key_debug_info(key, file, line, column);
}

#[cfg(test)]
pub(crate) fn location_key_debug_collision_count_for_test() -> usize {
    thread_state::cells(|cells| cells.location_key_collisions.get())
}

#[cfg(test)]
pub(crate) fn location_key_debug_info_for_test(key: Key) -> Option<LocationKeyDebugInfo> {
    thread_state::runtimes(|state| state.location_keys.borrow().get(&key).cloned())
}

const fn source_location_hash(file: &str, line: u32, column: u32) -> u64 {
    position_location_hash(file_location_hash(file), line, column)
}

const fn file_location_hash(file: &str) -> u64 {
    fnv1a_location_key_bytes(0xcbf2_9ce4_8422_2325u64, file.as_bytes())
}

/// [`file_location_hash`] of a caller's source path, remembered by the
/// path's address: a `&'static str` keeps its bytes for the whole run, and
/// hashing the path on every composable call cost an eighth of composing a
/// feed item.
fn static_file_location_hash(cells: &thread_state::ThreadCells, file: &'static str) -> u64 {
    let address = file.as_ptr() as usize;
    let cached = &cells.file_hashes[(address >> 4) % thread_state::FILE_HASH_SLOTS];
    let (cached_address, cached_len, hash) = cached.get();
    if cached_address == address && cached_len == file.len() {
        return hash;
    }
    let hash = file_location_hash(file);
    cached.set((address, file.len(), hash));
    hash
}

/// The file's hash with a position in it: line and column fill the two
/// halves of a word, so two positions of a file never give the same word.
/// The keys made from it go through [`avalanche_location_key`], which mixes
/// every bit.
const fn position_location_hash(hash: u64, line: u32, column: u32) -> u64 {
    hash ^ (((line as u64) << 32) | column as u64)
}

const fn fnv1a_location_key_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    let mut index = 0;
    while index < bytes.len() {
        hash = (hash ^ bytes[index] as u64).wrapping_mul(0x0000_0100_0000_01b3);
        index += 1;
    }
    hash
}

pub(crate) const fn avalanche_location_key(mut value: u64) -> u64 {
    value ^= value >> 33;
    value = value.wrapping_mul(0xff51_afd7_ed55_8ccd);
    value ^= value >> 33;
    value = value.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    value ^ (value >> 33)
}

#[doc(hidden)]
#[track_caller]
pub fn caller_location_key() -> Key {
    let caller = std::panic::Location::caller();
    let file = caller.file();
    if cfg!(feature = "hot-reload")
        && let Some(key) = hot_call_site_key(file, caller.line(), caller.column())
    {
        return key;
    }
    call_site_key(caller)
}

/// The key of a call site, made from its `Location`'s contents, so two
/// statics for one location give the same key. The thread keeps the key by
/// the `Location`'s address, so a call site it has seen costs a load and a
/// compare; [`static_file_location_hash`] and the finalizer make a new one.
fn call_site_key(caller: &'static std::panic::Location<'static>) -> Key {
    let address = std::ptr::from_ref(caller) as usize;
    thread_state::cells(|cells| {
        let slot = address / size_of::<std::panic::Location<'static>>()
            % thread_state::CALL_SITE_KEY_SLOTS;
        let cached = &cells.call_site_keys[slot];
        let (cached_address, cached_key) = cached.get();
        if cached_address == address {
            return cached_key;
        }
        let file = caller.file();
        let key = registered_location_key(
            static_file_location_hash(cells, file),
            file,
            caller.line(),
            caller.column(),
        );
        cached.set((address, key));
        key
    })
}

#[derive(Clone, Copy)]
pub(crate) struct HotOrigin {
    file: &'static str,
    line: u32,
    end: u32,
    identity: Key,
}

impl HotOrigin {
    pub(crate) const NONE: Self = Self {
        file: "",
        line: 0,
        end: 0,
        identity: Key(0),
    };
}

/// Restores the previous hot-reload origin when a composable body or closure ends.
#[doc(hidden)]
pub struct HotOriginGuard {
    previous: HotOrigin,
}

impl Drop for HotOriginGuard {
    fn drop(&mut self) {
        let previous = self.previous;
        thread_state::cells(|cells| cells.hot_origin.set(previous));
    }
}

/// Marks the lexically enclosing composable definition for the
/// development-only `hot-reload` expansion. Call sites inside its source,
/// lines `line..=end` of `file`, are keyed by the definition's identity and
/// their line relative to it, so code moved by an edit elsewhere in the file
/// keeps its identity after a hot patch. Other call sites keep absolute keys.
/// Within one build the keys are as distinct as absolute locations.
#[doc(hidden)]
pub fn hot_origin(file: &'static str, line: u32, end: u32, identity: Key) -> HotOriginGuard {
    let next = HotOrigin {
        file,
        line,
        end,
        identity,
    };
    HotOriginGuard {
        previous: thread_state::cells(|cells| cells.hot_origin.replace(next)),
    }
}

pub(crate) fn hot_call_site_key(file: &str, line: u32, column: u32) -> Option<Key> {
    let origin = thread_state::cells(|cells| cells.hot_origin.get());
    if origin.file.is_empty() || !(origin.line..=origin.end).contains(&line) || origin.file != file
    {
        return None;
    }
    let relative = line.wrapping_sub(origin.line);
    let hash = fnv1a_location_key_bytes(origin.identity.0, &relative.to_le_bytes());
    Some(Key(avalanche_location_key(fnv1a_location_key_bytes(
        hash ^ 0xfc,
        &column.to_le_bytes(),
    ))))
}

/// Branch group key for the development-only `hot-reload` expansion: the
/// file and a hash of the guard's structural path, independent of lines.
#[doc(hidden)]
pub const fn hot_branch_key(file: &str, path_hash: u64) -> Key {
    let hash = file_location_hash(file);
    Key(avalanche_location_key(fnv1a_location_key_bytes(
        hash ^ 0xfd,
        &path_hash.to_le_bytes(),
    )))
}

/// Composable definition key for the development-only `hot-reload`
/// expansion: the file, module and function name, independent of lines.
#[doc(hidden)]
pub const fn hot_definition_key(file: &str, module: &str, name: &str) -> Key {
    let mut hash = file_location_hash(file);
    hash = fnv1a_location_key_bytes(hash ^ 0xfe, module.as_bytes());
    Key(avalanche_location_key(fnv1a_location_key_bytes(
        hash ^ 0xff,
        name.as_bytes(),
    )))
}

#[doc(hidden)]
#[track_caller]
pub fn composable_identity_key(definition: Key) -> Key {
    Key(
        (definition.0.wrapping_mul(0x0000_0100_0000_01b3) ^ caller_location_key().0)
            .wrapping_mul(0x0000_0100_0000_01b3),
    )
}

#[doc(hidden)]
pub fn composable_definition_key(file: &str, line: u32, column: u32, marker: TypeId) -> Key {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    Hash::hash(&marker, &mut hasher);
    Key(location_key(file, line, column).0 ^ avalanche_location_key(Hasher::finish(&hasher)))
}

/// The key [`caller_location_key`] gives a call site, as a constant the
/// `#[composable]` macro writes, so the call takes no thread-local lookup.
#[doc(hidden)]
pub const fn const_location_key(file: &str, line: u32, column: u32) -> Key {
    Key(avalanche_location_key(source_location_hash(
        file, line, column,
    )))
}

/// [`location_key()`] of the place it is written, worked out at build time
/// instead of hashing the source path on every call.
#[macro_export]
macro_rules! location_key {
    () => {{
        const KEY: $crate::Key = $crate::const_location_key(file!(), line!(), column!());
        $crate::noted_location_key(KEY, file!(), line!(), column!())
    }};
}

/// Returns a constant key the `#[composable]` macro wrote, after recording
/// where it came from for collision diagnostics in debug builds.
#[doc(hidden)]
#[inline(always)]
pub fn noted_location_key(key: Key, file: &str, line: u32, column: u32) -> Key {
    note_location_key(key, file, line, column);
    key
}

#[doc(hidden)]
pub fn cached_composable_definition_key(
    cell: &OnceLock<Key>,
    file: &str,
    line: u32,
    column: u32,
    marker: TypeId,
) -> Key {
    *cell.get_or_init(|| composable_definition_key(file, line, column, marker))
}

/// Returns the key of a source location: a hash of the file path together with the line and column.
/// The same location always gives the same key.
pub fn location_key(file: &str, line: u32, column: u32) -> Key {
    registered_location_key(file_location_hash(file), file, line, column)
}

fn registered_location_key(file_hash: u64, file: &str, line: u32, column: u32) -> Key {
    let key = Key(avalanche_location_key(position_location_hash(
        file_hash, line, column,
    )));
    note_location_key(key, file, line, column);
    key
}

/// Records where `key` came from, for collision diagnostics in debug builds.
#[cfg(any(test, debug_assertions))]
fn note_location_key(key: Key, file: &str, line: u32, column: u32) {
    #[cfg(test)]
    register_location_key_debug_info(key, file, line, column);
    #[cfg(all(debug_assertions, not(test)))]
    if location_key_diagnostics_enabled() {
        register_location_key_debug_info(key, file, line, column);
    }
}

#[cfg(not(any(test, debug_assertions)))]
fn note_location_key(_key: Key, _file: &str, _line: u32, _column: u32) {}

/// The key of the `branch`th branch guard the `#[composable]` macro wrote at
/// a source position, as a constant.
#[doc(hidden)]
pub const fn branch_location_key(file: &str, line: u32, column: u32, branch: u32) -> Key {
    let mut hash = source_location_hash(file, line, column);
    hash = fnv1a_location_key_bytes(hash, &[0xfd]);
    hash = fnv1a_location_key_bytes(hash, &branch.to_le_bytes());
    Key(avalanche_location_key(hash))
}

/// The key of the `index`th constant value the `#[composable]` macro keeps
/// at a source position, as a constant.
#[doc(hidden)]
pub const fn constant_value_key(file: &str, line: u32, column: u32, index: u32) -> Key {
    let mut hash = source_location_hash(file, line, column);
    hash = fnv1a_location_key_bytes(hash, &[0xfc]);
    hash = fnv1a_location_key_bytes(hash, &index.to_le_bytes());
    Key(avalanche_location_key(hash))
}

/// Slots of the per-thread table of constant values, as a power of two.
const CONSTANT_SLOTS: usize = 128;

/// The value `make` gives, for an expression `#[composable]` found to give
/// the same value every time it runs, such as a modifier chain of
/// constants: `key` names the expression. The thread keeps the value it
/// made last for `key` in a table of [`CONSTANT_SLOTS`] slots, so the
/// expression is made again only when another expression took its slot.
/// It needs no composition: the expression may sit in a closure that runs
/// in an event handler, an effect or another thread.
#[doc(hidden)]
pub fn __constant<T: Clone + 'static>(key: Key, make: impl FnOnce() -> T) -> T {
    let slot = (key.0 as usize) & (CONSTANT_SLOTS - 1);
    if let Some(value) = kept_constant(key, slot).and_then(|kept| kept.downcast_ref::<T>().cloned())
    {
        return value;
    }
    let value = make();
    keep_constant(key, slot, Rc::new(value.clone()));
    value
}

// The value kept for `key` in `slot`, outside `__constant` so its callers'
// types make no thread state access each.
fn kept_constant(key: Key, slot: usize) -> Option<Rc<dyn Any>> {
    thread_state::try_runtimes(|state| {
        let constants = state.constants.try_borrow().ok()?;
        let (kept_key, value) = constants.get(slot)?.as_ref()?;
        (*kept_key == key).then(|| Rc::clone(value))
    })
    .flatten()
}

fn keep_constant(key: Key, slot: usize, stored: Rc<dyn Any>) {
    thread_state::try_runtimes(|state| {
        let Ok(mut constants) = state.constants.try_borrow_mut() else {
            return;
        };
        if constants.is_empty() {
            constants.resize_with(CONSTANT_SLOTS, || None);
        }
        if let Some(entry) = constants.get_mut(slot) {
            *entry = Some((key, stored));
        }
    });
}
