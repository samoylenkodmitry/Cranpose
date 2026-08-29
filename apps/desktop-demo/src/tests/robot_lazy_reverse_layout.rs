use cranpose_foundation::lazy::{rememberLazyListState, LazyListScope};
use cranpose_testing::robot::create_headless_robot_test;
use cranpose_ui::{
    widgets::{LazyColumn, LazyColumnSpec, Text},
    Modifier, TextStyle,
};

#[test]
fn test_lazy_column_reverse_layout() {
    let mut robot = create_headless_robot_test(800, 600, || {
        let state = rememberLazyListState();
        let spec = LazyColumnSpec::default().reverse_layout(true);

        LazyColumn(Modifier::empty(), state, spec, |scope| {
            scope.items(3, |i| {
                Text(
                    format!("Item {}", i),
                    Modifier::empty(),
                    TextStyle::default(),
                );
            });
        });
    });

    robot.wait_for_idle();

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
