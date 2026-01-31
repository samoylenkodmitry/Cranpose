# Cranpose Testing

A testing framework for validating Cranpose apps, supporting both unit tests and full end-to-end (E2E) robot tests.

## Overview

Cranpose provides a "Robot" pattern for testing, where you write scripts that interact with your application programmatically (clicking, dragging, asserting text) just like a user would. These tests can run in **headless mode**, making them ideal for CI/CD pipelines.

## End-to-End Robot Testing

The primary way to test Cranpose apps is via "Robot Runners" — specialized test binaries that launch the actual application in a headless mode and drive it from a separate thread.

### Architecture

1.  **Headless Host**: The app launches using `AppLauncher` with `with_headless(true)`.
2.  **Test Driver**: A separate thread waits for the app to become idle, inspects the semantic tree, and injects input events.
3.  **Semantic Inspection**: Tests do not look at pixels; they inspect the **Semantics Tree** (accessibility tree) to find elements by text, role, or other properties.

### Running Tests

Use the `run_robot_test.sh` script in the project root to run all robot runners defined in `apps/desktop-demo/robot-runners/`:

```bash
# Run all tests in parallel (headless)
./run_robot_test.sh

# Run sequentially (easier for debugging)
./run_robot_test.sh --sequential
```

### Writing a Robot Test

A robot test is typically a Cargo example that uses the `robot-app` feature.

**Basic Pattern:**

```rust
use cranpose::AppLauncher;
use cranpose_testing::{find_button, find_in_semantics, find_text};
use desktop_app::app; // Your app entry point
use std::time::Duration;

fn main() {
    AppLauncher::new()
        .with_title("My Robot Test")
        .with_size(800, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            // --- This code runs on a separate test thread ---
            
            // 1. Wait for app to be ready
            robot.wait_for_idle().expect("Failed to wait for idle");

            // 2. Interact with the app
            if let Some((x, y, w, h)) = find_in_semantics(&robot, |elem| find_button(elem, "Click Me")) {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;

                // Simulate mouse interaction
                robot.mouse_move(cx, cy);
                robot.mouse_down();
                robot.mouse_up();
                
                // Wait for reaction
                std::thread::sleep(Duration::from_millis(100));
            } else {
                panic!("Button not found!");
            }

            // 3. Verify state
            if find_in_semantics(&robot, |elem| find_text(elem, "Clicked!")).is_some() {
                println!("✓ Test Passed");
            } else {
                panic!("✗ Test Failed");
            }
            
            // 4. Exit
            robot.exit();
        })
        .run(|| {
            // --- This runs on the main thread ---
            app::combined_app(); 
        });
}
```

## Key APIs

### `cranpose::Robot`
The `robot` object passed to the test driver provides low-level control:
-   `wait_for_idle()`: Blocks until the main thread has finished processing layout and drawing.
-   `mouse_move(x, y)`, `mouse_down()`, `mouse_up()`: Simulates pointer events.
-   `get_semantics()`: Returns the current semantic tree for inspection.
-   `exit()`: Shuts down the application.

### `cranpose_testing` Helpers
High-level helpers for finding elements in the semantic tree.

| Function | Description |
| :--- | :--- |
| `find_in_semantics(&robot, finder)` | Generic search. Applies `finder` to every node. Returns bounds `(x, y, w, h)` of the first match. |
| `find_text(elem, "text")` | Use with `find_in_semantics`. Matches if element contains text. |
| `find_text_exact(elem, "text")` | Matches exact text only. |
| `find_button(elem, "text")` | Matches clickable elements containing text. |
| `find_button_center(elem, "text")` | Returns center `(x, y)` of a matched button. |

**Example Usage:**

```rust
// Find a button and get its center
let center = find_in_semantics(&robot, |elem| find_button_center(elem, "Submit"));

// Find text and check existence
let exists = find_in_semantics(&robot, |elem| find_text(elem, "Success")).is_some();
```

## Debugging

If a test fails, you can print the entire semantics tree to understand what the robot sees:

```rust
use cranpose_testing::robot_test_utils::print_semantics_with_bounds;

// Inside test driver:
if let Ok(semantics) = robot.get_semantics() {
    print_semantics_with_bounds(&semantics, 0);
}
```
