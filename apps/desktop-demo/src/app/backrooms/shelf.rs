// The backrooms' own source files, which the walls print: fetched from the
// repository at the commit the app was built from, each once, the first
// time a wall wants it. The tab holds the fetches, so a fetch outlives the
// room that wanted it.

use std::rc::Rc;

use cranpose_core::{key, mutableStateOf, remember, LaunchedEffect, MutableState};
use cranpose_services::local_http_client;
use cranpose_ui::{composable, SharedText};

use super::plan::PRINTS;
use crate::app::source_view::repository_file_url;

const DIRECTORY: &str = "apps/desktop-demo/src/app/backrooms";
pub(super) const FILES: [&str; PRINTS as usize] = [
    "anomaly.rs",
    "figure.rs",
    "finish.rs",
    "finish.wgsl",
    "hud.rs",
    "journey.rs",
    "level.rs",
    "notes.rs",
    "place.rs",
    "plan.rs",
    "room.rs",
    "stalker.rs",
    "tab.rs",
];

// One file on the shelf.
#[derive(Clone, PartialEq)]
pub(super) enum Page {
    // No wall has shown it yet.
    Blank,
    // A wall shows it, and the shelf fetches it.
    Wanted,
    Printed(Rc<[SharedText]>),
    // The fetch failed, for the reason it holds.
    Torn(SharedText),
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Shelf([MutableState<Page>; PRINTS as usize]);

#[composable]
pub(super) fn rememberShelf() -> Shelf {
    remember(|| Shelf(std::array::from_fn(|_| mutableStateOf(Page::Blank)))).with(|shelf| *shelf)
}

impl Shelf {
    // The page of file `file`, one of `FILES`.
    pub(super) fn page(&self, file: usize) -> MutableState<Page> {
        self.0[file.min(self.0.len() - 1)]
    }
}

// Fetches the pages the walls want. A page's fetch runs while the page is
// wanted, and its result ends that.
#[composable]
pub(super) fn Printer(shelf: Shelf) {
    for (file, page) in shelf.0.into_iter().enumerate() {
        if page.get() != Page::Wanted {
            continue;
        }
        key(file, move || Fetch(page, FILES[file]));
    }
}

#[composable]
fn Fetch(page: MutableState<Page>, name: &'static str) {
    let client = local_http_client().current();
    LaunchedEffect((), move |scope| {
        let url = repository_file_url(&format!("{DIRECTORY}/{name}"));
        scope.launch_background(
            move |_token| async move {
                client
                    .get_text(&url)
                    .await
                    .map_err(|error| error.to_string())
            },
            move |fetched| {
                page.set(match fetched {
                    Ok(text) => {
                        Page::Printed(text.lines().map(|line| line.to_owned().into()).collect())
                    }
                    Err(error) => Page::Torn(error.into()),
                });
            },
        );
    });
}
