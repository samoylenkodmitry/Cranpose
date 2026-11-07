// How many items of each kind a scene holds up to a place in its draw
// order, as the scene works with them and as a record keeps them.

/// How many items of each kind a scene holds where a run of its items starts
/// or stops.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ItemCounts {
    pub order: usize,
    pub shapes: usize,
    pub images: usize,
    pub texts: usize,
    pub shadows: usize,
}

impl ItemCounts {
    /// How many items of each kind lie from `start` up to `self`.
    pub(crate) fn since(self, start: Self) -> Self {
        Self {
            order: self.order - start.order,
            shapes: self.shapes - start.shapes,
            images: self.images - start.images,
            texts: self.texts - start.texts,
            shadows: self.shadows - start.shadows,
        }
    }

    // The counts after a run of `removed` items before them gave way to a
    // run of `added`.
    pub(crate) fn shifted(self, removed: Self, added: Self) -> Self {
        Self {
            order: self.order - removed.order + added.order,
            shapes: self.shapes - removed.shapes + added.shapes,
            images: self.images - removed.images + added.images,
            texts: self.texts - removed.texts + added.texts,
            shadows: self.shadows - removed.shadows + added.shadows,
        }
    }

    /// The counts of a run copied from where `from` began in one scene to
    /// where `to` begins in another.
    pub(crate) fn moved(self, from: Self, to: Self) -> Self {
        Self {
            order: to.order + self.order.saturating_sub(from.order),
            shapes: to.shapes + self.shapes.saturating_sub(from.shapes),
            images: to.images + self.images.saturating_sub(from.images),
            texts: to.texts + self.texts.saturating_sub(from.texts),
            shadows: to.shadows + self.shadows.saturating_sub(from.shadows),
        }
    }
}

// Item counts as a record keeps them: a scene holds fewer than 2^32 items,
// so each count takes half the room of a `usize` on 64-bit targets.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PackedCounts([u32; 5]);

impl PackedCounts {
    // The counts as the scene works with them.
    #[inline]
    pub(crate) fn get(self) -> ItemCounts {
        let [order, shapes, images, texts, shadows] = self.0.map(|count| count as usize);
        ItemCounts {
            order,
            shapes,
            images,
            texts,
            shadows,
        }
    }
}

impl From<ItemCounts> for PackedCounts {
    #[inline]
    fn from(counts: ItemCounts) -> Self {
        let ItemCounts {
            order,
            shapes,
            images,
            texts,
            shadows,
        } = counts;
        Self([order, shapes, images, texts, shadows].map(as_u32))
    }
}

// `index`, a place in a scene's lists, as the 32 bits a record keeps it in.
#[inline]
pub(crate) fn as_u32(index: usize) -> u32 {
    debug_assert!(
        u32::try_from(index).is_ok(),
        "a scene's list outgrew 32 bits"
    );
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a scene's lists hold far fewer than 2^32 items, records and entries"
    )]
    let index = index as u32;
    index
}
