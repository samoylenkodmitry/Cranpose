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
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_text_native(url: &str) -> Result<String, HttpError> {
    use std::sync::OnceLock;
    use std::time::Duration;

    static CLIENT: OnceLock<Result<reqwest::blocking::Client, HttpError>> = OnceLock::new();
    let client = CLIENT
        .get_or_init(|| {
            reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(10))
                .user_agent("cranpose/0.1")
                .build()
                .map_err(|err| HttpError::ClientInit(err.to_string()))
        })
        .as_ref()
        .map_err(|err| err.clone())?;

    let response = client
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

    response.text().map_err(|err| HttpError::BodyReadFailed {
        url: url.to_string(),
        message: err.to_string(),
    })
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
