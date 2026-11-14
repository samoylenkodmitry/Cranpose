// What a screen shows of a request: a skeleton or spinner while it runs, the
// error state when it fails, then the data.

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Load<T> {
    Loading,
    Failed,
    Loaded(T),
}

impl<T> Load<T> {
    pub(crate) fn from_result<E>(result: Result<T, E>) -> Self {
        match result {
            Ok(data) => Self::Loaded(data),
            Err(_) => Self::Failed,
        }
    }
}
