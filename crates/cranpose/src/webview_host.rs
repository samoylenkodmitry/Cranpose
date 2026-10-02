use std::sync::mpsc::{self, Receiver, Sender};

use cranpose_ui::{LayoutTree, Rect};

use crate::native_view::NativeViewHost;

pub(crate) type Events = Sender<(u64, String)>;

pub(crate) trait BrowserView {
    fn navigate(&mut self, url: &str) -> Result<(), String>;
    fn place(&mut self, bounds: Rect) -> Result<(), String>;
}

struct Child<V> {
    id: u64,
    url: String,
    bounds: Rect,
    view: Option<V>,
    seen: bool,
}

pub(crate) struct WebViews<V> {
    children: Vec<Child<V>>,
    revision: u64,
    pub(crate) events: Events,
    incoming: Receiver<(u64, String)>,
}

impl<V> Default for WebViews<V> {
    fn default() -> Self {
        let (events, incoming) = mpsc::channel();
        Self {
            children: Vec::new(),
            revision: 0,
            events,
            incoming,
        }
    }
}

impl<V: BrowserView> WebViews<V> {
    #[cfg(target_os = "ios")]
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    #[cfg(any(target_os = "ios", target_os = "linux"))]
    pub(crate) fn views(&self) -> impl Iterator<Item = &V> {
        self.children.iter().filter_map(|child| child.view.as_ref())
    }
    pub(crate) fn dispatch(&self, host: &NativeViewHost) {
        for (id, event) in self.incoming.try_iter() {
            host.dispatch(id, &event);
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    pub(crate) fn sync(
        &mut self,
        host: &NativeViewHost,
        tree: Option<&LayoutTree>,
        mut create: impl FnMut(u64, &str, Rect, &Events) -> Result<V, String>,
    ) {
        if host.is_empty() && self.is_empty() {
            return;
        }
        for child in &mut self.children {
            child.seen = false;
        }
        host.for_each_layout(tree, |id, kind, url, bounds| {
            if kind != "web" {
                return;
            }
            if let Some(child) = self.children.iter_mut().find(|child| child.id == id) {
                child.seen = true;
                if let Some(view) = &mut child.view {
                    if child.url != url {
                        if let Err(error) = view.navigate(url) {
                            let _ = self.events.send((id, format!("error:{error}")));
                        }
                        url.clone_into(&mut child.url);
                    }
                    if child.bounds != bounds {
                        if let Err(error) = view.place(bounds) {
                            let _ = self.events.send((id, format!("error:{error}")));
                        }
                        child.bounds = bounds;
                    }
                }
                return;
            }
            let view = match create(id, url, bounds, &self.events) {
                Ok(view) => Some(view),
                Err(error) => {
                    let _ = self.events.send((id, format!("error:{error}")));
                    None
                }
            };
            self.children.push(Child {
                id,
                url: url.to_owned(),
                bounds,
                view,
                seen: true,
            });
            self.revision = self.revision.wrapping_add(1);
        });
        let before = self.children.len();
        self.children.retain(|child| child.seen);
        if before != self.children.len() {
            self.revision = self.revision.wrapping_add(1);
        }
        self.dispatch(host);
    }
}
