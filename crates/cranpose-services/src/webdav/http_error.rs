use crate::{ContentError, HttpError};

pub(super) fn content_error(error: HttpError) -> ContentError {
    match error {
        HttpError::RequestFailed { .. } | HttpError::BodyReadFailed { .. } => {
            ContentError::Unreachable(error.to_string())
        }
        HttpError::HttpStatus {
            url,
            status: 401 | 403,
        } => ContentError::PermissionDenied(url),
        HttpError::HttpStatus { url, status: 404 } => ContentError::NotFound(url),
        error => ContentError::Io(error.to_string()),
    }
}
