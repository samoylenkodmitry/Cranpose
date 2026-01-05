use compose_foundation::lazy::{remember_lazy_list_state, LazyListScope};
use compose_testing::robot::create_headless_robot_test;
use compose_ui::widgets::{LazyColumn, LazyColumnSpec, Text};
use compose_ui::Modifier;

#[test]
fn test_lazy_column_reverse_layout() {
    let mut robot = create_headless_robot_test(800, 600, || {
        let state = remember_lazy_list_state();
        // Enable reverse_layout
        let spec = LazyColumnSpec::default().reverse_layout(true);

        LazyColumn(Modifier::empty(), state, spec, |scope| {
            scope.items(3, None::<fn(usize) -> u64>, None::<fn(usize) -> u64>, |i| {
                // Items: "Item 0", "Item 1", "Item 2"
                Text(format!("Item {}", i), Modifier::empty());
            });
        });
    });

    // In a normal column:
    // Item 0 is at Top
    // Item 1 is below Item 0
    // Item 2 is below Item 1

    // In a reverse_layout column:
    // Item 0 should be at Bottom
    // Item 1 should be above Item 0
    // Item 2 should be above Item 1

    // Wait for layout
    robot.wait_for_idle();

    // Robot finder API usage might need adjustment based on RobotTestRule
    // robot.find_text("Item 0") -> robot.find_by_text("Item 0")

    // Verify items exist and get current bounds sequentially to avoid multiple mutable borrows
    let rect0 = {
        let mut finder = robot.find_by_text("Item 0");
        finder.assert_exists();
        finder.bounds().expect("Item 0 bounds missing")
    };

    let rect1 = {
        let mut finder = robot.find_by_text("Item 1");
        finder.assert_exists();
        finder.bounds().expect("Item 1 bounds missing")
    };

    let rect2 = {
        let mut finder = robot.find_by_text("Item 2");
        finder.assert_exists();
        finder.bounds().expect("Item 2 bounds missing")
    };

    println!("Item 0: {:?}", rect0);
    println!("Item 1: {:?}", rect1);
    println!("Item 2: {:?}", rect2);

    // Verification: Item 0 (start of list) should be visually below Item 1
    // Because "reverse" means the list starts from the bottom/end.
    // So index 0 should have the largest Y coordinate (or be at the bottom).

    assert!(
        rect0.y > rect1.y,
        "Item 0 (y: {}) should be below Item 1 (y: {}) in reverse layout",
        rect0.y,
        rect1.y
    );
    assert!(
        rect1.y > rect2.y,
        "Item 1 (y: {}) should be below Item 2 (y: {}) in reverse layout",
        rect1.y,
        rect2.y
    );

    println!("Reverse layout verification passed!");
}
