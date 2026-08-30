#[cfg(test)]
mod robot_tests {
    use cranpose_testing::robot::create_headless_robot_test;

    use crate::app;

    #[test]
    fn test_counter_app_increment() {
        let mut robot = create_headless_robot_test(800, 600, || {
            app::combined_app();
        });

        robot.wait_for_idle();

        assert_eq!(robot.viewport_size(), (800, 600));

        println!("=== Initial Screen State ===");
        robot.dump_screen();

        let texts = robot.get_all_text();
        println!("All text on screen: {:?}", texts);
    }

    #[test]
    fn test_app_interactions() {
        let mut robot = create_headless_robot_test(800, 600, || {
            app::combined_app();
        });

        robot.wait_for_idle();

        robot.click_at(400.0, 300.0);
        robot.wait_for_idle();

        println!("=== After First Click ===");
        robot.dump_screen();

        robot.click_at(200.0, 100.0);
        robot.wait_for_idle();
    }

    #[test]
    fn test_app_drag() {
        let mut robot = create_headless_robot_test(800, 600, || {
            app::combined_app();
        });

        robot.wait_for_idle();

        robot.drag(100.0, 100.0, 300.0, 300.0);
        robot.wait_for_idle();

        println!("=== After Drag ===");
        robot.dump_screen();
    }

    #[test]
    fn test_app_resize() {
        let mut robot = create_headless_robot_test(800, 600, || {
            app::combined_app();
        });

        robot.wait_for_idle();
        println!("=== Initial 800x600 ===");
        robot.dump_screen();

        robot.set_viewport(1024, 768);
        robot.wait_for_idle();

        println!("=== After Resize to 1024x768 ===");
        robot.dump_screen();

        assert_eq!(robot.viewport_size(), (1024, 768));
    }

    #[test]
    fn test_app_complex_flow() {
        let mut robot = create_headless_robot_test(800, 600, || {
            app::combined_app();
        });

        robot.wait_for_idle();

        robot.click_at(100.0, 50.0);
        robot.wait_for_idle();

        robot.move_to(400.0, 300.0);
        robot.wait_for_idle();

        robot.click_at(700.0, 50.0);
        robot.wait_for_idle();

        println!("=== After Interaction Sequence ===");
        robot.dump_screen();
    }

    #[test]
    fn test_app_get_bounds() {
        let mut robot = create_headless_robot_test(800, 600, || {
            app::combined_app();
        });

        robot.wait_for_idle();

        let rects = robot.get_all_rects();
        println!("Found {} UI elements with bounds", rects.len());

        for (i, (rect, text)) in rects.iter().enumerate() {
            println!(
                "Element {}: bounds=({:.1}, {:.1}, {:.1}x{:.1}), text={:?}",
                i, rect.x, rect.y, rect.width, rect.height, text
            );
        }
    }

    #[test]
    fn test_find_by_position() {
        let mut robot = create_headless_robot_test(800, 600, || {
            app::combined_app();
        });

        robot.wait_for_idle();

        let positions = vec![(100.0, 50.0), (400.0, 300.0), (700.0, 500.0)];

        for (x, y) in positions {
            let mut finder = robot.find_at_position(x, y);
            if finder.exists() {
                println!("Found element at ({}, {})", x, y);
                if let Some(bounds) = finder.bounds() {
                    println!("  Bounds: {:?}", bounds);
                }
            } else {
                println!("No element at ({}, {})", x, y);
            }
        }
    }

    #[test]
    fn test_app_long_press() {
        let mut robot = create_headless_robot_test(800, 600, || {
            app::combined_app();
        });

        robot.wait_for_idle();

        let mut finder = robot.find_at_position(400.0, 300.0);
        finder.long_press();

        robot.wait_for_idle();
        println!("=== After Long Press ===");
        robot.dump_screen();
    }
}
