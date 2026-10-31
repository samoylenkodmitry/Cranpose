use std::{path::PathBuf, rc::Rc};

use cranpose_services::{
    ContentError, ContentFuture, ContentHandle, ContentStream, ContentStreamRef, file_folder,
    folder_files,
};

use super::{IosFile, ScopeAccess, SecurityScope};

pub(super) fn open(scope: Rc<SecurityScope>, path: PathBuf) -> ContentStreamRef {
    let access = scope.access();
    Rc::new(Stream {
        scope,
        _access: access,
        inner: folder_files(file_folder(path)),
    })
}

struct Stream {
    scope: Rc<SecurityScope>,
    _access: ScopeAccess,
    inner: ContentStreamRef,
}

impl ContentStream for Stream {
    fn next(&self) -> ContentFuture<'_, Result<Option<ContentHandle>, ContentError>> {
        Box::pin(async move {
            Ok(self.inner.next().await?.map(|file| {
                let metadata = file.metadata();
                Rc::new(IosFile {
                    scope: Rc::clone(&self.scope),
                    path: PathBuf::from(&metadata.identifier),
                    metadata: Some(metadata),
                }) as ContentHandle
            }))
        })
    }

    fn produced(&self) -> Option<usize> {
        self.inner.produced()
    }
}
