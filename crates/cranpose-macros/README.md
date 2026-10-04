# Cranpose macros

Procedural macros used by Cranpose. Applications normally import them through
the `cranpose` crate.

`#[composable]` marks a function for composition. The macro analyzes its
parameters and body and emits the runtime bookkeeping needed to retain values
and callbacks across recomposition. Its expansion is an implementation detail;
the exact generated Rust and runtime strategy can change with the macro.

`#[preview]` registers a parameterless composable for IDE previews with the
`preview` feature. Apply `#[preview]` and `#[composable]` together. Options are
`name`, `group`, `width`, `height` and `dark`.
