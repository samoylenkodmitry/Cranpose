//! Properties few values set, kept in a box that a value holds only while
//! one of them is set, so a value without any stays small.

/// Properties few values set, with an instance that has none of them set.
pub trait RareProperties: PartialEq + Default + 'static {
    /// The instance with every property at its default: what a value that
    /// holds no box reports.
    const EMPTY: &'static Self;
}

/// The properties `slot` holds, or [`RareProperties::EMPTY`] when it holds
/// none.
pub fn rare<T: RareProperties>(slot: &Option<Box<T>>) -> &T {
    slot.as_deref().unwrap_or(T::EMPTY)
}

/// Stores `properties` in `slot`: no box when every property is at its
/// default, and the box `slot` already holds when it holds one.
pub fn set_rare<T: RareProperties>(slot: &mut Option<Box<T>>, properties: T) {
    if properties == *T::EMPTY {
        *slot = None;
    } else if let Some(held) = slot {
        **held = properties;
    } else {
        *slot = Some(Box::new(properties));
    }
}

/// Changes the properties `slot` holds in place, then stores them the way
/// [`set_rare`] does.
pub fn update_rare<T: RareProperties>(slot: &mut Option<Box<T>>, update: impl FnOnce(&mut T)) {
    let mut properties = slot.as_deref_mut().map(std::mem::take).unwrap_or_default();
    update(&mut properties);
    set_rare(slot, properties);
}
