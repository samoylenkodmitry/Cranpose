mod robot_exit;
mod robot_launch;

use cranpose::{Robot, SemanticElement};
use cranpose_testing::{changed_pixel_count_in_region, find_element_by_text_exact};
use desktop_app::app::{self, DemoTab, GLASS_FEED_LIST_TAG};

fn receipts(elements: &[SemanticElement]) -> Vec<(usize, f32)> {
    fn collect(element: &SemanticElement, values: &mut Vec<(usize, f32)>) {
        if let Some(index) = element
            .text
            .as_deref()
            .and_then(|text| text.strip_prefix("Receipt #"))
            .and_then(|text| text.split_whitespace().next())
            .and_then(|index| index.parse().ok())
        {
            values.push((index, element.bounds.y));
        }
        for child in &element.children {
            collect(child, values);
        }
    }
    let mut values = Vec::new();
    for element in elements {
        collect(element, &mut values);
    }
    values.sort_by_key(|&(index, _)| index);
    values.dedup_by_key(|value| value.0);
    values
}

fn check_scroll(robot: &Robot) {
    robot.wait_for_idle().expect("initial layout");
    let semantics = robot.get_semantics().expect("initial semantics");
    let viewport = find_element_by_text_exact(&semantics, GLASS_FEED_LIST_TAG)
        .expect("Glass Feed viewport")
        .bounds;
    robot
        .mouse_move(
            viewport.x + viewport.width * 0.5,
            viewport.y + viewport.height * 0.8,
        )
        .expect("hover the unobscured list");
    let initial = receipts(&semantics);
    assert!(initial.len() >= 2, "expected multiple receipt anchors");
    let stride = (initial[1].1 - initial[0].1) / (initial[1].0 - initial[0].0) as f32;
    assert!(stride > 0.0);
    let region = (
        viewport.x,
        viewport.y + viewport.height * 0.5,
        viewport.width,
        viewport.height * 0.4,
    );
    let mut previous = robot.screenshot().expect("initial frame");
    let mut expected_scroll = 0.0;
    for (step, delta) in [-300.0, 300.0, -40.0, -1.0, -50.0, -77.0]
        .into_iter()
        .chain(std::iter::repeat_n(-50.0, 16))
        .chain(std::iter::repeat_n(50.0, 16))
        .enumerate()
    {
        robot.mouse_scroll(0.0, delta).expect("dispatch wheel");
        robot.wait_for_idle().expect("settle wheel");
        let frame = robot.screenshot().expect("wheel frame");
        let anchors = receipts(&robot.get_semantics().expect("wheel semantics"));
        let &(index, y) = anchors.first().expect("receipt anchor after wheel");
        expected_scroll -= delta;
        let observed_scroll = initial[0].1 + (index - initial[0].0) as f32 * stride - y;
        let changed = changed_pixel_count_in_region(&previous, &frame, region, 0);
        println!(
            "wheel step={step} delta={delta} expected={expected_scroll} observed={observed_scroll} changed_pixels={changed}"
        );
        if (observed_scroll - expected_scroll).abs() > 0.1 || changed == 0 {
            robot_exit::fail(
                robot,
                "wheel input did not move receipt geometry and pixels",
            );
        }
        previous = frame;
    }
}

fn main() {
    env_logger::init();
    robot_exit::arm_timeout(90);
    robot_launch::launch("Glass Feed Wheel", 800, 800)
        .with_test_driver(|robot| {
            check_scroll(&robot);
            println!("PASS: large, one-pixel and repeated wheel inputs move Glass Feed");
            robot.exit().expect("exit");
        })
        .run(|| app::combined_app_with_initial_tab(Some(DemoTab::GlassFeed)));
}
