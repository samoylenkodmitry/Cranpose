# Agent Notes for compose-rs-proposal

- no unsafe
- cargo test > 1.tmp 2>& # then read and fix all
- cargo clippy > 2.tmp 2>& # then read and fix all
- cargo fmt
- Use `cargo add <crate>` to add dependencies.
- Use `cargo upgrade` to upgrade dependencies.
- Use `anyhow` for error handling in application code; use `thiserror` for library code.
- Write unit tests for all public functions and methods.
- Write integration tests in the `tests` directory.
- Follow idiomatic Rust naming conventions (snake_case for variables and functions, CamelCase for types and traits).
- CamelCase for #[composable] functions
- Use `Result<T, E>` for functions that can fail; prefer specific error types over `Box<dyn Error>`.
- Use `Option<T>` for values that can be absent.
- Use `async`/`await` for asynchronous code; prefer `tokio` as the async runtime.
- Document any architectural decisions in a `DECISIONS.md` file if applicable.