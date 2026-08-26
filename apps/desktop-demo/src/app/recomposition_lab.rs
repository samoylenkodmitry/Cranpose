//! The Recomposition Lab: every conditional shape the branch-group transform
//! rewrites, live on one page, with per-section recomposition counters.
//!
//! Each section owns its recompose scope, so the counters show exactly which
//! scopes a state change re-runs: flipping the phase must bump the phase and
//! gauge sections and nothing else, and bumping one keyed row must leave every
//! other counter still. Branch cards carry a global instance number, so a
//! branch switch visibly composes a fresh card while keyed rows visibly keep
//! theirs across visibility toggles.

use std::cell::Cell;

use cranpose_core::{self, MutableState};
use cranpose_ui::{
    composable,
    text::{FontWeight, SpanStyle, TextUnit},
    BoxWithConstraints, BoxWithConstraintsScope, Button, ButtonSpec, Color, Column, ColumnSpec,
    LinearArrangement, Modifier, Row, RowSpec, Text, TextStyle,
};

thread_local! {
    static INSTANCE_SEQ: Cell<u32> = const { Cell::new(0) };
}

fn next_instance() -> u32 {
    INSTANCE_SEQ.with(|seq| {
        seq.set(seq.get() + 1);
        seq.get()
    })
}

fn lab_style(size: f32, color: Color, bold: bool) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: TextUnit::Sp(size),
            font_weight: bold.then_some(FontWeight::BOLD),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn title_style() -> TextStyle {
    lab_style(22.0, Color(0.10, 0.12, 0.18, 1.0), true)
}

fn label_style() -> TextStyle {
    lab_style(14.0, Color(0.16, 0.19, 0.25, 1.0), false)
}

fn counter_style() -> TextStyle {
    lab_style(12.0, Color(0.12, 0.45, 0.25, 1.0), false)
}

#[allow(non_snake_case)]
#[composable]
fn LabButton(label: &'static str, on_click: impl Fn() + 'static) {
    Button(
        Modifier::empty().padding(2.0),
        ButtonSpec::default(),
        on_click,
        move || {
            Text(label, Modifier::empty().padding(4.0), label_style());
        },
    );
}

// `no_skip`: the counter's job is to run exactly when its section's body
// runs; the skip machinery would freeze it at 1 because its argument never
// changes.
#[allow(non_snake_case)]
#[composable(no_skip)]
fn SectionCounter(name: &'static str) {
    let composed = cranpose_core::remember(|| Cell::new(0u32));
    composed.with(|count| count.set(count.get() + 1));
    let count = composed.with(Cell::get);
    Text(
        format!("{name} composed: {count}"),
        Modifier::empty(),
        counter_style(),
    );
}

/// Both branches call this same composable: before branch groups, the two
/// branches were one slot and the card kept the other phase's state.
#[allow(non_snake_case)]
#[composable]
fn PhaseCard(phase: &'static str) {
    let instance = cranpose_core::remember(next_instance);
    let clicks = cranpose_core::rememberMutableStateOf(|| 0);
    let instance = instance.with(|value| *value);
    Text(
        format!("Phase {phase} card, instance #{instance}"),
        Modifier::empty(),
        label_style(),
    );
    Text(
        format!("Phase {phase} clicks: {}", clicks.get()),
        Modifier::empty(),
        label_style(),
    );
    LabButton("Count in phase", move || clicks.set(clicks.get() + 1));
}

#[allow(non_snake_case)]
#[composable]
fn PhaseSection(phase: MutableState<bool>) {
    SectionCounter("phase section");
    if phase.get() {
        PhaseCard("A");
    } else {
        PhaseCard("B");
    }
    LabButton("Flip phase", move || phase.set(!phase.get()));
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LabRoute {
    Overview,
    Detail,
    Settings,
}

/// One shared card called from every `match` arm — three source branches,
/// three identities.
#[allow(non_snake_case)]
#[composable]
fn RouteCard(name: &'static str, show_extra: bool) {
    let instance = cranpose_core::remember(next_instance);
    let instance = instance.with(|value| *value);
    Text(
        format!("route card for {name}, instance #{instance}"),
        Modifier::empty(),
        label_style(),
    );
    if show_extra {
        Text(
            format!("{name} extra line"),
            Modifier::empty(),
            counter_style(),
        );
    }
}

#[allow(non_snake_case)]
#[composable]
fn RouteSection(route: MutableState<LabRoute>) {
    SectionCounter("route section");
    match route.get() {
        LabRoute::Overview => RouteCard("overview", false),
        LabRoute::Detail => RouteCard("detail", true),
        LabRoute::Settings => RouteCard("settings", false),
    }
    Row(
        Modifier::empty(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(6.0)),
        move || {
            LabButton("Route overview", move || route.set(LabRoute::Overview));
            LabButton("Route detail", move || route.set(LabRoute::Detail));
            LabButton("Route settings", move || route.set(LabRoute::Settings));
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn KeyedRow(id: u64) {
    let instance = cranpose_core::remember(next_instance);
    let count = cranpose_core::rememberMutableStateOf(|| 0);
    let instance = instance.with(|value| *value);
    Row(
        Modifier::empty(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(6.0)),
        move || {
            Text(
                format!("Row {id} count: {} (instance #{instance})", count.get()),
                Modifier::empty(),
                label_style(),
            );
            match id {
                1 => LabButton("Row 1 bump", move || count.set(count.get() + 1)),
                2 => LabButton("Row 2 bump", move || count.set(count.get() + 1)),
                3 => LabButton("Row 3 bump", move || count.set(count.get() + 1)),
                5 => LabButton("Row 5 bump", move || count.set(count.get() + 1)),
                _ => LabButton("Row 6 bump", move || count.set(count.get() + 1)),
            }
        },
    );
}

/// The canonical list shape: a fully keyed branch, which opens no bracket and
/// keeps the keyed sibling-move semantics.
#[allow(non_snake_case)]
#[composable]
fn KeyedRowsSection(hide_first: MutableState<bool>) {
    SectionCounter("rows section");
    for id in [1u64, 2, 3] {
        if id != 1 || !hide_first.get() {
            cranpose_core::with_key(&id, move || KeyedRow(id));
        }
    }
    LabButton("Toggle row 1", move || hide_first.set(!hide_first.get()));
}

/// The bracketed variant: unkeyed content beside the keyed call keeps the
/// bracket, so the keyed rows travel between brackets through the orphan pool
/// and the sibling steal when row 5 toggles.
#[allow(non_snake_case)]
#[composable]
fn BracketedRowsSection(hide_first: MutableState<bool>) {
    SectionCounter("bracketed section");
    for id in [5u64, 6] {
        if id != 5 || !hide_first.get() {
            Text(
                format!("bracket crumb {id}"),
                Modifier::empty(),
                counter_style(),
            );
            cranpose_core::with_key(&id, move || KeyedRow(id));
        }
    }
    LabButton("Toggle row 5", move || hide_first.set(!hide_first.get()));
}

/// A `SubcomposeLayout` (through `BoxWithConstraints`) inside a conditional,
/// with another conditional inside its measure-time content lambda.
#[allow(non_snake_case)]
#[composable]
fn GaugeSection(show_gauge: MutableState<bool>, phase: MutableState<bool>) {
    SectionCounter("gauge section");
    if show_gauge.get() {
        BoxWithConstraints(Modifier::empty().fill_max_width(), move |scope| {
            let width = scope.max_width().0.round() as i32;
            Column(Modifier::empty(), ColumnSpec::new(), move || {
                Text(
                    format!("gauge width: {width}dp"),
                    Modifier::empty(),
                    label_style(),
                );
                if phase.get() {
                    Text("gauge sees phase A", Modifier::empty(), counter_style());
                } else {
                    Text("gauge sees phase B", Modifier::empty(), counter_style());
                }
            });
        });
    } else {
        Text("gauge hidden", Modifier::empty(), counter_style());
    }
    LabButton("Toggle gauge", move || show_gauge.set(!show_gauge.get()));
}

#[allow(non_snake_case)]
#[composable]
fn LabFooter() {
    SectionCounter("footer");
    Text(
        "the footer never recomposes",
        Modifier::empty(),
        counter_style(),
    );
}

#[allow(non_snake_case)]
#[composable]
pub fn RecompositionLabTab() {
    let phase = cranpose_core::rememberMutableStateOf(|| true);
    let route = cranpose_core::rememberMutableStateOf(|| LabRoute::Overview);
    let hide_keyed_first = cranpose_core::rememberMutableStateOf(|| false);
    let hide_bracketed_first = cranpose_core::rememberMutableStateOf(|| false);
    let show_gauge = cranpose_core::rememberMutableStateOf(|| true);
    Column(
        Modifier::empty()
            .fill_max_width()
            .padding(super::DEMO_PAGE_PADDING),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(10.0)),
        move || {
            Text("Recomposition Lab", Modifier::empty(), title_style());
            PhaseSection(phase);
            RouteSection(route);
            KeyedRowsSection(hide_keyed_first);
            BracketedRowsSection(hide_bracketed_first);
            GaugeSection(show_gauge, phase);
            LabFooter();
        },
    );
}
