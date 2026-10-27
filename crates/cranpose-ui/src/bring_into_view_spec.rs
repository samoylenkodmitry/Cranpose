use cranpose_core::{
    CompositionLocal, composer_context::try_with_composer, compositionLocalWithComputedDefaultOf,
};
use cranpose_foundation::BringIntoViewSpec;

use crate::{UiModeType, local_configuration};

// Compose's pivot spec on Android TV.
const TV_PIVOT: BringIntoViewSpec = BringIntoViewSpec::Pivot {
    parent_fraction: 0.3,
    child_fraction: 0.0,
};

/// CompositionLocal carrying the [`BringIntoViewSpec`] of the scroll
/// containers inside it, as `LocalBringIntoViewSpec` does in Compose. Without
/// a provider it is the pivot spec on a TV, which keeps the focused item 30%
/// into the container, and [`BringIntoViewSpec::Default`] elsewhere.
pub fn local_bring_into_view_spec() -> CompositionLocal<BringIntoViewSpec> {
    crate::environment_locals::cached_local(
        |locals| &locals.bring_into_view_spec,
        || {
            compositionLocalWithComputedDefaultOf(|scope| {
                if scope.current_value(&local_configuration()).ui_mode_type
                    == UiModeType::Television
                {
                    TV_PIVOT
                } else {
                    BringIntoViewSpec::Default
                }
            })
        },
    )
}

// The spec where a scroll modifier is built. A modifier built outside
// composition has no locals to read and takes the default.
pub(crate) fn current_bring_into_view_spec() -> BringIntoViewSpec {
    try_with_composer(|_| local_bring_into_view_spec().current()).unwrap_or_default()
}
