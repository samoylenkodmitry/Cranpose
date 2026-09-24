use super::*;

struct FlatStore {
    handle: String,
}

impl WritableFolderStore for FlatStore {
    fn write(&self, _name: &str, _contents: &[u8]) -> Result<(), FolderError> {
        Ok(())
    }
    fn read(&self, _name: &str) -> Result<Vec<u8>, FolderError> {
        Ok(Vec::new())
    }
    fn list(&self) -> Result<Vec<FolderEntry>, FolderError> {
        Ok(vec![FolderEntry {
            name: "sync.json".into(),
            len: 12,
            modified_millis: Some(5),
        }])
    }
    fn remove(&self, _name: &str) -> Result<(), FolderError> {
        Ok(())
    }
    fn open_read(&self, _name: &str) -> Result<Box<dyn FolderReader>, FolderError> {
        Err(FolderError::Unsupported)
    }
    fn open_write(&self, _name: &str) -> Result<Box<dyn FolderWriter>, FolderError> {
        Err(FolderError::Unsupported)
    }
    fn is_writable(&self) -> bool {
        true
    }
    fn handle(&self) -> String {
        self.handle.clone()
    }
}

#[test]
fn folder_error_messages_are_distinct() {
    assert_eq!(
        FolderError::ReadOnly.to_string(),
        "writable folder is read-only"
    );
    assert!(
        FolderError::NotFound("a.txt".into())
            .to_string()
            .contains("a.txt")
    );
}

#[test]
fn the_default_display_name_is_the_last_handle_segment() {
    let store = FlatStore {
        handle: "/home/user/Shared Sync/".into(),
    };
    assert_eq!(store.display_name(), "Shared Sync");
    let tree = FlatStore {
        handle: "content://com.android.externalstorage.documents/tree/primary%3ASync".into(),
    };
    assert_eq!(tree.display_name(), "primary%3ASync");
}

#[test]
fn the_default_entry_lookup_filters_the_listing() {
    let store = FlatStore {
        handle: "sync-root".into(),
    };
    assert_eq!(store.entry("sync.json").unwrap().len, 12);
    assert_eq!(
        store.entry("absent.json"),
        Err(FolderError::NotFound("absent.json".into()))
    );
}
