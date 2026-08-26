//! Foundation slider with caller-owned state and composable visual content.

#![allow(non_snake_case)]

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{NodeId, State, rememberMutableStateOf, rememberUpdatedState};
use cranpose_foundation::{PointerEventKind, PointerId};

use crate::{
    Modifier, MutableInteractionSource, composable,
    widgets::{Box, BoxSpec, BoxWithConstraints, scopes::BoxWithConstraintsScope},
};

/// Main axis used by a [`Slider`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SliderOrientation {
    /// Values increase from start to end.
    #[default]
    Horizontal,
    /// Values increase from top to bottom unless reversed.
    Vertical,
}

/// Input and geometry policy for a [`Slider`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderSpec {
    pub orientation: SliderOrientation,
    pub reverse_direction: bool,
    pub thumb_extent: f32,
    pub enabled: bool,
    pub rotary_step: f32,
}

impl SliderSpec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn orientation(mut self, orientation: SliderOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    pub fn reverse_direction(mut self, reverse_direction: bool) -> Self {
        self.reverse_direction = reverse_direction;
        self
    }

    pub fn thumb_extent(mut self, thumb_extent: f32) -> Self {
        self.thumb_extent = thumb_extent.max(0.0);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn rotary_step(mut self, rotary_step: f32) -> Self {
        self.rotary_step = rotary_step.abs();
        self
    }
}

impl Default for SliderSpec {
    fn default() -> Self {
        Self {
            orientation: SliderOrientation::Horizontal,
            reverse_direction: false,
            thumb_extent: 0.0,
            enabled: true,
            rotary_step: 0.05,
        }
    }
}

/// Values available to custom slider track and thumb content.
#[derive(Clone)]
pub struct SliderScope {
    value: f32,
    track_extent: f32,
    thumb_offset: f32,
    dragging: State<bool>,
    interaction_source: MutableInteractionSource,
}

impl SliderScope {
    /// Current caller-owned value, clamped to `0..=1`.
    pub fn value(&self) -> f32 {
        self.value
    }

    /// Main-axis space through which the thumb can travel.
    pub fn track_extent(&self) -> f32 {
        self.track_extent
    }

    /// Main-axis offset for the leading edge of the thumb.
    pub fn thumb_offset(&self) -> f32 {
        self.thumb_offset
    }

    /// Whether direct pointer input is currently changing the value.
    pub fn is_dragging(&self) -> bool {
        self.dragging.get()
    }

    /// Interaction source shared by the slider surface.
    pub fn interaction_source(&self) -> MutableInteractionSource {
        self.interaction_source
    }
}

/// A `0..=1` slider whose visuals are ordinary composables supplied by
/// `content`. The framework owns pointer capture, cancellation, rotary input,
/// pressed interactions, value mapping, and completion delivery.
#[composable]
pub fn Slider<F>(
    modifier: Modifier,
    value: f32,
    on_value_change: impl Fn(f32) + 'static,
    on_value_change_finished: impl Fn() + 'static,
    spec: SliderSpec,
    content: F,
) -> NodeId
where
    F: FnMut(SliderScope) + 'static,
{
    let value = value.clamp(0.0, 1.0);
    let current_value = rememberUpdatedState(value);
    let on_value_change: Rc<dyn Fn(f32)> = Rc::new(on_value_change);
    let on_value_change = rememberUpdatedState(on_value_change);
    let on_value_change_finished: Rc<dyn Fn()> = Rc::new(on_value_change_finished);
    let on_value_change_finished = rememberUpdatedState(on_value_change_finished);
    let dragging = rememberMutableStateOf(|| false);
    let interaction_source = crate::rememberMutableInteractionSource();
    let content = Rc::new(RefCell::new(content));

    BoxWithConstraints(modifier, move |constraints_scope| {
        let constraints = constraints_scope.constraints();
        let extent = match spec.orientation {
            SliderOrientation::Horizontal => constraints.max_width,
            SliderOrientation::Vertical => constraints.max_height,
        }
        .max(0.0);
        let track_extent = (extent - spec.thumb_extent).max(0.0);
        let logical_value = if spec.reverse_direction {
            1.0 - value
        } else {
            value
        };
        let slider_scope = SliderScope {
            value,
            track_extent,
            thumb_offset: track_extent * logical_value,
            dragging: dragging.as_state(),
            interaction_source,
        };
        let interaction = interaction_source;
        let content = Rc::clone(&content);
        let input = Modifier::empty()
            .fill_max_size()
            .semantics(move |config| {
                config.enabled = spec.enabled;
                config.state_description = Some(format!("{}%", (value * 100.0).round() as u32));
            })
            .pointer_input(
                (
                    spec.orientation,
                    spec.reverse_direction,
                    spec.thumb_extent.to_bits(),
                    spec.enabled,
                    spec.rotary_step.to_bits(),
                    extent.to_bits(),
                ),
                move |pointer_scope| {
                    let interaction = interaction;
                    async move {
                        pointer_scope
                            .await_pointer_event_scope(|await_scope| async move {
                                let mut active_pointer: Option<PointerId> = None;
                                let mut active_press = None;
                                loop {
                                    let event = await_scope.await_pointer_event().await;
                                    match event.kind {
                                        PointerEventKind::Down
                                            if spec.enabled && active_pointer.is_none() =>
                                        {
                                            active_pointer = Some(event.id);
                                            dragging.set(true);
                                            active_press = Some(interaction.press(event.position));
                                            let next = value_for_position(
                                                axis_position(
                                                    event.position.x,
                                                    event.position.y,
                                                    spec,
                                                ),
                                                extent,
                                                spec.thumb_extent,
                                                spec.reverse_direction,
                                            );
                                            (on_value_change.value())(next);
                                            event.consume();
                                        }
                                        PointerEventKind::Move
                                            if active_pointer == Some(event.id) =>
                                        {
                                            let next = value_for_position(
                                                axis_position(
                                                    event.position.x,
                                                    event.position.y,
                                                    spec,
                                                ),
                                                extent,
                                                spec.thumb_extent,
                                                spec.reverse_direction,
                                            );
                                            (on_value_change.value())(next);
                                            event.consume();
                                        }
                                        PointerEventKind::Up
                                            if active_pointer == Some(event.id) =>
                                        {
                                            let next = value_for_position(
                                                axis_position(
                                                    event.position.x,
                                                    event.position.y,
                                                    spec,
                                                ),
                                                extent,
                                                spec.thumb_extent,
                                                spec.reverse_direction,
                                            );
                                            (on_value_change.value())(next);
                                            if let Some(press) = active_press.take() {
                                                interaction.release(press);
                                            }
                                            dragging.set(false);
                                            active_pointer = None;
                                            (on_value_change_finished.value())();
                                            event.consume();
                                        }
                                        PointerEventKind::Cancel
                                            if active_pointer == Some(event.id) =>
                                        {
                                            if let Some(press) = active_press.take() {
                                                interaction.cancel(press);
                                            }
                                            dragging.set(false);
                                            active_pointer = None;
                                            (on_value_change_finished.value())();
                                            event.consume();
                                        }
                                        PointerEventKind::RotaryScroll if spec.enabled => {
                                            let delta = event.scroll_delta.y;
                                            if delta != 0.0 {
                                                let direction =
                                                    if spec.reverse_direction { 1.0 } else { -1.0 };
                                                let next = (current_value.value()
                                                    + direction
                                                        * delta.signum()
                                                        * spec.rotary_step)
                                                    .clamp(0.0, 1.0);
                                                (on_value_change.value())(next);
                                                (on_value_change_finished.value())();
                                                event.consume();
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            })
                            .await;
                    }
                },
            );
        Box(input, BoxSpec::default(), move || {
            (content.borrow_mut())(slider_scope.clone())
        });
    })
}

fn axis_position(x: f32, y: f32, spec: SliderSpec) -> f32 {
    match spec.orientation {
        SliderOrientation::Horizontal => x,
        SliderOrientation::Vertical => y,
    }
}

fn value_for_position(position: f32, extent: f32, thumb_extent: f32, reverse: bool) -> f32 {
    let travel = (extent - thumb_extent).max(0.0);
    let mut value = if travel > 0.0 {
        ((position - thumb_extent * 0.5) / travel).clamp(0.0, 1.0)
    } else {
        0.0
    };
    if reverse {
        value = 1.0 - value;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_builders_define_orientation_and_input_policy() {
        let spec = SliderSpec::new()
            .orientation(SliderOrientation::Vertical)
            .reverse_direction(true)
            .thumb_extent(11.0)
            .enabled(false)
            .rotary_step(-0.2);
        assert_eq!(spec.orientation, SliderOrientation::Vertical);
        assert!(spec.reverse_direction);
        assert_eq!(spec.thumb_extent, 11.0);
        assert!(!spec.enabled);
        assert_eq!(spec.rotary_step, 0.2);
    }

    #[test]
    fn pointer_position_tracks_thumb_centre_and_reverse_direction() {
        assert_eq!(value_for_position(5.0, 110.0, 10.0, false), 0.0);
        assert_eq!(value_for_position(105.0, 110.0, 10.0, false), 1.0);
        assert_eq!(value_for_position(55.0, 110.0, 10.0, false), 0.5);
        assert_eq!(value_for_position(5.0, 110.0, 10.0, true), 1.0);
    }

    #[test]
    fn zero_travel_is_stable() {
        assert_eq!(value_for_position(0.0, 0.0, 0.0, false), 0.0);
        assert_eq!(value_for_position(0.0, 0.0, 0.0, true), 1.0);
    }
}
