use cranpose_core::{compositionLocalOfWithPolicy, CompositionLocal};
#[cfg(all(target_arch = "wasm32", feature = "web-http"))]
use futures_util::{stream, StreamExt};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

#[derive(thiserror::Error, Debug, Clone)]
pub enum HttpError {
    #[error("Failed to build HTTP client: {0}")]
    ClientInit(String),
    #[error("Request failed for {url}: {message}")]
    RequestFailed { url: String, message: String },
    #[error("Request failed with status {status} for {url}")]
    HttpStatus { url: String, status: u16 },
    #[error("Failed to read response body for {url}: {message}")]
    BodyReadFailed { url: String, message: String },
    #[error("Invalid response for {url}: {message}")]
    InvalidResponse { url: String, message: String },
    #[error("No window object available")]
    NoWindow,
    #[error("{operation} worker thread panicked")]
    WorkerPanicked { operation: &'static str },
    #[error("{operation} requires cranpose-services feature `{feature}`")]
    UnsupportedFeature {
        operation: &'static str,
        feature: &'static str,
    },
}

#[cfg(not(target_arch = "wasm32"))]
pub type HttpFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, HttpError>> + Send + 'a>>;

#[cfg(target_arch = "wasm32")]
pub type HttpFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, HttpError>> + 'a>>;

pub trait HttpClient: Send + Sync {
    fn get_text<'a>(&'a self, url: &'a str) -> HttpFuture<'a, String>;

    fn get_bytes<'a>(&'a self, url: &'a str) -> HttpFuture<'a, Vec<u8>> {
        Box::pin(async move { self.get_text(url).await.map(|text| text.into_bytes()) })
    }
}

pub type HttpClientRef = Arc<dyn HttpClient>;

#[cfg(not(target_arch = "wasm32"))]
pub async fn map_ordered_concurrent<I, T, F, Fut>(
    items: &[I],
    concurrency: usize,
    task: F,
) -> Result<Vec<T>, HttpError>
where
    I: Clone + Send,
    T: Send,
    F: Fn(I) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = T> + Send,
{
    let task = Arc::new(task);
    let mut results = Vec::with_capacity(items.len());

    for chunk in items.chunks(concurrency.max(1)) {
        let chunk_results = std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(chunk.len());
            for item in chunk.iter().cloned() {
                let task = Arc::clone(&task);
                handles.push(scope.spawn(move || pollster::block_on(task(item))));
            }

            let mut chunk_results = Vec::with_capacity(handles.len());
            for handle in handles {
                let value = handle.join().map_err(|_| HttpError::WorkerPanicked {
                    operation: "ordered concurrent task",
                })?;
                chunk_results.push(value);
            }
            Ok::<Vec<T>, HttpError>(chunk_results)
        })?;
        results.extend(chunk_results);
    }

    Ok(results)
}

#[cfg(all(target_arch = "wasm32", feature = "web-http"))]
pub async fn map_ordered_concurrent<I, T, F, Fut>(
    items: &[I],
    concurrency: usize,
    task: F,
) -> Result<Vec<T>, HttpError>
where
    I: Clone,
    F: Fn(I) -> Fut + Clone,
    Fut: Future<Output = T>,
{
    let mut results = stream::iter(items.iter().cloned().enumerate().map(|(index, item)| {
        let task = task.clone();
        async move { (index, task(item).await) }
    }))
    .buffer_unordered(concurrency.max(1))
    .collect::<Vec<_>>()
    .await;

    results.sort_by_key(|(index, _)| *index);
    Ok(results.into_iter().map(|(_, value)| value).collect())
}

#[cfg(all(target_arch = "wasm32", not(feature = "web-http")))]
pub async fn map_ordered_concurrent<I, T, F, Fut>(
    items: &[I],
    _concurrency: usize,
    task: F,
) -> Result<Vec<T>, HttpError>
where
    I: Clone,
    F: Fn(I) -> Fut,
    Fut: Future<Output = T>,
{
    Ok(map_ordered_sequential(items, task).await)
}

#[cfg(all(target_arch = "wasm32", not(feature = "web-http")))]
async fn map_ordered_sequential<I, T, F, Fut>(items: &[I], task: F) -> Vec<T>
where
    I: Clone,
    F: Fn(I) -> Fut,
    Fut: Future<Output = T>,
{
    let mut results = Vec::with_capacity(items.len());
    for item in items.iter().cloned() {
        results.push(task(item).await);
    }
    results
}

struct DefaultHttpClient {
    #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
    native_client: Result<reqwest::blocking::Client, HttpError>,
}

impl DefaultHttpClient {
    fn new() -> Self {
        Self {
            #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
            native_client: build_native_client(),
        }
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
    fn native_response(&self, url: &str) -> Result<reqwest::blocking::Response, HttpError> {
        let response = self
            .native_client
            .as_ref()
            .map_err(Clone::clone)?
            .get(url)
            .send()
            .map_err(|err| HttpError::RequestFailed {
                url: url.to_string(),
                message: err.to_string(),
            })?;

        let status = response.status();
        if !status.is_success() {
            return Err(HttpError::HttpStatus {
                url: url.to_string(),
                status: status.as_u16(),
            });
        }

        Ok(response)
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
    fn fetch_text_native(&self, url: &str) -> Result<String, HttpError> {
        self.native_response(url)?
            .text()
            .map_err(|err| HttpError::BodyReadFailed {
                url: url.to_string(),
                message: err.to_string(),
            })
    }

    #[cfg(all(not(target_arch = "wasm32"), not(feature = "http-native")))]
    fn fetch_text_native(&self, _url: &str) -> Result<String, HttpError> {
        Err(HttpError::UnsupportedFeature {
            operation: "native HTTP text requests",
            feature: "http-native",
        })
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
    fn fetch_bytes_native(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        self.native_response(url)?
            .bytes()
            .map(|bytes| bytes.to_vec())
            .map_err(|err| HttpError::BodyReadFailed {
                url: url.to_string(),
                message: err.to_string(),
            })
    }

    #[cfg(all(not(target_arch = "wasm32"), not(feature = "http-native")))]
    fn fetch_bytes_native(&self, _url: &str) -> Result<Vec<u8>, HttpError> {
        Err(HttpError::UnsupportedFeature {
            operation: "native HTTP byte requests",
            feature: "http-native",
        })
    }
}

impl HttpClient for DefaultHttpClient {
    fn get_text<'a>(&'a self, url: &'a str) -> HttpFuture<'a, String> {
        Box::pin(async move {
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.fetch_text_native(url)
            }

            #[cfg(target_arch = "wasm32")]
            {
                fetch_text_web(url).await
            }
        })
    }

    fn get_bytes<'a>(&'a self, url: &'a str) -> HttpFuture<'a, Vec<u8>> {
        Box::pin(async move {
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.fetch_bytes_native(url)
            }

            #[cfg(target_arch = "wasm32")]
            {
                fetch_bytes_web(url).await
            }
        })
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
fn build_native_client() -> Result<reqwest::blocking::Client, HttpError> {
    use std::time::Duration;

    configure_native_client_builder(
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("cranpose/0.1"),
    )?
    .build()
    .map_err(|err| HttpError::ClientInit(err.to_string()))
}

#[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
fn configure_native_client_builder(
    builder: reqwest::blocking::ClientBuilder,
) -> Result<reqwest::blocking::ClientBuilder, HttpError> {
    #[cfg(target_os = "android")]
    {
        return Ok(builder.tls_certs_only(android_root_certificates()?));
    }

    #[cfg(not(target_os = "android"))]
    {
        Ok(builder)
    }
}

#[cfg(all(target_os = "android", feature = "http-native"))]
fn android_root_certificates() -> Result<Vec<reqwest::Certificate>, HttpError> {
    certificates_from_der_chain(
        webpki_root_certs::TLS_SERVER_ROOT_CERTS
            .iter()
            .map(|certificate| certificate.as_ref()),
    )
}

#[cfg(any(
    all(test, not(target_arch = "wasm32"), feature = "http-native"),
    all(target_os = "android", feature = "http-native")
))]
fn certificates_from_der_chain<'a, I>(
    certificates: I,
) -> Result<Vec<reqwest::Certificate>, HttpError>
where
    I: IntoIterator<Item = &'a [u8]>,
{
    certificates
        .into_iter()
        .enumerate()
        .map(|(index, der)| {
            reqwest::Certificate::from_der(der).map_err(|err| {
                HttpError::ClientInit(format!(
                    "Failed to load TLS root certificate {index}: {err}"
                ))
            })
        })
        .collect()
}

#[cfg(all(target_arch = "wasm32", feature = "web-http"))]
async fn fetch_text_web(url: &str) -> Result<String, HttpError> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{Request, RequestInit, RequestMode, Response};

    let opts = RequestInit::new();
    opts.set_method("GET");
    opts.set_mode(RequestMode::Cors);

    let request =
        Request::new_with_str_and_init(url, &opts).map_err(|err| HttpError::RequestFailed {
            url: url.to_string(),
            message: format!("{:?}", err),
        })?;

    let window = web_sys::window().ok_or(HttpError::NoWindow)?;
    let resp_value = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|err| HttpError::RequestFailed {
            url: url.to_string(),
            message: format!("{:?}", err),
        })?;

    let resp: Response = resp_value
        .dyn_into()
        .map_err(|_| HttpError::InvalidResponse {
            url: url.to_string(),
            message: "Response is not a Response object".to_string(),
        })?;

    if !resp.ok() {
        return Err(HttpError::HttpStatus {
            url: url.to_string(),
            status: resp.status(),
        });
    }

    let text_promise = resp.text().map_err(|err| HttpError::BodyReadFailed {
        url: url.to_string(),
        message: format!("{:?}", err),
    })?;
    let text_value =
        JsFuture::from(text_promise)
            .await
            .map_err(|err| HttpError::BodyReadFailed {
                url: url.to_string(),
                message: format!("{:?}", err),
            })?;

    text_value
        .as_string()
        .ok_or_else(|| HttpError::InvalidResponse {
            url: url.to_string(),
            message: "Response body is not a string".to_string(),
        })
}

#[cfg(all(target_arch = "wasm32", not(feature = "web-http")))]
async fn fetch_text_web(_url: &str) -> Result<String, HttpError> {
    Err(HttpError::UnsupportedFeature {
        operation: "web HTTP text requests",
        feature: "web-http",
    })
}

#[cfg(all(target_arch = "wasm32", feature = "web-http"))]
async fn fetch_bytes_web(url: &str) -> Result<Vec<u8>, HttpError> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{Request, RequestInit, RequestMode, Response};

    let opts = RequestInit::new();
    opts.set_method("GET");
    opts.set_mode(RequestMode::Cors);

    let request =
        Request::new_with_str_and_init(url, &opts).map_err(|err| HttpError::RequestFailed {
            url: url.to_string(),
            message: format!("{:?}", err),
        })?;

    let window = web_sys::window().ok_or(HttpError::NoWindow)?;
    let resp_value = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|err| HttpError::RequestFailed {
            url: url.to_string(),
            message: format!("{:?}", err),
        })?;

    let resp: Response = resp_value
        .dyn_into()
        .map_err(|_| HttpError::InvalidResponse {
            url: url.to_string(),
            message: "Response is not a Response object".to_string(),
        })?;

    if !resp.ok() {
        return Err(HttpError::HttpStatus {
            url: url.to_string(),
            status: resp.status(),
        });
    }

    let bytes_promise = resp
        .array_buffer()
        .map_err(|err| HttpError::BodyReadFailed {
            url: url.to_string(),
            message: format!("{:?}", err),
        })?;
    let bytes_value =
        JsFuture::from(bytes_promise)
            .await
            .map_err(|err| HttpError::BodyReadFailed {
                url: url.to_string(),
                message: format!("{:?}", err),
            })?;

    let array = js_sys::Uint8Array::new(&bytes_value);
    Ok(array.to_vec())
}

#[cfg(all(target_arch = "wasm32", not(feature = "web-http")))]
async fn fetch_bytes_web(_url: &str) -> Result<Vec<u8>, HttpError> {
    Err(HttpError::UnsupportedFeature {
        operation: "web HTTP byte requests",
        feature: "web-http",
    })
}

pub fn default_http_client() -> HttpClientRef {
    Arc::new(DefaultHttpClient::new())
}

pub fn local_http_client() -> CompositionLocal<HttpClientRef> {
    thread_local! {
        static LOCAL_HTTP_CLIENT: std::cell::RefCell<Option<CompositionLocal<HttpClientRef>>> = const { std::cell::RefCell::new(None) };
    }

    LOCAL_HTTP_CLIENT.with(|cell| {
        let mut local = cell.borrow_mut();
        local
            .get_or_insert_with(|| compositionLocalOfWithPolicy(default_http_client, Arc::ptr_eq))
            .clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_test_composition;
    use cranpose_core::CompositionLocalProvider;
    use std::cell::RefCell;
    use std::rc::Rc;
    #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
    use std::thread;

    struct TestHttpClient;

    impl HttpClient for TestHttpClient {
        fn get_text<'a>(&'a self, _url: &'a str) -> HttpFuture<'a, String> {
            Box::pin(async { Ok("ok".to_string()) })
        }
    }

    #[test]
    fn default_http_client_is_available() {
        let client = default_http_client();
        let cloned = client.clone();
        assert_eq!(Arc::strong_count(&client), 2);
        drop(cloned);
        assert_eq!(Arc::strong_count(&client), 1);
    }

    #[test]
    fn default_http_client_has_no_process_global_native_client_cache() {
        let source = include_str!("http.rs");
        let once_lock = ["Once", "Lock"].concat();
        let static_client = ["static ", "CLIENT"].concat();
        let native_client_fn = ["fn ", "native_client()"].concat();

        assert!(
            !source.contains(&static_client)
                && !source.contains(&native_client_fn)
                && !source.contains(&once_lock),
            "native HTTP client state must be owned by DefaultHttpClient instead of a process-global cache"
        );
    }

    #[test]
    fn test_client_uses_default_get_bytes_from_text() {
        let client = TestHttpClient;
        let bytes = pollster::block_on(client.get_bytes("https://example.com")).expect("bytes");
        assert_eq!(bytes, b"ok".to_vec());
    }

    #[test]
    fn map_ordered_concurrent_preserves_input_order() {
        let inputs = [3usize, 1, 4, 1, 5];
        let outputs = pollster::block_on(map_ordered_concurrent(&inputs, 2, |value| async move {
            value * 10
        }))
        .expect("ordered concurrent mapping");

        assert_eq!(outputs, vec![30, 10, 40, 10, 50]);
    }

    #[test]
    fn native_ordered_concurrency_is_not_tied_to_native_http_client_feature() {
        let source = include_str!("http.rs");
        assert!(
            source.contains(
                "#[cfg(not(target_arch = \"wasm32\"))]\npub async fn map_ordered_concurrent"
            ),
            "native ordered concurrency must stay available without the HTTP client feature"
        );
        assert!(
            !source.contains(
                "all(not(target_arch = \"wasm32\"), feature = \"http-native\")]\npub async fn map_ordered_concurrent"
            ) && !source.contains(
                "all(not(target_arch = \"wasm32\"), not(feature = \"http-native\"))]\npub async fn map_ordered_concurrent"
            ),
            "native ordered concurrency must not split into HTTP-enabled and HTTP-disabled behavior"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn map_ordered_concurrent_reports_worker_panic() {
        let inputs = [1usize];
        let should_panic = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let should_panic_for_task = Arc::clone(&should_panic);

        let error = pollster::block_on(map_ordered_concurrent(&inputs, 1, move |_| {
            let should_panic = Arc::clone(&should_panic_for_task);
            async move {
                if should_panic.load(std::sync::atomic::Ordering::SeqCst) {
                    panic!("test worker panic");
                }
                1usize
            }
        }))
        .expect_err("worker panic should be reported");

        assert!(matches!(error, HttpError::WorkerPanicked { .. }));
    }

    #[test]
    fn local_http_client_can_be_overridden() {
        let local = local_http_client();
        let default_client = default_http_client();
        let custom_client: HttpClientRef = Arc::new(TestHttpClient);
        let captured = Rc::new(RefCell::new(None));

        {
            let captured_for_closure = Rc::clone(&captured);
            let custom_client = custom_client.clone();
            let local_for_provider = local.clone();
            let local_for_read = local.clone();
            run_test_composition(move || {
                let captured = Rc::clone(&captured_for_closure);
                let local_for_read = local_for_read.clone();
                CompositionLocalProvider(
                    vec![local_for_provider.provides(custom_client.clone())],
                    move || {
                        let current = local_for_read.current();
                        *captured.borrow_mut() = Some(current);
                    },
                );
            });
        }

        let current = captured.borrow().as_ref().expect("client captured").clone();
        assert!(Arc::ptr_eq(&current, &custom_client));
        assert!(!Arc::ptr_eq(&current, &default_client));
    }

    #[cfg(all(not(target_arch = "wasm32"), not(feature = "http-native")))]
    #[test]
    fn default_http_client_reports_disabled_native_http_feature() {
        let error = pollster::block_on(default_http_client().get_text("https://example.com"))
            .expect_err("native HTTP should be feature-gated");

        assert!(matches!(
            error,
            HttpError::UnsupportedFeature {
                feature: "http-native",
                ..
            }
        ));
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
    #[test]
    fn native_http_client_builds() {
        build_native_client().expect("native HTTP client should initialize");
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
    #[test]
    fn certificates_from_der_chain_accepts_valid_roots() {
        let certificates = certificates_from_der_chain(
            webpki_root_certs::TLS_SERVER_ROOT_CERTS
                .iter()
                .take(3)
                .map(|certificate| certificate.as_ref()),
        )
        .expect("root certificates should parse");

        assert_eq!(certificates.len(), 3);
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
    #[test]
    fn default_http_client_fetches_text_from_local_server() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = match TcpListener::bind("127.0.0.1:0") {
            Ok(listener) => listener,
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                eprintln!("skipping local HTTP server bind in restricted test environment: {err}");
                return;
            }
            Err(err) => panic!("bind local test server: {err}"),
        };
        let address = listener
            .local_addr()
            .expect("read local test server address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept local test request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).expect("read local test request");
            let body = "cranpose-http-test";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("write local test response");
        });

        let url = format!("http://{address}");
        let text = pollster::block_on(default_http_client().get_text(&url))
            .expect("fetch text from local test server");
        server.join().expect("join local test server");

        assert_eq!(text, "cranpose-http-test");
    }
}
