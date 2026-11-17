// The Explainer tab: a scrolling article whose sections are scenes drawn
// from live composables. A section is one or more window heights tall; a
// section's scene runs only while some of it shows.

use cranpose_core::{derivedStateOf, remember};
use cranpose_ui::{
    composable, Alignment, Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Brush,
    Button, ButtonSpec, Canvas, Color, Column, ColumnSpec, LinearArrangement, Modifier, Point, Row,
    RowSpec, ScrollState, Size, Stroke, StrokeCap, Text, VerticalAlignment,
};

use super::{
    how::{HowSection, HOW_PAGES},
    lamp::{LampSection, LAMP_PAGES},
    lettering::strong,
    melt::{MeltSection, MELT_PAGES},
    page::{PageSection, PAGE_PAGES},
    paint::{PaintSection, PAINT_PAGES},
    pool::{PoolSection, POOL_PAGES},
    silk::{SilkSection, SILK_PAGES},
    stamps::{StampsSection, STAMPS_PAGES},
};

// The sections, top to bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Paint,
    Stamps,
    Silk,
    Melt,
    Lamp,
    How,
    Pool,
    Page,
}

impl Section {
    // How many window heights the section takes.
    fn pages(self) -> f32 {
        match self {
            Self::Paint => PAINT_PAGES,
            Self::Stamps => STAMPS_PAGES,
            Self::Silk => SILK_PAGES,
            Self::Melt => MELT_PAGES,
            Self::Lamp => LAMP_PAGES,
            Self::How => HOW_PAGES,
            Self::Pool => POOL_PAGES,
            Self::Page => PAGE_PAGES,
        }
    }
}

const SECTIONS: [Section; 8] = [
    Section::Paint,
    Section::Stamps,
    Section::Silk,
    Section::How,
    Section::Melt,
    Section::Lamp,
    Section::Pool,
    Section::Page,
];
const PILL: Color = Color(0.07, 0.08, 0.16, 0.92);

#[composable]
pub(crate) fn ExplainerTab() {
    let scroll = remember(|| ScrollState::new(0.0)).with(|state| *state);
    BoxWithConstraints(Modifier::empty().fill_max_size(), move |scope| {
        let constraints = scope.constraints();
        let finite = |extent: f32| if extent.is_finite() { extent } else { 800.0 };
        let size = Size::new(
            finite(constraints.max_width),
            finite(constraints.max_height),
        );
        Box(
            Modifier::empty().fill_max_size(),
            BoxSpec {
                content_alignment: Alignment::BOTTOM_CENTER,
                ..BoxSpec::default()
            },
            move || {
                Column(
                    Modifier::empty()
                        .fill_max_size()
                        .vertical_scroll(scroll, false),
                    ColumnSpec::default(),
                    move || {
                        let mut top = 0.0;
                        for section in SECTIONS {
                            let height = section.pages() * size.height;
                            let shows = derivedStateOf(move || {
                                let offset = scroll.value();
                                offset > top - size.height && offset < top + height
                            });
                            SectionAt(section, size, (scroll, top), shows.get());
                            top += height;
                        }
                    },
                );
                ScrollPill(scroll, size.height);
            },
        );
    });
}

#[composable(no_skip)]
fn SectionAt(section: Section, size: Size, (scroll, top): (ScrollState, f32), shows: bool) {
    match section {
        Section::Paint => {
            PaintSection(size, scroll, (top, top + section.pages() * size.height));
        }
        Section::Stamps => StampsSection(size, scroll, top),
        Section::Silk => SilkSection(size, scroll, top, shows),
        Section::Melt => MeltSection(size, scroll, top, shows),
        Section::Lamp => LampSection(size, scroll, top, shows),
        Section::How => HowSection(size, scroll, top, shows),
        Section::Pool => PoolSection(size, shows),
        Section::Page => PageSection(size, shows),
    }
}

// The pill at the foot of the window that moves the article on a window
// height.
#[composable(no_skip)]
fn ScrollPill(scroll: ScrollState, page_height: f32) {
    Button(
        Modifier::empty()
            .padding_each(0.0, 0.0, 0.0, 18.0)
            .rounded_corners(18.0)
            .background(PILL)
            .padding_symmetric(16.0, 8.0),
        ButtonSpec::default(),
        move || {
            let page = (scroll.value_non_reactive() / page_height.max(1.0)).floor() + 1.0;
            scroll.animate_scroll_to(page * page_height);
        },
        || {
            Row(
                Modifier::empty(),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(8.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                || {
                    Text("Scroll", Modifier::empty(), strong(15.0, Color::WHITE));
                    Canvas(Modifier::empty().size_points(12.0, 12.0), |scope| {
                        let stroke = Stroke::new(2.0).with_cap(StrokeCap::Round);
                        let brush = Brush::solid(Color::WHITE);
                        let (left, middle, right) = (
                            Point::new(2.0, 4.0),
                            Point::new(6.0, 8.5),
                            Point::new(10.0, 4.0),
                        );
                        scope.draw_line(brush.clone(), left, middle, stroke);
                        scope.draw_line(brush, middle, right, stroke);
                    });
                },
            );
        },
    );
}
