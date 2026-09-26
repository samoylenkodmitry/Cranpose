# IntelliJ IDEA and RustRover

[Cranpose for IntelliJ](https://github.com/samoylenkodmitry/cranpose-idea) adds
component previews beside Rust source, a live layout inspector, Cargo controls,
saved run configurations and composable navigation. Its project tool window is
rendered with Cranpose. The [host template](https://github.com/samoylenkodmitry/cranpose-intellij-plugin-template)
provides the embedding layer for other IntelliJ plugins.

## Register component previews

Enable the `cranpose/preview` feature alongside your desktop features. Add a
parameterless fixture around components that require arguments:

```rust,ignore
use cranpose::{AppLauncher, Modifier, Text, TextStyle, composable};

#[cranpose::preview(name = "Compact", group = "Cards", width = 360, height = 180)]
#[cranpose::preview(name = "Dark", group = "Cards", width = 360, height = 180, dark = true)]
#[composable]
fn GreetingPreview() {
    Text("Hello", Modifier::empty(), TextStyle::default());
}

fn main() {
    AppLauncher::new().run(GreetingPreview);
}
```

The attribute registers compiler-provided function names, source files and lines.
It accepts `name`, `group`, `width`, `height` and `dark`. Dimensions must be
between 1 and 8192 logical pixels. Functions must be synchronous, take no
arguments or generic parameters, and return `()`. The attribute can be repeated
to define several viewport/theme variants.

The IDE builds a binary or example with its required Cargo features and the
direct Cranpose dependency's `preview` feature. It reads the executable path
from Cargo's JSON artifact stream, including custom target directories and
dependency aliases. A library can expose its fixtures through a small example
binary that links the library and launches `AppLauncher`.

## Run the same application inside and outside the IDE

When both `CRANPOSE_EMBED_ADDRESS` and `CRANPOSE_EMBED_TOKEN` are present,
desktop `AppLauncher::run` and `try_run` use the embedded renderer. Without
them, the binary opens its normal desktop window.

In an embedded preview build, `CRANPOSE_PREVIEW` selects an exact descriptor ID
or an unambiguous function/display name. An absent value renders the normal
application root. A missing or ambiguous selector is reported as a launch error.
The IDE uses the full descriptor ID to distinguish variants.

The `embed` feature alone supports application embedding without component
registration or source instrumentation. These additions follow version 0.1.164;
use a framework revision containing the preview feature until it is released.

## Inspection protocol

The authenticated loopback host-message transport carries these channels:

| Direction | Channel | Payload |
| --- | --- | --- |
| App → host, after hello | `cranpose.previews.v1` | JSON array of compiled preview descriptors |
| Host → app | `cranpose.inspector.v2.request` | Unsigned request ID as decimal text |
| App → host | `cranpose.inspector.v2.snapshot` | JSON layout snapshot |

A snapshot includes its schema and request ID, capture duration, a truncation
flag, and primary-surface nodes in preorder. Node identities include allocation
generations. Each node has a parent ID, logical bounds, type, text, ordered
modifier properties, and composable source origins. Hosts can match selection
across captures by identity, discard old responses, and pick the frontmost
matching node by reverse traversal.

The host request caps a snapshot at 10,000 nodes. Captures happen on demand.
The `preview` feature retains modifier metadata and composition origins;
ordinary builds do not retain that data. Origins describe composable definitions,
including during partial recomposition. Custom nodes may have no origin.

The human-readable `cranpose.inspector.v1.request` /
`cranpose.inspector.v1.snapshot` report remains available for text diagnostics.
Use the structured channel for tree navigation and properties.
