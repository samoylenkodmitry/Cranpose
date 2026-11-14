// The bottom tab bar: Home, Search and Cart, with the cart's quantity in a
// badge on Cart.

use cranpose::{
    Alignment, Box, BoxSpec, Column, ColumnSpec, HorizontalAlignment, LinearArrangement, Modifier,
    Row, RowSpec, Spacer, Text, composable, local_safe_area_insets, text::FontWeight,
};

use crate::{
    icons::{IconKind, StrokeIcon},
    theme::{BACKGROUND, BORDER, SALE, TAB_BAR_HEIGHT, TEXT, TEXT_MUTED, WHITE, style},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tab {
    Home,
    Search,
    Cart,
}

impl Tab {
    const ALL: [Self; 3] = [Self::Home, Self::Search, Self::Cart];

    fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Search => "Search",
            Self::Cart => "Cart",
        }
    }

    fn tag(self) -> &'static str {
        match self {
            Self::Home => "tab-home",
            Self::Search => "tab-search",
            Self::Cart => "tab-cart",
        }
    }

    fn icon(self) -> IconKind {
        match self {
            Self::Home => IconKind::Home,
            Self::Search => IconKind::Search,
            Self::Cart => IconKind::Cart,
        }
    }
}

#[composable]
pub(crate) fn TabBar(selected: Tab, badge: u32, select: impl Fn(Tab) + Clone + 'static) {
    let bottom = local_safe_area_insets().current().bottom;
    Column(
        Modifier::empty()
            .fill_max_width()
            .background(BACKGROUND)
            .padding_each(0.0, 0.0, 0.0, bottom),
        ColumnSpec::default(),
        move || {
            Spacer(
                Modifier::empty()
                    .fill_max_width()
                    .height(1.0)
                    .background(BORDER),
            );
            let select = select.clone();
            Box(
                Modifier::empty()
                    .fill_max_width()
                    .height(TAB_BAR_HEIGHT - 1.0),
                BoxSpec::default(),
                move || {
                    let select = select.clone();
                    Row(
                        Modifier::empty().fill_max_size(),
                        RowSpec::default(),
                        move || {
                            for tab in Tab::ALL {
                                let select = select.clone();
                                TabItem(tab, tab == selected, move || select(tab));
                            }
                        },
                    );
                    if badge > 0 {
                        CartBadge(badge);
                    }
                },
            );
        },
    );
}

#[composable]
fn TabItem(tab: Tab, active: bool, select: impl Fn() + 'static) {
    let color = if active { TEXT } else { TEXT_MUTED };
    let weight = if active {
        FontWeight::SEMI_BOLD
    } else {
        FontWeight::MEDIUM
    };
    Column(
        Modifier::empty()
            .weight(1.0)
            .fill_max_height()
            .test_tag(tab.tag())
            .clickable(move |_| select()),
        ColumnSpec::new()
            .vertical_arrangement(LinearArrangement::Center)
            .horizontal_alignment(HorizontalAlignment::CenterHorizontally),
        move || {
            StrokeIcon(tab.icon(), 24.0, 1.75, color);
            Text(
                tab.label(),
                Modifier::empty().padding_each(0.0, 2.0, 0.0, 0.0),
                style(11.0, weight, color),
            );
        },
    );
}

// Over the Cart tab, its left edge 6 pt right of the tab's center and 4 pt
// from the top: the right half of the Cart tab starts at that center.
#[composable]
fn CartBadge(count: u32) {
    Row(
        Modifier::empty().fill_max_size(),
        RowSpec::default(),
        move || {
            Spacer(Modifier::empty().weight(2.5));
            Box(
                Modifier::empty().weight(0.5).fill_max_height(),
                BoxSpec::new().content_alignment(Alignment::TOP_START),
                move || {
                    Box(
                        Modifier::empty()
                            .padding_each(6.0, 4.0, 0.0, 0.0)
                            .height_in(18.0, f32::INFINITY)
                            .width_in(18.0, f32::INFINITY)
                            .rounded_corners(9.0)
                            .background(SALE)
                            .test_tag("cart-badge")
                            .merge_descendants()
                            .padding_horizontal(5.0),
                        BoxSpec::new().content_alignment(Alignment::CENTER),
                        move || {
                            Text(
                                count.to_string(),
                                Modifier::empty(),
                                style(11.0, FontWeight::BOLD, WHITE),
                            );
                        },
                    );
                },
            );
        },
    );
}
