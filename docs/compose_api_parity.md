# Jetpack Compose <-> Cranpose API Parity

## Current source audit: 2026-10-14

The audit uses the [Cranpose source revision](https://github.com/samoylenkodmitry/Cranpose/tree/d165444c2e57d6398f3e1aeda33965afce030ff9). Generated comparison counts describe the
**2026-08-29 historical snapshot**. A current coverage percentage requires a
fresh generation.

- [Text support](text.md) now points to the implemented AnnotatedString,
  LinkedText and mixed-style baseline integration tests.
- [Clickable](../crates/cranpose-ui/src/modifier/clickable.rs) includes reader
  click semantics and focus behavior. Its callback still receives a Point.
- [Lazy lists](../crates/cranpose-ui/src/widgets/lazy_list.rs) expose
  `LazyColumn` and `LazyRow` as well as lower-level node functions.
- [Robot helpers](../crates/cranpose-testing/src/robot_helpers.rs) find controls
  in semantic trees, and `Modifier::test_tag` names a node for them to find.
- [Physical-iPhone VoiceOver](accessibility_voiceover_validation.md) and
  [native text-input flows](accessibility_screen_identity_validation.md) have
  dated device evidence. API presence and reader usability are separate claims.

Use the [application guide](guide.md) for current usage and the
[release ledger](release_readiness.md) for 1.0 acceptance. The historical
comparison below is not a claim of complete behavioral parity.

## Cranpose surface, 2026-10-14

Generated with `tools/api-surface/dump-cranpose-api` from the source this
audit links: 34 publishable library crates, 7,718 reachable public
items, and 11,986 rows once each item's members (fields, variants,
methods, trait items) count as their own rows. The 2026-08-29 snapshot below
had 23 crates, 5,341 items and 8,464 rows. The candidate-match counts under
**Coverage** still describe that snapshot: matching needs the Compose
listing of the same day, and the Compose side has not been fetched again
(see **Sources**).

| Crate | Reachable items | With members | `#[composable]` fns |
|---|---:|---:|---:|
| `cranpose-ui` | 2,110 | 3,087 | 78 |
| `cranpose-services` | 925 | 1,566 | 21 |
| `cranpose-ui-graphics` | 817 | 1,314 | 0 |
| `cranpose-core` | 789 | 1,134 | 0 |
| `cranpose-foundation` | 578 | 927 | 2 |
| `cranpose` | 397 | 668 | 8 |
| `cranpose-render-common` | 331 | 630 | 0 |
| `cranpose-liquid` | 317 | 553 | 50 |
| `coroflow` | 309 | 423 | 0 |
| `cranpose-app-shell` | 280 | 394 | 0 |
| `cranpose-testing` | 183 | 218 | 0 |
| `cranpose-ui-layout` | 115 | 191 | 0 |
| `cranpose-localization` | 105 | 158 | 0 |
| `cranpose-animation` | 89 | 135 | 0 |
| `cranpose-render-wgpu` | 79 | 182 | 0 |
| `cranpose-coroflow` | 46 | 53 | 0 |
| `cranpose-capabilities` | 44 | 71 | 0 |
| `cranpose-render-pixels` | 31 | 44 | 0 |
| `cranpose-audio` | 30 | 38 | 0 |
| `cranpose-navigation` | 28 | 28 | 7 |
| `cranpose-runtime-std` | 25 | 25 | 0 |
| `cranpose-native` | 23 | 46 | 0 |
| `cranpose-assets` | 10 | 24 | 0 |
| `cranpose-gl-templates` | 9 | 29 | 0 |
| `cranpose-fonts` | 8 | 8 | 0 |
| `cranpose-media` | 8 | 8 | 0 |
| `cranpose-platform-android` | 8 | 8 | 0 |
| `cranpose-macros` | 6 | 6 | 0 |
| `cranpose-platform-desktop-winit` | 6 | 6 | 0 |
| `cranpose-platform-web` | 5 | 5 | 0 |
| `cranpose-storekit` | 4 | 4 | 0 |
| `cranpose-fonts-cjk-data-1` | 1 | 1 | 0 |
| `cranpose-fonts-cjk-data-2` | 1 | 1 | 0 |
| `cranpose-fonts-cjk-data-3` | 1 | 1 | 0 |

## Historical generated comparison (2026-08-29)

> **Generated against Jetpack Compose 1.12.0** (the frozen
> `api/1.12.0-beta01.txt` snapshot across the compose-ui/foundation/runtime/
> animation release train -- Compose's API freezes at a module's first beta,
> so this is 1.12.0's actual shipped surface, not an in-progress one), read
> at upstream `androidx/androidx` commit `5ba2cdd61be7b6945db999b238d14f3c626136fb`
> (androidx-main, fetched 2026-08-29). An earlier table used the
> 2023-06-26 snapshot (~Compose 1.5.0-beta). The before/after counts appear
> under Sources as historical comparison evidence.

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

### Cranpose historical snapshot: 23 crates, computed reachability

This crate count and the generated API totals below describe the 2026-08-29
surface snapshot. The current workspace has a different crate set.

The public surface is not a hand-picked list: it is "reachable via an
unbroken chain of plain `pub` from a crate root, excluding `#[doc(hidden)]`
and `cfg(test)`" -- the same definition `cargo doc` uses -- computed
directly from the syntax tree of every crate `cargo metadata` reports as
both publishable (`publish` absent, `null`, or a non-empty registry list)
and living under `crates/` (as opposed to `apps/`, which holds demo
binaries that expose a `lib` target only for their own tests, not a surface
meant for outside consumers). The snapshot contained 23 publishable crates
under `crates/`.

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
is confirmed present in this refresh's data and was still absent from
Cranpose by name at that refresh, which made it an honest "Absent" verdict
instead of an invisible one. Cranpose has `SharedTransitionLayout` and
`SharedElement` since then; the **Animation** table under Curated
correspondence holds the current verdict.

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
| `rememberSaveable(vararg inputs, stateSaver, key) { ... }` | `cranpose_services::rememberSaveable(key, saver, initial)`, returning `MutableState<T>` | Renamed/reshaped | Lives in `cranpose-services`, not `cranpose-core`. The `key` is an explicit `&'static str` and the `Saver<T>` is required; it converts the value to and from a `String`, where Compose derives the key from the position and the saver from the type. The value is saved in `preferences` at each save boundary and read back when the composable returns, after a process restart too. It is saved in the nearest `SaveableScope`, so each navigation entry keeps its own. No `inputs` argument resets the value when it changes. See **Navigation and saved state** for the scopes and the saved scroll positions. |
| `LaunchedEffect(keys) { ... }` | `cranpose_core::LaunchedEffect(keys, effect)` | Implemented | A plain `#[track_caller]` function, matching `remember`. Was a `macro_rules!`; see Findings for why the macro was not merely unnecessary but wrong. |
| `DisposableEffect(keys) { onDispose { ... } }` | `cranpose_core::DisposableEffect(keys, effect)` | Implemented | Same conversion as `LaunchedEffect`. |
| `SideEffect { ... }` | `cranpose_core::SideEffect` | Implemented | Plain `fn(impl FnOnce())`, no macro needed since there's no key list. |
| `CompositionLocal<T>` / `compositionLocalOf` / `staticCompositionLocalOf` | `cranpose_core::composition_locals::{CompositionLocal, compositionLocalOf, staticCompositionLocalOf}` | Implemented | Same three-way split (local, dynamic default, static default). |
| `CompositionLocalProvider(...)` | `cranpose_core::CompositionLocalProvider` | Implemented | |
| `rememberCoroutineScope()` | `cranpose_core::concurrency::rememberCoroutineScope` | Implemented | |
| `key(vararg keys) { ... }` | `cranpose_core::key(keys, content)` | Renamed/reshaped | Owned-value keys -- a tuple for multiple -- rather than a vararg, which Rust has no equivalent of. Seeds the composition group through the same `with_key` machinery that already existed internally but was unreachable from the facade. |
| `Modifier.Node` / `LayoutModifierNode` / `DrawModifierNode` / `PointerInputNode` (Compose 1.4+ node-based modifiers) | `cranpose_foundation::modifier::{ModifierNode, LayoutModifierNode, DrawModifierNode, PointerInputNode}` | Implemented | Same names, same architecture (Compose's newer node-based modifier system, not the older factory-based one). A `DrawModifierNode` draws through deferred closures (`create_draw_closure`, `create_behind_draw_closure`) that the renderer runs at the node's size, not an immediate `ContentDrawScope.draw()`. |

### Modifier & layout

The August snapshot counted 102 `Modifier` builder methods in `cranpose-ui`.
Sampled against Compose's `androidx.compose.ui`/`foundation-layout`:

| Compose | Cranpose | Verdict | Note |
|---|---|---|---|
| `Modifier.padding(...)`, `.fillMaxSize()`, `.fillMaxWidth()`, `.fillMaxHeight()`, `.weight()`, `.size()`, `.offset()`, `.background()`, `.clip()` | `.padding`, `.fill_max_size`, `.fill_max_width`, `.fill_max_height`, `.weight`, `.size`, `.offset`, `.background` | Implemented | snake_case per Rust convention; matched by the case/separator-insensitive key. |
| `Modifier.clickable(onClick: () -> Unit, enabled, role, onClickLabel, interactionSource, indication)` | `Modifier::clickable(handler: impl Fn(Point))` | **Partial, same name different signature** | See Findings -- this is the most consequential divergence found. |
| `Modifier.pointerInput(vararg keys, block: suspend PointerInputScope.() -> Unit)` | `Modifier::pointer_input(key: K, handler: F)` | Renamed/reshaped | Single key (use a tuple for Compose's variadic-key case), same idea otherwise. |
| `Modifier.draggable(state, orientation)` | `Modifier::draggable(axis, state)` | Implemented | |
| `Modifier.toggleable(value, onValueChange)` | `Modifier::toggleable(value, description, role, on_value_change)` | Implemented | Cranpose's takes `role`/`description` directly rather than through a separate `semantics {}` block. |
| `Modifier.semantics { ... }` | `Modifier::semantics(recorder: impl Fn(&mut SemanticsConfiguration))` | Implemented | `SemanticsConfiguration`'s own fields carry doc comments naming the exact Compose property they mirror (e.g. `state_description` is documented as "Compose's `stateDescription`") -- this one was built as a deliberate mirror, not a coincidental name match. |
| `Modifier.semantics { … }` (the only semantics entry point Compose has, beside `clearAndSetSemantics`, `testTag`, `progressSemantics` and `selectableGroup`) | `Modifier::semantics(\|config\| …)`, and `Modifier::semantics_spec(SemanticsSpec::new()…)` beside it | Implemented, one shape added | Kotlin's receiver lambda has no Rust match, so a spec value sits next to the closure: one chain element rather than one per property, and two specs can be compared. `SemanticsSpec` is `SemanticsConfiguration` under a short name and every builder method is named after its field. |
| The per-property `Modifier::` shorthands (`error`, `password`, `pane_title`, `traversal_index`, `heading`, `role`, `content_description`, `live_region`, `hide_from_accessibility`, `merge_descendants`, `dropdown_list`, `value_picker`, `expand`, `collapse`, `on_long_click`, `dismiss`, `scroll_to_index`) | -- | Cranpose's own shape | Compose has none of these as modifiers; each is a property inside `semantics { }`. They read better than a closure for one property. The rows above name the Compose property each one mirrors. |
| `Modifier.focusTarget()` | `Modifier::focus_target()` | Implemented | The low-level primitive. |
| `Modifier.focusable()` (the common convenience wrapper over `focusTarget` + indication) | `Modifier::focusable()` | Implemented | A focus target that draws a ring while the keyboard made the last focus move, the indication Compose leaves to the theme. `clickable`, `toggleable` and `selectable` include it, as Compose's do, so Tab reaches every control and Enter or Space presses it. |
| `Modifier.minimumInteractiveComponentSize()` | `Modifier::minimum_interactive_component_size()` | Implemented | At least 48 by 48 points for the press, as far as the parent allows, with the drawn content in the middle. Always on when declared; Compose's `LocalMinimumInteractiveComponentSize` switch has no match yet. |
| `LocalFocusManager`, `focusManager.moveFocus(FocusDirection.Next)`, `clearFocus()` | `local_focus_manager()`, `FocusManager::move_focus(FocusDirection::Next)`, `clear_focus()` | Implemented | Tab and Shift+Tab reach it through the app shell. `clear_focus()` takes no `force` flag: no focus target in Cranpose refuses to give focus up. Directional moves pick the nearest target in that direction, measured on the laid-out rectangles. |
| `Modifier.semantics { liveRegion = LiveRegionMode.Polite }` | `Modifier::live_region(LiveRegionMode::Polite)`, or `config.live_region` in a `semantics` closure | Implemented | Same two modes. accesskit and ARIA carry it natively; on iOS and Android Cranpose reads the changed text out itself, because iOS has no live region and Android reads one on a virtual view only through its host view. |
| `Modifier.progressSemantics(value, valueRange, steps)`, `semantics { setProgress { } }` | `Modifier::progress_semantics(value, start, end, steps)`, `config.progress`, `config.set_progress` | Implemented | Same range and step vocabulary. `Slider` declares both on its own, so it is adjustable from VoiceOver, TalkBack, accesskit and the web mirror with no app code. |
| `Modifier.semantics { customActions = listOf(CustomAccessibilityAction("Dismiss") { .. }) }` | `config.custom_actions.push(SemanticsCustomAction::new("Dismiss", \|\| ..))` | Implemented | accesskit's custom actions, VoiceOver's actions rotor, TalkBack's actions menu, and one button per action on the web mirror. `SwipeToDismiss` and `LinkedText` fill it on their own. |
| `Popup(onDismissRequest = ..)`, `DropdownMenu` close on the back gesture and on Escape | `PopupDismissable(.., on_dismiss, ..)` closes on the back gesture, on Escape and on a VoiceOver two-finger scrub, when no dialog is open | Implemented | The dialog stack goes first, then the dismissable popup on top of the most recent `PopupHost`. |
| `Modifier.selectable(selected, enabled, role, onClick)` | `Modifier::selectable(selected, role, on_click)` | Implemented | Same `selected` semantics and role. No `enabled` flag yet; `interactionSource` and `indication` are absent as on `clickable`. |
| `Modifier.semantics { heading() }`, `semantics { role = Role.Button }`, `semantics { contentDescription = "..." }` | `Modifier::heading()`, `Modifier::role(SemanticsWidgetRole::Button)`, `Modifier::content_description("...")` | Implemented | Shorthands over `Modifier::semantics`; the closure form stays for anything else. |
| `Role` (Button, Checkbox, Switch, RadioButton, Tab, Image, DropdownList, ValuePicker, Carousel) | `SemanticsWidgetRole`, with Header, Dialog, Link, SearchField, ProgressBar, ToggleButton, Alert, Toolbar, Menu, MenuItem, TabBar, List and ListItem beside Compose's | Implemented, more roles | Compose leaves the rest to the platform's own view classes, which Cranpose has none of, so every role a reader names on the four platforms lives in the enum. No `Carousel`. |
| `TextField(...)` with `Modifier.semantics { contentDescription = "..." }`; `editableText` | `Modifier::content_description` on `BasicTextField`; `config.text` | Implemented | The name stays the label and the text becomes the value; with no name the text stands in, and an empty field is still published. |
| `semantics { setText { text -> ... } }` | `config.set_text`; `BasicTextField` declares it | Implemented | Android's set-text action and accesskit's set-value action hand the field whole text; the field answers whether it took it. |
| SwiftUI's `accessibilityAction(.magicTap)`, `accessibilityInputLabels`, `accessibilityLanguage`, `@Environment(\.accessibilityVoiceOverEnabled)`; Compose has none of the four | `Modifier::on_magic_tap(label, ..)`, `Modifier::input_labels(..)`, `Modifier::language(..)`, `cranpose_services::local_accessibility_state()` | Cranpose's own shape | The magic tap runs on VoiceOver's gesture and is listed by its label on the other platforms; input labels reach Voice Control; the language reaches VoiceOver, accesskit and the browser; the reader state comes from VoiceOver, an Android accessibility service or an accesskit reader, and the web says no. |
| `semantics { setSelection { start, end, relativeToOriginalText -> ... } }`, `textSelectionRange` | `config.set_selection`, `config.text_selection`; `BasicTextField` declares both | Implemented | The caret and the picked stretch of text reach every reader, and a reader's move comes back: accesskit's text runs and set-text-selection action, Android's granularity and set-selection actions, VoiceOver through the keyboard's `UITextInput` view, and an `<input>` on the web mirror. No `relativeToOriginalText`: Cranpose has no output transformation, so the two texts are one. |
| `semantics { onLongClick("Remove receipt") { … } }`, `Modifier.combinedClickable(onLongClickLabel = …, onLongClick = …)` | `Modifier::on_long_click("Remove receipt", \|\| …)`, `SemanticsSpec::on_long_click(..)`, `config.on_long_click` | Implemented, one platform native | TalkBack gets Android's own `ACTION_LONG_CLICK` with the label. accesskit's `Action` enum has no long press, VoiceOver has none and ARIA has none, so on those three it rides the custom-action road: the last action a reader lists, named by the label. Cranpose asks for the label where Compose takes `null`, because three of the four platforms have nothing to read without one. |
| `Modifier.semantics { hideFromAccessibility() }`, `invisibleToUser()` | `Modifier::hide_from_accessibility()` | Implemented | The node and everything under it stay out of what a reader sees, and out of the label a clickable parent gathers from its children. |
| `Modifier.semantics(mergeDescendants = true) {}` | `Modifier::merge_descendants()` | Implemented | The node and the text under it read as one stop with the texts joined by commas, the way a button does; the children are not published on their own. |
| `Modifier.selectableGroup()` | `Modifier::selectable_group()` | Implemented | The selectable controls under the node form one group; a reader says which of how many each is. `LiquidTabBar` declares it on its own. |
| `LocalDensity.current.fontScale` | `AccessibilityOptions::font_scale`, and every `Sp` size through the shell's font scale | Implemented | Dynamic Type on iOS, the font size on Android and the web, GNOME's and Windows' text scale on the desktop; an app reads the number for what it draws itself. |
| `LocalConfiguration.current.fontWeightAdjustment`, `Settings.Global.ANIMATOR_DURATION_SCALE`, the high contrast text setting | `AccessibilityOptions::{bold_text, reduce_motion, increase_contrast, reduce_transparency, invert_colors}` | Implemented, one value on every platform | Compose leaves these to the platform and to each app. Cranpose reads them on iOS, Android, the web and the desktop and acts on them in the framework: animations end on their first frame, glass draws flat, the theme gains contrast and weight, and iOS Smart Invert swaps the palette with pictures left as they are. |
| `Modifier.semantics { paneTitle = "..." }` | `Modifier::pane_title("...")` | Implemented | A new or changed title is read out on every platform when the app moves on; the root carries it as a pane title on Android, a region landmark on the web and a labeled region in accesskit. |
| `Modifier.semantics { error("...") }` | `Modifier::error("...")` | Implemented | A reader hears "invalid" and the reason after the control's state on every platform; TalkBack gets the node's error, accesskit the invalid flag, the web mirror `aria-invalid`. |
| `Modifier.semantics { password() }` | `Modifier::password()` | Implemented | The field's text leaves the projection, so no reader reads a password out; the name stands, and with no name a reader hears "password". |
| `Modifier.semantics { traversalIndex = -1f }`, `isTraversalGroup = true` | `Modifier::traversal_index(-1.0)` | Implemented, one half needed | A smaller index is read first among the nodes beside it, on all four platforms at once, because every platform reads the projection's order. Compose's `isTraversalGroup` has no twin: Cranpose projects each container's nodes together already, so a group is what a container is. |
| `semantics { expand { … } }`, `semantics { collapse { … } }` | `Modifier::expand(..)`, `Modifier::collapse(..)` | Implemented | Which of the two a control declares says whether it is open now, as it does in Compose. accesskit gets the expanded flag and the matching action, TalkBack `ACTION_EXPAND`/`ACTION_COLLAPSE`, the web mirror `aria-expanded`, and VoiceOver the word as the value. |
| `semantics { dismiss { … } }` (`SemanticsActions.Dismiss`) | `Modifier::dismiss(..)`, `config.dismiss` | Implemented, two platforms carry it as a named action | TalkBack gets `ACTION_DISMISS` and VoiceOver the two-finger scrub on the control itself. accesskit 0.24 has no dismiss action and ARIA has none either, so on the desktop and on the web the way out is listed after the actions the app named: one more entry in the node's accesskit custom actions, one more `<button>` on the mirror. The handler answers whether the control took the ask, as Compose's does. `SwipeToDismiss` declares it on its own. |
| `Role.DropdownList`, `Role.ValuePicker` | `Modifier::dropdown_list()`, `Modifier::value_picker()` | Implemented | accesskit gets `ComboBox` and `SpinButton`, the web mirror `combobox` and `spinbutton`, Android the `Spinner` and `NumberPicker` class names, and VoiceOver a button and an adjustable element. Compose's `Role.Carousel` has no twin yet. |
| `semantics { collectionItemInfo = CollectionItemInfo(...) }` | computed for a selectable inside a `selectable_group` | Implemented | TalkBack gets a collection item with its row or column, VoiceOver reads "2 of 5" as the value, the web mirror sets `aria-posinset` and `aria-setsize`, accesskit gets the position and size of the set. |
| A changed toggle, value or text under the reader's cursor (`sendSemanticsPropertyChangeEvents`) | automatic | Implemented | Every publish marks the controls that now say something else; TalkBack gets one subtree change on the host and a content-changed event for the control under its cursor, and VoiceOver reads the one under its cursor again. |
| `Text(buildAnnotatedString { withLink(LinkAnnotation.Url(...)) { } })`, each link a node a reader can activate | `LinkedText` publishes one custom action per link, "Open <link text>" | Implemented, different shape | Compose splits the text into link nodes; Cranpose keeps one text and lists the links in the reader's actions menu, which VoiceOver, TalkBack, accesskit and the web mirror all carry. |
| `Dialog` closes on Escape (desktop) and on the back gesture (Android) | `Dialog` closes on Escape through `AppShell::dismiss_top_modal`, on the back gesture, and on a VoiceOver two-finger scrub | Implemented | One modal stack takes all three; `BackHandler` sits under it for the app's own back handling. |
| `semantics { verticalScrollAxisRange = ScrollAxisRange(...) }`, `horizontalScrollAxisRange`, `scrollBy { x, y -> }` | `config.vertical_scroll`, `config.horizontal_scroll`, `config.scroll_by` | Implemented | Same `value`, `maxValue` and `reverseScrolling` fields. `verticalScroll`, `horizontalScroll`, `LazyColumn` and `LazyRow` declare them on their own; a reader pages the container around its cursor by nine tenths of the viewport. |
| `semantics { scrollToIndex { index -> ... } }` (`SemanticsActions.ScrollToIndex`), and `indexForKey` beside it | `Modifier::scroll_to_index(\|index\| ..)`, `config.scroll_to_index` | Implemented, one half needed | `LazyColumn` and `LazyRow` declare it on their own, so a reader reaches row 300 of 500 at once instead of paging fifty times. Android gets `ACTION_SCROLL_TO_POSITION` with a row or column argument, and reaches any row. accesskit 0.24.1 has no scroll-to-index action, so the list publishes its scroll range in rows and takes `SetScrollOffset` as a row number; that reaches any row too. UIKit and ARIA have neither, so VoiceOver gets "first row" and "last row" in the actions rotor of every row, and the web mirror answers Home and End: the two ends, not an arbitrary row. Compose's `indexForKey` has no twin: it turns an app's item key into a row number for the test finder, and no reader on any platform asks for it. |
| `semantics { collectionInfo = CollectionInfo(rowCount, columnCount) }` | `config.collection` | Implemented | `LazyColumn` and `LazyRow` declare the count on their own. TalkBack reads "list, N items" as its cursor enters; accesskit gives the container the list role. |
| `View.announceForAccessibility(text)` (what Compose on Android reaches for; no multiplatform API) | `local_announcer()`, `Announcer::announce`, `announce_assertive` | Implemented | One call on all four platforms: `UIAccessibilityAnnouncementNotification`, `announceForAccessibility`, an `aria-live` region, and an accesskit live node. |
| `Icon(imageVector, contentDescription)`, `Image(painter, contentDescription)` -- the name is a required argument, `null` means decoration | `Icon(path, content_description, size, color)` in `cranpose-ui` and in `cranpose-liquid`; `Image(painter, content_description, ..)` | Implemented | `Image` already asked. `Icon` had a short form that answered `None` for the writer, and the liquid `Icon` could not carry a name at all; both now ask, and `None` says the icon is decoration. Compose asks at these two widgets and no others, and neither does Cranpose. |
| Coil's `AsyncImage(model = url)` / `rememberAsyncImagePainter` | `AsyncImage(ImageRequest::new(url), ..)`, which decodes at the size of its box, and `rememberAsyncImage(request)` returning `State<ImageLoadState>`, with `ImageLoader` (`image-loading` feature) | Implemented | Not Compose itself: Coil, which Compose apps use. Decodes off the UI thread at the requested size; memory cache by decoded bytes, disk cache with a size limit; one download per image; a request nobody waits for stops. PNG, JPEG, WebP and GIF (first frame). |
| `Modifier.testTag("x")` | `Modifier::test_tag("x")` | Implemented | `PlacedSemanticsNode::find_by_tag`, the headless robot's `find_by_tag` and the desktop robot's `find_by_tag` / `click_by_tag` find the node. Screen readers never receive the tag. |
| `Modifier.rotate()`, `.scale()` | `Modifier::rotate()`, `Modifier::scale()`, `Modifier::scale_xy()` | Done | Thin wrappers over `graphics_layer_value`; they retain `TransformOrigin::CENTER`, and positive degrees rotate clockwise in Cranpose's y-down space. Rust names the two-axis overload `scale_xy`. |
| `Modifier.zIndex()` | `Modifier::z_index(z)` | Implemented | Orders siblings for drawing and taps; equal ones keep the composition order, and several on one node add up. `Placement::z_index` from a custom `MeasurePolicy` is not read yet. |
| `FocusRequester`, `Modifier.focusRequester()` | `FocusRequester`, `Modifier::focus_requester()` | Implemented | `request_focus()` returns `Result`, with `NotAttached` (never attached, or the node left composition) and `NoFocusTarget` distinguished -- no panic and no lying `Ok`. Reentrancy is safe by construction: a `request_focus()` from inside `on_focus_changed` enqueues and returns, and the outer dispatch loop drains it, with the manager's `RefCell` never held across a callback. |
| `Constraints`, `Density`, `Alignment`, `Arrangement`, `Measurable`, `Placeable`, `MeasureResult`, `MeasurePolicy` | `cranpose-ui-layout::{Constraints, ..., MeasurePolicy}`, `cranpose-ui::Density` | Implemented | Full layout-contract vocabulary present under the same names. |
| `Dp`, `Sp`, `Px` (typed units, used everywhere in Compose's own modifier signatures) | `cranpose-ui-graphics::unit::{Dp, Sp, Px}` (types exist) | **Partial** | The types exist, but the widget/modifier surface itself (`padding`, `size`, `weight`, `offset`, ...) takes raw `f32`, not `Dp` -- Compose's type-safety guarantee against mixing raw pixels and density-independent units is not carried through to the call sites that would benefit from it. |

### Core widgets (`#[composable]` fns)

| Compose | Cranpose | Verdict |
|---|---|---|
| `Box`, `Row`, `Column`, `Spacer`, `BoxWithConstraints` | Same names in `cranpose-ui::widgets::*` | Implemented |
| `Text` / `BasicText` | `Text`, `BasicText`, `BasicTextWithOptions`, `TextWithOptions` | Implemented |
| `Image`, `Canvas`, `Icon` | Same names | Implemented |
| `LazyColumn`, `LazyRow` | `LazyColumn`, `LazyRow`; lower-level node functions also exist | Implemented; checked against current source |
| `LazyListScope.stickyHeader` | `LazyListScope::sticky_header`, `sticky_header_keyed` | Implemented |
| `LazyListState.animateScrollBy`, `animateScrollToItem` | `AnimateScroll::animate_scroll_by`, `animate_scroll_to_item` (a trait on `LazyListState`) | Implemented; a press on the list stops the animation |
| `Popup`, `Dialog` | Same names, plus `PopupAnchored`/`PopupDismissable`/`PopupDismissableWhen`/`DialogWithScrim` variants | Implemented, plus extensions |
| `AnimatedVisibility`, `Crossfade` | Same names | Implemented; the arguments differ, see **Animation** |
| `Layout`, `SubcomposeLayout` | Same names | Implemented |
| `CircularProgressIndicator`, `LinearProgressIndicator`, `Slider`, `Button`, `IconButton` | Same names (Material-styled names; see Scope decision above) | Implemented, names only -- no Material visual/behavioral parity claimed |
| (no Compose stdlib equivalent -- `for` over Kotlin collections is used directly) | `ForEach` | Rejected-in-reverse: a Cranpose-only convenience the Compose side doesn't need |

Lazy grids, staggered grids and pagers have their own table under **Lazy
layouts and pagers**.

`cranpose-liquid` runs a parallel, intentionally-non-Material set under its
own names (`GlassButton`, `LiquidCard`, `LiquidChip`, `LiquidMenu`,
`LiquidTabBar`, `LiquidToggle`, ...) -- not matched against Compose symbol
by symbol, per the Scope decision above.

### Lazy layouts and pagers

Grids, staggered grids and pagers share the measure pass of `LazyColumn` and
`LazyRow`: a grid lays out lines of cells, a staggered grid lays out lanes,
and a pager lays out its pages as the items of a lazy row or column. Item keys
and content types are `u64` values. The options sit in a spec struct beside
the state, in place of Compose's long argument lists. None of the six layouts
takes a fling behavior or a switch for user scrolling.

| Compose | Cranpose | Verdict | Note |
|---|---|---|---|
| `LazyVerticalGrid(columns, modifier, state, contentPadding, reverseLayout, verticalArrangement, horizontalArrangement, flingBehavior, userScrollEnabled, content)` | `LazyVerticalGrid(modifier, state, LazyVerticalGridSpec::new(cells).spacing(..).content_padding(..), content)` | Renamed/reshaped | The spec holds `columns`, `vertical_arrangement` (a `LinearArrangement`), `horizontal_spacing`, `content_padding`, `beyond_bounds_item_count` and `reverse_layout`. The gap between the cells of a line is a number of logical pixels, where Compose takes an `Arrangement.Horizontal`. `content` adds items as a `LazyColumn`'s does. A sticky header takes a whole line and holds the top while the lines under it scroll. The grid tells screen readers its line and column counts. |
| `LazyHorizontalGrid(rows, ...)` | `LazyHorizontalGrid(modifier, state, LazyHorizontalGridSpec::new(cells), content)` | Renamed/reshaped | The same shape on the other axis, built on `LazyRow`: columns of cells from top to bottom. The spec holds `rows`, `horizontal_arrangement` and `vertical_spacing`. A sticky header takes a whole column and holds the start edge. |
| `GridCells.Fixed(count)`, `GridCells.Adaptive(minSize)`, `GridCells.FixedSize(size)` | `GridCells::Fixed(usize)`, `GridCells::Adaptive(f32)`, `GridCells::FixedSize(f32)` | Partial | `Adaptive` takes as many columns as fit at least that many logical pixels wide, with the spacing counted, and shares the width left over. `FixedSize` takes as many columns of exactly that width as fit and leaves the rest empty at the end, or one column of the whole width when none fits. `GridCells` is a closed enum, so Compose's custom `GridCells` implementations have no twin. |
| `rememberLazyGridState(initialFirstVisibleItemIndex, initialFirstVisibleItemScrollOffset)`, `LazyGridState.firstVisibleItemIndex`, `scrollToItem(index, scrollOffset)`, `animateScrollToItem(index, scrollOffset)` | `rememberLazyGridState()`, `LazyGridState::{first_visible_item_index, first_visible_item_scroll_offset, scroll_to_item(index, offset), animate_scroll_to_item(index, offset)}` | Partial | `rememberLazyGridState` takes no start position; a `scroll_to_item` before the first layout goes there on that layout. The first visible index is the first item of the first visible line. `animate_scroll_to_item` is a spring that a press on the grid stops. No `animateScrollBy`, `layoutInfo` or `canScrollForward` on the grid state; `lazy_list_state()` returns the state of the lazy column or row whose items are the lines. |
| `item(span = { GridItemSpan(n) })`, `items(count, span = { index -> GridItemSpan(n) })`, `LazyGridItemSpanScope.maxLineSpan`, `maxCurrentLineSpan` | `LazyItems::new(count).span(\|scope, index\| cells)`, `LazyGridItemSpanScope { max_line_span, max_current_line_span }` | Renamed/reshaped | The closure returns a cell count as a `usize`; there is no `GridItemSpan` type. A span belongs to a run of items, so one spanned item is a run of one, and `item` takes no span. An item that takes more cells than its line has left starts the next line, and a grid clamps a span to the line. A list ignores `span`. |
| `LazyVerticalStaggeredGrid(columns, modifier, state, contentPadding, reverseLayout, verticalItemSpacing, horizontalArrangement, flingBehavior, userScrollEnabled, content)` | `LazyVerticalStaggeredGrid(modifier, state, LazyVerticalStaggeredGridSpec::new(cells), content)` | Renamed/reshaped | Each item takes the lane that ends first and keeps it while the grid scrolls. The spec holds `columns`, `vertical_item_spacing`, `horizontal_spacing`, `content_padding`, `beyond_bounds_item_count` and `reverse_layout`. A focused item stays placed when scrolled out of view, and Tab reaches the items beyond the viewport. |
| `LazyHorizontalStaggeredGrid(rows, ...)` | `LazyHorizontalStaggeredGrid(modifier, state, LazyHorizontalStaggeredGridSpec::new(cells), content)` | Renamed/reshaped | The same on the other axis, built on `LazyRow`. The spec holds `rows`, `horizontal_item_spacing` and `vertical_spacing`. |
| `StaggeredGridCells.Fixed`, `StaggeredGridCells.Adaptive`, `StaggeredGridCells.FixedSize` | `GridCells::{Fixed, Adaptive, FixedSize}`, the type the lazy grids use | Renamed/reshaped | There is no separate staggered type; the lanes take the same three kinds of cells. |
| `StaggeredGridItemSpan.FullLine`, `StaggeredGridItemSpan.SingleLane`, set through `span` | `LazyItems::span(..)` | Renamed/reshaped | An item whose span is at least the lane count takes the whole line, and any smaller span takes one lane. Both Compose cases exist, as a number in place of two values. |
| `rememberLazyStaggeredGridState(initialFirstVisibleItemIndex, initialFirstVisibleItemScrollOffset)`, `LazyStaggeredGridState` | `rememberLazyStaggeredGridState()`, `LazyStaggeredGridState::{first_visible_item_index, first_visible_item_scroll_offset, scroll_to_item, animate_scroll_to_item}`, `LazyStaggeredGridState::new(index, offset)` | Partial | The same gaps as `LazyGridState`: the remember function takes no start position, the state has no `layoutInfo` or `animateScrollBy`, and `lazy_list_state()` returns the state of the lazy column or row that holds the lanes. |
| `HorizontalPager(state, modifier, contentPadding, pageSize, beyondViewportPageCount, pageSpacing, verticalAlignment, userScrollEnabled, reverseLayout, key, snapPosition, flingBehavior, pageContent)` | `HorizontalPager(modifier, state, PagerSpec, page_count, page_content)` | Partial | The pages are the items of a lazy row and compose as they come into view; the page key is the page index. `PagerSpec` holds `page_size`, `page_spacing`, `content_padding` (a page rests after the start padding, with its neighbours in the padding) and `beyond_viewport_page_count` (2 by default; Compose's default is 0). The page count is an argument of the pager, where Compose reads it from the state. The pager needs a bounded size. Absent: `userScrollEnabled`, `reverseLayout`, `key`, `flingBehavior`, `snapPosition` and the page alignment. |
| `VerticalPager(...)` | `VerticalPager(modifier, state, PagerSpec, page_count, page_content)` | Partial | The same pager with the pages stacked from top to bottom, built on a lazy column. The same arguments are absent. |
| `PageSize.Fill`, `PageSize.Fixed(pageSize)` | `PageSize::Fill`, `PageSize::Fixed(f32)` | Partial | A fixed page shorter than the pager shows the start of the next page beside it. Compose's `PageSize` is an interface that takes custom sizes; Cranpose's is a closed enum. |
| `rememberPagerState(initialPage, initialPageOffsetFraction, pageCount)`, `PagerState.currentPage`, `currentPageOffsetFraction`, `scrollToPage(page, pageOffsetFraction)`, `animateScrollToPage(page, pageOffsetFraction, animationSpec)` | `rememberPagerState(initial_page)`, `PagerState::{current_page, current_page_offset_fraction, scroll_to_page(page), animate_scroll_to_page(page)}` | Partial | `PagerState` is `Copy`. `current_page` is the page that fills most of the pager, and the offset fraction runs from -0.5 to 0.5, as in Compose. No offset-fraction arguments. `animate_scroll_to_page` is a spring with no animation spec argument, and a press on the pager stops it. `settled_page` keeps the page a scroll began on until it comes to rest, and `is_scroll_in_progress` is true while a drag, a fling or an animation moves the pager; lazy lists, grids and `ScrollState` have `is_scroll_in_progress` too. `target_page` is the page a running animation or the settle after a release ends on, and the current page during a drag and at rest. `LazyListState::is_dragged` answers `collectIsDraggedAsState`. `pageCount` and `layoutInfo` have no twin; `lazy_list_state()` returns the state of the row or column whose items are the pages. `rememberSaveablePagerState` (see **Navigation and saved state**) keeps the page across a restart. |
| `PagerDefaults.flingBehavior(state, pagerSnapDistance, snapAnimationSpec, snapPositionalThreshold)`, `SnapPosition.Start`, `.Center`, `.End`, `PagerSnapDistance.atMost(n)`, and `rememberSnapFlingBehavior` for lazy lists | A settle policy that the pager sets on its row with `LazyListState::set_settle_policy`; `LazySettlePolicy` is `Rc<dyn Fn(f32, f32) -> f32>` | Partial | A drag or fling that the user lets go of comes to rest at the start of the first visible page or of the page after it. A release of 400 logical pixels per second or faster goes the way of the fling; a slower one goes to the page start nearer to where the release would stop. One release moves at most one page. The snap position is always the page start: no `Center` or `End`, no snap distance and no snap animation spec. `set_settle_policy` is public on any `LazyListState`, so a list of other items can snap with a policy of its own, but there is no `rememberSnapFlingBehavior`. |

### Navigation and saved state

Compose's navigation (`androidx.navigation:navigation-compose`), its view
models (`androidx.lifecycle`) and its saved-state handle sit outside the
`compose/` tree that this doc scanned, as Coil does. The rows compare with
those libraries by the Android names that the docs of `cranpose-navigation`
and `cranpose-coroflow` give for their own items. `rememberSaveable` itself
is in **Runtime & composition model**.

| Compose | Cranpose | Verdict | Note |
|---|---|---|---|
| `rememberNavController()` with `NavHost(startDestination = ..)` | `rememberNavController(start)`, returning `NavController<R>` | Renamed/reshaped | The start route is an argument of the controller. The route type `R: Clone + PartialEq` is usually an enum whose variants carry the screen's arguments. The controller is `Copy`, so it moves into any number of `move` closures. The back stack lives in the nearest view model store and in memory only; a `NavHost` nested in a screen keeps its back stack while another screen covers that screen. |
| `NavHost(navController, startDestination, modifier, contentAlignment, route, enterTransition, exitTransition, popEnterTransition, popExitTransition, sizeTransform, builder)` | `NavHost(controller, content)`, `NavHostWith(controller, transition, content)`, `NavHostWithDialogs(controller, transition, dialog, content)` | Partial | One closure receives the route and composes its screen, usually with a `match`, so the compiler checks that every route has a screen. There is no `NavGraphBuilder`, no `composable<T>()` and no `navigation()` graph; a nested graph is a `NavHost` composed inside a screen. Screens crossfade, 700 ms by default, and `NavHostWith` sets the one `AnimationType` of that fade. `NavHostWithDialogs` gives the routes that are `dialog` destinations a `DialogSpec`: such an entry shows in a `Dialog` over the screen beneath it, and dismissing it pops it. Absent: separate enter, exit, pop-enter and pop-exit transitions, `sizeTransform`, `modifier` and `contentAlignment`. |
| String routes with `{argument}` patterns, `composable<Route>()`, `NavBackStackEntry.toRoute<Route>()` | The route value | Cranpose's own shape | The route value reaches the screen as it is, so the compiler checks the arguments. There are no string route patterns and no argument parsing. |
| `navigate(route)`, `navigate(route) { popUpTo(route) { inclusive = true; saveState = true }; launchSingleTop = true; restoreState = true }` | `NavController::navigate(route)`, `navigate_with(route, NavOptions::new().pop_up_to(route, inclusive).save_state().launch_single_top().restore_state())` | Implemented | `NavOptions` has `pop_up_to`, `launch_single_top`, `save_state` and `restore_state`. `pop_up_to` pops the entries above the topmost equal route, and that route too when inclusive; nothing is popped when the route is not on the stack. With `save_state` the popped entries keep their view models and saved state, and `restore_state` brings the kept segment that holds the route back on top, as a tab bar needs; a later kept segment with one of the same routes replaces the earlier one. `rememberSaveableNavController` saves the kept segments too. |
| `popBackStack()`, `popBackStack(route, inclusive)`, `navigateUp()` | `pop_back_stack()`, `pop_back_stack_to(&route, inclusive)`, `navigate_up()` | Implemented | Each returns whether it popped. `pop_back_stack` can pop the last entry and leaves the host empty; `navigate_up` pops only when an entry lies beneath the top one. |
| `currentBackStackEntryAsState()`, `currentBackStack`, `previousBackStackEntry`, `getBackStackEntry(route)`, `NavBackStackEntry`, `NavDestination` | `NavController::current_route() -> Option<R>`, `back_stack() -> Vec<R>`, `composing_entry()`, `previous_back_stack_entry()`, `back_stack_entry(&R)`, `NavBackStackEntry` | Partial | A composable that calls `current_route` or `back_stack` recomposes when the back stack changes. `composing_entry` gives the entry whose screen composes the caller, also from a host nested in that screen; the entry lookups do not subscribe. `NavBackStackEntry::route` gives the route value, which stands for the arguments. There is no `NavDestination` object. |
| `previousBackStackEntry?.savedStateHandle?.set(key, value)` to return a result | `NavBackStackEntry::set_result(value)`, `result::<T>()`, `take_result::<T>()` | Renamed/reshaped | The value's type is its key, and any `Clone` type works. `result` recomposes its reader when the value changes; only result readers subscribe. Results live in memory: they go with a popped entry, stay with an entry that a `save_state` pop keeps, and a restored back stack starts without them. |
| Deep links: `navDeepLink`, `handleDeepLink` | -- | Absent | The one input from a link is the URL fragment that `BindBrowserHistory` reads on the web. |
| Compose Multiplatform `window.bindToNavigation(navController)` | `BindBrowserHistory(controller, saver)` | Implemented | Each navigation adds a history entry whose URL fragment is `#/` followed by the route's saved form, percent-encoded. The browser's Back pops the back stack and its Forward navigates to the entry's route again. A page opened or reloaded at a route's fragment shows that route over the start. Outside a browser the call does nothing, so an app can call it on every platform. |
| Predictive back in `NavHost` | `NavHost`, through `PredictiveBackHandler` and `SeekableCrossfade` | Implemented | The platform back gesture pops while a screen lies beneath the top one. While an interactive gesture moves (Android's predictive back, the iOS edge swipe), the screen beneath shows through as far as the gesture went; when the user lets go, the fade finishes or goes back from there. `PredictiveBackHandler` serves custom screens. |
| `viewModel()` and `hiltViewModel()` inside a destination, `NavBackStackEntry` as a `ViewModelStoreOwner` | `cranpose_coroflow::viewModel(key, factory)`, `ViewModelStore`, `ViewModelStoreOwner`, `ProvideViewModelStore` | Renamed/reshaped | Every entry has its own `ViewModelStore`. `viewModel` in a screen finds that screen's view models, which stay while other screens cover it and drop after it is popped and has faded out. The key is any `Hash + Eq` value, `()` when the store holds one model of the type. The factory receives the model's `viewModelScope` as a `MainScope`. Dropping a view model cancels that scope; there is no `onCleared` and no `ViewModelProvider.Factory`. |
| `NavBackStackEntry.savedStateHandle`, `SavedStateHandle` | `cranpose_coroflow::SavedStateHandle::current()` | Renamed/reshaped | `current()` returns the handle of the saveable scope where it is composed, which in a `NavHost` is the entry's: two visits of one route keep their keys apart, the values return with a restored back stack, and they go when the entry is popped. `get`, `set` and `get_mutable_state_flow` take values that round-trip through `Display` and `FromStr`. A key asked for with another type gives `SavedStateError::TypeMismatch`. |
| `rememberSaveable` in a destination, scoped to the back stack entry | `rememberSaveable` inside a screen; `NavHost` gives each entry a `SaveableScope` with `ProvideSaveableScope` | Implemented | A covered screen gets its state back, a popped one forgets it, and two visits of one route keep apart. |
| `rememberNavController()`, which Compose saves by default through `NavController.saveState` | `rememberSaveableNavController(key, start, saver)` | Renamed/reshaped | Plain `rememberNavController` is in memory only. The saveable form saves the back stack and the `rememberSaveable` state of each screen at each save boundary and restores them when it is next created, after a process restart too. The `Saver<R>` gives each route its saved form and may read forms that older builds wrote. Restoring keeps the entries beneath the first route the saver cannot read; when it cannot read the bottom one, or nothing was saved, the back stack starts at `start`. |
| `rememberSaveableStateHolder()`, `SaveableStateHolder.SaveableStateProvider(key) { }`, `removeState(key)` | `SaveableScope::current().child(name)`, `ProvideSaveableScope(scope, content)`, `SaveableScope::forget()` | Renamed/reshaped | No `SaveableStateHolder` type exists. Content under `ProvideSaveableScope` saves its `rememberSaveable` values under the child scope's name, and `forget` removes them at the next save boundary. `in_memory()` keeps a scope's values in memory only, and `forget_children_except(keep)` forgets everything in a scope apart from the child scopes it names. `NavHost` uses these calls for its entries. |
| `rememberLazyListState()`, `rememberLazyGridState()`, `rememberLazyStaggeredGridState()`, `rememberPagerState()` and `rememberScrollState()`, each saved by default through its `Saver` | `rememberSaveableLazyListState(key)`, `rememberSaveableLazyGridState(key)`, `rememberSaveableLazyStaggeredGridState(key)`, `rememberSaveablePagerState(key, initial_page)`, `rememberSaveableScrollState(key)` | Renamed/reshaped | Cranpose's plain remember functions do not save, and each saveable twin takes an explicit `&'static str` key. The position is saved in the `SaveableScope` of the screen at each save boundary and when the scroller leaves the composition, and restored when it is composed again. A list comes back where the user left it after a covering screen is popped and after a restart. A list and a staggered grid save the first visible item and its offset; a lazy grid comes back to the same item when the number of columns changes, as after a rotation. The pager saves the page without an offset fraction, and the scroll state saves its logical pixels. These functions live in `cranpose-navigation`, not in `cranpose-ui`. |
| `Saver<Original, Saveable>`, `listSaver`, `mapSaver`, `autoSaver`, `SaveableStateRegistry` | `Saver<T>::new(save, restore)`, `Saver::of_display()`, `Saver::list(save, restore)`, `Saver::map(save, restore)` | Partial | A saver converts a value to and from one `String`. `of_display` covers types that round-trip through `Display` and `FromStr`. `list` stores a value as a list of strings and `map` as named strings, as `listSaver` and `mapSaver` do; each string may hold any text, another saver's stored form too. The saver is explicit on purpose, because the stored form must stay readable by later builds, so there is no `autoSaver`. A registry type is absent. |

### Animation

| Compose | Cranpose | Verdict | Note |
|---|---|---|---|
| `animateFloatAsState`, `animateColorAsState` | Same names (`cranpose-animation::animation`) | Implemented | |
| `animateDpAsState`, `animateOffsetAsState`, `animateSizeAsState`, `animateRectAsState` | Same names | Implemented | Built on a generic `animateValueAsState<T: SpringScalar + PartialEq>`. The vector-converter core already existed as the `SpringScalar` + `Lerp` traits and was extended to `Dp`, `Sp`, `Point`, `Size` and `Rect` rather than duplicated; `animateFloatAsState`/`animateColorAsState` now delegate to the same function. |
| `animateIntAsState`, `animateIntOffsetAsState`, `animateIntSizeAsState` | -- | Absent, deliberately | Cranpose has no `Int`-typed geometry vocabulary, so these would be unused surface. |
| `tween(durationMillis, easing)`, `spring(dampingRatio, stiffness)` | Same names | Implemented | |
| `Animatable<T>` | Same name | Implemented | |
| `rememberInfiniteTransition()` / `InfiniteTransition.animateFloat` | Same names | Implemented | Animated state uses structural equality, matching Compose: frame callbacks keep time, but equivalent values do not invalidate readers. `AnimationSpec::stepped(durationMillis, stepCount)` provides discrete updates for render work that does not need a new value every display frame. |
| `updateTransition(targetState)` / `Transition<S>` / `transition.animateFloat { }` | Same names (`animateValue`/`animateFloat`/`animateDp`/`animateColor`) | Implemented, different settled semantics | Each child owns an `Animatable` and reports its own running state; `Transition::is_running()` is true until every child settles. Compose instead drives children from one shared total-duration play-time. Registration reuses `InfiniteTransition`'s disposable-effect-by-pointer-identity pattern. |
| `DecayAnimationSpec`, `FloatDecayAnimationSpec` (fling physics contract) | `cranpose-animation::decay_spec::{FloatDecayAnimationSpec, ExponentialDecaySpec}` | Implemented, deliberately different physics | The trait matches; the concrete spec does not. `SplineBasedDecaySpec` (a port of `android.widget.Scroller`'s math) was replaced in #541 (landed after this doc's data was last generated, caught while re-verifying for this refresh) by `ExponentialDecaySpec` + `IOS_DECELERATION_RATE_{FAST,NORMAL}`, physics measured directly from a real iOS `UIScrollView` -- a deliberate divergence (Cranpose targets iOS feel, not an Android-physics port), not a gap. |
| `AnimatedContent(targetState, modifier, transitionSpec, contentAlignment, label, contentKey) { state -> }` | `AnimatedContent(target_state, transition_spec, content)`, and `SeekableAnimatedContent(target_state, seek, transition_spec, content)` | Partial | `transition_spec` receives references to the state that goes and the state that comes, and returns a `ContentTransform`. The first content shows at rest. Content that leaves stays composed until its exit ends, and a change back while it leaves brings it back from where it was. A running change moves layers and recomposes nothing. `AnimatedContentWithKey(target_state, content_key, transition_spec, content)` is `contentKey`: a target with the key of the content shown changes it in place, without a transition. Absent: `modifier`, `contentAlignment`, `label`, the `AnimatedContentTransitionScope` receiver with `slideIntoContainer` and `slideOutOfContainer`, and `ContentTransform.targetContentZIndex`. |
| `enter togetherWith exit`, `ContentTransform`, `using SizeTransform(clip, sizeAnimationSpec)` | `EnterTransition::together_with(exit)`, `ContentTransform { enter, exit, size_transform }`, `ContentTransform::using(SizeTransform { clip, animation })` | Partial | `SizeTransform` carries one `AnimationType`, where Compose's `sizeAnimationSpec` is chosen from the initial and the target size. Without a size transform the container stays as large as the largest content during the change. |
| `fadeIn()`, `fadeOut()`, `slideInVertically { }`, `slideInHorizontally { }`, `slideOutVertically { }`, `slideOutHorizontally { }`, `scaleIn()`, `scaleOut()`, combined with `+` | `fade_in()`, `fade_out()`, `slide_in_vertically(fraction)`, `slide_in_horizontally(fraction)`, `slide_out_vertically(fraction)`, `slide_out_horizontally(fraction)`, `scale_in(scale)`, `scale_out(scale)`, combined with `+` | Partial | A slide offset is a fraction of the content's own size, where Compose takes a lambda over the measured size. The animation spec is a builder method, `with_animation(spec)`, and the effects of a combined transition share one spec. The expand and shrink transitions, `slideIn` with an offset lambda and the `transformOrigin` argument of `scaleIn` are absent. |
| `AnimatedVisibility(visible, modifier, enter, exit, label) { }` | `AnimatedVisibility(visible, enter, exit, content)` | Partial | Built on `SeekableAnimatedContent`. Content that is visible on first composition appears without animation; when `visible` turns `false` the content stays composed for the whole exit, keeping its size, and leaves the composition when the exit completes; toggling mid-transition retargets the running animation. Absent: `modifier`, `label`, `MutableTransitionState` and the `Transition.AnimatedVisibility` form. |
| `Crossfade(targetState, modifier, animationSpec, label, contentKey) { state -> }` | `Crossfade(target_state, animation, content)` | Partial | Built on `AnimatedContent` with `fade_in` and `fade_out`. `animation` is a required argument, where Compose defaults it. Both compositions stay alive during the fade, each in an alpha layer, and the outgoing one leaves when its fade ends. The first content shows without animation, and retargeting mid-fade keeps every in-flight content fading. `content` receives each in-flight state, so it must not capture the current target. Absent: `modifier`, `label` and `contentKey`. |
| `SeekableTransitionState` (`seekTo`, `snapTo`, `animateTo`) | `SeekableAnimatedContent`, `SeekableCrossfade`, `ContentSeek { toward, fraction }` | Cranpose's own shape | While `seek` is set, the `toward` content shows at `fraction` and the current content at `1.0 - fraction`. When the gesture ends, the target either becomes `toward` or stays, and both contents animate on from where the gesture left them. `NavHost` uses it for the predictive back gesture. |
| `SharedTransitionLayout { }` with the `SharedTransitionScope` receiver | `SharedTransitionLayout(modifier, content)` | Partial | A box that keeps the `SharedElement`s composed in it and draws the elements in flight above its content. It has no scope receiver: a `SharedElement` finds its layout through a composition local. |
| `Modifier.sharedElement(rememberSharedContentState(key), animatedVisibilityScope)` | `SharedElement(key, modifier, content)` | Partial | A composable that wraps its content in a box with `modifier`; it is not a modifier. Two contents of an `AnimatedContent` pair their elements by string `key`. `Crossfade` and the screens of a `NavHost` are built on `AnimatedContent`, so the same pairing applies to them. While the change runs and both contents hold the key, both copies hide in place and the layout draws the incoming content's copy above everything, at a box between the two resting boxes. Its position and size follow the incoming content's progress linearly, also when a gesture seeks it. Each frame measures that box again, and nothing recomposes. The flying copy is the incoming content composed once more, so state it remembers starts afresh. Outside a layout, or outside a change, `SharedElement` is `content` in a box with `modifier`. |
| `Modifier.sharedBounds(..)`, `boundsTransform`, `renderInSharedTransitionScopeOverlay`, `clipInOverlayDuringTransition`, `skipToLookaheadSize`, `SharedTransitionScope.isTransitionActive`, sharing through an `AnimatedVisibilityScope` | -- | Absent | Only `SharedElement` exists. The box follows the incoming content's progress, so there is no `BoundsTransform` spring. The flying copy is the incoming content alone, so content that differs between the two sides cannot cross-fade inside shared bounds. An `AnimatedVisibility` composes one content at a time, so it has no second copy to pair with. |

### Text

| Compose | Cranpose | Verdict |
|---|---|---|
| `TextStyle`, `AnnotatedString` | `cranpose-ui::text::style::TextStyle` (authored style, Compose-parity) and `cranpose-ui-graphics::typography::DrawTextStyle` (resolved draw primitive) | Implemented | The two types are a real architectural split, not drift -- see Findings. The graphics one was renamed to `DrawTextStyle` so they no longer share a name. |
| `BasicTextField`, `TextFieldState` | Same names | Implemented |
| `SelectionContainer`, `DisableSelection` | Same names | Implemented | Mouse drag, double and triple click, touch long press and drag, Ctrl/Cmd+C, Ctrl/Cmd+A and Escape. Touch selection exposes draggable start and end handles, a loupe, and Copy / Select all actions. |

### Testing

| Compose (`androidx.compose.ui.test`) | Cranpose (`cranpose-testing`) | Verdict |
|---|---|---|
| `ComposeTestRule`, `SemanticsNodeInteraction`-style finder/assertion chain | `ComposeTestRule`, `ElementFinder`, `FinderQuery`, `assert_*` fns | Implemented, different finder model |
| `Modifier.testTag` + `onNodeWithTag` | `Modifier::test_tag` + `find_by_tag` | Implemented, see Findings |
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
separate name). `Modifier.testTag` is unchanged; Cranpose has matched it
since `Modifier::test_tag`. The `remember`/`LaunchedEffect`
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
is supplied separately. Current `clickable` adds reader activation semantics
and keyboard focus through its shared pressable helper; applications still
provide appropriate names and roles.

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

### A dedicated `testTag` finder -- resolved

Compose's `Modifier.testTag("x")` + `onNodeWithTag("x")` is the standard,
locale-independent way to address a specific UI element in a test.
`Modifier::test_tag("x")` names a node, and `PlacedSemanticsNode::find_by_tag`,
the headless robot's `find_by_tag` and the desktop robot's `find_by_tag` /
`click_by_tag` find it. Screen readers never receive the tag. The text and
geometry finders stay for tests that read what a user reads.

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
