# Live UI runtime

`cranpose-live` runs an editable UI document inside the normal Cranpose composition
runtime. After the first native build, supported UI edits change that document
without invoking Cargo, rustc, or a linker. Rust source translation and structured
agent edits use the same registry, validation, revision, state bindings, and native
widgets.

## Try it

From the repository root:

```sh
cargo run -p cranpose-live-demo
```

Edit [the demo screen](../apps/live-demo/screen.rs) and save. Change `Column` to
`Row`, change labels, add/remove calls, or change `counter.add(1)` to
`counter.add(5)`. The window stays open and the counter retains its value.
An optional first argument selects another source file containing `fn Screen()`.

The watcher reads saves, including atomic editor replacements. The status line
shows translation/validation/commit time; it does not measure frame presentation.

The same process accepts one JSON request per stdin line and writes one JSON
response per stdout line. For example:

```json
{"method":"catalogue"}
{"method":"snapshot"}
{"method":"dispatch","node":"increment","event":"on_click"}
{"method":"patch","patch":{"base_revision":0,"edits":[{"kind":"set_argument","target":"increment","name":"label","value":{"kind":"literal","value":"Changed by an agent"}}]}}
```

Use the revision returned by `snapshot` as `base_revision`. Each successful edit
advances it; equivalent documents and empty patches do no work. A stale edit or an invalid program is rejected atomically, preserving
the last good UI. Dispatching an action does not advance the document revision.

## Automatic composables

Keep the ordinary annotation:

```rust
#[composable]
fn CounterLabel(count: i64) {
    Text(format!("Count: {count}"), Modifier::empty(), TextStyle::default());
}
```

In a crate with a normal, non-optional `cranpose-live` dependency, the existing
macro automatically emits a linked registration. There is no extra annotation,
registration list, or `composable(live)` option. Use `Registry::discover()` once
when creating a session. Renamed runtime dependencies are resolved by the macro.

Each catalogue entry includes its qualified name, original Rust signature,
documentation, supported parameters and content slot. Callable adapters currently
support `String`, `i64`, `f64`, `bool`, zero-argument `impl Fn…` callbacks, and
a final callback named `content`. They require a synchronous, non-generic
unit-returning function.

Other signatures remain native Rust and appear in the catalogue with
`unavailable` explaining why the interpreter cannot invoke them. Composables in
dependencies that do not depend on the live runtime are not automatically added.
Runtime dependencies only in `dev-dependencies`, optional dependencies, and
target-specific runtime dependencies are outside this first implementation.

The runtime supplies `widgets::Text(text)`, `Column(content)`, `Row(content)`
and `Button(label, on_click)`. These are small live adapters around Cranpose's
native widgets. They intentionally have simpler signatures than the full widget
APIs. The full framework API has not yet been exported.

## View models

Application code owns the model instance and its lifecycle:

```rust
#[live_api]
impl Counter {
    #[live(state)]
    fn count(&self) -> StateFlow<i64> {
        self.count.as_state_flow()
    }

    #[live(action)]
    fn add(&self, amount: i64) {
        self.count.set(self.count.value().saturating_add(amount));
    }
}

registry.bind("counter", Rc::clone(&counter))?;
```

State getters expose `StateFlow<T>` for the supported scalar types. Rendering uses
Cranpose's composition-scoped `collectAsState` integration. Removing a reader
removes its collection with the composition. Actions are synchronous `&self`
methods returning unit. Their arguments are evaluated when the event occurs, so
an action sees current flow values. The interpreter never recreates the bound
model during a document update.

## Source and document model

The source frontend translates one parameterless top-level function. It supports
registered calls, nested content closures, scalar literals, `.into()`,
`.to_string()`, state reads such as
`counter.count().collectAsState().get()`, and action closures such as
`|| counter.add(1)`. A direct registered state getter is also accepted as a
live-expression shorthand. Names are resolved against the catalogue, not Rust
imports; qualified names disambiguate duplicate short names.

Use `key("stable-id", || Widget(...))` for editable nodes that need stable identity.
Generated identities use component name and sibling ordinal. Explicit keys retain
native remembered state across sibling insertions under the same parent. Moving a
node to another parent or changing its native component can reset its local
composition state; application-owned view-model state remains bound.

The JSON document describes component nodes, named argument expressions and
children. `replace` replaces a subtree and `set_argument` changes an argument.
A patch applies all its edits together. Component and API references are resolved
when a program is committed, avoiding catalogue searches during rendering.
Document snapshots share immutable subtrees; edits copy only the affected paths.

Set `Registry::limits` before constructing the session to bound source size,
node count, nesting, transaction length, and retained diagnostics. The catalogue
reports those limits to editors and agents.

This is a UI interpreter with native escape hatches. Arbitrary Rust statements,
local bindings, loops, branches, imports/alias resolution, custom value types,
general closures, and new native implementations are not interpreted. Unsupported
source returns a diagnostic. Adding or changing a native adapter or model method
still requires a build.

## Editor and agent integration

`Session::handle(Request)` is the transport-independent entry point on the UI
thread. Commands are `catalogue`, `snapshot`, `source`, `patch`, and `dispatch`.
A `source` request carries `base_revision`, `source`, and `function`.

An editor can send supported source edits through this entry point. An agent can
read the catalogue and snapshot, then submit a revision-checked patch. Natural
language interpretation belongs to the agent; the running application supplies
the actual callable contract and executes the resulting document.

The desktop demo implements file watching, stdin commands, and the embedding host
transport through `host::HostedSession` and `host::LiveHost`. It runs both standalone
and embedded. The Studio integration needs the matching `cranpose-idea` live-runtime
branch; released plugin versions do not discover these documents yet.

Studio supplies `CRANPOSE_DEV_SOURCE_MAP` to map its private build checkout back to
the user's files. `host::original_source_path` applies that mapping before the demo
starts its watcher. Saving the dedicated `screen.rs` document therefore reaches the
running interpreter directly. The runner recognizes its `cranposeLive` registration
and excludes that document from native recompilation. Native Rust source files keep
the normal build/reload behavior. This path currently updates on save; unsaved
structural edits are not delivered by Studio.

An application using `HostedSession` must install a watcher for its dedicated UI
document and apply saves to the same `Session`. Do not register a file that also
contains native Rust implementations: it is owned by the interpreter until restart.
The demo is a complete example of that setup. No composable annotation changes.

The authenticated embedding transport uses three channels:

| Channel | Direction | Contents |
| --- | --- | --- |
| `cranpose.live.v1.ready` | app → host | Protocol, document ID, source file/function, revision, limits |
| `cranpose.live.v1.request` | host → app | `{ "id": 1, "document": "…", "request": { "method": "catalogue" } }` |
| `cranpose.live.v1.response` | app → host | ID, document, current revision, response or error |

Hosts assign increasing request IDs per document. A replay of the latest request
returns the cached response without repeating an action. Older IDs are rejected.
Source/patch commands still require their document's `base_revision`; invalid or
stale edits preserve the last good UI and model state. Studio checkpoints retain
request sequencing when its UI reconnects to the same application.

The demo does not expose an MCP server or embed an AI model. Agents can use the
same runtime command protocol through a host integration or the stdin transport.

## Validation and measurement

```sh
cargo test -p cranpose-live
cargo test -p cranpose-live --test reload_latency -- --ignored --nocapture
cargo clippy -p cranpose-live -p cranpose-live-demo --all-targets -- -D warnings
```

The latency harness alternates two 101-node source programs and applies argument
patches in one process. It reports commit latency, excluding compilation,
rendering and presentation. Set `CRANPOSE_LIVE_SAMPLES` to change the sample count.
