# Layout and Behavioral Parity Benchmark

This suite compares Jetpack Compose Desktop and Cranpose on a single-screen 20-zone layout atlas (462 tagged nodes) at `1.0x` and `1.75x` density. It also runs a behavioral no-panic stress suite (20 Cranpose scenarios and 8 Compose scenarios).

## Structure

- `compose-app/`: Jetpack Compose Desktop reference app (`ImageComposeScene`) using `tools/compose-twin/gradlew`.
- `cranpose-app/`: Standalone Cranpose app (`AppShell<PixelsRenderer>`) rendering the same 20 zones and behavioral scenarios.
- `divergences.toml`: Machine-readable ratchet ledger separating expected engine differences from unexpected bugs linked to GitHub issues.
- `compare.py`: Runs both apps, compares layout bounds and rendered PNGs, checks `divergences.toml`, and writes `summary.md` plus `parity_report.json`.
- `tests/test_compare.py`: Unit tests for `compare.py` and ledger schema validation.

## Running locally

```sh
just layout-parity
```

The command writes reports, rendered PNGs, side-by-side composites, and diff heatmaps to `target/layout-parity/`.

To run only the Cranpose clippy gate and ledger unit tests without invoking Gradle:

```sh
just layout-parity-check
```

## Updating the divergence ledger

`compare.py --check-ledger` enforces a two-way ratchet:

1. Any new layout divergence, non-finite bound, missing node, worsened delta, or stress panic fails the run.
2. Any tracked unexpected bug that starts passing also fails the run until its entry is removed from `divergences.toml` and `docs/compose_api_parity.md`.

Every change to `divergences.toml` must keep the **Layout and behavioral parity** section of [docs/compose_api_parity.md](../../docs/compose_api_parity.md) synchronized.
