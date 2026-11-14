# Notes

Decisions where the benchmark's prompt is silent, and what differs from it.

- **Element names.** `Modifier::test_tag` names every element the runner
  looks for. XCUITest finds them once Cranpose's iOS host publishes a node's
  test tag as its accessibility identifier, as Compose Multiplatform's iOS
  host does. The test-tag change brings that for every platform. Containers
  that are not accessibility elements (the scroll views) reach XCUITest
  through the host's automation elements.
- **Tabs and stacks.** Home and Search each have a `NavController` with its
  own `NavHost`; Cart has one screen. Only the selected tab is composed. The
  back stacks, the view models of their screens and their saved scroll
  positions stay in the controllers while another tab shows, so switching
  back finds each stack as it was. Screens cross-fade as Compose's `NavHost`
  does by default.
- **3:4 images.** Cranpose has no `Modifier.aspectRatio`; `src/aspect_ratio.rs`
  is a small `Layout` that does the same for the product images.
- **Back on Product.** The round back button stays over the top-left corner
  of the screen while the page scrolls under it.
- **Page dots** show the carousel's page; they do not take taps.
- **Check out** opens `checkoutUrl` in the system browser through
  `local_uri_handler`: Cranpose has no in-app browser.
- **Images** load through Cranpose's `AsyncImage`, with its memory and disk
  caches.
- **Cart errors** open a dialog with the first message and OK.
- **No added dependencies** beyond `serde`/`serde_json` (JSON),
  `anyhow` (errors) and `oslog` (the log line on iOS); the HTTP client is
  Cranpose's.
- **Android** is not built here: the benchmark measures the iPhone build.
  The code holds nothing iOS-specific apart from the log line.
