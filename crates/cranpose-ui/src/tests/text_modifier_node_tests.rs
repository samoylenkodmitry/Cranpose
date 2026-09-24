use std::{collections::hash_map::DefaultHasher, sync::mpsc};

use cranpose_core::NodeId;
use cranpose_foundation::BasicModifierNodeContext;

use super::*;
use crate::{text::TextUnit, text_layout_result::TextLayoutResult};

fn hash_of(element: &TextModifierElement) -> u64 {
    let mut hasher = DefaultHasher::new();
    element.hash(&mut hasher);
    hasher.finish()
}

struct RecordingPreparedLayoutMeasurer {
    recorded: std::rc::Rc<std::cell::RefCell<Vec<Option<NodeId>>>>,
}

impl crate::text::TextMeasurer for RecordingPreparedLayoutMeasurer {
    fn measure(
        &self,
        _text: &crate::text::AnnotatedString,
        _style: &TextStyle,
    ) -> crate::text::TextMetrics {
        crate::text::TextMetrics {
            width: 12.0,
            height: 18.0,
            line_height: 18.0,
            line_count: 1,
        }
    }

    fn prepare_with_options_for_node(
        &self,
        node_id: Option<NodeId>,
        text: &crate::text::AnnotatedString,
        _style: &TextStyle,
        _options: TextLayoutOptions,
        _max_width: Option<f32>,
    ) -> crate::text::PreparedTextLayout {
        self.recorded.borrow_mut().push(node_id);
        crate::text::PreparedTextLayout {
            text: Rc::new(text.clone()),
            visual_style: TextStyle::default(),
            metrics: crate::text::TextMetrics {
                width: 12.0,
                height: 18.0,
                line_height: 18.0,
                line_count: 1,
            },
            did_overflow: false,
        }
    }

    // forwards on purpose: standard trait methods for test helper
    fn get_offset_for_position(
        &self,
        _text: &crate::text::AnnotatedString,
        _style: &TextStyle,
        _x: f32,
        _y: f32,
    ) -> usize {
        0
    }

    fn get_cursor_x_for_offset(
        &self,
        _text: &crate::text::AnnotatedString,
        _style: &TextStyle,
        _offset: usize,
    ) -> f32 {
        0.0
    }

    fn layout(&self, _text: &crate::text::AnnotatedString, _style: &TextStyle) -> TextLayoutResult {
        panic!("layout is not used in this test");
    }
}

struct FixedPreparedLayoutMeasurer {
    height: f32,
    line_height: f32,
}

struct FontSizePreparedLayoutMeasurer {
    recorded: Rc<RefCell<Vec<f32>>>,
}

// forwards on purpose: different test measurer with a distinct trait impl, not a redirect
impl crate::text::TextMeasurer for FontSizePreparedLayoutMeasurer {
    fn measure(
        &self,
        _text: &crate::text::AnnotatedString,
        style: &TextStyle,
    ) -> crate::text::TextMetrics {
        let size = style.resolve_font_size(14.0);
        crate::text::TextMetrics {
            width: size,
            height: size,
            line_height: size,
            line_count: 1,
        }
    }

    fn prepare_with_options_for_node(
        &self,
        _node_id: Option<NodeId>,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        _options: TextLayoutOptions,
        _max_width: Option<f32>,
    ) -> crate::text::PreparedTextLayout {
        let size = style.resolve_font_size(14.0);
        self.recorded.borrow_mut().push(size);
        crate::text::PreparedTextLayout {
            text: Rc::new(text.clone()),
            visual_style: style.clone(),
            metrics: crate::text::TextMetrics {
                width: size,
                height: size,
                line_height: size,
                line_count: 1,
            },
            did_overflow: false,
        }
    }

    fn get_offset_for_position(
        &self,
        _text: &crate::text::AnnotatedString,
        _style: &TextStyle,
        _x: f32,
        _y: f32,
    ) -> usize {
        0
    }

    fn get_cursor_x_for_offset(
        &self,
        _text: &crate::text::AnnotatedString,
        _style: &TextStyle,
        _offset: usize,
    ) -> f32 {
        0.0
    }

    fn layout(&self, _text: &crate::text::AnnotatedString, _style: &TextStyle) -> TextLayoutResult {
        panic!("layout is not used in this test");
    }
}

// forwards on purpose: different test measurer with a distinct trait impl, not a redirect
impl crate::text::TextMeasurer for FixedPreparedLayoutMeasurer {
    fn measure(
        &self,
        _text: &crate::text::AnnotatedString,
        _style: &TextStyle,
    ) -> crate::text::TextMetrics {
        crate::text::TextMetrics {
            width: 24.0,
            height: self.height,
            line_height: self.line_height,
            line_count: (self.height / self.line_height).round().max(1.0) as usize,
        }
    }

    fn prepare_with_options_for_node(
        &self,
        _node_id: Option<NodeId>,
        text: &crate::text::AnnotatedString,
        _style: &TextStyle,
        _options: TextLayoutOptions,
        _max_width: Option<f32>,
    ) -> crate::text::PreparedTextLayout {
        crate::text::PreparedTextLayout {
            text: Rc::new(text.clone()),
            visual_style: TextStyle::default(),
            metrics: crate::text::TextMetrics {
                width: 24.0,
                height: self.height,
                line_height: self.line_height,
                line_count: (self.height / self.line_height).round().max(1.0) as usize,
            },
            did_overflow: false,
        }
    }

    fn get_offset_for_position(
        &self,
        _text: &crate::text::AnnotatedString,
        _style: &TextStyle,
        _x: f32,
        _y: f32,
    ) -> usize {
        0
    }

    fn get_cursor_x_for_offset(
        &self,
        _text: &crate::text::AnnotatedString,
        _style: &TextStyle,
        _offset: usize,
    ) -> f32 {
        0.0
    }

    fn layout(&self, _text: &crate::text::AnnotatedString, _style: &TextStyle) -> TextLayoutResult {
        panic!("layout is not used in this test");
    }
}

#[test]
fn hash_changes_when_style_changes() {
    let text = Rc::new(AnnotatedString::from("Hello"));
    let element_a = TextModifierElement::new(
        text.clone(),
        TextStyle::default(),
        TextLayoutOptions::default(),
    );
    let style_b = TextStyle {
        span_style: crate::text::SpanStyle {
            font_size: TextUnit::Sp(18.0),
            ..Default::default()
        },
        ..Default::default()
    };
    let element_b = TextModifierElement::new(text, style_b, TextLayoutOptions::default());

    assert_ne!(element_a, element_b);
    assert_ne!(hash_of(&element_a), hash_of(&element_b));
}

#[test]
fn hash_matches_for_equal_elements() {
    let style = TextStyle {
        span_style: crate::text::SpanStyle {
            font_size: TextUnit::Sp(14.0),
            letter_spacing: TextUnit::Em(0.1),
            ..Default::default()
        },
        ..Default::default()
    };
    let options = TextLayoutOptions::default();
    let text = Rc::new(AnnotatedString::from("Hash me"));
    let element_a = TextModifierElement::new(text.clone(), style.clone(), options);
    let element_b = TextModifierElement::new(text, style, options);

    assert_eq!(element_a, element_b);
    assert_eq!(hash_of(&element_a), hash_of(&element_b));
}

#[test]
fn measure_uses_attached_node_identity() {
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let recorded = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let app_context = crate::AppContext::new();
        app_context.enter(|| {
            crate::text::set_text_measurer(RecordingPreparedLayoutMeasurer {
                recorded: recorded.clone(),
            });

            let mut node = TextModifierNode::new(
                Rc::new(AnnotatedString::from("identity")),
                TextStyle::default(),
                TextLayoutOptions::default(),
            );
            let mut context = BasicModifierNodeContext::new();
            context.set_node_id(Some(77));
            node.on_attach(&mut context);

            let size = node.measure_text_content(Some(96.0));
            tx.send((recorded.borrow().clone(), size.width, size.height))
                .expect("send measurement result");
        });
    });

    let (recorded, width, height) = rx.recv().expect("receive measurement result");
    assert_eq!(recorded, vec![Some(77)]);
    assert_eq!(width, 12.0);
    assert_eq!(height, 18.0);
}

#[test]
fn prepared_layout_cache_reuses_node_snapshot() {
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let recorded = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let app_context = crate::AppContext::new();
        app_context.enter(|| {
            crate::text::set_text_measurer(RecordingPreparedLayoutMeasurer {
                recorded: recorded.clone(),
            });

            let mut node = TextModifierNode::new(
                Rc::new(AnnotatedString::from("reuse")),
                TextStyle::default(),
                TextLayoutOptions::default(),
            );
            let mut context = BasicModifierNodeContext::new();
            context.set_node_id(Some(88));
            node.on_attach(&mut context);

            let measured = node.measure_text_content(Some(120.0));
            let prepared = node.prepared_layout_handle().prepare(Some(120.0));
            tx.send((
                recorded.borrow().clone(),
                measured.width,
                measured.height,
                prepared.metrics.width,
                prepared.metrics.height,
            ))
            .expect("send cached layout result");
        });
    });

    let (recorded, measured_width, measured_height, prepared_width, prepared_height) =
        rx.recv().expect("receive cached layout result");
    assert_eq!(recorded, vec![Some(88)]);
    assert_eq!(measured_width, prepared_width);
    assert_eq!(measured_height, prepared_height);
}

#[test]
fn prepared_layout_cache_refreshes_when_text_service_changes() {
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let app_context = crate::AppContext::new();
        app_context.enter(|| {
            crate::text::set_text_measurer(FixedPreparedLayoutMeasurer {
                height: 30.0,
                line_height: 10.0,
            });

            let node = TextModifierNode::new(
                Rc::new(AnnotatedString::from("a\nb\nc")),
                TextStyle::default(),
                TextLayoutOptions::default(),
            );

            let first = node.measure_text_content(Some(160.0));
            crate::text::set_text_measurer(FixedPreparedLayoutMeasurer {
                height: 60.0,
                line_height: 20.0,
            });
            let second = node.measure_text_content(Some(160.0));
            tx.send((first.height, second.height))
                .expect("send measurement result");
        });
    });

    let (first_height, second_height) = rx.recv().expect("receive measurement result");
    assert_eq!(first_height, 30.0);
    assert_eq!(second_height, 60.0);
}

#[test]
fn prepared_layout_cache_refreshes_when_system_font_scale_changes() {
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let recorded = Rc::new(RefCell::new(Vec::new()));
        let app_context = crate::AppContext::new();
        app_context.enter(|| {
            crate::text::set_text_measurer(FontSizePreparedLayoutMeasurer {
                recorded: Rc::clone(&recorded),
            });
            let node = TextModifierNode::new(
                Rc::new(AnnotatedString::from("scale")),
                TextStyle {
                    span_style: crate::text::SpanStyle {
                        font_size: TextUnit::Sp(10.0),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                TextLayoutOptions::default(),
            );

            let first = node.measure_text_content(None);
            crate::set_font_scale(1.5);
            let second = node.measure_text_content(None);
            tx.send((recorded.borrow().clone(), first.height, second.height))
                .expect("send measurement result");
        });
    });

    let (recorded, first, second) = rx.recv().expect("receive measurement result");
    assert_eq!(recorded, vec![10.0, 15.0]);
    assert_eq!(first, 10.0);
    assert_eq!(second, 15.0);
}

#[test]
fn semantics_uses_source_text_for_scaled_overflow() {
    let node = TextModifierNode::new(
        Rc::new(AnnotatedString::from("Save Cranpose WebP")),
        TextStyle::default(),
        TextLayoutOptions {
            overflow: crate::text::TextOverflow::ScaleDown {
                min_font_size_sp: 9.0,
            },
            soft_wrap: false,
            max_lines: 1,
            min_lines: 1,
        },
    );
    let mut config = SemanticsConfiguration::default();

    node.merge_semantics(&mut config);

    assert_eq!(
        config.content_description.as_deref(),
        Some("Save Cranpose WebP")
    );
}
