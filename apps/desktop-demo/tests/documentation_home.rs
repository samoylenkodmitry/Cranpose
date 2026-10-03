use cranpose_services::{AccessibilityOptions, ProvideAccessibilityOptions};
use cranpose_testing::robot::{RobotTestRule, TestRenderer};
use desktop_app::app::{combined_app_with_initial_tab, startup_tab_from_args, DemoTab};

fn text_bounds(rects: &[(cranpose_ui::Rect, Option<String>)], label: &str) -> cranpose_ui::Rect {
    rects
        .iter()
        .find(|(_, text)| text.as_deref() == Some(label))
        .map(|(bounds, _)| *bounds)
        .expect("text in page snapshot")
}

fn semantic_bounds(root: &cranpose_testing::PlacedSemanticsNode, label: &str) -> cranpose_ui::Rect {
    let mut found = None;
    root.visit(&mut |node| {
        if (found.is_none() || node.clickable) && node.label.as_deref() == Some(label) {
            found = Some(node.target_bounds());
        }
    });
    found.expect("named surface in accessibility tree")
}

fn click_control_unsettled(robot: &mut RobotTestRule<TestRenderer>, label: &str) -> bool {
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    let tree = cranpose_testing::placed_semantics_from_shell(robot.shell_mut())
        .expect("control semantics");
    let (_, height) = robot.viewport_size();
    let mut target = None;
    tree.visit(&mut |node| {
        let bounds = node.target_bounds();
        if node.clickable
            && node.label.as_deref() == Some(label)
            && bounds.y >= 0.0
            && bounds.y + bounds.height <= height as f32
        {
            target = Some(bounds);
        }
    });
    let Some(bounds) = target else {
        return false;
    };
    robot.click_at(
        bounds.x + bounds.width * 0.5,
        bounds.y + bounds.height * 0.5,
    )
}

fn settle_motion(robot: &mut RobotTestRule<TestRenderer>) {
    for _ in 0..120 {
        robot.advance_time(16_666_667);
        if !robot.shell_mut().has_active_animations()
            && !robot.shell_mut().has_transient_frame_callbacks()
        {
            robot.wait_for_idle();
            return;
        }
    }
    panic!("documentation motion must settle");
}

fn click_control(robot: &mut RobotTestRule<TestRenderer>, label: &str) -> bool {
    let clicked = click_control_unsettled(robot, label);
    settle_motion(robot);
    clicked
}

#[test]
fn guide_tables_place_headers_and_values_in_columns() {
    let mut robot = RobotTestRule::new(390, 780, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Guide));
    });
    robot.move_to(120.0, 500.0);
    robot.shell_mut().pointer_scrolled(0.0, -13.0 * 120.0);
    robot.wait_for_idle();
    assert!(click_control(&mut robot, "Testing"));
    robot.move_to(320.0, 600.0);
    robot.shell_mut().pointer_scrolled(0.0, -420.0);
    robot.wait_for_idle();
    let rects = robot.get_all_rects();
    let scope = text_bounds(&rects, "Test scope");
    let api = text_bounds(&rects, "API");
    let value = text_bounds(&rects, "Composition and layout");
    assert!(api.x > scope.x + 40.0, "table columns: {scope:?}, {api:?}");
    assert!((api.y - scope.y).abs() < 2.0, "one header row");
    assert!(value.y > scope.y + scope.height, "data follows the header");
    assert!(robot
        .get_all_text()
        .iter()
        .all(|text| !text.contains("| --- |")));
}

#[test]
fn mobile_chapter_list_stays_visible_beside_the_wheel() {
    let mut robot = RobotTestRule::new(390, 780, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Guide));
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    let tree = cranpose_testing::placed_semantics_from_shell(robot.shell_mut())
        .expect("mobile guide navigation");
    let list = semantic_bounds(&tree, "Guide chapters");
    assert!(list.x > 150.0 && list.x + list.width <= 390.0);
    let initial = text_bounds(&robot.get_all_rects(), "Get started");
    robot.move_to(80.0, 500.0);
    robot.shell_mut().pointer_scrolled(0.0, -120.0);
    robot.wait_for_idle();
    let after = text_bounds(&robot.get_all_rects(), "Get started");
    assert!(
        (initial.y - after.y).abs() < 1.0,
        "chapter list stays in place"
    );
    assert!(click_control(&mut robot, "Get started"));
    assert!(robot
        .find_by_text("Start with the project template")
        .exists());
}

#[test]
#[ignore = "CPU scroll measurement; run before and after a guide layout change"]
fn guide_scroll_cpu_profile() {
    let mut robot = RobotTestRule::new(390, 780, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Guide));
    });
    assert!(click_control(&mut robot, "Get started"));
    robot.shell_mut().set_semantics_enabled(false);
    robot.move_to(320.0, 620.0);
    let mut samples = Vec::with_capacity(120);
    for frame in 0..140 {
        let start = std::time::Instant::now();
        robot.shell_mut().pointer_scrolled(0.0, -18.0);
        robot.advance_time(16_666_667);
        robot.wait_for_idle();
        if frame >= 20 {
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    samples.sort_by(f64::total_cmp);
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    eprintln!("guide_scroll_cpu_ms mean={mean:.3} p95={:.3}", samples[114]);
}

#[test]
fn selecting_a_chapter_animates_the_wheel_and_reader_together() {
    let mut robot = RobotTestRule::new(1200, 800, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Guide));
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    let before = semantic_bounds(
        &cranpose_testing::placed_semantics_from_shell(robot.shell_mut()).expect("wheel semantics"),
        "Ownership",
    );
    assert!(click_control_unsettled(&mut robot, "Get started"));
    for _ in 0..20 {
        robot.advance_time(16_666_667);
    }
    let during = semantic_bounds(
        &cranpose_testing::placed_semantics_from_shell(robot.shell_mut()).expect("moving wheel"),
        "Ownership",
    );
    for _ in 0..60 {
        robot.advance_time(16_666_667);
    }
    let after = semantic_bounds(
        &cranpose_testing::placed_semantics_from_shell(robot.shell_mut()).expect("settled wheel"),
        "Ownership",
    );
    assert!(
        before.y > during.y + 5.0 && during.y > after.y + 5.0,
        "selection needs intermediate positions: {before:?} -> {during:?} -> {after:?}"
    );
    assert!(robot
        .find_by_text("Start with the project template")
        .exists());
}

#[test]
fn manual_reader_scroll_interrupts_a_chapter_animation() {
    let mut robot = RobotTestRule::new(1200, 800, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Guide));
    });
    assert!(click_control_unsettled(&mut robot, "Get started"));
    for _ in 0..18 {
        robot.advance_time(16_666_667);
    }
    robot.shell_mut().set_cursor(1040.0, 650.0);
    robot.shell_mut().pointer_scrolled(0.0, 9000.0);
    settle_motion(&mut robot);
    assert!(robot
        .find_by_text("Build apps with Compose in Rust.")
        .exists());
    assert!(click_control(&mut robot, "Get started"));
    assert!(robot
        .find_by_text("Start with the project template")
        .exists());
}

#[test]
fn reading_scroll_moves_the_tab_row_and_document_together() {
    let mut robot = RobotTestRule::new(1200, 800, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Guide));
    });
    let before = robot.get_all_rects();
    let tabs_before = text_bounds(&before, "Counter App");
    let title_before = text_bounds(&before, "Build apps with Compose in Rust.");
    robot.move_to(1040.0, 650.0);
    robot.shell_mut().pointer_scrolled(0.0, -40.0);
    robot.wait_for_idle();
    let after = robot.get_all_rects();
    let tabs_after = text_bounds(&after, "Counter App");
    let title_after = text_bounds(&after, "Build apps with Compose in Rust.");
    let displacement = tabs_before.y - tabs_after.y;
    assert!(
        displacement > 20.0,
        "the document must carry its tab row out of view"
    );
    assert!(
        (title_before.y - title_after.y - displacement).abs() < 1.0,
        "the article and tabs must share one scroll offset: tabs {tabs_before:?} -> {tabs_after:?}, title {title_before:?} -> {title_after:?}"
    );
    assert!(click_control(&mut robot, "Get started"));
    let after_jump = robot.get_all_rects();
    let tabs = text_bounds(&after_jump, "Counter App");
    assert!(
        tabs.y + tabs.height <= 0.0,
        "tabs must leave the window after a chapter jump: {tabs:?}"
    );
}

#[test]
fn dragging_and_releasing_keeps_the_wheel_moving() {
    let mut robot = RobotTestRule::new(390, 780, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Guide));
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    let initial = semantic_bounds(
        &cranpose_testing::placed_semantics_from_shell(robot.shell_mut()).expect("wheel semantics"),
        "Ownership",
    );
    robot.shell_mut().set_cursor(120.0, 550.0);
    robot.shell_mut().pointer_pressed_at_time(Some(0));
    for step in 1..=8 {
        robot.advance_time(16_666_667);
        robot
            .shell_mut()
            .set_cursor_at_time(120.0, 550.0 - step as f32 * 12.0, Some(step * 16));
    }
    let tree =
        cranpose_testing::placed_semantics_from_shell(robot.shell_mut()).expect("rotating wheel");
    let held = semantic_bounds(&tree, "Ownership");
    assert!(
        held.y < initial.y - 30.0,
        "dragging must rotate the wheel: {initial:?} -> {held:?}"
    );
    robot
        .shell_mut()
        .pointer_released_at_position_time(120.0, 454.0, Some(136));
    for _ in 0..30 {
        robot.advance_time(16_666_667);
    }
    let released = semantic_bounds(
        &cranpose_testing::placed_semantics_from_shell(robot.shell_mut()).expect("coasting wheel"),
        "Ownership",
    );
    assert!(
        released.y < held.y - 30.0,
        "the wheel must coast after a fast release: {held:?} -> {released:?}"
    );
}

#[test]
fn wide_article_aligns_with_the_wheel_edge() {
    let mut robot = RobotTestRule::new(1800, 1000, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Guide));
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    let tree =
        cranpose_testing::placed_semantics_from_shell(robot.shell_mut()).expect("reader semantics");
    let glass = semantic_bounds(&tree, "Guide glass");
    let text = text_bounds(&robot.get_all_rects(), "Build apps with Compose in Rust.");
    assert!(
        text.x - glass.x <= 25.0 && text.x <= 325.0,
        "article should align close to the wheel: {text:?}, {glass:?}"
    );
}

#[test]
fn wheel_brand_clears_the_tabs_in_a_short_window() {
    let mut robot = RobotTestRule::new(800, 600, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Guide));
    });
    let rects = robot.get_all_rects();
    let brand = text_bounds(&rects, "CRANPOSE");
    let tabs = text_bounds(&rects, "Counter App");
    assert!(
        brand.y > tabs.y + tabs.height + 12.0,
        "brand must clear the initial tabs: {brand:?}, {tabs:?}"
    );
}

#[test]
fn default_desktop_home_reads_offline_and_keeps_counter_navigation() {
    let mut robot = RobotTestRule::new(1200, 800, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(startup_tab_from_args(std::iter::empty())));
    });
    assert!(robot
        .find_by_text("Build apps with Compose in Rust.")
        .exists());
    assert!(click_control(&mut robot, "Get started"));
    assert!(robot
        .find_by_text("Start with the project template")
        .exists());
    robot.move_to(1040.0, 650.0);
    robot.shell_mut().pointer_scrolled(0.0, 9000.0);
    robot.wait_for_idle();
    assert!(click_control(&mut robot, "Counter App"));
    assert!(!robot.find_by_text("Cranpose guide").exists());
    assert!(click_control(&mut robot, "Cranpose Guide"));
    assert!(robot
        .find_by_text("Build apps with Compose in Rust.")
        .exists());
}

#[test]
fn wheel_extends_beneath_the_edge_to_edge_reader_without_stealing_clicks() {
    let mut robot = RobotTestRule::new(1200, 1400, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Guide));
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    let tree =
        cranpose_testing::placed_semantics_from_shell(robot.shell_mut()).expect("placed semantics");
    let page = semantic_bounds(&tree, "Cranpose guide");
    let glass = semantic_bounds(&tree, "Guide glass");
    let wheel = semantic_bounds(&tree, "Guide wheel");
    assert_eq!(page.x, 0.0);
    assert_eq!(page.width, 1200.0);
    assert_eq!(glass.x + glass.width, 1200.0);
    assert!(wheel.width <= glass.x && wheel.height > 800.0);
    let x = glass.x + glass.width - 20.0;
    let y = 650.0;
    robot.click_at(x, y);
    assert!(robot
        .find_by_text("Build apps with Compose in Rust.")
        .exists());
}

#[test]
fn documentation_is_usable_in_a_compact_window() {
    let mut robot = RobotTestRule::new(390, 780, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Guide));
    });
    assert!(!robot
        .find_by_text("Build apps with Compose in Rust.")
        .exists());
    assert!(robot.find_by_text("Welcome").exists());
    assert!(robot.find_by_text("View on GitHub").exists());
    robot.move_to(150.0, 620.0);
    robot.shell_mut().pointer_scrolled(0.0, -150.0);
    robot.wait_for_idle();
    assert!(!robot
        .find_by_text("Start with the project template")
        .exists());
    assert!(click_control(&mut robot, "Get started"));
    assert!(robot
        .find_by_text("Start with the project template")
        .exists());
    assert!(click_control(&mut robot, "Back to wheel"));
    assert!(!robot
        .find_by_text("Start with the project template")
        .exists());
    assert!(click_control(&mut robot, "Get started"));
    assert!(robot
        .find_by_text("Start with the project template")
        .exists());
}

#[test]
fn wheel_scroll_navigates_the_article_and_github_stays_at_the_window_corner() {
    let mut robot = RobotTestRule::new(1200, 800, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Guide));
    });
    let before = robot.get_all_rects();
    let github = before
        .iter()
        .find(|(_, label)| label.as_deref() == Some("View on GitHub"))
        .map(|(bounds, _)| *bounds)
        .expect("repository label");
    assert!(github.x < 100.0 && github.y > 740.0);
    robot.move_to(160.0, 620.0);
    robot.shell_mut().pointer_scrolled(0.0, -150.0);
    robot.wait_for_idle();
    let after = robot.get_all_rects();
    let anchored = after
        .iter()
        .find(|(_, label)| label.as_deref() == Some("View on GitHub"))
        .map(|(bounds, _)| *bounds)
        .expect("repository label");
    assert!((github.y - anchored.y).abs() < 1.0);
    assert!(click_control(&mut robot, "Get started"));
    let after = robot.get_all_rects();
    let article = text_bounds(&after, "Start with the project template");
    assert!(
        article.y >= 0.0 && article.y < 600.0,
        "wheel must bring the next section into view: {article:?}"
    );
}

#[test]
fn reader_actions_move_between_chapters_without_the_section_wheel() {
    let mut robot = RobotTestRule::new(1200, 800, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Guide));
    });
    assert!(robot.find_by_text("View on GitHub").exists());
    assert!(click_control(&mut robot, "Next section"));
    assert!(robot
        .find_by_text("Start with the project template")
        .exists());
    assert!(click_control(&mut robot, "Previous section"));
    assert!(robot
        .find_by_text("Build apps with Compose in Rust.")
        .exists());
}

#[test]
fn resized_guide_draws_beyond_its_original_window_edges() {
    let Some(mut shell) = guide_pixel_shell(800, 600) else {
        eprintln!("skipping guide resize pixels: no headless GPU");
        return;
    };
    for (width, height) in [(800, 600), (1440, 1100), (900, 700), (1700, 1200)] {
        shell.set_buffer_size(width, height);
        shell.set_viewport(width as f32, height as f32);
        shell.update();
        shell.set_cursor(120.0, 350.0);
        shell.pointer_scrolled(0.0, -600.0);
        for _ in 0..8 {
            shell.update();
            shell
                .renderer()
                .capture_frame(width, height)
                .expect("warm resize capture");
        }
        let frame = shell
            .renderer()
            .capture_frame(width, height)
            .expect("resized guide pixels");
        let pixel = |x, y| {
            let offset = ((y * width + x) * 4) as usize;
            &frame.pixels[offset..offset + 4]
        };
        let corner = pixel(width - 8, height - 8);
        let expanded = corner[2] > corner[0] && corner[1] > corner[0];
        let link_has_ink = (height - 35..height - 8).any(|y| {
            (8..130).any(|x| {
                let color = pixel(x, y);
                color[0] > 180 && color[1] > 180 && color[2] > 180
            })
        });
        let wheel_has_ink = (240..400).any(|y| {
            (40..190).any(|x| {
                let color = pixel(x, y);
                color[0] > 180 && color[1] > 180 && color[2] > 180
            })
        });
        if !expanded || !link_has_ink || !wheel_has_ink {
            let directory = std::env::var_os("CARGO_TARGET_DIR")
                .map_or_else(
                    || std::path::PathBuf::from("target"),
                    std::path::PathBuf::from,
                )
                .join("documentation-resize");
            std::fs::create_dir_all(&directory).expect("resize evidence directory");
            image::save_buffer(
                directory.join(format!("{width}-{height}.png")),
                &frame.pixels,
                width,
                height,
                image::ColorType::Rgba8,
            )
            .expect("resize evidence");
        }
        assert!(
            expanded,
            "guide must paint its expanded lower-right corner at {width}×{height}: {corner:?}"
        );
        assert!(
            link_has_ink,
            "GitHub must be painted at the new bottom-left at {width}×{height}"
        );
        assert!(
            wheel_has_ink,
            "rotated chapter labels must stay visible after resizing to {width}×{height}"
        );
    }
}

fn guide_pixel_shell(
    width: u32,
    height: u32,
) -> Option<cranpose_app_shell::AppShell<cranpose_render_wgpu::WgpuRenderer>> {
    let renderer = crate::liquid_page_support::headless_renderer("Guide visual regression")?;
    Some(cranpose_app_shell::AppShell::new_with_size_and_density(
        renderer,
        cranpose_core::location_key(file!(), line!(), column!()),
        || combined_app_with_initial_tab(Some(DemoTab::Guide)),
        (width, height),
        (width as f32, height as f32),
        1.0,
    ))
}

#[test]
fn mobile_wheel_ring_remains_visible_after_rotation() {
    let Some(mut shell) = guide_pixel_shell(390, 780) else {
        eprintln!("skipping wheel pixels: no headless GPU");
        return;
    };
    let mut initial_rows = 0;
    for (step, delta) in [0.0, -480.0, -480.0].into_iter().enumerate() {
        shell.set_cursor(80.0, 500.0);
        shell.pointer_scrolled(0.0, delta);
        for _ in 0..8 {
            shell.update();
            shell
                .renderer()
                .capture_frame(390, 780)
                .expect("warm wheel");
        }
        let frame = shell
            .renderer()
            .capture_frame(390, 780)
            .expect("wheel pixels");
        let rows = (130..480)
            .filter(|y| {
                (150..390).any(|x| {
                    let index = (y * 390 + x) * 4;
                    let pixel = &frame.pixels[index..index + 4];
                    pixel[0] < 150
                        && pixel[1] > 110
                        && pixel[2] > 110
                        && u16::from(pixel[1]) > u16::from(pixel[0]) * 3 / 2
                })
            })
            .count();
        eprintln!("wheel ring rows step={step} count={rows}");
        if step == 0 {
            initial_rows = rows;
            assert!(initial_rows > 250, "ring covers the visible arc");
        } else {
            assert!(
                rows * 10 >= initial_rows * 9,
                "wheel rotation preserves its ring: {initial_rows} -> {rows}"
            );
        }
    }
}

#[test]
fn growing_the_window_expands_the_guide_and_preserves_the_corner_link() {
    let mut robot = RobotTestRule::new(800, 600, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Guide));
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    for (width, height) in [(1440, 1100), (900, 700), (1700, 1200)] {
        robot.set_viewport(width, height);
        robot.wait_for_idle();
        let tree = cranpose_testing::placed_semantics_from_shell(robot.shell_mut())
            .expect("resized documentation");
        let page = semantic_bounds(&tree, "Cranpose guide");
        let glass = semantic_bounds(&tree, "Guide glass");
        let github = semantic_bounds(&tree, "View on GitHub");
        assert_eq!(page.width, width as f32);
        assert_eq!(page.height, height as f32);
        assert!((glass.x + glass.width - width as f32).abs() < 1.0);
        assert!(
            github.y > height as f32 - 55.0,
            "corner link must follow a growing window: {github:?}"
        );
        assert!(github.x < 20.0);
        assert!(click_control(&mut robot, "Next section"));
        assert!(robot
            .find_by_text("Start with the project template")
            .exists());
        assert!(click_control(&mut robot, "Previous section"));
    }
}

#[test]
fn selected_chapter_survives_resizing_between_reader_layouts() {
    let mut robot = RobotTestRule::new(1200, 800, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Guide));
    });
    assert!(click_control(&mut robot, "Get started"));
    robot.set_viewport(390, 780);
    assert!(robot
        .find_by_text("Start with the project template")
        .exists());
    assert!(click_control(&mut robot, "Back to wheel"));
    assert!(click_control(&mut robot, "Get started"));
    assert!(robot
        .find_by_text("Start with the project template")
        .exists());
    robot.set_viewport(1200, 800);
    assert!(robot
        .find_by_text("Start with the project template")
        .exists());
    assert!(robot.find_by_text("View on GitHub").exists());
}

#[test]
fn compact_wheel_reveals_the_current_chapter_after_reading_to_the_end() {
    let mut robot = RobotTestRule::new(390, 780, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Guide));
    });
    assert!(click_control(&mut robot, "Welcome"));
    for _ in 1..include_str!("../../../docs/guide.md")
        .matches("\n## ")
        .count()
    {
        assert!(click_control(&mut robot, "Next section"));
    }
    assert!(robot
        .find_by_text("Place a platform control in your Cranpose layout.")
        .exists());
    assert!(click_control(&mut robot, "Back to wheel"));
    let current = semantic_bounds(
        &cranpose_testing::placed_semantics_from_shell(robot.shell_mut()).expect("current wheel"),
        "Native views",
    );
    assert!(current.y >= 0.0 && current.y + current.height <= 780.0);
    assert!(click_control(&mut robot, "Native views"));
    assert!(robot
        .find_by_text("Place a platform control in your Cranpose layout.")
        .exists());
}

#[test]
fn reader_navigation_fits_with_larger_text_and_reduced_effects() {
    let mut robot = RobotTestRule::new(390, 780, TestRenderer::default(), || {
        ProvideAccessibilityOptions(
            AccessibilityOptions {
                font_scale: 1.5,
                reduce_motion: true,
                reduce_transparency: true,
                ..Default::default()
            },
            || combined_app_with_initial_tab(Some(DemoTab::Guide)),
        );
    });
    assert!(click_control(&mut robot, "Welcome"));
    let action = robot
        .find_by_text("Next section")
        .bounds()
        .expect("next chapter action");
    assert!(action.x >= 0.0 && action.x + action.width <= 390.0);
    assert!(action.y >= 0.0 && action.y + action.height <= 780.0);
    assert!(click_control(&mut robot, "Next section"));
    assert!(robot
        .find_by_text("Start with the project template")
        .exists());
}

#[test]
fn documentation_aliases_and_existing_startup_routes_are_available() {
    for alias in ["guide", "cranposeguide"] {
        assert_eq!(DemoTab::from_startup_name(alias), Some(DemoTab::Guide));
    }
    assert_eq!(
        DemoTab::from_startup_name("counter"),
        Some(DemoTab::Counter)
    );
}

#[test]
fn article_scroll_rotates_the_wheel_to_the_current_section() {
    let mut robot = RobotTestRule::new(1200, 800, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Guide));
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    let tree =
        cranpose_testing::placed_semantics_from_shell(robot.shell_mut()).expect("wheel semantics");
    let welcome_before = semantic_bounds(&tree, "Get started");
    robot.move_to(1040.0, 650.0);
    for _ in 0..20 {
        robot.shell_mut().pointer_scrolled(0.0, -180.0);
        robot.wait_for_idle();
        let tree =
            cranpose_testing::placed_semantics_from_shell(robot.shell_mut()).expect("semantics");
        let mut current = false;
        tree.visit(&mut |node| {
            if node.label.as_deref() == Some("Get started") && node.selected == Some(true) {
                current = true;
            }
        });
        if current {
            let welcome_after = semantic_bounds(&tree, "Get started");
            assert!(welcome_after.y < welcome_before.y - 70.0);
            return;
        }
    }
    panic!("reading into Get started must select and rotate its wheel entry");
}
