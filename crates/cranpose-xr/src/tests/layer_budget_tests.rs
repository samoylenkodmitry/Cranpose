use super::*;

// What the specification guarantees, and what a Quest 3 takes
const LIMIT: usize = 16;

const PANEL: PanelNeed = PanelNeed {
    id: 0,
    distance: 0.0,
    priority: 0,
    pointed: false,
    content: true,
    surfaces: 0,
    handles: 0,
};

const NO_FIXED: FixedLayers = FixedLayers {
    layers: 0,
    projection: Projection::Absent,
};

// Plans a frame of `panels` in the order of `push`, the farthest first, each 10 cm nearer than the
// one before it and named by its place.
fn plan_frame(
    budget: &mut LayerBudget,
    limit: usize,
    fixed: FixedLayers,
    panels: &[PanelNeed],
) -> Budget {
    budget.clear();
    for (index, &panel) in panels.iter().enumerate() {
        budget.push(PanelNeed {
            id: index as u64,
            distance: 0.1 * (panels.len() - index) as f32,
            ..panel
        });
    }
    budget.plan(limit, fixed)
}

fn planned(limit: usize, fixed: FixedLayers, panels: &[PanelNeed]) -> (Budget, Vec<Placement>) {
    let mut budget = LayerBudget::default();
    let plan = plan_frame(&mut budget, limit, fixed, panels);
    let placements = (0..panels.len())
        .map(|index| budget.placement(index))
        .collect();
    (plan, placements)
}

fn contents(placements: &[Placement], place: Place) -> usize {
    placements
        .iter()
        .filter(|placement| placement.content == place)
        .count()
}

#[test]
fn fifteen_and_sixteen_panels_keep_their_own_layers() {
    for panels in [15, 16] {
        let (plan, placements) = planned(LIMIT, NO_FIXED, &vec![PANEL; panels]);
        assert_eq!(
            plan,
            Budget {
                needed: panels,
                layers: panels,
                projected: 0,
                left_out: 0,
            }
        );
        assert!(
            placements
                .iter()
                .all(|placement| *placement == Placement::default())
        );
    }
}

#[test]
fn seventeen_panels_draw_the_two_farthest_into_a_new_projection_layer() {
    let (plan, placements) = planned(LIMIT, NO_FIXED, &[PANEL; 17]);
    // The projection layer takes one place, so 15 panels keep theirs
    assert_eq!(
        plan,
        Budget {
            needed: 17,
            layers: 16,
            projected: 2,
            left_out: 0,
        }
    );
    assert_eq!(placements[0].content, Place::Projected);
    assert_eq!(placements[1].content, Place::Projected);
    assert_eq!(contents(&placements[2..], Place::Own), 15);
}

#[test]
fn nineteen_panels_draw_the_four_farthest_into_the_projection_layer() {
    let (plan, placements) = planned(LIMIT, NO_FIXED, &[PANEL; 19]);
    assert_eq!((plan.layers, plan.projected, plan.left_out), (16, 4, 0));
    assert_eq!(contents(&placements[..4], Place::Projected), 4);
    assert_eq!(contents(&placements[4..], Place::Own), 15);
}

#[test]
fn thirty_two_panels_keep_their_own_layers_only_where_the_runtime_takes_them() {
    let (plan, placements) = planned(LIMIT, NO_FIXED, &[PANEL; 32]);
    assert_eq!((plan.layers, plan.projected, plan.left_out), (16, 17, 0));
    assert_eq!(contents(&placements, Place::LeftOut), 0);
    let (plan, placements) = planned(32, NO_FIXED, &[PANEL; 32]);
    assert_eq!((plan.layers, plan.projected), (32, 0));
    assert_eq!(contents(&placements, Place::Own), 32);
}

#[test]
fn the_projection_layer_of_the_volumes_takes_the_drawn_panels_at_no_extra_place() {
    let fixed = FixedLayers {
        layers: 1,
        projection: Projection::Present,
    };
    // The room, the projection layer and 15 panels
    let (plan, _) = planned(LIMIT, fixed, &[PANEL; 15]);
    assert_eq!((plan.needed, plan.layers, plan.projected), (17, 16, 1));
}

#[test]
fn handles_go_into_the_projection_layer_before_any_panel() {
    let handled = PanelNeed {
        handles: 2,
        ..PANEL
    };
    // The room, the rays and 6 panels with 2 handles each: 21 layers
    let fixed = FixedLayers {
        layers: 3,
        projection: Projection::Absent,
    };
    let (plan, placements) = planned(LIMIT, fixed, &[handled; 6]);
    assert_eq!(plan.needed, 21);
    // 4 fixed with the projection layer, 6 panels and the handles of the 3 nearest panels
    assert_eq!((plan.layers, plan.projected), (16, 6));
    assert_eq!(contents(&placements, Place::Own), 6);
    let handles = |place| {
        placements
            .iter()
            .filter(|placement| placement.handles == place)
            .count()
    };
    assert_eq!(handles(Place::Projected), 3);
    assert_eq!(placements[5].handles, Place::Own, "the nearest keeps them");
    assert_eq!(placements[0].handles, Place::Projected);
}

#[test]
fn a_higher_priority_and_a_pointer_keep_a_far_panel_on_its_own_layer() {
    let mut panels = [PANEL; 19];
    panels[0].priority = 1;
    panels[1].pointed = true;
    let (_, placements) = planned(LIMIT, NO_FIXED, &panels);
    assert_eq!(placements[0].content, Place::Own);
    assert_eq!(placements[1].content, Place::Own);
    // The four lowest ranks are the farthest panels of priority 0 that no hand points at
    assert_eq!(contents(&placements[2..6], Place::Projected), 4);
    // A negative priority ranks under the farthest panel
    panels[18].priority = -1;
    let (_, placements) = planned(LIMIT, NO_FIXED, &panels);
    assert_eq!(placements[18].content, Place::Projected);
}

#[test]
fn surfaces_keep_their_layers_and_never_go_into_the_projection_layer() {
    let stereo = PanelNeed {
        content: false,
        surfaces: 2,
        ..PANEL
    };
    let mut panels = [PANEL; 17];
    panels[0] = stereo;
    let (plan, placements) = planned(LIMIT, NO_FIXED, &panels);
    assert_eq!(
        placements[0].surfaces,
        Place::Own,
        "the farthest is a surface"
    );
    assert_eq!((plan.needed, plan.layers, plan.projected), (18, 16, 3));
}

#[test]
fn surfaces_beyond_the_limit_are_left_out_the_farthest_first() {
    let surface = PanelNeed {
        content: false,
        surfaces: 1,
        ..PANEL
    };
    let mut panels = [surface; 18];
    panels[17] = PANEL;
    let (plan, placements) = planned(LIMIT, NO_FIXED, &panels);
    // The projection layer takes one place and draws the panel, which leaves 15 for the surfaces
    assert_eq!(placements[17].content, Place::Projected);
    assert_eq!(placements[0].surfaces, Place::LeftOut);
    assert_eq!(placements[1].surfaces, Place::LeftOut);
    assert_eq!(placements[2].surfaces, Place::Own);
    assert_eq!((plan.layers, plan.projected, plan.left_out), (16, 1, 2));
}

#[test]
fn a_frame_whose_projection_layer_failed_leaves_the_lowest_ranks_out() {
    let fixed = FixedLayers {
        layers: 0,
        projection: Projection::Unavailable,
    };
    let (plan, placements) = planned(LIMIT, fixed, &[PANEL; 19]);
    assert_eq!((plan.layers, plan.projected, plan.left_out), (16, 0, 3));
    assert_eq!(contents(&placements[..3], Place::LeftOut), 3);
}

#[test]
fn the_crowded_demo_frame_shows_every_panel() {
    // The frame of the demo that a Quest 3 cut to 16 layers: the room, a streamed scene, the 3D
    // content and both rays, a stereo surface, a mono surface with a handle, a volume's handle,
    // a panel with two handles and 6 more panels: 19 layers.
    let fixed = FixedLayers {
        layers: 4,
        projection: Projection::Present,
    };
    let mut panels = vec![PANEL; 6];
    panels.push(PanelNeed {
        content: false,
        surfaces: 2,
        ..PANEL
    });
    panels.push(PanelNeed {
        content: false,
        surfaces: 1,
        handles: 1,
        ..PANEL
    });
    panels.push(PanelNeed {
        content: false,
        handles: 1,
        ..PANEL
    });
    panels.push(PanelNeed {
        handles: 2,
        ..PANEL
    });
    let (plan, placements) = planned(LIMIT, fixed, &panels);
    assert_eq!(plan.needed, 19);
    // Three handles go into the projection layer, and every panel keeps its own layer
    assert_eq!((plan.layers, plan.projected, plan.left_out), (16, 3, 0));
    assert!(
        placements
            .iter()
            .all(|placement| placement.surfaces == Place::Own)
    );
    assert_eq!(contents(&placements, Place::Own), placements.len());
}

#[test]
fn a_panel_from_the_projection_layer_takes_a_layer_only_when_it_stands_nearer_by_the_margin() {
    let mut budget = LayerBudget::default();
    let mut frame = |distances: [f32; 3]| {
        budget.clear();
        for id in 0..17 {
            let distance = distances.get(id).copied().unwrap_or(1.0);
            budget.push(PanelNeed {
                id: id as u64,
                distance,
                ..PANEL
            });
        }
        budget.plan(LIMIT, NO_FIXED);
        [0, 1, 2].map(|index| budget.placement(index).content)
    };
    use Place::{Own, Projected};
    assert_eq!(frame([3.0, 2.9, 2.8]), [Projected, Projected, Own]);
    // The farthest panel comes 2 cm nearer than the third, which keeps its layer
    assert_eq!(frame([2.78, 2.9, 2.8]), [Projected, Projected, Own]);
    // 10 cm nearer is past the margin
    assert_eq!(frame([2.7, 2.9, 2.8]), [Own, Projected, Projected]);
}
