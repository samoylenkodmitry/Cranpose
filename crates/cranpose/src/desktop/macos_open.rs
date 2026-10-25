//! What the Mac asks the application to open: documents from Finder, and
//! links in the application's URL schemes. AppKit hands both to the
//! application delegate's `application:openURLs:`, and winit leaves the
//! delegate to the application. Documents arrive as incoming content and
//! links as opened links, at launch too.
#![expect(unsafe_code, reason = "Objective-C messages to AppKit")]

use std::path::Path;

use objc2::{
    MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained, runtime::ProtocolObject,
};
use objc2_app_kit::{NSApplication, NSApplicationDelegate};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSURL};

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CranposeApplicationDelegate"]
    #[ivars = ()]
    pub(crate) struct ApplicationDelegate;

    unsafe impl NSObjectProtocol for ApplicationDelegate {}

    unsafe impl NSApplicationDelegate for ApplicationDelegate {
        #[unsafe(method(application:openURLs:))]
        fn application_open_urls(&self, _application: &NSApplication, urls: &NSArray<NSURL>) {
            for url in urls {
                open(&url);
            }
        }
    }
);

impl ApplicationDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

/// Makes Cranpose's delegate the application's, after the event loop is
/// built and before it runs, so the URLs that launch the application reach
/// it. The application holds its delegate weakly: the caller keeps the
/// returned one while the event loop runs.
pub(crate) fn install() -> Option<Retained<ApplicationDelegate>> {
    let mtm = MainThreadMarker::new()?;
    let delegate = ApplicationDelegate::new(mtm);
    NSApplication::sharedApplication(mtm).setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    Some(delegate)
}

fn open(url: &NSURL) {
    if url.isFileURL() {
        if let Some(path) = url.path() {
            cranpose_services::publish_incoming_content(
                cranpose_services::IncomingContent::from_file(Path::new(&path.to_string())),
            );
        }
    } else if let Some(link) = url.absoluteString() {
        cranpose_services::publish_opened_link(link.to_string());
    }
}

/// Sends `urls` to the application's delegate, as AppKit does when the user
/// opens them.
#[cfg(feature = "robot")]
pub(crate) fn deliver(urls: &[String]) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("the delegate runs on the main thread")?;
    let app = NSApplication::sharedApplication(mtm);
    let delegate = app.delegate().ok_or("the application has no delegate")?;
    let urls: Vec<Retained<NSURL>> = urls
        .iter()
        .map(|url| {
            NSURL::URLWithString(&objc2_foundation::NSString::from_str(url))
                .ok_or_else(|| format!("{url} is no URL"))
        })
        .collect::<Result<_, _>>()?;
    let urls = NSArray::from_retained_slice(&urls);
    let _: () = unsafe { msg_send![&*delegate, application: &*app, openURLs: &*urls] };
    Ok(())
}
