# Jetpack Compose <-> Cranpose API Parity

No prior version of this document existed. `docs/capability_parity.md` was
checked first and is a different thing entirely: a per-platform capability
matrix (file picker, haptics, clipboard, ...), not a Compose-API-to-Cranpose
mapping. `docs/dependency_alignment.md` is about Cargo dependency-version
skew, also unrelated. A search of `docs/`, the working tree, and
`git log --all -i --grep=compose` (and `--grep=parity`) turned up nothing
else. This is the first API-surface correspondence doc for this repo.

## What's generated and what's curated

Everything under **Coverage** and the raw candidate-match counts is produced
by a command, from a machine-readable listing of each side's public API --
not recalled from memory or scraped from prose docs. The **Curated
correspondence** and **Findings** sections below that are a human read of a
sample of that generated data: verdicts on individual symbols, which is
inherently a judgment call no tool can make. Refreshing the generated
numbers does not overwrite the curated verdicts; re-running the pipeline
below only touches the *Coverage* numbers, and any verdict below should be
re-checked by hand against the regenerated candidate list before being
trusted long-term, since Cranpose is pre-alpha and the surface moves fast.

## Sources

### Cranpose: 23 crates, computed reachability

The public surface is not a hand-picked list: it is "reachable via an
unbroken chain of plain `pub` from a crate root, excluding `#[doc(hidden)]`
and `cfg(test)`" -- the same definition `cargo doc` uses -- computed
directly from the syntax tree of every crate `cargo metadata` reports as
both publishable (`publish` absent, `null`, or a non-empty registry list)
and living under `crates/` (as opposed to `apps/`, which holds demo
binaries that expose a `lib` target only for their own tests, not a surface
meant for outside consumers). That is exactly the set of 23 crates that
publish to crates.io.

The tool computing this, `tools/api-surface/`, is a new standalone Cargo
project checked into this repo (its own `[workspace]`, like
`apps/isolated-demo`, so it never enters the main workspace graph or its
dependency budget). Its `resolve.rs` is the reachability resolver an
earlier session built for stripping stale comments from this repo
(`rustc_lexer` + `syn`), ported here rather than reimplemented, per the
"duplicated tooling is forbidden" rule: same reachability algorithm, same
test suite (extended, not replaced), with the doc-comment-stripping parts
removed and item kind, rendered signature, and `#[composable]` detection
added, since a stripping tool doesn't need those but a surface dump does.
Porting it surfaced two real bugs in the original that are fixed here (with
regression tests) and would otherwise have silently produced wrong or
missing data for parts of the surface:

- **`#[path]`-redirected files used the wrong child directory.** Given
  `#[cfg(test)] #[path = "tests/main_tests.rs"] mod tests;` (this is the
  actual pattern in `apps/desktop-demo/src/app.rs`), a plain `mod x;`
  inside `main_tests.rs` resolves to `tests/x.rs`, not
  `tests/main_tests/x.rs` -- confirmed against real `rustc` with a
  throwaway crate before writing the fix. The old code derived the child
  directory from the redirected file's own name.
- **An inline `mod outer { mod inner; }` block used the wrong directory
  too.** `inner` resolves to `<enclosing file's dir>/outer/inner.rs`, not
  a directory named after the *enclosing* file. Also confirmed against real
  `rustc` first. Both are now `tools/api-surface/src/resolve.rs` unit
  tests: `path_attr_child_module_resolves_next_to_the_redirected_file` and
  `inline_mod_block_nests_child_file_under_its_own_name`.

Both bugs only matter for `apps/`, which this pass excludes from the
surface anyway, but they would have caused a hard parse error (not silent
wrong data, in this instance) on any crate hitting that shape in the
future, so they are fixed rather than left as a footnote.

Refresh:

```
cd tools/api-surface && cargo build --release
target/release/dump-cranpose-api --workspace-root ../.. \
  --out /tmp/cranpose_api_surface.json
```

### Jetpack Compose: androidx binary-compatibility-validator files

The task pointed at `/media/huge/composerepo/` on `samarch-1`; that path
does not exist. The actual checkout is
`/media/huge/projects/android/androidx` (`git remote`: fork
`samoylenkodmitry/androidx` tracking `androidx/androidx`, branch
`androidx-main`). Its `compose/*/api/current.txt` files are exactly the
"entire public API of the module in a stable machine-readable format" the
task asked for: one line per class/interface/enum/annotation declaration
and one per method/field/property/constructor/enum-constant, in the
metalava signature format.

**This checkout is stale relative to today's Jetpack Compose.** Its
`current.txt` files reflect an in-progress `1.5.0-beta01`-era snapshot; the
last commit touching `compose/foundation/foundation/api/current.txt` is
`be18a1188a13a253d2a6784f812815c88454775c`, dated **2023-06-26**. Anything
Compose shipped in 1.6/1.7/1.8+ (pull-to-refresh, shared-element
transitions, `LookaheadScope` becoming stable, newer `Modifier.Node`
additions, etc.) is invisible to this comparison and will read as "absent
from Compose" here when it is really just absent from this old snapshot.
Re-cloning `androidx/androidx` at a current tag before the next refresh
would fix this; it was out of scope for this pass (a multi-GB clone on a
loaded CI host).

`tools/api-surface/dump-compose-api` parses every `api/current.txt` under a
given root (auto-discovered by directory name, not a hardcoded module
list) into structured JSON: package, class, class kind, member kind, name,
raw signature, static/deprecated/experimental flags. Because `samarch-1` is
also a CI host under load, the actual `current.txt` files were pulled to a
local machine with `rsync` (reading files is fine, `cargo build` was not
run there) rather than parsed in place:

```
rsync -av --prune-empty-dirs --include='*/' --include='api/current.txt' \
  --exclude='*' samarch-1:/media/huge/projects/android/androidx/compose/ \
  /tmp/compose-mirror/
cd tools/api-surface && cargo build --release
target/release/dump-compose-api --root /tmp/compose-mirror \
  --out /tmp/compose_api_surface.json
```

### Matching the two

`tools/api-surface/match-api` joins the two JSON files on a
case/separator-insensitive key (`fillMaxSize`, `fill_max_size`, and
`FillMaxSize` all collapse to `fillmaxsize`) -- Kotlin's camelCase against
Rust's `snake_case` for ordinary functions, and CamelCase against CamelCase
for the `#[composable]` functions this repo's own convention (see
`AGENTS.md`: "CamelCase for `#[composable]` functions") already aligns with
Compose's `@Composable fun Foo`. This is a coarse heuristic and produces
both false positives (two unrelated concepts that happen to share a common
short name -- `size`, `offset`, `key`) and false negatives (a real
correspondence under an unrelated name is invisible to it). Every row it
produces is a *candidate for review*, not a verdict; that review is the
**Curated correspondence** section below, done for the parts of the
surface most likely to be asked about, not the whole 8,477-row table.

```
target/release/match-api --compose /tmp/compose_api_surface.json \
  --cranpose /tmp/cranpose_api_surface.json --out /tmp/match_result.json
```

The generated JSON files are not committed (they total ~14 MB and are
fully reproducible from the three commands above plus the external
checkout); this document's numbers are a point-in-time snapshot, stamped
below.

**As of:** Cranpose `5b399b71` (workspace version 0.1.104), androidx
`be18a1188a13a253d2a6784f812815c88454775c` (2023-06-26), generated
2026-08-29.

## Scope

Jetpack Compose ships 33 Gradle modules under `compose/`. Not all of them
are in Cranpose's problem domain, and forcing a match attempt on ones that
aren't would manufacture noise, not signal. Three buckets:

| Bucket | Compose modules | Entries | In this doc |
|---|---|---:|---|
| **Core UI toolkit** (compared in depth) | `runtime`, `runtime-saveable`, `ui`, `ui-geometry`, `ui-graphics`, `ui-unit`, `ui-text`, `ui-util`, `foundation`, `foundation-layout`, `animation`, `animation-core`, `ui-test`, `ui-test-junit4` (14 modules) | 9,213 | Curated correspondence + Findings |
| **Material / Material3** (design-system widget libraries) | `material`, `material-icons-core`, `material-ripple`, `material3`, `material3-adaptive`, `material3-window-size-class` (6 modules) | 2,953 | One decision, not a per-symbol match (see below) |
| **Android/JVM interop, tooling, and IDE support** | `runtime-livedata`, `runtime-rxjava2/3`, `runtime-tracing`, `ui-tooling*`, `ui-viewbinding`, `ui-android-stubs`, `ui-test-manifest`, `ui-text-google-fonts`, `animation-graphics`, `animation-tooling-internal` (13 modules) | 363 | Excluded: bridges to Android View/RxJava/LiveData/Android Studio preview tooling that Cranpose's architecture (no Android View interop, no Android Studio compiler plugin) has no analog for |

**Material/Material3 decision:** Cranpose does not attempt to be a
Material Design implementation. `cranpose-liquid`'s own doc comment calls
it "an iOS-26-style 'Liquid Glass' design system" -- a deliberate,
named-different design language, not a Material port. Several
`cranpose-ui` widgets (`Button`, `Slider`, `IconButton`,
`CircularProgressIndicator`, `LinearProgressIndicator`, `Scaffold`-shaped
code) do carry Material's *names*, which is worth knowing if you're
scanning for "does Cranpose have a Button" -- but nothing here claims or
checks Material Design visual/behavioral parity for them, and this doc
does not attempt a Material-symbol-by-symbol table. This is recorded as a
divergence-by-decision, not a gap.

On the Cranpose side, 10 of the 23 crates are genuinely outside Compose's
domain and are excluded from item-level matching for the same reason
(forcing a match would manufacture noise): `cranpose-audio`,
`cranpose-media`, `cranpose-storekit`, `cranpose-assets`,
`cranpose-platform-android`, `cranpose-platform-desktop-winit`,
`cranpose-platform-web`, `cranpose-render-common`, `cranpose-render-pixels`,
`cranpose-render-wgpu` (audio/media/IAP backends, asset loading, and the
renderer backends and platform-embedding glue that Compose does not expose
as public API -- Android's Canvas/Skia backend is an implementation
detail, not something `androidx.compose.ui` documents or lets you swap).
That leaves 12 crates covered below and one crate that is neither:
`cranpose-services` is split -- its `theme`/`uri_handler`/clipboard-adjacent
slice does correspond to `androidx.compose.ui.platform.*`
(`isSystemInDarkTheme`, `LocalUriHandler`) and is covered below; its
`audio`/`file_picker`/`image_picker`/`notifier`/`share_sheet` slice is a
platform *capability*, already the subject of `docs/capability_parity.md`,
and is not re-covered here. `androidx.activity.compose` (the
`setContent`/`ComponentActivity` half of "how do you start a Compose app,"
which would be the natural comparison for `cranpose-app-shell`) lives
outside the `compose/` subtree this pass scanned and was not fetched; a
future refresh widening `--root` to include `androidx/activity` would
close that.

## Coverage

| | Count |
|---|---:|
| Cranpose crates considered (23 total; all are in scope for the raw count, 12 for item-level Compose matching) | 23 |
| Cranpose reachable public items (top-level: fns, types, traits, consts, statics, macros) | 5,351 |
| ...flattened with struct/enum/trait/impl members and re-exports as their own rows | 8,477 |
| Compose entries considered, all 33 modules (class/interface/enum/annotation declarations + method/field/property/ctor/enum-constant) | 12,529 |
| ...of which in the 14 core-UI-toolkit modules | 9,213 |
| Cranpose rows with >=1 name-squash Compose candidate (of 8,477) | 2,284 (27%) |
| ...restricted to core-module Compose keys only | 2,174 (26%) |
| Compose core-module entries with >=1 Cranpose candidate (of 9,213) | 2,449 (27%) |
| Compose entries (all 33 modules) with >=1 Cranpose candidate (of 12,529) | 2,834 (23%) |
| **Unclassified** (Cranpose rows with zero candidate) | 6,193 (73%) |

That 73% unclassified figure is the honest number, not an oversight: most
of it is Rust-internal plumbing with no Kotlin analog to search for
(`Cell`/`Rc`/`RefCell` field accessors, `impl Default`, private-module
re-export bookkeeping counted at the flattened level, trait bound
machinery) and the long tail of both APIs that a coarse name-squash simply
won't line up. A generated 0% unclassified would have meant the heuristic
was silently dropping rows, not that parity was perfect.

By crate (in-domain crates only, sorted by size):

| Crate | Reachable items | `#[composable]` fns |
|---|---:|---:|
| `cranpose-ui` | 1,671 | 62 |
| `cranpose-core` | 691 | 0 |
| `cranpose-ui-graphics` | 453 | 0 |
| `cranpose-foundation` | 438 | 2 |
| `cranpose-liquid` | 222 | 32 |
| `cranpose` | 194 | 5 |
| `cranpose-testing` | 177 | 0 |
| `cranpose-app-shell` | 137 | 0 |
| `cranpose-animation` | 82 | 0 |
| `cranpose-ui-layout` | 65 | 0 |
| `cranpose-runtime-std` | 25 | 0 |
| `cranpose-macros` | 1 | 0 (defines `#[composable]` itself) |

## Curated correspondence

Verdict key: **Implemented** (equivalent name and semantics) ·
**Renamed/reshaped** (implemented, different name or call shape) ·
**Partial** (implemented, narrower than Compose's version) · **Absent** ·
**Rejected** (Cranpose deliberately does not mirror this).

### Runtime & composition model

| Compose | Cranpose | Verdict | Note |
|---|---|---|---|
| `@Composable fun Foo(...)` | `#[composable] fn Foo(...)` | Implemented | `cranpose-macros::composable`; the naming convention (`CamelCase for #[composable] functions`) is deliberately Compose-shaped per `AGENTS.md`. |
| `remember { ... }` | `cranpose_core::hooks::remember` | Renamed/reshaped | Returns `Owned<T>` (an `Rc<RefCell<T>>` wrapper), not `T` directly the way Kotlin's `remember` does; callers see an extra indirection layer Compose callers don't. |
| `remember(key1) { ... }` (recompute when `key1` changes) | `cranpose_core::hooks::rememberKeyed(key, init)` | Renamed/reshaped | Same contract, distinct name rather than an overload of `remember` (Rust has no overloading). |
| `mutableStateOf(x)` | `cranpose_core::hooks::mutableStateOf` | Implemented | Same shape: `(initial: T) -> MutableState<T>`. |
| `derivedStateOf { ... }` | `cranpose_core::hooks::derivedStateOf` | Implemented | Same shape. |
| `rememberSaveable { ... }` | `cranpose_services::preferences::rememberSaveable` | Renamed/reshaped | Lives in `cranpose-services`, not `cranpose-core`, and takes an explicit `key: &'static str` + `Saver<T>` up front rather than Compose's implicit-position + optional `Saver`. |
| `LaunchedEffect(keys) { ... }` | `cranpose_core::LaunchedEffect!(keys, effect)` | Renamed/reshaped | Same semantics (re-run when `keys` changes, call-site identity via `location_key(file!(), line!(), column!())` inside a `macro_rules!`). Notably `remember` gets the same kind of call-site identity from a plain `#[track_caller]` function, not a macro -- `LaunchedEffect`/`DisposableEffect` did not have to become macros for that reason alone; see Findings. |
| `DisposableEffect(keys) { onDispose { ... } }` | `cranpose_core::DisposableEffect!` | Renamed/reshaped | Same reasoning as `LaunchedEffect`. |
| `SideEffect { ... }` | `cranpose_core::SideEffect` | Implemented | Plain `fn(impl FnOnce())`, no macro needed since there's no key list. |
| `CompositionLocal<T>` / `compositionLocalOf` / `staticCompositionLocalOf` | `cranpose_core::composition_locals::{CompositionLocal, compositionLocalOf, staticCompositionLocalOf}` | Implemented | Same three-way split (local, dynamic default, static default). |
| `CompositionLocalProvider(...)` | `cranpose_core::CompositionLocalProvider` | Implemented | |
| `rememberCoroutineScope()` | `cranpose_core::concurrency::rememberCoroutineScope` | Implemented | |
| `key(vararg keys) { ... }` | -- | Absent | No standalone identity-scoping construct found under this or a squashed name; `location_key` is the internal call-site identity primitive `LaunchedEffect!`/`DisposableEffect!` use, not a public scoping composable. |
| `Modifier.Node` / `LayoutModifierNode` / `DrawModifierNode` / `PointerInputNode` (Compose 1.4+ node-based modifiers) | `cranpose_foundation::modifier::{ModifierNode, LayoutModifierNode, DrawModifierNode, PointerInputNode}` | Implemented | Same names, same architecture (Compose's newer node-based modifier system, not the older factory-based one). |

### Modifier & layout

Cranpose's `Modifier` (in `cranpose-ui`) carries 102 builder methods.
Sampled against Compose's `androidx.compose.ui`/`foundation-layout`:

| Compose | Cranpose | Verdict | Note |
|---|---|---|---|
| `Modifier.padding(...)`, `.fillMaxSize()`, `.fillMaxWidth()`, `.fillMaxHeight()`, `.weight()`, `.size()`, `.offset()`, `.background()`, `.clip()` | `.padding`, `.fill_max_size`, `.fill_max_width`, `.fill_max_height`, `.weight`, `.size`, `.offset`, `.background` | Implemented | snake_case per Rust convention; matched by the case/separator-insensitive key. |
| `Modifier.clickable(onClick: () -> Unit, enabled, role, onClickLabel, interactionSource, indication)` | `Modifier::clickable(handler: impl Fn(Point))` | **Partial, same name different signature** | See Findings -- this is the most consequential divergence found. |
| `Modifier.pointerInput(vararg keys, block: suspend PointerInputScope.() -> Unit)` | `Modifier::pointer_input(key: K, handler: F)` | Renamed/reshaped | Single key (use a tuple for Compose's variadic-key case), same idea otherwise. |
| `Modifier.draggable(state, orientation)` | `Modifier::draggable(axis, state)` | Implemented | |
| `Modifier.toggleable(value, onValueChange)` | `Modifier::toggleable(value, description, role, on_value_change)` | Implemented | Cranpose's takes `role`/`description` directly rather than through a separate `semantics {}` block. |
| `Modifier.semantics { ... }` | `Modifier::semantics(recorder: impl Fn(&mut SemanticsConfiguration))` | Implemented | `SemanticsConfiguration`'s own fields carry doc comments naming the exact Compose property they mirror (e.g. `state_description` is documented as "Compose's `stateDescription`") -- this one was built as a deliberate mirror, not a coincidental name match. |
| `Modifier.focusTarget()` | `Modifier::focus_target()` | Implemented | The low-level primitive. |
| `Modifier.focusable()` (the common convenience wrapper over `focusTarget` + indication) | -- | Absent | Only the low-level `focus_target` was found; no convenience wrapper. |
| `Modifier.testTag("x")` | -- | Absent | See Findings -- `cranpose-testing` finds elements by text/geometry instead. |
| `Modifier.rotate()`, `.scale()`, `.zIndex()` | -- (folded into `graphics_layer`/`graphics_layer_params`) | Partial | The underlying `graphicsLayer`-equivalent primitive exists; Compose's individual convenience wrappers over it do not. |
| `Modifier.testTag`, `FocusRequester`, `Modifier.focusRequester()` | -- | Absent | No imperative focus-request mechanism found (`requester.requestFocus()`); only reactive `on_focus_changed`. |
| `Constraints`, `Density`, `Alignment`, `Arrangement`, `Measurable`, `Placeable`, `MeasureResult`, `MeasurePolicy` | `cranpose-ui-layout::{Constraints, ..., MeasurePolicy}`, `cranpose-ui::Density` | Implemented | Full layout-contract vocabulary present under the same names. |
| `Dp`, `Sp`, `Px` (typed units, used everywhere in Compose's own modifier signatures) | `cranpose-ui-graphics::unit::{Dp, Sp, Px}` (types exist) | **Partial** | The types exist, but the widget/modifier surface itself (`padding`, `size`, `weight`, `offset`, ...) takes raw `f32`, not `Dp` -- Compose's type-safety guarantee against mixing raw pixels and density-independent units is not carried through to the call sites that would benefit from it. |

### Core widgets (`#[composable]` fns)

| Compose | Cranpose | Verdict |
|---|---|---|
| `Box`, `Row`, `Column`, `Spacer`, `BoxWithConstraints` | Same names in `cranpose-ui::widgets::*` | Implemented |
| `Text` / `BasicText` | `Text`, `BasicText`, `BasicTextWithOptions`, `TextWithOptions` | Implemented |
| `Image`, `Canvas`, `Icon` | Same names | Implemented |
| `LazyColumn`, `LazyRow` | `LazyColumnNode`, `LazyRowNode` | Renamed/reshaped -- `Node` suffix, otherwise same role |
| `Popup`, `Dialog` | Same names, plus `PopupAnchored`/`PopupDismissable`/`PopupDismissableWhen`/`DialogWithScrim` variants | Implemented, plus extensions |
| `AnimatedVisibility`, `Crossfade` | Same names | Implemented |
| `Layout`, `SubcomposeLayout` | Same names | Implemented |
| `CircularProgressIndicator`, `LinearProgressIndicator`, `Slider`, `Button`, `IconButton` | Same names (Material-styled names; see Scope decision above) | Implemented, names only -- no Material visual/behavioral parity claimed |
| (no Compose stdlib equivalent -- `for` over Kotlin collections is used directly) | `ForEach` | Rejected-in-reverse: a Cranpose-only convenience the Compose side doesn't need |

`cranpose-liquid` runs a parallel, intentionally-non-Material set under its
own names (`GlassButton`, `LiquidCard`, `LiquidChip`, `LiquidMenu`,
`LiquidTabBar`, `LiquidToggle`, ...) -- not matched against Compose symbol
by symbol, per the Scope decision above.

### Animation

| Compose | Cranpose | Verdict | Note |
|---|---|---|---|
| `animateFloatAsState`, `animateColorAsState` | Same names (`cranpose-animation::animation`) | Implemented | |
| `animateDpAsState`, `animateOffsetAsState`, `animateIntAsState`, ... (the rest of the `animate*AsState` family) | -- | Absent | Only the Float and Color specializations were found; no generic `animateValueAsState`. |
| `tween(durationMillis, easing)`, `spring(dampingRatio, stiffness)` | Same names | Implemented | |
| `Animatable<T>` | Same name | Implemented | |
| `rememberInfiniteTransition()` / `InfiniteTransition.animateFloat` | Same names | Implemented | |
| `updateTransition(targetState)` / `Transition<S>` / `transition.animateFloat { }` (finite, multi-property, state-machine-driven transitions) | -- | Absent | Cranpose covers single-value animation and infinite repetition, not the general finite state-driven multi-property orchestration API. |
| `DecayAnimationSpec`, `FloatDecayAnimationSpec`, `SplineBasedDecay` (fling physics) | `cranpose-animation::decay_spec::{FlingCalculator, FlingInfo, FloatDecayAnimationSpec, SplineBasedDecaySpec}` | Implemented | |

### Text

| Compose | Cranpose | Verdict |
|---|---|---|
| `TextStyle`, `AnnotatedString` | Same names (two `TextStyle`s exist -- `cranpose-ui::text::style` and a distinct `cranpose-ui-graphics::typography` one; worth resolving which is canonical in a future pass) | Implemented, with an internal duplication worth a closer look |
| `BasicTextField`, `TextFieldState` | Same names | Implemented |

### Testing

| Compose (`androidx.compose.ui.test`) | Cranpose (`cranpose-testing`) | Verdict |
|---|---|---|
| `ComposeTestRule`, `SemanticsNodeInteraction`-style finder/assertion chain | `ComposeTestRule`, `ElementFinder`, `FinderQuery`, `assert_*` fns | Implemented, different finder model |
| `Modifier.testTag` + `onNodeWithTag` | -- (finds by `TextMatcher`/geometry instead) | **Different approach**, see Findings |
| (screenshot testing is a separate library, e.g. Roborazzi/Paparazzi -- not part of `ui-test`) | `capture_screenshot`, `changed_pixel_count`, `ScreenshotPixelDifference`, `expected_screenshot_pixel_bytes` | Cranpose-only extension, not a Compose analog |

## Findings

### Same name, different semantics: `Modifier.clickable`

The single most consequential finding. Compose's
`Modifier.clickable(onClick: () -> Unit, enabled = true, role = null, onClickLabel = null, interactionSource, indication)`
hands the callback **no position information at all** -- getting a tap
coordinate in Compose means dropping to
`Modifier.pointerInput { detectTapGestures(onTap = { offset -> ... }) }`,
a different, lower-level API. Cranpose's only `Modifier::clickable`
overload is:

```
fn clickable(self, handler: impl Fn(Point) + 'static) -> Self
```

It conflates the two: every click handler receives a `Point`, whether it
wants one or not, and there is no `enabled`/`role`/`onClickLabel`/
`indication` parameter set (a `clickable_on_press` sibling exists for
press-vs-release timing, itself a Cranpose-only extension Compose doesn't
have). Code migrating from Compose that expects "clickable means simple
tap, no position" will get a wider signature than expected but not
silently wrong behavior; the risk runs the other way -- a Cranpose author
skimming Compose docs and assuming `clickable`'s accessibility parameters
(`role`, `onClickLabel`) exist here will find they don't, and semantics
information gets no default anywhere they'd expect one from Compose
experience.

### `testTag` has no equivalent -- semantics-driven testing is not available

Compose's `Modifier.testTag("x")` + `onNodeWithTag("x")` is the standard,
locale-independent way to address a specific UI element in a test.
`cranpose-testing`'s finders (`TextMatcher`, `FinderQuery`,
`find_bounds_by_text`) work by matched text and geometry instead. This
works until UI text changes or is localized, at which point tests written
against text break where a `testTag`-based Compose test would not. Given
how central `testTag` is to Compose's own testing guidance, this is worth
treating as a real gap rather than a stylistic difference, not something
to close inside this doc, but worth a spawned follow-up.

### `remember` returns a wrapper type, not the value

`cranpose_core::hooks::remember` returns `Owned<T>`, a thin wrapper around
`Rc<RefCell<T>>` with no `Deref<Target = T>` impl -- reading or writing the
value means `owned.with(|v| ...)` or `owned.update(|v| ...)`, a
closure-scoped borrow, not direct field/method access. Compose's
`remember { ... }` returns `T` itself (or, when `T` already is a
`MutableState`, the state, whose reads/writes go straight through). This is
likely unavoidable in Rust without unsafe aliasing, but it means "translate
this `remember` line from Kotlin" is not a drop-in replacement the way most
of the rest of the runtime layer is -- flagging it because same-name-
different-shape is exactly the kind of thing this doc exists to catch, and
`remember` is the single most common Compose function, so its wrapper
widens on every call site that uses it.

### `LaunchedEffect`/`DisposableEffect` are macros; `remember` is not, for no evidenced reason

`cranpose_core::hooks::remember` is a plain `#[track_caller]` function --
`Location::caller()` gives it correct per-call-site identity without any
macro involved. `LaunchedEffect!`/`DisposableEffect!` instead expand to
`location_key(file!(), line!(), column!())` inside a `macro_rules!`. Both
techniques solve the identical problem (a hook needs to know where in the
source it was called from); Rust does not force the macro choice here,
since `remember` next to it in the same crate proves the function-based
route works. Whether the macro exists for an unrelated reason (composing
with `LaunchedEffectAsync`'s future-boxing, or predating `#[track_caller]`
being adopted elsewhere) was not chased down -- recorded as an
inconsistency worth resolving, not a semantic gap: callers should not read
"macro vs function" here as meaningful, since nothing in the evidence
gathered this pass explains it as intentional.

### Positive divergences (deliberate, not gaps)

- `cranpose-testing`'s built-in screenshot-diff tooling
  (`capture_screenshot`, `changed_pixel_count`, `ScreenshotPixelDifference`)
  has no Compose `ui-test` equivalent; Compose relies on a separate library
  (Roborazzi, Paparazzi) for this. Cranpose folds it into the same crate.
- `Modifier::clickable_on_press` (react on press rather than release) has
  no Compose modifier-level equivalent (Compose exposes this only through
  the lower-level `PressGestureScope`/`detectTapGestures` API).
- `cranpose-liquid` is a from-scratch, non-Material design system, not a
  Material port -- see Scope.

### Two `TextStyle` types

`cranpose-ui::text::style::TextStyle` and
`cranpose-ui-graphics::typography::TextStyle` both exist as distinct
structs. Compose has exactly one `androidx.compose.ui.text.TextStyle`.
Whether both Cranpose types are meant to coexist (one text-layout-facing,
one lower-level typography-facing) or this is drift from having two
crates evolve the same concept independently was not resolved here --
recorded as a finding, not chased down, since establishing intent needs a
git-blame/design-doc dig this pass didn't do.

## Regenerating the full generated candidate table

The three commands under **Sources** produce `/tmp/cranpose_api_surface.json`,
`/tmp/compose_api_surface.json`, and `/tmp/match_result.json`. The last one
is the full 8,477-row candidate table (Cranpose item -> every Compose entry
whose squashed name matches), including everything not mentioned in the
Curated correspondence section above -- most of it correctly unclassified,
some of it worth someone's next look.
