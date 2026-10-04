# Cranpose gaps and limits

This file lists current framework limits. [Release readiness](docs/release_readiness.md)
lists acceptance criteria. [Compose API parity](docs/compose_api_parity.md) and
[platform capabilities](docs/capability_parity.md) describe API scope. Git history
holds completed plans. Each downstream project owns its application issues.

## Release and performance

- The 0.9 line permits API changes. The release ledger lists the API, platform,
  accessibility, consumer and device evidence required for 1.0.
- Device results apply to the recorded revision, app, route and temperature.
  The [mobile measurements](docs/mobile_watch_performance.md) and
  [desktop VSync report](docs/desktop_vsync_performance.md) describe specific
  experiments. Each release candidate requires fresh device measurements.
- `python3 scripts/public_api_test_coverage.py` reports public function names
  absent from test source and a ratio of names with references. Use
  integration and end-to-end tests to check observable behavior and corner cases.

## Framework limits

- The `#[composable]` macro keeps generated names separate from caller locals.
  A module constant with a generated name such as
  `__cranpose_caller_key` can resolve as a constant pattern. Reserve generated
  `__cranpose` and `__composer` names for the macro. See the
  [macro guide](crates/cranpose-macros/README.md).
- Native HTTP requires `cranpose-services/http-native`. Enable the feature on
  the service crate. See the [service features](crates/cranpose-services/Cargo.toml).
- Update discovery and installation have separate capabilities. Desktop and
  iOS support discovery through an HTTP backend. Application installation
  requires a platform distribution channel. Check `AppUpdateCapabilities`
  before each operation.
- A media provider can stall inside a blocking `Read`. The spool downloader
  thread waits for the provider to return. Timeouts and cancellation release
  the spool consumers. See the [spool source](crates/cranpose-media/src/spool.rs).
- Duration probes use file paths or HTTP sources. A document-provider URI
  acquires duration when playback opens the media. See the
  [decoder](crates/cranpose-media/src/decode.rs).

## Validation limits

- Some robot runners require X11 tools, Python image libraries or a surface
  with frame presentation. The [robot guide](docs/ROBOT_TESTING.md) lists
  requirements. Review capability skips in each result.
- Host locks coordinate commands through the shared lock protocol. Other jobs
  can affect measurements. Follow the [host workflow](docs/agent-workflows.md#builds-and-shell)
  and retain load and temperature evidence for each comparison.
