use super::{DimensionConstraint, Modifier, inspector_metadata};
use crate::modifier_nodes::FillElement;

fn fill_modifier(
    fraction: f32,
    full: fn(&crate::thread_state::UiThreadState) -> &std::cell::OnceCell<Modifier>,
    element: fn(f32) -> FillElement,
) -> Modifier {
    if fraction == 1.0 {
        crate::thread_state::with(|state| {
            full(state)
                .get_or_init(|| Modifier::with_element(element(1.0)))
                .clone()
        })
    } else {
        Modifier::with_element(element(fraction))
    }
}

impl Modifier {
    /// Have the content fill the maximum available width.
    ///
    /// The `fraction` parameter allows filling only a portion of the available width (0.0 to 1.0).
    ///
    /// Matches Kotlin: `Modifier.fillMaxWidth(fraction: Float)`
    ///
    /// Example: `Modifier::empty().fill_max_width()`
    pub fn fill_max_width(self) -> Self {
        self.fill_max_width_fraction(1.0)
    }

    /// Fill a fraction of the maximum available width.
    ///
    /// Example: `Modifier::empty().fill_max_width_fraction(0.5)`
    pub fn fill_max_width_fraction(self, fraction: f32) -> Self {
        let clamped = fraction.clamp(0.0, 1.0);
        let modifier = fill_modifier(clamped, |state| &state.fill_width, FillElement::width)
            .with_inspector_metadata(inspector_metadata("fillMaxWidth", move |info| {
                info.add_dimension("width", DimensionConstraint::Fraction(clamped));
            }));
        self.then(modifier)
    }

    /// Have the content fill the maximum available height.
    ///
    /// The `fraction` parameter allows filling only a portion of the available height (0.0 to 1.0).
    ///
    /// Matches Kotlin: `Modifier.fillMaxHeight(fraction: Float)`
    ///
    /// Example: `Modifier::empty().fill_max_height()`
    pub fn fill_max_height(self) -> Self {
        self.fill_max_height_fraction(1.0)
    }

    /// Fill a fraction of the maximum available height.
    ///
    /// Example: `Modifier::empty().fill_max_height_fraction(0.5)`
    pub fn fill_max_height_fraction(self, fraction: f32) -> Self {
        let clamped = fraction.clamp(0.0, 1.0);
        let modifier = fill_modifier(clamped, |state| &state.fill_height, FillElement::height)
            .with_inspector_metadata(inspector_metadata("fillMaxHeight", move |info| {
                info.add_dimension("height", DimensionConstraint::Fraction(clamped));
            }));
        self.then(modifier)
    }

    /// Have the content fill the maximum available size (both width and height).
    ///
    /// The `fraction` parameter allows filling only a portion of the available size (0.0 to 1.0).
    ///
    /// Matches Kotlin: `Modifier.fillMaxSize(fraction: Float)`
    ///
    /// Example: `Modifier::empty().fill_max_size()`
    pub fn fill_max_size(self) -> Self {
        self.fill_max_size_fraction(1.0)
    }

    /// Fill a fraction of the maximum available size.
    ///
    /// Example: `Modifier::empty().fill_max_size_fraction(0.8)`
    pub fn fill_max_size_fraction(self, fraction: f32) -> Self {
        let clamped = fraction.clamp(0.0, 1.0);
        let modifier = fill_modifier(clamped, |state| &state.fill_size, FillElement::size)
            .with_inspector_metadata(inspector_metadata("fillMaxSize", move |info| {
                info.add_dimension("width", DimensionConstraint::Fraction(clamped));
                info.add_dimension("height", DimensionConstraint::Fraction(clamped));
            }));
        self.then(modifier)
    }
}
