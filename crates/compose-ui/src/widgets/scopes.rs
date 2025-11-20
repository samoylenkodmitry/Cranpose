//! Scope traits and implementations for Box, Column, and Row

use crate::modifier::Modifier;
use compose_ui_graphics::Dp;
use compose_ui_layout::{Alignment, Constraints, HorizontalAlignment, VerticalAlignment};

/// Marker trait matching Jetpack Compose's `BoxScope` API.
#[allow(dead_code)]
pub trait BoxScope {
    /// Align content within the Box using 2D alignment.
    fn align(&self, alignment: Alignment) -> Modifier;
}

/// Marker trait for Column scope - provides horizontal alignment.
#[allow(dead_code)]
pub trait ColumnScope {
    /// Align content horizontally within the Column.
    fn align(&self, alignment: HorizontalAlignment) -> Modifier;
    /// Apply weight to distribute remaining space proportionally.
    fn weight(&self, weight: f32, fill: bool) -> Modifier;
}

/// Marker trait for Row scope - provides vertical alignment.
#[allow(dead_code)]
pub trait RowScope {
    /// Align content vertically within the Row.
    fn align(&self, alignment: VerticalAlignment) -> Modifier;
    /// Apply weight to distribute remaining space proportionally.
    fn weight(&self, weight: f32, fill: bool) -> Modifier;
}

/// Scope exposed to [`BoxWithConstraints`] content.
pub trait BoxWithConstraintsScope: BoxScope {
    fn constraints(&self) -> Constraints;
    fn min_width(&self) -> Dp;
    fn max_width(&self) -> Dp;
    fn min_height(&self) -> Dp;
    fn max_height(&self) -> Dp;
}

/// Concrete implementation of [`BoxWithConstraintsScope`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxWithConstraintsScopeImpl {
    constraints: Constraints,
    density: f32,
}

impl BoxWithConstraintsScopeImpl {
    pub fn new(constraints: Constraints) -> Self {
        Self {
            constraints,
            density: 1.0,
        }
    }

    pub fn with_density(constraints: Constraints, density: f32) -> Self {
        Self {
            constraints,
            density,
        }
    }

    fn to_dp(self, raw: f32) -> Dp {
        Dp::from_px(raw, self.density)
    }

    pub fn to_px(&self, dp: Dp) -> f32 {
        dp.to_px(self.density)
    }

    pub fn density(&self) -> f32 {
        self.density
    }
}

impl BoxScope for BoxWithConstraintsScopeImpl {
    fn align(&self, alignment: Alignment) -> Modifier {
        BoxScopeImpl.align(alignment)
    }
}

/// Concrete implementation of BoxScope.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxScopeImpl;

impl BoxScope for BoxScopeImpl {
    fn align(&self, alignment: Alignment) -> Modifier {
        Modifier::empty().alignInBox(alignment)
    }
}

/// Concrete implementation of ColumnScope.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]
pub struct ColumnScopeImpl;

impl ColumnScope for ColumnScopeImpl {
    fn align(&self, alignment: HorizontalAlignment) -> Modifier {
        Modifier::empty().alignInColumn(alignment)
    }

    fn weight(&self, weight: f32, fill: bool) -> Modifier {
        Modifier::empty().columnWeight(weight, fill)
    }
}

/// Concrete implementation of RowScope.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]
pub struct RowScopeImpl;

impl RowScope for RowScopeImpl {
    fn align(&self, alignment: VerticalAlignment) -> Modifier {
        Modifier::empty().alignInRow(alignment)
    }

    fn weight(&self, weight: f32, fill: bool) -> Modifier {
        Modifier::empty().rowWeight(weight, fill)
    }
}

impl BoxWithConstraintsScope for BoxWithConstraintsScopeImpl {
    fn constraints(&self) -> Constraints {
        self.constraints
    }

    fn min_width(&self) -> Dp {
        self.to_dp(self.constraints.min_width)
    }

    fn max_width(&self) -> Dp {
        self.to_dp(self.constraints.max_width)
    }

    fn min_height(&self) -> Dp {
        self.to_dp(self.constraints.min_height)
    }

    fn max_height(&self) -> Dp {
        self.to_dp(self.constraints.max_height)
    }
}
