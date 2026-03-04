use cranpose_core::{compositionLocalOf, CompositionLocal};
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

struct DefaultHttpClient;

impl HttpClient for DefaultHttpClient {
    fn get_text<'a>(&'a self, url: &'a str) -> HttpFuture<'a, String> {
        Box::pin(async move {
            #[cfg(not(target_arch = "wasm32"))]
            {
                fetch_text_native(url)
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
                fetch_bytes_native(url)
            }

            #[cfg(target_arch = "wasm32")]
            {
                fetch_bytes_web(url).await
            }
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_text_native(url: &str) -> Result<String, HttpError> {
    native_response(url)?
        .text()
        .map_err(|err| HttpError::BodyReadFailed {
            url: url.to_string(),
            message: err.to_string(),
        })
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_bytes_native(url: &str) -> Result<Vec<u8>, HttpError> {
    native_response(url)?
        .bytes()
        .map(|bytes| bytes.to_vec())
        .map_err(|err| HttpError::BodyReadFailed {
            url: url.to_string(),
            message: err.to_string(),
        })
}

#[cfg(not(target_arch = "wasm32"))]
fn native_response(url: &str) -> Result<reqwest::blocking::Response, HttpError> {
    let response = native_client()?
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

#[cfg(not(target_arch = "wasm32"))]
fn native_client() -> Result<&'static reqwest::blocking::Client, HttpError> {
    use std::sync::OnceLock;

    static CLIENT: OnceLock<Result<reqwest::blocking::Client, HttpError>> = OnceLock::new();
    CLIENT
        .get_or_init(build_native_client)
        .as_ref()
        .map_err(Clone::clone)
}

#[cfg(not(target_arch = "wasm32"))]
fn build_native_client() -> Result<reqwest::blocking::Client, HttpError> {
    use std::time::Duration;

    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("cranpose/0.1")
        .build()
        .map_err(|err| HttpError::ClientInit(err.to_string()))
}

#[cfg(target_arch = "wasm32")]
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

#[cfg(target_arch = "wasm32")]
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

pub fn default_http_client() -> HttpClientRef {
    Arc::new(DefaultHttpClient)
}

pub fn local_http_client() -> CompositionLocal<HttpClientRef> {
    thread_local! {
        static LOCAL_HTTP_CLIENT: std::cell::RefCell<Option<CompositionLocal<HttpClientRef>>> = const { std::cell::RefCell::new(None) };
    }

    LOCAL_HTTP_CLIENT.with(|cell| {
        let mut local = cell.borrow_mut();
        if local.is_none() {
            *local = Some(compositionLocalOf(default_http_client));
        }
        local
            .as_ref()
            .expect("HTTP client composition local must be initialized")
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
    #[cfg(not(target_arch = "wasm32"))]
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
    fn test_client_uses_default_get_bytes_from_text() {
        let client = TestHttpClient;
        let bytes = pollster::block_on(client.get_bytes("https://example.com")).expect("bytes");
        assert_eq!(bytes, b"ok".to_vec());
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

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_http_client_builds() {
        build_native_client().expect("native HTTP client should initialize");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn default_http_client_fetches_text_from_local_server() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local test server");
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
