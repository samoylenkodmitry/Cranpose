// Whether a pointer dispatch stops at the first target that consumes the
// event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stop {
    AtConsumer,
    Never,
}
