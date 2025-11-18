# Agent Notes for compose-rs-proposal

- This repo is a Rust workspace; run `cargo fmt` and `cargo clippy --all-targets --all-features` before committing major code changes.
- Most code lives in the `compose-*` crates. Add nested `AGENTS.md` files for crate-specific guidance if necessary.
- no unsafe
- cargo test > 1.tmp 2>& # then read and fix