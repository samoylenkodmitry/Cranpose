#![allow(
    clippy::disallowed_methods,
    reason = "a build script reads cargo's environment"
)]

use cranpose_capabilities::declare;

fn main() {
    declare(&[]).emit();
}
