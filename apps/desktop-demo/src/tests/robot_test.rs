//! Robot tests for the desktop demo app
//!
//! These tests wrap the real app and test it with robot interactions

#[cfg(test)]
mod robot_tests {
    use crate::app;
    use compose_testing::robot::create_headless_robot_test;

    /// Test the real counter app with robot interactions
    #[test]
    fn test_counter_app_increment() {
        // Launch the REAL app
        let mut robot = create_headless_robot_test(800, 600, || {
            app::combined_app();
        });

        // Wait for initial render
        robot.wait_for_idle();

        // The app should be running
        assert_eq!(robot.viewport_size(), (800, 600));

        // Debug: print current screen state
        println!("=== Initial Screen State ===");
        robot.dump_screen();

        // Get all text to see what's on screen
        let texts = robot.get_all_text();
        println!("All text on screen: {:?}", texts);
    }

    /// Test clicking different positions in the app
    #[test]
    fn test_app_interactions() {
        let mut robot = create_headless_robot_test(800, 600, || {
            app::combined_app();
        });

        robot.wait_for_idle();

        // Click somewhere in the middle of the screen
        robot.click_at(400.0, 300.0);
        robot.wait_for_idle();

        println!("=== After First Click ===");
        robot.dump_screen();

        // Try clicking in another location
        robot.click_at(200.0, 100.0);
        robot.wait_for_idle();
    }

    /// Test drag interaction in the real app
    #[test]
    fn test_app_drag() {
        let mut robot = create_headless_robot_test(800, 600, || {
            app::combined_app();
        });

        robot.wait_for_idle();

        // Perform a drag
        robot.drag(100.0, 100.0, 300.0, 300.0);
        robot.wait_for_idle();

        println!("=== After Drag ===");
        robot.dump_screen();
    }

    /// Test viewport resizing with the real app
    #[test]
    fn test_app_resize() {
        let mut robot = create_headless_robot_test(800, 600, || {
            app::combined_app();
        });

        robot.wait_for_idle();
        println!("=== Initial 800x600 ===");
        robot.dump_screen();

        // Resize to a different size
        robot.set_viewport(1024, 768);
        robot.wait_for_idle();

        println!("=== After Resize to 1024x768 ===");
        robot.dump_screen();

        assert_eq!(robot.viewport_size(), (1024, 768));
    }

    /// Test multiple interactions in sequence
    #[test]
    fn test_app_complex_flow() {
        let mut robot = create_headless_robot_test(800, 600, || {
            app::combined_app();
        });

        robot.wait_for_idle();

        // Sequence of interactions
        robot.click_at(100.0, 50.0); // Click top-left area
        robot.wait_for_idle();

        robot.move_to(400.0, 300.0); // Move to center
        robot.wait_for_idle();

        robot.click_at(700.0, 50.0); // Click top-right area
        robot.wait_for_idle();

        println!("=== After Interaction Sequence ===");
        robot.dump_screen();
    }

    /// Test getting all rectangles/bounds from the real app
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

    /// Test finding elements by position in the real app
    #[test]
    fn test_find_by_position() {
        let mut robot = create_headless_robot_test(800, 600, || {
            app::combined_app();
        });

        robot.wait_for_idle();

        // Try to find element at various positions
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

    /// Test long press on the real app
    #[test]
    fn test_app_long_press() {
        let mut robot = create_headless_robot_test(800, 600, || {
            app::combined_app();
        });

        robot.wait_for_idle();

        // Long press in the center
        let mut finder = robot.find_at_position(400.0, 300.0);
        finder.long_press();

        robot.wait_for_idle();
        println!("=== After Long Press ===");
        robot.dump_screen();
    }
}
