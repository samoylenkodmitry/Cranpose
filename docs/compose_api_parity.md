# Jetpack Compose <-> Cranpose API Parity

> **Generated against Jetpack Compose 1.12.0** (the frozen
> `api/1.12.0-beta01.txt` snapshot across the compose-ui/foundation/runtime/
> animation release train -- Compose's API freezes at a module's first beta,
> so this is 1.12.0's actual shipped surface, not an in-progress one), read
> at upstream `androidx/androidx` commit `5ba2cdd61be7b6945db999b238d14f3c626136fb`
> (androidx-main, fetched 2026-08-29). The first version of this doc was
> generated against a stale 2023-06-26 snapshot (~Compose 1.5.0-beta) by
> mistake; **How this doc was refreshed** under Sources has the before/after
> coverage numbers and confirms the four headline findings below still hold
> against current Compose.

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

**The working tree there is not this doc's source and was never touched.**
Someone else may depend on that checkout being exactly where it is (it was
already mid-refactor with hundreds of uncommitted deletions when this pass
found it), so nothing here does a `git checkout`, `reset`, `pull`, or
`merge` against it. Instead:

1. `git fetch --depth=1 --filter=blob:none upstream androidx-main:refs/heads/compose-parity-snapshot`,
   invoked through `scripts/ci/with_host_lock.sh --shared` from the CI
   runner's own Cranpose checkout
   (`/home/s/actions-runner-cranpose/_work/Cranpose/Cranpose`). **That
   wrapper turned out not to actually hold the lock**: it closed the lock's
   file descriptors as part of the same `exec` that launched the wrapped
   command, before the command's image loaded, so the lock was released in
   milliseconds every time it ran (fix pending in #552) -- this fetch ran
   unlocked, and that claim should not be repeated until #552 lands. What
   actually made it safe here was the operation's own shape, not the lock:
   `--depth=1` + `--filter=blob:none` fetch only the *tip* commit's tree
   and commit objects, not three years of history or blob content --
   `.git` grew by ~33 MB (831 MB to 864 MB), not the multi-gigabyte
   full-history clone a plain `git fetch` of a monorepo this size would
   cost. Blobs (the actual file content) are fetched lazily, one small
   text file at a time, only when something asks to read one. This
   created one new local ref (`compose-parity-snapshot`) and updated the
   `upstream/androidx-main` remote-tracking ref; the checked-out branch,
   its HEAD, and its dirty working tree were verified unchanged
   afterward (same commit, same 311 modified-file count) -- that
   verification, not the lock, is what backs the "nothing else's state
   changed" claim below.
2. `androidx/androidx` has **no release git tags** (`git ls-remote --tags
   upstream` returns nothing) -- androidx versions its Maven artifacts
   without tagging the monorepo, so "pick a released ref" means picking a
   frozen *file*, not a tag. The compose-ui/foundation/runtime/animation
   family freezes its public API at each version's first beta: every
   module's `api/` directory carries one `api/<version>-beta01.txt` per
   cycle, and `api/current.txt` only diverges from the latest such file
   once work starts on the *next* version. At the fetched commit, every one
   of those modules' newest frozen file was **`1.12.0-beta01.txt`**, and
   `current.txt` had already drifted 461 lines from it (`git diff --stat`)
   -- meaning current.txt itself was live 1.13.0-alpha work-in-progress,
   not something to trust as "released." Reading `1.12.0-beta01.txt`
   instead is both more honest and exactly reproducible: given the pinned
   commit, that path's content cannot change out from under a later reader
   the way `current.txt` would.
3. Paths were listed with `git ls-tree -r compose-parity-snapshot`, and
   each module's chosen file was read with
   `git show compose-parity-snapshot:compose/<module>/api/<file>` over
   `ssh samarch-1`, piping stdout straight into a local mirror tree --
   `git show` reads an object, it does not touch the working tree, and
   nothing was written to disk on `samarch-1` at any point in this step.

Two of the 33 modules from the first pass no longer exist at that path:
`material/material-icons-core` and `material3/material3-adaptive` were
removed or renamed (the latter now lives at `compose/material3/adaptive`)
sometime in the three years since the 2023 snapshot. Both are in the
Material bucket, already out of this doc's deep-comparison scope, so they
are simply dropped from the current pass's module count (33 -> 31) rather
than chased to their new location.

**Pinned commit:** `5ba2cdd61be7b6945db999b238d14f3c626136fb` on
`upstream/androidx-main` (dated 2026-08-28T23:36:29-07:00, i.e. the branch
tip as fetched on 2026-08-29). **Compose version represented:** 1.12.0 for
the 27 compose-ui-family modules (`api/1.12.0-beta01.txt`); Material3
1.4.0-beta03 for the 2 material3 modules that still had a frozen snapshot
present (`material3`, `material3-window-size-class`) -- Material3 versions
independently of the compose-ui-family and was already out of this doc's
deep-comparison scope.

#### How this doc was refreshed, and what changed

The first version of this doc (merged as #546) was generated against a
2023-06-26 snapshot by mistake -- the checkout above was already three
years stale and nobody had noticed. Refreshing it validates the concern
that motivated calling that staleness out in the first place: the Compose
side grew enough to visibly shift every coverage number.

| | 2023-06-26 snapshot (1.5.0-beta) | 2026-08-29 refresh (1.12.0) |
|---|---:|---:|
| Compose modules considered | 33 | 31 (2 removed/renamed, see above) |
| Compose entries, all modules | 12,529 | 19,838 (+58%) |
| Compose entries, 14 core modules | 9,213 | 15,373 (+67%) |
| Cranpose flattened items | 8,477 | 8,464 (ordinary drift from commits landed on `main` since) |
| Cranpose rows matched (vs all Compose keys) | 2,284 (27%) | 2,414 (28.5%) |
| Cranpose rows matched (vs core-module keys only) | 2,174 (26%) | 2,303 (27.2%) |
| Compose core-module entries matched | 2,449 (27%) | 3,300 (21.5%) |
| Compose all-module entries matched | 2,834 (23%) | 3,838 (19.3%) |
| Cranpose unclassified | 6,193 (73%) | 6,050 (71.5%) |

Reading this: Cranpose's own match rate barely moved (27% -> 28.5%) because
Cranpose's surface didn't change much in three years. Compose's match rate
*dropped* (27% -> 21.5% on core modules) despite Cranpose matching more
entries in absolute terms (2,449 -> 3,300), because Compose's surface grew
faster than Cranpose's did -- exactly the shape of gap a stale comparison
hides and a fresh one exposes. `SharedTransitionLayout`/`SharedTransitionScope`
(the shared-element transition API the 2023 snapshot could not see at all)
is confirmed present in this refresh's data and confirmed still absent from
Cranpose by name -- it is now an honest "Absent" verdict instead of an
invisible one.

The four same-name-different-semantics and mechanism findings below were
re-verified against this refreshed data before being left unchanged; see
the note at the top of **Findings**.

`tools/api-surface/dump-compose-api` parses every `api/current.txt` (or, as
used for this refresh, any metalava-format file passed under that name in
the local mirror) under a given root (auto-discovered by directory name,
not a hardcoded module list) into structured JSON: package, class, class
kind, member kind, name, raw signature, static/deprecated/experimental
flags. This refresh also fixed two real parsing bugs the 2023 data never
exercised -- `nonexhaustive`/`exhaustive` sealed-class modifiers and
`package @Annotation pkg.name {}`-prefixed package declarations, both new
metalava output since 2023 -- and added a `typealias` entry kind, all with
regression tests (`tools/api-surface/src/bin/dump_compose_api.rs`).

To refresh again later, from a host with the androidx checkout: confirm
`scripts/ci/with_host_lock.sh` actually holds the lock for the duration of
the command before relying on it again (#552 fixes a real bug where it
released immediately on `exec`; verify the fix landed, don't just check
the wrapper is present), and re-derive step 2 above for whatever the
current highest frozen version is by then. Regardless of the lock, verify
`git status`/`git log -1` on the checkout are unchanged before and after,
the way this refresh did -- that check is what actually establishes
nothing else's state moved:

```
ssh samarch-1 "cd /home/s/actions-runner-cranpose/_work/Cranpose/Cranpose && ./scripts/ci/with_host_lock.sh --shared git -C /media/huge/projects/android/androidx fetch --depth=1 --filter=blob:none upstream androidx-main:refs/heads/compose-parity-snapshot"
```

Then, for each module, find its highest frozen `<version>-betaNN.txt` and
read it with `git show compose-parity-snapshot:compose/<module>/api/<file>`
over ssh, writing each into a local `<module>/api/current.txt` mirror
(exactly what step 3 above did), then:

```
cd tools/api-surface && cargo build --release
target/release/dump-compose-api --root /tmp/compose-mirror --out /tmp/compose_api_surface.json
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
surface most likely to be asked about, not the whole 8,464-row table.

```
target/release/match-api --compose /tmp/compose_api_surface.json \
  --cranpose /tmp/cranpose_api_surface.json --out /tmp/match_result.json
```

The generated JSON files are not committed (they total ~19 MB and are
fully reproducible from the commands under Sources above plus the external
checkout); this document's numbers are a point-in-time snapshot, stamped
below.

**As of:** Cranpose `a0d22b1a` (workspace version 0.1.104), androidx
`5ba2cdd61be7b6945db999b238d14f3c626136fb` (androidx-main tip, Compose
1.12.0 frozen surface), generated 2026-08-29. Supersedes the first version
of this doc, generated against Cranpose `5b399b71` / androidx
`be18a1188a1` (2023-06-26).

## Scope

Jetpack Compose ships 31 Gradle modules under `compose/` as read for this
pass (33 in the first pass; see **How this doc was refreshed** above for
the 2 that were removed or renamed). Not all of them are in Cranpose's
problem domain, and forcing a match attempt on ones that aren't would
manufacture noise, not signal. Three buckets:

| Bucket | Compose modules | Entries | In this doc |
|---|---|---:|---|
| **Core UI toolkit** (compared in depth) | `runtime`, `runtime-saveable`, `ui`, `ui-geometry`, `ui-graphics`, `ui-unit`, `ui-text`, `ui-util`, `foundation`, `foundation-layout`, `animation`, `animation-core`, `ui-test`, `ui-test-junit4` (14 modules) | 15,373 | Curated correspondence + Findings |
| **Material / Material3** (design-system widget libraries) | `material`, `material-ripple`, `material3`, `material3-window-size-class` (4 modules; `material-icons-core` and `material3-adaptive` no longer at this path) | 3,953 | One decision, not a per-symbol match (see below) |
| **Android/JVM interop, tooling, and IDE support** | `runtime-livedata`, `runtime-rxjava2/3`, `runtime-tracing`, `ui-tooling*`, `ui-viewbinding`, `ui-android-stubs`, `ui-test-manifest`, `ui-text-google-fonts`, `animation-graphics`, `animation-tooling-internal` (13 modules) | 512 | Excluded: bridges to Android View/RxJava/LiveData/Android Studio preview tooling that Cranpose's architecture (no Android View interop, no Android Studio compiler plugin) has no analog for |

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
| Cranpose reachable public items (top-level: fns, types, traits, consts, statics, macros) | 5,341 |
| ...flattened with struct/enum/trait/impl members and re-exports as their own rows | 8,464 |
| Compose entries considered, all 31 modules (class/interface/enum/annotation declarations + method/field/property/ctor/enum-constant) | 19,838 |
| ...of which in the 14 core-UI-toolkit modules | 15,373 |
| Cranpose rows with >=1 name-squash Compose candidate (of 8,464) | 2,414 (28.5%) |
| ...restricted to core-module Compose keys only | 2,303 (27.2%) |
| Compose core-module entries with >=1 Cranpose candidate (of 15,373) | 3,300 (21.5%) |
| Compose entries (all 31 modules) with >=1 Cranpose candidate (of 19,838) | 3,838 (19.3%) |
| **Unclassified** (Cranpose rows with zero candidate) | 6,050 (71.5%) |

(The first version of this doc, against the 2023 snapshot, reported 33
modules / 12,529 Compose entries / 2,834 matched (23%) / 73% unclassified
-- see the before/after table under **How this doc was refreshed** above.)

That 71.5% unclassified figure is the honest number, not an oversight:
most of it is Rust-internal plumbing with no Kotlin analog to search for
(`Cell`/`Rc`/`RefCell` field accessors, `impl Default`, private-module
re-export bookkeeping counted at the flattened level, trait bound
machinery) and the long tail of both APIs that a coarse name-squash simply
won't line up. A generated 0% unclassified would have meant the heuristic
was silently dropping rows, not that parity was perfect.

By crate (in-domain crates only, sorted by size):

| Crate | Reachable items | `#[composable]` fns |
|---|---:|---:|
| `cranpose-ui` | 1,675 | 62 |
| `cranpose-core` | 691 | 0 |
| `cranpose-ui-graphics` | 453 | 0 |
| `cranpose-foundation` | 438 | 2 |
| `cranpose-liquid` | 222 | 32 |
| `cranpose` | 194 | 5 |
| `cranpose-testing` | 177 | 0 |
| `cranpose-app-shell` | 137 | 0 |
| `cranpose-animation` | 68 | 0 |
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
| `LaunchedEffect(keys) { ... }` | `cranpose_core::LaunchedEffect(keys, effect)` | Implemented | A plain `#[track_caller]` function, matching `remember`. Was a `macro_rules!`; see Findings for why the macro was not merely unnecessary but wrong. |
| `DisposableEffect(keys) { onDispose { ... } }` | `cranpose_core::DisposableEffect(keys, effect)` | Implemented | Same conversion as `LaunchedEffect`. |
| `SideEffect { ... }` | `cranpose_core::SideEffect` | Implemented | Plain `fn(impl FnOnce())`, no macro needed since there's no key list. |
| `CompositionLocal<T>` / `compositionLocalOf` / `staticCompositionLocalOf` | `cranpose_core::composition_locals::{CompositionLocal, compositionLocalOf, staticCompositionLocalOf}` | Implemented | Same three-way split (local, dynamic default, static default). |
| `CompositionLocalProvider(...)` | `cranpose_core::CompositionLocalProvider` | Implemented | |
| `rememberCoroutineScope()` | `cranpose_core::concurrency::rememberCoroutineScope` | Implemented | |
| `key(vararg keys) { ... }` | `cranpose_core::key(keys, content)` | Renamed/reshaped | Owned-value keys -- a tuple for multiple -- rather than a vararg, which Rust has no equivalent of. Seeds the composition group through the same `with_key` machinery that already existed internally but was unreachable from the facade. |
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
| `Modifier.focusable()` (the common convenience wrapper over `focusTarget` + indication) | `Modifier::focusable()` | Implemented, deliberately a subset | Wraps `focus_target` and nothing else. Cranpose has no indication concept to compose with -- `MutableInteractionSource` emits only press interactions -- so the wrapper stops at focus rather than inventing one. |
| `Modifier.testTag("x")` | -- | Absent | See Findings -- `cranpose-testing` finds elements by text/geometry instead. |
| `Modifier.rotate()`, `.scale()` | -- (folded into `graphics_layer`/`graphics_layer_params`) | Partial | The `graphicsLayer`-equivalent primitive exists; the convenience wrappers do not. They are straightforward over it -- the pivot already matches Compose, since `TransformOrigin::CENTER` is the layer default and positive degrees rotate clockwise in Cranpose's y-down space. |
| `Modifier.zIndex()` | -- | Absent | Not a `graphicsLayer` wrapper: draw order is a layout concern. `Placement::z_index` exists but is hardcoded per widget and never exposed through `ParentData`, so a real modifier needs a parent-data field, a node populating it, and every `Placement::new(.., 0)` site reading it instead. |
| `FocusRequester`, `Modifier.focusRequester()` | `FocusRequester`, `Modifier::focus_requester()` | Implemented | `request_focus()` returns `Result`, with `NotAttached` (never attached, or the node left composition) and `NoFocusTarget` distinguished -- no panic and no lying `Ok`. Reentrancy is safe by construction: a `request_focus()` from inside `on_focus_changed` enqueues and returns, and the outer dispatch loop drains it, with the manager's `RefCell` never held across a callback. |
| `Modifier.testTag` | -- | Absent | See Findings: semantics-driven testing is not available. |
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
| `animateDpAsState`, `animateOffsetAsState`, `animateSizeAsState`, `animateRectAsState` | Same names | Implemented | Built on a generic `animateValueAsState<T: SpringScalar + PartialEq>`. The vector-converter core already existed as the `SpringScalar` + `Lerp` traits and was extended to `Dp`, `Sp`, `Point`, `Size` and `Rect` rather than duplicated; `animateFloatAsState`/`animateColorAsState` now delegate to the same function. |
| `animateIntAsState`, `animateIntOffsetAsState`, `animateIntSizeAsState` | -- | Absent, deliberately | Cranpose has no `Int`-typed geometry vocabulary, so these would be unused surface. |
| `tween(durationMillis, easing)`, `spring(dampingRatio, stiffness)` | Same names | Implemented | |
| `Animatable<T>` | Same name | Implemented | |
| `rememberInfiniteTransition()` / `InfiniteTransition.animateFloat` | Same names | Implemented | |
| `updateTransition(targetState)` / `Transition<S>` / `transition.animateFloat { }` | Same names (`animateValue`/`animateFloat`/`animateDp`/`animateColor`) | Implemented, different settled semantics | Each child owns an `Animatable` and reports its own running state; `Transition::is_running()` is true until every child settles. Compose instead drives children from one shared total-duration play-time. Registration reuses `InfiniteTransition`'s disposable-effect-by-pointer-identity pattern. |
| `DecayAnimationSpec`, `FloatDecayAnimationSpec` (fling physics contract) | `cranpose-animation::decay_spec::{FloatDecayAnimationSpec, ExponentialDecaySpec}` | Implemented, deliberately different physics | The trait matches; the concrete spec does not. `SplineBasedDecaySpec` (a port of `android.widget.Scroller`'s math) was replaced in #541 (landed after this doc's data was last generated, caught while re-verifying for this refresh) by `ExponentialDecaySpec` + `IOS_DECELERATION_RATE_{FAST,NORMAL}`, physics measured directly from a real iOS `UIScrollView` -- a deliberate divergence (Cranpose targets iOS feel, not an Android-physics port), not a gap. |

### Text

| Compose | Cranpose | Verdict |
|---|---|---|
| `TextStyle`, `AnnotatedString` | `cranpose-ui::text::style::TextStyle` (authored style, Compose-parity) and `cranpose-ui-graphics::typography::DrawTextStyle` (resolved draw primitive) | Implemented | The two types are a real architectural split, not drift -- see Findings. The graphics one was renamed to `DrawTextStyle` so they no longer share a name. |
| `BasicTextField`, `TextFieldState` | Same names | Implemented |

### Testing

| Compose (`androidx.compose.ui.test`) | Cranpose (`cranpose-testing`) | Verdict |
|---|---|---|
| `ComposeTestRule`, `SemanticsNodeInteraction`-style finder/assertion chain | `ComposeTestRule`, `ElementFinder`, `FinderQuery`, `assert_*` fns | Implemented, different finder model |
| `Modifier.testTag` + `onNodeWithTag` | -- (finds by `TextMatcher`/geometry instead) | **Different approach**, see Findings |
| (screenshot testing is a separate library, e.g. Roborazzi/Paparazzi -- not part of `ui-test`) | `capture_screenshot`, `changed_pixel_count`, `ScreenshotPixelDifference`, `expected_screenshot_pixel_bytes` | Cranpose-only extension, not a Compose analog |

## Findings

The two findings below are the ones worth reading closely: both are
**same name, different semantics** -- the API a Compose developer would
reach for first, under a name that matches, behaving differently enough
to bite. Everything after them is either an outright absence or a
deliberate divergence, which are lower-risk because nothing about them
looks familiar enough to trust on sight.

**All four findings below were re-checked against this refresh's data
(Compose 1.12.0, not the original 2023 snapshot) and hold unchanged.**
Specifically: `Modifier.clickable` in 1.12.0 still has no position
parameter on `onClick` and still carries `enabled`/`onClickLabel`/`role`/
`interactionSource`/`indication` that Cranpose's version lacks (the
signature shown below is 1.12.0's, not 2023's -- it reordered slightly but
the shape didn't change). `remember` in 1.12.0 still returns `T` directly
(and its `vararg keys` overload, now visible in the refreshed data, is
built into `remember` itself as an overload family Kotlin can express and
Rust cannot -- confirming, not weakening, why `rememberKeyed` had to be a
separate name). `Modifier.testTag` is unchanged and Cranpose still has
nothing matching it under any spelling. The `remember`/`LaunchedEffect`
mechanism split is entirely internal to Cranpose and does not depend on
Compose's version at all. None of the four needed a rewrite.

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

### `LaunchedEffect`/`DisposableEffect` were macros -- resolved, and the macro was wrong

This pass recorded the macro-vs-function split as an unexplained
inconsistency and did not chase it down. It has been chased down, and the
answer is stronger than "the macro was unnecessary": the macro was
actively worse, and it had already cost something.

None of the candidate justifications survive -- future-boxing, variadic
keys, lazy evaluation of the effect body and how `keys` is captured are
all identical whether the call site is a macro or a function. The
function route was already proven in-tree: the same internal impls
(`__launched_effect_impl`, `__disposable_effect_impl`) were being invoked
directly, with a `#[track_caller]`-computed key, by non-macro code in
`cranpose-core::concurrency` and `cranpose-services`. `TaskSite`'s
`From<&'static Location>` impl was dead code, which reads as the
track_caller route having been anticipated and never finished.

The defect: `file!()/line!()/column!()` are lexical, so every call routed
through a second-order wrapper that is not itself `#[composable]` gets
**one fixed key** -- every instance colliding on the wrapper's own source
position instead of the caller's. `#[track_caller]` propagates through
such a wrapper; a macro cannot. That is not hypothetical: the round-36
caller-identity sweep in `a1f64e59` hit exactly this in
`cranpose-animation` and worked around it with a manual XOR salt at the
call site rather than at the root.

Both are now `#[track_caller]` functions with the same bounds, exported
through the facade and prelude.

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

### Two `TextStyle` types -- resolved: a real split, now renamed

The dig this pass deferred has since been done. The two types are not
drift. `cranpose-ui-graphics::typography` (2025-10-17) is a resolved,
self-positioning draw primitive: plain `f32` fields, and `align`/
`vertical_align` that the other type has no use for, because a canvas
call has no parent layout to position it. `cranpose-ui::text::style`
(2026-02-06) is the authored, Compose-parity style --
`TextStyle { span_style, paragraph_style }` over `TextUnit` -- and is
what `Text` consumes. A single documented conversion,
`text_style_for_draw_style`, bridges them and is deliberately partial
rather than lossy.

So both should exist. What should not is both being called `TextStyle`:
four files had independently invented `as DrawTextStyle`/`as UiTextStyle`
import aliases and one doc link had to fully qualify itself, which is the
collision being felt rather than argued about. The graphics-crate type is
now `DrawTextStyle`; the `cranpose-ui` one keeps the name that matches
Compose. A split being justified is not a reason for two types in one
framework to share a name.

## Regenerating the full generated candidate table

The commands under **Sources** produce `/tmp/cranpose_api_surface.json`,
`/tmp/compose_api_surface.json`, and `/tmp/match_result.json`. The last one
is the full 8,464-row candidate table (Cranpose item -> every Compose entry
whose squashed name matches), including everything not mentioned in the
Curated correspondence section above -- most of it correctly unclassified,
some of it worth someone's next look.
