/// Data a [`WorkerJob`] takes or returns. It crosses to the worker and back
/// as bytes: a Web Worker has a memory of its own, so nothing that points
/// into the page's memory can cross.
pub trait WorkerData: Send + Sized + 'static {
    /// The bytes [`from_bytes`](Self::from_bytes) reads the value back from.
    fn into_bytes(self) -> Vec<u8>;

    /// The value [`into_bytes`](Self::into_bytes) wrote as `bytes`.
    fn from_bytes(bytes: Vec<u8>) -> Self;
}

impl WorkerData for Vec<u8> {
    fn into_bytes(self) -> Vec<u8> {
        self
    }

    fn from_bytes(bytes: Vec<u8>) -> Self {
        bytes
    }
}

impl WorkerData for String {
    fn into_bytes(self) -> Vec<u8> {
        self.into_bytes()
    }

    fn from_bytes(bytes: Vec<u8>) -> Self {
        Self::from_utf8(bytes)
            .unwrap_or_else(|error| Self::from_utf8_lossy(error.as_bytes()).into_owned())
    }
}

impl WorkerData for () {
    fn into_bytes(self) -> Vec<u8> {
        Vec::new()
    }

    fn from_bytes(_: Vec<u8>) -> Self {}
}

// A number crosses as its little-endian bytes; bytes that are no such number
// read as zero.
macro_rules! number_data {
    ($($number:ty),*) => {$(
        impl WorkerData for $number {
            fn into_bytes(self) -> Vec<u8> {
                self.to_le_bytes().to_vec()
            }

            fn from_bytes(bytes: Vec<u8>) -> Self {
                bytes
                    .try_into()
                    .map(Self::from_le_bytes)
                    .unwrap_or_default()
            }
        }
    )*};
}

number_data!(u8, u16, u32, u64, i8, i16, i32, i64, f32, f64);

/// A function [`withBlocking`](crate::withBlocking) runs off the UI thread on
/// every platform: on a pool thread where the platform has threads, in a Web
/// Worker in a browser. The `#[worker_job]` attribute implements it for a
/// function, and the function's `with` makes the call `withBlocking` takes:
///
/// ```ignore
/// #[worker_job]
/// fn word_count(text: String) -> u32 {
///     text.split_whitespace().count() as u32
/// }
///
/// scope.launch(async move {
///     if let Ok(count) = withBlocking(word_count.with(text)).await {
///         shown.set(count);
///     }
/// });
/// ```
pub trait WorkerJob: Copy + 'static {
    /// What the job takes.
    type Input: WorkerData;
    /// What the job returns.
    type Output: WorkerData;
    /// The job's name, one of a kind in the app: a Web Worker finds the job
    /// by it.
    const NAME: &'static str;

    /// Does the work.
    fn run(input: Self::Input) -> Self::Output;
}

/// A [`WorkerJob`] with its input: the work [`withBlocking`](crate::withBlocking)
/// runs. A job's `with` makes it.
pub struct WorkerCall<J: WorkerJob> {
    pub(crate) input: J::Input,
}

/// What the code `#[worker_job]` writes calls.
#[doc(hidden)]
pub mod __worker {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    pub use wasm_bindgen;

    use super::{WorkerCall, WorkerData, WorkerJob};

    /// Job `J` with `input`.
    pub fn call<J: WorkerJob>(input: J::Input) -> WorkerCall<J> {
        WorkerCall { input }
    }

    /// Runs job `J` on the bytes of its input and returns the bytes of its
    /// output: the call a Web Worker makes.
    pub fn run_bytes<J: WorkerJob>(input: Vec<u8>) -> Vec<u8> {
        J::run(J::Input::from_bytes(input)).into_bytes()
    }
}
