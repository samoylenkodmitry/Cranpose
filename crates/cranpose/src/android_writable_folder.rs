//! Android writable-folder store built on the Storage Access Framework.
//!
//! The write-side complement of [`crate::android_file_picker`]. The user grants
//! a persistent read/write tree through the chooser in that module; this turns
//! the granted tree URI into a [`WritableFolderStore`]. Document I/O runs
//! synchronously over JNI and is safe to call from a background worker thread —
//! [`crate::android_jni::with_android_activity_env`] attaches the calling
//! thread.
#![allow(unsafe_code)]

use std::{
    fs::File,
    io::{Read, Write},
    os::fd::FromRawFd,
    sync::{Arc, OnceLock},
};

use android_activity::AndroidApp;
use cranpose_services::{
    DEFAULT_CHUNK_LEN, FolderEntry, FolderError, FolderReader, FolderWriter, WritableFolderStore,
    WritableFolderStoreRef, set_writable_folder_store_factory,
};
use jni::{
    Env, jni_sig, jni_str,
    objects::{JByteArray, JObject, JString, JValue},
};

static APP: OnceLock<AndroidApp> = OnceLock::new();

/// Registers the Android writable-folder store factory. Called once at startup
/// from [`crate::android::run`].
pub(crate) fn register(app: AndroidApp) {
    let _ = APP.set(app);
    set_writable_folder_store_factory(Box::new(|handle| {
        Some(Arc::new(AndroidWritableFolder {
            tree: handle.to_string(),
        }) as WritableFolderStoreRef)
    }));
}

fn app() -> Result<&'static AndroidApp, String> {
    APP.get()
        .ok_or_else(|| "Android writable folder backend is not registered".to_string())
}

fn with_env<T, F>(f: F) -> Result<T, String>
where
    F: for<'local> FnOnce(&mut Env<'local>, JObject<'local>) -> Result<T, String>,
{
    crate::android_jni::with_android_activity_env(app()?, f)
}

fn string_err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// Staging suffix used by [`AndroidFolderWriter`], committed with
/// `cranposeFolderCommit` so a partial write never replaces a good document.
const STAGING_SUFFIX: &str = ".tmp";

struct AndroidWritableFolder {
    /// The persisted SAF tree URI (`content://…`).
    tree: String,
}

impl WritableFolderStore for AndroidWritableFolder {
    fn write(&self, name: &str, contents: &[u8]) -> Result<(), FolderError> {
        match call_folder_write(&self.tree, name, contents).map_err(FolderError::Io)? {
            0 => Ok(()),
            1 => Err(FolderError::ReadOnly),
            _ => Err(FolderError::Io("SAF write failed".to_string())),
        }
    }

    fn read(&self, name: &str) -> Result<Vec<u8>, FolderError> {
        match call_folder_read(&self.tree, name).map_err(FolderError::Io)? {
            Some(bytes) => Ok(bytes),
            None => Err(FolderError::NotFound(name.to_string())),
        }
    }

    fn list(&self) -> Result<Vec<FolderEntry>, FolderError> {
        match call_folder_list(&self.tree).map_err(FolderError::Io)? {
            Some(text) => Ok(text.lines().filter_map(parse_entry).collect()),
            None => Err(FolderError::Io("SAF list failed".to_string())),
        }
    }

    fn remove(&self, name: &str) -> Result<(), FolderError> {
        match call_folder_remove(&self.tree, name).map_err(FolderError::Io)? {
            0 => Ok(()),
            _ => Err(FolderError::Io("SAF remove failed".to_string())),
        }
    }

    fn open_read(&self, name: &str) -> Result<Box<dyn FolderReader>, FolderError> {
        let fd =
            call_folder_descriptor(&self.tree, name, Descriptor::Read).map_err(FolderError::Io)?;
        if fd < 0 {
            return Err(FolderError::NotFound(name.to_string()));
        }
        // SAFETY: the Java side detaches the descriptor from its
        // `ParcelFileDescriptor`, transferring ownership to this process; the
        // `File` closes it on drop.
        Ok(Box::new(AndroidFolderReader {
            file: Some(unsafe { File::from_raw_fd(fd) }),
        }))
    }

    fn open_write(&self, name: &str) -> Result<Box<dyn FolderWriter>, FolderError> {
        let staging = format!("{name}{STAGING_SUFFIX}");
        let fd = call_folder_descriptor(&self.tree, &staging, Descriptor::Write)
            .map_err(FolderError::Io)?;
        if fd < 0 {
            return Err(FolderError::ReadOnly);
        }
        // SAFETY: as above — ownership of the detached descriptor moves here.
        Ok(Box::new(AndroidFolderWriter {
            tree: self.tree.clone(),
            staging,
            target: name.to_string(),
            file: Some(unsafe { File::from_raw_fd(fd) }),
        }))
    }

    fn is_writable(&self) -> bool {
        call_folder_writable(&self.tree).unwrap_or(false)
    }

    fn handle(&self) -> String {
        self.tree.clone()
    }
}

/// Parses one `name\tsize\tmodified` row from `cranposeFolderList`.
fn parse_entry(row: &str) -> Option<FolderEntry> {
    let mut fields = row.split('\t');
    let name = fields.next()?;
    if name.is_empty() {
        return None;
    }
    Some(FolderEntry {
        name: name.to_string(),
        len: fields.next().unwrap_or_default().parse().unwrap_or(0),
        modified_millis: fields.next().unwrap_or_default().parse().ok(),
    })
}

struct AndroidFolderReader {
    file: Option<File>,
}

impl FolderReader for AndroidFolderReader {
    fn read_chunk(&mut self) -> Result<Option<Vec<u8>>, FolderError> {
        let Some(file) = self.file.as_mut() else {
            return Ok(None);
        };
        let mut buffer = vec![0u8; DEFAULT_CHUNK_LEN];
        let mut filled = 0;
        while filled < buffer.len() {
            match file.read(&mut buffer[filled..]) {
                Ok(0) => break,
                Ok(read) => filled += read,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(FolderError::Io(error.to_string())),
            }
        }
        if filled == 0 {
            self.file = None;
            return Ok(None);
        }
        buffer.truncate(filled);
        Ok(Some(buffer))
    }
}

struct AndroidFolderWriter {
    tree: String,
    staging: String,
    target: String,
    file: Option<File>,
}

impl FolderWriter for AndroidFolderWriter {
    fn write_chunk(&mut self, bytes: &[u8]) -> Result<(), FolderError> {
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| FolderError::Io("folder writer is already finished".into()))?;
        file.write_all(bytes)
            .map_err(|error| FolderError::Io(error.to_string()))
    }

    fn finish(mut self: Box<Self>) -> Result<(), FolderError> {
        let Some(mut file) = self.file.take() else {
            return Ok(());
        };
        file.flush()
            .map_err(|error| FolderError::Io(error.to_string()))?;
        file.sync_all()
            .map_err(|error| FolderError::Io(error.to_string()))?;
        drop(file);
        match call_folder_commit(&self.tree, &self.staging, &self.target)
            .map_err(FolderError::Io)?
        {
            0 => Ok(()),
            1 => Err(FolderError::ReadOnly),
            _ => Err(FolderError::Io("SAF commit failed".to_string())),
        }
    }
}

impl Drop for AndroidFolderWriter {
    fn drop(&mut self) {
        if self.file.is_some() {
            let _ = call_folder_remove(&self.tree, &self.staging);
        }
    }
}

/// Which descriptor mode `cranposeFolderOpen*` should hand back.
#[derive(Clone, Copy)]
enum Descriptor {
    Read,
    Write,
}

fn call_folder_descriptor(tree: &str, name: &str, mode: Descriptor) -> Result<i32, String> {
    with_env(|env, activity| {
        let tree = env.new_string(tree).map_err(string_err)?;
        let name = env.new_string(name).map_err(string_err)?;
        let tree_obj: &JObject = tree.as_ref();
        let name_obj: &JObject = name.as_ref();
        let arguments = [JValue::Object(tree_obj), JValue::Object(name_obj)];
        let signature = jni_sig!("(Ljava/lang/String;Ljava/lang/String;)I");
        match mode {
            Descriptor::Read => env.call_method(
                &activity,
                jni_str!("cranposeFolderOpenRead"),
                signature,
                &arguments,
            ),
            Descriptor::Write => env.call_method(
                &activity,
                jni_str!("cranposeFolderOpenWrite"),
                signature,
                &arguments,
            ),
        }
        .and_then(|value| value.i())
        .map_err(string_err)
    })
}

fn call_folder_commit(tree: &str, staging: &str, target: &str) -> Result<i32, String> {
    with_env(|env, activity| {
        let tree = env.new_string(tree).map_err(string_err)?;
        let staging = env.new_string(staging).map_err(string_err)?;
        let target = env.new_string(target).map_err(string_err)?;
        let tree_obj: &JObject = tree.as_ref();
        let staging_obj: &JObject = staging.as_ref();
        let target_obj: &JObject = target.as_ref();
        env.call_method(
            &activity,
            jni_str!("cranposeFolderCommit"),
            jni_sig!("(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)I"),
            &[
                JValue::Object(tree_obj),
                JValue::Object(staging_obj),
                JValue::Object(target_obj),
            ],
        )
        .and_then(|value| value.i())
        .map_err(string_err)
    })
}

fn call_folder_write(tree: &str, name: &str, contents: &[u8]) -> Result<i32, String> {
    with_env(|env, activity| {
        let tree = env.new_string(tree).map_err(string_err)?;
        let name = env.new_string(name).map_err(string_err)?;
        let bytes = env.byte_array_from_slice(contents).map_err(string_err)?;
        let tree_obj: &JObject = tree.as_ref();
        let name_obj: &JObject = name.as_ref();
        let bytes_obj: &JObject = bytes.as_ref();
        env.call_method(
            &activity,
            jni_str!("cranposeFolderWrite"),
            jni_sig!("(Ljava/lang/String;Ljava/lang/String;[B)I"),
            &[
                JValue::Object(tree_obj),
                JValue::Object(name_obj),
                JValue::Object(bytes_obj),
            ],
        )
        .and_then(|value| value.i())
        .map_err(string_err)
    })
}

fn call_folder_read(tree: &str, name: &str) -> Result<Option<Vec<u8>>, String> {
    with_env(|env, activity| {
        let tree = env.new_string(tree).map_err(string_err)?;
        let name = env.new_string(name).map_err(string_err)?;
        let tree_obj: &JObject = tree.as_ref();
        let name_obj: &JObject = name.as_ref();
        let result = env
            .call_method(
                &activity,
                jni_str!("cranposeFolderRead"),
                jni_sig!("(Ljava/lang/String;Ljava/lang/String;)[B"),
                &[JValue::Object(tree_obj), JValue::Object(name_obj)],
            )
            .and_then(|value| value.l())
            .map_err(string_err)?;
        if result.is_null() {
            return Ok(None);
        }
        let array = JByteArray::cast_local(env, result).map_err(string_err)?;
        Ok(Some(env.convert_byte_array(&array).map_err(string_err)?))
    })
}

fn call_folder_list(tree: &str) -> Result<Option<String>, String> {
    with_env(|env, activity| {
        let tree = env.new_string(tree).map_err(string_err)?;
        let tree_obj: &JObject = tree.as_ref();
        let result = env
            .call_method(
                &activity,
                jni_str!("cranposeFolderList"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/String;"),
                &[JValue::Object(tree_obj)],
            )
            .and_then(|value| value.l())
            .map_err(string_err)?;
        if result.is_null() {
            return Ok(None);
        }
        let text = JString::cast_local(env, result)
            .map_err(string_err)?
            .try_to_string(env)
            .map_err(string_err)?;
        Ok(Some(text))
    })
}

fn call_folder_remove(tree: &str, name: &str) -> Result<i32, String> {
    with_env(|env, activity| {
        let tree = env.new_string(tree).map_err(string_err)?;
        let name = env.new_string(name).map_err(string_err)?;
        let tree_obj: &JObject = tree.as_ref();
        let name_obj: &JObject = name.as_ref();
        env.call_method(
            &activity,
            jni_str!("cranposeFolderRemove"),
            jni_sig!("(Ljava/lang/String;Ljava/lang/String;)I"),
            &[JValue::Object(tree_obj), JValue::Object(name_obj)],
        )
        .and_then(|value| value.i())
        .map_err(string_err)
    })
}

fn call_folder_writable(tree: &str) -> Result<bool, String> {
    with_env(|env, activity| {
        let tree = env.new_string(tree).map_err(string_err)?;
        let tree_obj: &JObject = tree.as_ref();
        env.call_method(
            &activity,
            jni_str!("cranposeFolderWritable"),
            jni_sig!("(Ljava/lang/String;)Z"),
            &[JValue::Object(tree_obj)],
        )
        .and_then(|value| value.z())
        .map_err(string_err)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_rows_carry_size_and_modified_time() {
        let entry = parse_entry("sync.json\t128\t1700000000000").expect("a row parses");
        assert_eq!(entry.name, "sync.json");
        assert_eq!(entry.len, 128);
        assert_eq!(entry.modified_millis, Some(1_700_000_000_000));
    }

    #[test]
    fn a_provider_without_metadata_still_lists_the_name() {
        let entry = parse_entry("plain.bin\t\t").expect("a row parses");
        assert_eq!(entry.len, 0);
        assert_eq!(entry.modified_millis, None);
        assert!(parse_entry("").is_none());
    }
}
