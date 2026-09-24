use std::rc::Rc;

use cranpose_core::NodeId;
use cranpose_ui::{
    LayoutBox, LayoutNodeData, LayoutNodeKind, Modifier, ModifierNodeSlices, Point, Rect,
    ResolvedModifiers, SemanticsAction, SemanticsCallback, SemanticsNode, SemanticsRole,
};

use super::{
    HashMap, SemanticQueryResult, SemanticRect, SemanticTextMatchKind, bounds_from_layout_box,
    find_button_in_semantics_tree, find_text_in_semantics_tree, panic_payload_message,
    robot_wait_for_idle_animation_loop_only, semantic_element_from_semantics_node,
    semantics_node_clickable, semantics_node_text, semantics_text_matches,
    subtree_contains_matching_text,
};

#[cfg(feature = "renderer-wgpu")]
#[test]
fn presentation_info_reports_constraints_and_channel_errors() {
    use cranpose_app_shell::FramePacingMode;

    use super::{RobotChannel, RobotCommand, RobotPresentationInfo, RobotResponse};
    for mode in [
        wgpu::PresentMode::Immediate,
        wgpu::PresentMode::Mailbox,
        wgpu::PresentMode::Fifo,
        wgpu::PresentMode::FifoRelaxed,
        wgpu::PresentMode::AutoNoVsync,
    ] {
        for pacing in [
            FramePacingMode::NoVsync,
            FramePacingMode::Vsync,
            FramePacingMode::Hard60,
            FramePacingMode::Hard120,
        ] {
            let info = RobotPresentationInfo {
                requested_mode: Some("immediate".into()),
                present_mode: mode,
                supported_modes: vec![mode],
                frame_pacing_mode: pacing,
                refresh_rate_hz: 60.0,
            };
            let expected = pacing == FramePacingMode::NoVsync
                && matches!(
                    mode,
                    wgpu::PresentMode::Immediate | wgpu::PresentMode::Mailbox
                );
            assert_eq!(info.uses_unpaced_configuration(), expected);
            let (channel, robot) = RobotChannel::new(|| {});
            channel
                .tx
                .send(RobotResponse::PresentationInfo(info))
                .unwrap();
            let response = robot.presentation_info().unwrap();
            assert!(matches!(
                channel.rx.recv().unwrap(),
                RobotCommand::GetPresentationInfo
            ));
            assert_eq!(response.present_mode, mode);
            assert_eq!(response.uses_unpaced_configuration(), expected);
            channel
                .tx
                .send(RobotResponse::Error("surface unavailable".into()))
                .unwrap();
            assert_eq!(
                robot.presentation_info().unwrap_err(),
                "surface unavailable"
            );
            drop(channel);
            assert!(robot.presentation_info().is_err());
        }
    }
}

fn find_text_in_trees(
    sem_node: &SemanticsNode,
    layout_box: &LayoutBox,
    query: &str,
    match_kind: SemanticTextMatchKind,
) -> Option<SemanticQueryResult> {
    if let Some(text) = semantics_node_text(sem_node)
        && semantics_text_matches(text, query, match_kind)
    {
        return Some(SemanticQueryResult {
            node_id: layout_box.node_id,
            bounds: bounds_from_layout_box(layout_box),
            text: Some(text.to_string()),
        });
    }

    sem_node
        .children
        .iter()
        .zip(layout_box.children.iter())
        .find_map(|(sem_child, layout_child)| {
            find_text_in_trees(sem_child, layout_child, query, match_kind)
        })
}

fn find_button_in_trees(
    sem_node: &SemanticsNode,
    layout_box: &LayoutBox,
    query: &str,
    match_kind: SemanticTextMatchKind,
) -> Option<SemanticQueryResult> {
    if semantics_node_clickable(sem_node)
        && subtree_contains_matching_text(sem_node, query, match_kind)
    {
        return Some(SemanticQueryResult {
            node_id: layout_box.node_id,
            bounds: bounds_from_layout_box(layout_box),
            text: semantics_node_text(sem_node).map(str::to_string),
        });
    }

    sem_node
        .children
        .iter()
        .zip(layout_box.children.iter())
        .find_map(|(sem_child, layout_child)| {
            find_button_in_trees(sem_child, layout_child, query, match_kind)
        })
}

fn sample_layout_box(
    node_id: u64,
    rect: (f32, f32, f32, f32),
    children: Vec<LayoutBox>,
) -> LayoutBox {
    LayoutBox::new(
        node_id as NodeId,
        Rect {
            x: rect.0,
            y: rect.1,
            width: rect.2,
            height: rect.3,
        },
        Point { x: 0.0, y: 0.0 },
        LayoutNodeData::new(
            Modifier::empty(),
            ResolvedModifiers::default(),
            Rc::new(ModifierNodeSlices::default()),
            LayoutNodeKind::Spacer,
        ),
        children,
    )
}

fn sample_semantics_node(
    node_id: u64,
    role: SemanticsRole,
    clickable: bool,
    description: Option<&str>,
    children: Vec<SemanticsNode>,
) -> SemanticsNode {
    let mut actions = Vec::new();
    if clickable {
        actions.push(SemanticsAction::Click {
            handler: SemanticsCallback::new(node_id as NodeId),
        });
    }
    SemanticsNode {
        node_id: node_id as NodeId,
        role,
        actions,
        children,
        description: description.map(str::to_string),
        ..SemanticsNode::default()
    }
}

#[test]
fn queries_find_a_drawn_control_ahead_of_the_canvas_that_drew_it() {
    use cranpose_ui::CanvasSemanticsNode;

    let mut canvas = sample_semantics_node(
        2,
        SemanticsRole::Layout,
        true,
        Some("Settings screen"),
        Vec::new(),
    );
    canvas.canvas_children = vec![
        CanvasSemanticsNode::text(
            1,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 30.0,
            },
            "CROWN",
        ),
        CanvasSemanticsNode::control(
            2,
            Rect {
                x: 0.0,
                y: 40.0,
                width: 200.0,
                height: 52.0,
            },
            "Haptics",
        ),
    ];
    let bounds_by_node = HashMap::from_iter([(
        2usize,
        SemanticRect {
            x: 10.0,
            y: 20.0,
            width: 200.0,
            height: 400.0,
        },
    )]);

    let found = find_text_in_semantics_tree(
        &bounds_by_node,
        &canvas,
        "CROWN",
        SemanticTextMatchKind::Exact,
    )
    .expect("a drawn label should be findable");
    assert_eq!(found.text.as_deref(), Some("CROWN"));
    assert_eq!((found.bounds.x, found.bounds.y), (10.0, 20.0));
    assert_eq!(found.bounds.height, 30.0);

    let button = find_button_in_semantics_tree(
        &bounds_by_node,
        &canvas,
        "Haptics",
        SemanticTextMatchKind::Exact,
    )
    .expect("a drawn control should be findable as a button");
    assert_eq!(button.text.as_deref(), Some("Haptics"));
    assert_eq!((button.bounds.y, button.bounds.height), (60.0, 52.0));

    assert!(
        find_button_in_semantics_tree(
            &bounds_by_node,
            &canvas,
            "CROWN",
            SemanticTextMatchKind::Exact,
        )
        .is_none()
    );
}

#[test]
fn robot_snapshots_report_the_controls_a_canvas_published() {
    use cranpose_ui::{CanvasSemanticsNode, SemanticsWidgetRole};

    let mut canvas = sample_semantics_node(2, SemanticsRole::Layout, false, None, Vec::new());
    canvas.canvas_children = vec![
        CanvasSemanticsNode::control(
            1,
            cranpose_ui::Rect {
                x: 4.0,
                y: 8.0,
                width: 100.0,
                height: 52.0,
            },
            "Haptics",
        )
        .with_role(SemanticsWidgetRole::Switch)
        .with_state_description("On"),
        CanvasSemanticsNode::text(
            2,
            cranpose_ui::Rect {
                x: 4.0,
                y: 70.0,
                width: 100.0,
                height: 20.0,
            },
            "CROWN",
        ),
    ];

    let mut bounds_for = |node_id: NodeId| {
        assert_eq!(node_id, 2);
        SemanticRect {
            x: 10.0,
            y: 20.0,
            width: 200.0,
            height: 200.0,
        }
    };
    let element = semantic_element_from_semantics_node(&canvas, &mut bounds_for);

    assert_eq!(element.children.len(), 2);
    let switch = &element.children[0];
    assert_eq!(switch.role, "Switch");
    assert_eq!(switch.text.as_deref(), Some("Haptics"));
    assert_eq!(switch.state_description.as_deref(), Some("On"));
    assert!(switch.clickable);
    assert_eq!((switch.bounds.x, switch.bounds.y), (14.0, 28.0));

    let header = &element.children[1];
    assert_eq!(header.role, "Text");
    assert!(!header.clickable);
}

fn sample_semantics_and_layout() -> (SemanticsNode, LayoutBox) {
    let button_label = sample_semantics_node(
        3,
        SemanticsRole::Text {
            value: "Increase depth".to_string(),
        },
        false,
        None,
        Vec::new(),
    );
    let depth_label = sample_semantics_node(
        4,
        SemanticsRole::Text {
            value: "Current depth: 15".to_string(),
        },
        false,
        None,
        Vec::new(),
    );
    let root = sample_semantics_node(
        1,
        SemanticsRole::Layout,
        false,
        Some("Root"),
        vec![
            sample_semantics_node(2, SemanticsRole::Button, true, None, vec![button_label]),
            depth_label,
        ],
    );
    let layout = sample_layout_box(
        1,
        (0.0, 0.0, 100.0, 100.0),
        vec![
            sample_layout_box(
                2,
                (10.0, 10.0, 40.0, 20.0),
                vec![sample_layout_box(3, (12.0, 12.0, 36.0, 12.0), Vec::new())],
            ),
            sample_layout_box(4, (10.0, 40.0, 60.0, 12.0), Vec::new()),
        ],
    );
    (root, layout)
}

#[test]
fn robot_idle_wait_animation_loop_only_requires_settled_frames() {
    assert!(robot_wait_for_idle_animation_loop_only(
        true, false, false, 1, 1
    ));
    assert!(!robot_wait_for_idle_animation_loop_only(
        true, true, false, 1, 1
    ));
    assert!(!robot_wait_for_idle_animation_loop_only(
        true, false, true, 1, 1
    ));
    assert!(!robot_wait_for_idle_animation_loop_only(
        true, false, false, 0, 1
    ));
    assert!(!robot_wait_for_idle_animation_loop_only(
        true, false, false, 1, 0
    ));
    assert!(!robot_wait_for_idle_animation_loop_only(
        false, false, false, 1, 1
    ));
}

#[test]
fn robot_driver_panic_payload_formats_static_str() {
    assert_eq!(
        panic_payload_message(Box::new("driver failed")),
        "driver failed"
    );
}

#[test]
fn robot_driver_panic_payload_formats_string() {
    assert_eq!(
        panic_payload_message(Box::new(String::from("driver failed"))),
        "driver failed"
    );
}

#[test]
fn robot_text_query_finds_prefix_without_building_snapshot() {
    let (semantics, layout) = sample_semantics_and_layout();
    let result = find_text_in_trees(
        &semantics,
        &layout,
        "Current depth:",
        SemanticTextMatchKind::Prefix,
    )
    .expect("prefix match");

    assert_eq!(result.text.as_deref(), Some("Current depth: 15"));
    assert_eq!(result.bounds.x, 10.0);
    assert_eq!(result.bounds.y, 40.0);
}

#[test]
fn robot_button_query_matches_descendant_text() {
    let (semantics, layout) = sample_semantics_and_layout();
    let result = find_button_in_trees(
        &semantics,
        &layout,
        "Increase depth",
        SemanticTextMatchKind::Exact,
    )
    .expect("button match");

    assert_eq!(result.bounds.width, 40.0);
    assert_eq!(result.bounds.height, 20.0);
}

#[test]
fn robot_subtree_text_match_honors_exact_mode() {
    let (semantics, _) = sample_semantics_and_layout();

    assert!(subtree_contains_matching_text(
        &semantics,
        "Current depth: 15",
        SemanticTextMatchKind::Exact,
    ));
    assert!(!subtree_contains_matching_text(
        &semantics,
        "Current depth:",
        SemanticTextMatchKind::Exact,
    ));
}

#[test]
fn robot_semantics_export_uses_node_ids_for_bounds() {
    let (semantics, _) = sample_semantics_and_layout();
    let mut bounds_for = |node_id: NodeId| SemanticRect {
        x: node_id as f32,
        y: node_id as f32 * 2.0,
        width: 10.0,
        height: 5.0,
    };

    let exported = semantic_element_from_semantics_node(&semantics, &mut bounds_for);

    assert_eq!(exported.bounds.x, 1.0);
    assert_eq!(exported.children.len(), 2);
    assert_eq!(exported.children[0].bounds.x, 2.0);
    assert_eq!(exported.children[0].children[0].bounds.x, 3.0);
    assert_eq!(
        exported.children[1].text.as_deref(),
        Some("Current depth: 15")
    );
}
