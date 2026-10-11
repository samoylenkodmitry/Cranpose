#![expect(unsafe_code)]

//! `download_to` on a background `URLSession`: the system carries the
//! transfer while the application is suspended, and finishes it when the
//! system closed the application. The session is made again at every start,
//! so a file that finished while the application was closed is put in place
//! before anything asks for it.

use std::{
    collections::{HashMap, hash_map::Entry},
    fs::{self, File, OpenOptions},
    io::{self, Seek, SeekFrom},
    path::{Path, PathBuf},
    ptr::NonNull,
    sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError},
};

use block2::RcBlock;
use cranpose_services::{
    CancelTransfer, HttpControl, HttpError, HttpFuture, HttpProgress, PlatformDownloader,
    async_io::Signal, set_platform_downloader,
};
use objc2::{
    AllocAnyThread, Message, define_class, msg_send, rc::Retained, runtime::ProtocolObject,
};
use objc2_foundation::{
    NSArray, NSBundle, NSData, NSError, NSHTTPURLResponse, NSMutableURLRequest, NSObject,
    NSObjectProtocol, NSOperationQueue, NSString, NSURL, NSURLSession, NSURLSessionConfiguration,
    NSURLSessionDelegate, NSURLSessionDownloadDelegate, NSURLSessionDownloadTask,
    NSURLSessionDownloadTaskResumeData, NSURLSessionTask, NSURLSessionTaskDelegate,
};

/// `NSURLErrorCancelled`.
const CANCELLED: isize = -999;

pub(crate) fn register() {
    let Some(identifier) = NSBundle::mainBundle().bundleIdentifier() else {
        log::warn!("background downloads need a bundle identifier");
        return;
    };
    let downloads = DOWNLOADS.get_or_init(|| {
        Arc::new(Downloads {
            session: OnceLock::new(),
            state: Mutex::new(State {
                listing: Some(Vec::new()),
                ..State::default()
            }),
        })
    });
    let configuration = NSURLSessionConfiguration::backgroundSessionConfigurationWithIdentifier(
        &NSString::from_str(&format!("{identifier}.cranpose.downloads")),
    );
    configuration.setDiscretionary(false);
    configuration.setSessionSendsLaunchEvents(true);
    let queue = NSOperationQueue::new();
    queue.setMaxConcurrentOperationCount(1);
    let delegate = SessionDelegate::new();
    let session = unsafe {
        NSURLSession::sessionWithConfiguration_delegate_delegateQueue(
            &configuration,
            Some(ProtocolObject::from_ref(&*delegate)),
            Some(&queue),
        )
    };
    let session = downloads.session.get_or_init(|| session);
    let listed = RcBlock::new(|tasks: NonNull<NSArray<NSURLSessionTask>>| {
        if let Some(downloads) = DOWNLOADS.get() {
            downloads.adopt(unsafe { tasks.as_ref() });
        }
    });
    unsafe { session.getAllTasksWithCompletionHandler(&listed) };
    set_platform_downloader(Arc::clone(downloads) as Arc<dyn PlatformDownloader>);
}

static DOWNLOADS: OnceLock<Arc<Downloads>> = OnceLock::new();

struct Downloads {
    session: OnceLock<Retained<NSURLSession>>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// The tasks this process knows, by task identifier.
    transfers: HashMap<usize, Transfer>,
    /// Files that finished with nobody waiting, as a task a closed process
    /// left does; the next call for the file returns its length.
    finished: HashMap<PathBuf, u64>,
    /// Calls that wait for the list of tasks an earlier process left
    /// running; `None` once the list arrived.
    listing: Option<Vec<Signal<()>>>,
}

struct Transfer {
    task: Retained<NSURLSessionDownloadTask>,
    url: String,
    target: PathBuf,
    /// The length of `target` when the task started; the task brings the rest.
    offset: u64,
    waiter: Option<Waiter>,
    /// The file's length once the task's file is in place, or why it is not.
    placed: Option<Result<u64, HttpError>>,
    /// Pause asked for resume data, which ends the transfer instead of the
    /// task's own end.
    cancelling: bool,
}

struct Waiter {
    control: HttpControl,
    done: Signal<Result<u64, HttpError>>,
}

enum Begun {
    Waiting(Signal<Result<u64, HttpError>>, Arc<TaskCancel>),
    /// The file finished while nobody waited; its length.
    Finished(u64),
}

impl Downloads {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes in the tasks an earlier process left running, then lets the
    /// waiting calls go on.
    fn adopt(&self, tasks: &NSArray<NSURLSessionTask>) {
        let mut state = self.state();
        for task in tasks {
            let Ok(task) = task.downcast::<NSURLSessionDownloadTask>() else {
                continue;
            };
            if let Some(transfer) = described(&task) {
                state
                    .transfers
                    .entry(task.taskIdentifier())
                    .or_insert(transfer);
            }
        }
        for listed in state.listing.take().into_iter().flatten() {
            listed.set(());
        }
    }

    async fn listed(&self) {
        let waiting = self.state().listing.as_mut().map(|waiters| {
            let listed = Signal::new();
            waiters.push(listed.clone());
            listed
        });
        if let Some(listed) = waiting {
            listed.wait().await;
        }
    }

    /// Starts or joins the task that brings `target`.
    fn begin(&self, url: &str, target: &Path, control: &HttpControl) -> Result<Begun, HttpError> {
        let session = self
            .session
            .get()
            .ok_or_else(|| HttpError::ClientInit("the download session did not start".into()))?;
        let mut state = self.state();
        if let Some(length) = state.finished.remove(target) {
            return Ok(Begun::Finished(length));
        }
        let done = Signal::new();
        let waiter = Waiter {
            control: control.clone(),
            done: done.clone(),
        };
        let running = state
            .transfers
            .values_mut()
            .find(|transfer| transfer.target == target && !transfer.cancelling);
        let task = match running {
            Some(transfer) => {
                transfer.waiter = Some(waiter);
                transfer.task.clone()
            }
            None => {
                let offset = fs::metadata(target).map_or(0, |metadata| metadata.len());
                let task = match take_resume_data(target, offset) {
                    Some(data) => session.downloadTaskWithResumeData(&NSData::with_bytes(&data)),
                    None => {
                        let request = request(url, offset)?;
                        session.downloadTaskWithRequest(&request)
                    }
                };
                task.setTaskDescription(Some(&NSString::from_str(&describe(offset, target))));
                state.transfers.insert(
                    task.taskIdentifier(),
                    Transfer {
                        task: task.clone(),
                        url: url.to_owned(),
                        target: target.to_owned(),
                        offset,
                        waiter: Some(waiter),
                        placed: None,
                        cancelling: false,
                    },
                );
                task.resume();
                task
            }
        };
        Ok(Begun::Waiting(done, Arc::new(TaskCancel { task })))
    }

    fn wrote(&self, task: &NSURLSessionDownloadTask, written: i64, expected: i64) {
        let report = {
            let state = self.state();
            state
                .transfers
                .get(&task.taskIdentifier())
                .and_then(|transfer| {
                    let waiter = transfer.waiter.as_ref()?;
                    Some((waiter.control.clone(), transfer.offset))
                })
        };
        if let Some((control, offset)) = report {
            let written = u64::try_from(written).unwrap_or(0);
            control.report(HttpProgress {
                transferred: offset + written,
                total: u64::try_from(expected)
                    .ok()
                    .filter(|expected| *expected > 0)
                    .map(|expected| offset + expected),
            });
        }
    }

    /// Puts the task's file in place before the system deletes it, under the
    /// lock, so no call sees the file half written.
    fn finished(&self, task: &NSURLSessionDownloadTask, location: &NSURL) {
        let id = task.taskIdentifier();
        let mut state = self.state();
        let transfer = match state.transfers.entry(id) {
            Entry::Occupied(known) => known.into_mut(),
            Entry::Vacant(unknown) => match described(&task.retain()) {
                Some(transfer) => unknown.insert(transfer),
                None => return,
            },
        };
        transfer.placed = Some(place(task, location, transfer));
    }

    fn completed(&self, task: &NSURLSessionTask, error: Option<&NSError>) {
        let id = task.taskIdentifier();
        let mut state = self.state();
        if state
            .transfers
            .get(&id)
            .is_some_and(|transfer| transfer.cancelling)
        {
            // The pause's resume data ends it; see `cancelled`.
            return;
        }
        let Some(transfer) = state.transfers.remove(&id).or_else(|| {
            let task = task.retain().downcast::<NSURLSessionDownloadTask>().ok()?;
            described(&task)
        }) else {
            return;
        };
        let outcome = match error {
            None => transfer.placed.unwrap_or_else(|| {
                Err(HttpError::InvalidResponse {
                    url: transfer.url.clone(),
                    message: "the download ended without a file".into(),
                })
            }),
            Some(error) => {
                if let Some(data) = resume_data(error) {
                    save_resume_data(&transfer.target, transfer.offset, &data);
                }
                Err(if error.code() == CANCELLED {
                    HttpError::Cancelled
                } else {
                    HttpError::RequestFailed {
                        url: transfer.url.clone(),
                        message: error.localizedDescription().to_string(),
                    }
                })
            }
        };
        match transfer.waiter {
            Some(waiter) => waiter.done.set(outcome),
            None => {
                if let Ok(length) = outcome {
                    state.finished.insert(transfer.target, length);
                }
            }
        }
    }

    /// Keeps a paused task's resume data beside its file, then ends the call,
    /// so a new call right after continues from it.
    fn cancelled(&self, id: usize, data: Option<&NSData>) {
        let Some(transfer) = self.state().transfers.remove(&id) else {
            return;
        };
        if let Some(data) = data {
            save_resume_data(&transfer.target, transfer.offset, &data.to_vec());
        }
        if let Some(waiter) = transfer.waiter {
            waiter.done.set(Err(HttpError::Cancelled));
        }
    }
}

impl PlatformDownloader for Downloads {
    fn download<'a>(
        &'a self,
        url: &'a str,
        target: &'a Path,
        control: HttpControl,
    ) -> HttpFuture<'a, u64> {
        Box::pin(async move {
            self.listed().await;
            if control.is_cancelled() {
                return Err(HttpError::Cancelled);
            }
            let (done, cancel) = match self.begin(url, target, &control)? {
                Begun::Waiting(done, cancel) => (done, cancel),
                Begun::Finished(length) => return Ok(length),
            };
            control.on_cancel(Arc::<TaskCancel>::downgrade(&cancel));
            let outcome = done.wait().await;
            drop(cancel);
            outcome.unwrap_or_else(|| {
                Err(HttpError::RequestFailed {
                    url: url.to_owned(),
                    message: "the download ended without a result".into(),
                })
            })
        })
    }
}

struct TaskCancel {
    task: Retained<NSURLSessionDownloadTask>,
}

impl CancelTransfer for TaskCancel {
    fn cancel(&self) {
        let id = self.task.taskIdentifier();
        let Some(downloads) = DOWNLOADS.get() else {
            return;
        };
        match downloads.state().transfers.get_mut(&id) {
            Some(transfer) => transfer.cancelling = true,
            None => return,
        }
        let produced = RcBlock::new(move |data: *mut NSData| {
            if let Some(downloads) = DOWNLOADS.get() {
                downloads.cancelled(id, unsafe { data.as_ref() });
            }
        });
        unsafe { self.task.cancelByProducingResumeData(&produced) };
    }
}

fn request(url: &str, offset: u64) -> Result<Retained<NSMutableURLRequest>, HttpError> {
    let address = NSURL::URLWithString(&NSString::from_str(url)).ok_or_else(|| {
        HttpError::InvalidResponse {
            url: url.to_owned(),
            message: "not a URL".into(),
        }
    })?;
    let request = NSMutableURLRequest::requestWithURL(&address);
    if offset > 0 {
        request.setValue_forHTTPHeaderField(
            Some(&NSString::from_str(&format!("bytes={offset}-"))),
            &NSString::from_str("Range"),
        );
    }
    Ok(request)
}

/// The task's description: where its file goes and from which byte, so a
/// later process can place a file this one started.
fn describe(offset: u64, target: &Path) -> String {
    format!("{offset}\n{}", target.display())
}

fn described(task: &Retained<NSURLSessionDownloadTask>) -> Option<Transfer> {
    let description = task.taskDescription()?.to_string();
    let (offset, target) = description.split_once('\n')?;
    let url = task
        .originalRequest()
        .and_then(|request| request.URL())
        .and_then(|url| url.absoluteString())
        .map(|url| url.to_string())
        .unwrap_or_default();
    Some(Transfer {
        task: task.clone(),
        url,
        target: PathBuf::from(target),
        offset: offset.parse().ok()?,
        waiter: None,
        placed: None,
        cancelling: false,
    })
}

/// Puts the downloaded part after the bytes already in the target, or the
/// whole file in its place, and returns the target's length.
fn place(
    task: &NSURLSessionDownloadTask,
    location: &NSURL,
    transfer: &Transfer,
) -> Result<u64, HttpError> {
    let invalid = |message: &str| HttpError::InvalidResponse {
        url: transfer.url.clone(),
        message: message.to_owned(),
    };
    let response = task
        .response()
        .and_then(|response| response.downcast::<NSHTTPURLResponse>().ok())
        .ok_or_else(|| invalid("no HTTP response"))?;
    let part = location
        .to_file_path()
        .ok_or_else(|| invalid("the download has no file"))?;
    match response.statusCode() {
        200 => replace(&part, transfer),
        206 => {
            // A resumed task answers for its last request, so the part is
            // checked against the whole file's length, not the range start.
            let whole = response
                .valueForHTTPHeaderField(&NSString::from_str("Content-Range"))
                .and_then(|range| range.to_string().rsplit('/').next()?.parse::<u64>().ok());
            let part_length = fs::metadata(&part).map_or(0, |metadata| metadata.len());
            if whole != Some(transfer.offset + part_length) {
                return Err(invalid(&format!(
                    "a part of {part_length} bytes from byte {} does not end the file of {whole:?}",
                    transfer.offset
                )));
            }
            if transfer.offset == 0 {
                replace(&part, transfer)
            } else {
                append(&part, transfer)
            }
        }
        status => Err(HttpError::HttpStatus {
            url: transfer.url.clone(),
            status: u16::try_from(status).unwrap_or(0),
        }),
    }
}

/// The part becomes the whole target.
fn replace(part: &Path, transfer: &Transfer) -> Result<u64, HttpError> {
    let length = fs::metadata(part).map_or(0, |metadata| metadata.len());
    fs::rename(part, &transfer.target).map_err(|error| file_write(transfer, &error))?;
    Ok(length)
}

/// The part follows the target's first `offset` bytes.
fn append(part: &Path, transfer: &Transfer) -> Result<u64, HttpError> {
    let written = |error: io::Error| file_write(transfer, &error);
    let mut output = OpenOptions::new()
        .write(true)
        .open(&transfer.target)
        .map_err(written)?;
    output.set_len(transfer.offset).map_err(written)?;
    output.seek(SeekFrom::End(0)).map_err(written)?;
    let copied = io::copy(&mut File::open(part).map_err(written)?, &mut output).map_err(written)?;
    Ok(transfer.offset + copied)
}

fn file_write(transfer: &Transfer, error: &io::Error) -> HttpError {
    HttpError::FileWrite {
        path: transfer.target.display().to_string(),
        kind: error.kind(),
        message: error.to_string(),
    }
}

fn resume_data(error: &NSError) -> Option<Vec<u8>> {
    let key = unsafe { NSURLSessionDownloadTaskResumeData };
    let data = error.userInfo().objectForKey(key)?;
    Some(data.downcast::<NSData>().ok()?.to_vec())
}

fn resume_path(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_owned();
    name.push(".resume");
    PathBuf::from(name)
}

/// Resume data with the target's length when it was made: it continues only
/// a file that has not changed since.
fn save_resume_data(target: &Path, offset: u64, data: &[u8]) {
    let mut bytes = Vec::with_capacity(8 + data.len());
    bytes.extend_from_slice(&offset.to_le_bytes());
    bytes.extend_from_slice(data);
    if let Err(error) = fs::write(resume_path(target), bytes) {
        log::warn!(
            "the resume data of {} was not saved: {error}",
            target.display()
        );
    }
}

fn take_resume_data(target: &Path, offset: u64) -> Option<Vec<u8>> {
    let path = resume_path(target);
    let bytes = fs::read(&path).ok()?;
    let _ = fs::remove_file(&path);
    let (saved, data) = bytes.split_first_chunk::<8>()?;
    (u64::from_le_bytes(*saved) == offset).then(|| data.to_vec())
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "CranposeDownloadDelegate"]
    #[ivars = ()]
    struct SessionDelegate;

    unsafe impl NSObjectProtocol for SessionDelegate {}

    unsafe impl NSURLSessionDelegate for SessionDelegate {}

    unsafe impl NSURLSessionTaskDelegate for SessionDelegate {
        #[unsafe(method(URLSession:task:didCompleteWithError:))]
        fn did_complete(
            &self,
            _session: &NSURLSession,
            task: &NSURLSessionTask,
            error: Option<&NSError>,
        ) {
            if let Some(downloads) = DOWNLOADS.get() {
                downloads.completed(task, error);
            }
        }
    }

    unsafe impl NSURLSessionDownloadDelegate for SessionDelegate {
        #[unsafe(method(URLSession:downloadTask:didFinishDownloadingToURL:))]
        fn did_finish(
            &self,
            _session: &NSURLSession,
            task: &NSURLSessionDownloadTask,
            location: &NSURL,
        ) {
            if let Some(downloads) = DOWNLOADS.get() {
                downloads.finished(task, location);
            }
        }

        #[unsafe(method(URLSession:downloadTask:didWriteData:totalBytesWritten:totalBytesExpectedToWrite:))]
        fn did_write(
            &self,
            _session: &NSURLSession,
            task: &NSURLSessionDownloadTask,
            _bytes: i64,
            written: i64,
            expected: i64,
        ) {
            if let Some(downloads) = DOWNLOADS.get() {
                downloads.wrote(task, written, expected);
            }
        }
    }
);

impl SessionDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}
