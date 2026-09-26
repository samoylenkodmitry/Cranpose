# IntelliJ IDEA and RustRover

[Cranpose](https://github.com/samoylenkodmitry/cranpose-idea) provides a Cranpose-rendered
tool window, Cargo target controls, composable navigation, completions and interactive
previews. The [host template](https://github.com/samoylenkodmitry/cranpose-intellij-plugin-template)
is for authors building their own IntelliJ plugins with Cranpose.

## Preview an application

Enable `cranpose/embed` alongside your desktop features. When an IDE launches the
binary with both `CRANPOSE_EMBED_ADDRESS` and `CRANPOSE_EMBED_TOKEN`, desktop
`AppLauncher::run` and `try_run` use the embedded renderer. The same binary opens
a normal desktop window when these variables are absent. No alternate root
composable or generated source file is needed.

The plugin builds the selected Cargo binary or example with its required features
and the direct Cranpose dependency's `embed` feature, reads the executable path
from Cargo's JSON artifact stream, then launches that executable. A custom target
directory, dependency alias and paths containing spaces are supported.

This automatic dispatch was added after 0.1.164. On 0.1.164, select the endpoint
explicitly, as the host template does:

```rust,no_run
use cranpose::{AppLauncher, embed::EmbedEndpoint};

fn main() {
    let app = AppLauncher::new();
    match EmbedEndpoint::from_env() {
        Some(endpoint) => app.run_embedded(endpoint, || {}),
        None => app.run(|| {}),
    }
}
```

## Inspect an embedded application

The existing host-message transport reserves two versioned channels:

| Direction | Channel | Payload |
| --- | --- | --- |
| Host → app | `cranpose.inspector.v1.request` | Empty string |
| App → host | `cranpose.inspector.v1.snapshot` | UTF-8 primary-surface report |

The report includes the current layout tree, rendered scene and screen summary.
It is produced only when requested, so inspection adds no recurring snapshot work
to the frame loop. Reports may include application text; the plugin keeps them
local. The report is for display, and hosts should not parse its human-readable
format as a stable schema.

The socket uses the existing authenticated loopback protocol. Inspection requires
a framework revision containing these channels; older applications continue to
render, and the plugin reports when inspection is unavailable.
