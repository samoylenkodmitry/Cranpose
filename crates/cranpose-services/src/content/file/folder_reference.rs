use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use crate::content::ContentError;

pub(super) struct FolderReference {
    root: PathBuf,
    retained: OnceLock<Result<String, ContentError>>,
}

impl FolderReference {
    pub(super) fn new(root: PathBuf) -> Self {
        Self {
            root,
            retained: OnceLock::new(),
        }
    }

    pub(super) fn reference(&self, path: &Path) -> Result<String, ContentError> {
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| ContentError::Io("media path is outside the selected folder".into()))?;
        let retained = self
            .retained
            .get_or_init(|| {
                crate::media_source::media_reference(&self.root)
                    .map_err(|error| super::io_error(&self.root, error))
            })
            .as_ref()
            .map_err(Clone::clone)?;
        crate::media_source::media_child_reference(retained, relative)
            .map_err(|error| super::io_error(path, error))
    }
}
