# Everbloom in Cranpose

The shop app of the [shop app benchmark](https://github.com/whiskerrs/benchmarks)
(the frameworks comparison its authors run, and that
[DartNative](https://github.com/DartNative/dartnative/tree/main/benchmarks/frameworks_comparison)
ran too), written with Cranpose. The benchmark builds the same small
clothing shop with many frameworks and measures each on an iPhone with the
same tools: launch to the first frame, launch to Home's content, app size, and
a walk through the app for memory, scrolling, CPU and energy.

The app follows the benchmark's prompt (`prompt/PROMPT.md` there, at commit
`bf0fa20`): the same screens, layout, data and requests, and the element
names its XCUITest runner looks for, set with `Modifier::test_tag`, which the
iOS host publishes as the accessibility identifier.

## Attribution

The benchmark, its prompt, its design, its test runner and its measuring
tools are the work of its authors, under the MIT license in [`LICENSE`](LICENSE)
(Copyright (c) 2026 itome). The GraphQL documents in `src/api.rs` are copied
from its `prompt/storefront.graphql`, the colors and sizes in `src/theme.rs`
and the icon paths in `src/icons.rs` from its `design/`. The app's own code is
Cranpose's.

## The screens

| Module | Screen |
| --- | --- |
| `src/home.rs` | Home: collection chips, featured collection, collections carousel, best sellers |
| `src/collection.rs` | Collection: description, sort chips, a lazy two-column grid that loads page after page |
| `src/product.rs` | Product: image pager, options, description, recommendations, Add to cart |
| `src/search.rs` | Search: a field sent 300 ms after the typing stops, a lazy list of results |
| `src/cart.rs` | Cart: lines with quantity steppers, subtotal, Check out |
| `src/app.rs` | The three tabs: Home and Search keep a stack each (`NavHost`), Cart has one screen |

Each screen keeps its data in a view model (`viewModel`), so a screen covered
by another finds its data again.

## `SHOPBENCH home_ready`

The benchmark takes Home content as the moment the app writes the log line
`SHOPBENCH home_ready`, and asks for it when Home first shows its loaded
content. `src/home_ready.rs` writes it once per process, from an effect that
Home's loaded content starts, at the next frame callback: the frame that
composed the content draws it and presents it, and frame callbacks run at the
start of the frame after that. It is the only line the app writes.

On the iPhone the line lands after iOS's own record of that frame in the same
trace: the Core Animation commit that holds the content and the present of its
drawable. In 11 launches of a build that also logs the moment Home's content
is composed (`EXTRA_FEATURES=diag ios/build-app.sh`), the line came 9 to 17 ms after the end of
that commit, never before it.

## Build and measure

[`BUILD.md`](BUILD.md) has the commands. [`NOTES.md`](NOTES.md) has the
decisions the prompt left open.
