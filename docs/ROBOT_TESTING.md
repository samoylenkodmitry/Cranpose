# Robot Testing

Robot testing provides automated UI testing capabilities for Compose-RS applications. The robot can interact with your app programmatically, find elements by semantic properties, and validate UI state.

## Quick Start

### Enable Robot Testing

Add the `robot-app` feature to your dev dependencies:

```toml
[dev-dependencies]
compose-testing = { path = "../../crates/compose-testing", features = ["robot-app"] }
```

### Basic Example

```rust
use compose_app::{AppLauncher, Robot};

AppLauncher::new()
    .with_test_driver(|robot| {
        // Wait for app to be ready
        robot.wait_for_idle().ok();
        
        // Click a button by finding it semantically
        robot.click_by_text("Increment")?;
        
        // Validate content
        robot.validate_content("Counter: 1")?;
        
        robot.exit()?;
    })
    .run(|| {
        // Your app code
    });
```

## API Reference

### Core Methods

#### `click(x, y) -> Result<(), String>`
Click at specific coordinates (logical pixels).

```rust
robot.click(150.0, 560.0)?;
```

#### `move_to(x, y) -> Result<(), String>`
Move cursor to coordinates without clicking.

```rust
robot.move_to(150.0, 560.0)?;
```

#### `wait_for_idle() -> Result<(), String>`
Wait for the application to become idle (no redraws, no animations).

**Note:** This will timeout and return `Err` for tabs with continuous animations. This is expected behavior, not a failure.

```rust
match robot.wait_for_idle() {
    Ok(_) => println!("App is idle"),
    Err(e) => println!("Timeout (animations active): {}", e),
}
```

#### `exit() -> Result<(), String>`
Shutdown the application gracefully.

```rust
robot.exit()?;
```

### Semantic API

The semantic API allows you to find and interact with UI elements by their properties instead of hardcoded coordinates.

#### `get_semantics() -> Result<Vec<SemanticElement>, String>`
Retrieve the semantic tree with geometric bounds.

```rust
let semantics = robot.get_semantics()?;
```

#### `find_by_text(elements, text) -> Option<&SemanticElement>`
Find any element containing the specified text (recursive search).

```rust
let elem = Robot::find_by_text(&semantics, "Hello")?;
```

#### `find_button(elements, text) -> Option<&SemanticElement>`
Find clickable element by text content. Searches in both the element and its children (handles Compose's composite pattern where clickable Layouts contain Text children).

```rust
let button = Robot::find_button(&semantics, "Increment")
    .ok_or("Button not found")?;
```

### Helper Methods

#### `click_by_text(text) -> Result<(), String>`
Convenience method that finds a button and clicks its center in one call.

```rust
robot.click_by_text("Save")?;
```

This is equivalent to:
```rust
let semantics = robot.get_semantics()?;
let elem = Robot::find_button(&semantics, "Save")?;
let center_x = elem.bounds.x + elem.bounds.width / 2.0;
let center_y = elem.bounds.y + elem.bounds.height / 2.0;
robot.click(center_x, center_y)?;
```

#### `validate_content(text) -> Result<(), String>`
Assert that text exists anywhere in the semantic tree.

```rust
robot.validate_content("Success!")?;
```

#### `print_semantics(elements, indent)`
Print hierarchical view of semantic tree for debugging.

```rust
let semantics = robot.get_semantics()?;
Robot::print_semantics(&semantics, 0);
```

Output:
```
role=Layout
  role=Layout [CLICKABLE]
    role=Text text="Increment"
  role=Layout [CLICKABLE]
    role=Text text="Decrement"
```

## SemanticElement Structure

```rust
pub struct SemanticElement {
    pub role: String,              // "Button", "Text", "Layout", etc.
    pub text: Option<String>,      // Text content if available
    pub bounds: SemanticRect,      // Geometric bounds
    pub clickable: bool,           // Has click actions
    pub children: Vec<SemanticElement>,
}

pub struct SemanticRect {
    pub x: f32,      // X coordinate (logical pixels)
    pub y: f32,      // Y coordinate (logical pixels)  
    pub width: f32,  // Width
    pub height: f32, // Height
}
```

## Complete Example

```rust
use desktop_app::app;
use compose_app::{AppLauncher, Robot};
use std::time::Duration;

fn main() {
    AppLauncher::new()
        .with_title("Robot Test")
        .with_size(800, 600)
        .with_test_driver(|robot| {
            // Wait for initial render
            std::thread::sleep(Duration::from_millis(500));
            robot.wait_for_idle().ok();

            // Print semantic tree for debugging
            if let Ok(sem) = robot.get_semantics() {
                Robot::print_semantics(&sem, 0);
            }

            // Test counter workflow
            for i in 1..=5 {
                println!("Click {}", i);
                robot.click_by_text("Increment")?;
                std::thread::sleep(Duration::from_millis(300));
            }

            // Validate final state
            robot.validate_content("Counter: 5")?;

            // Test tab navigation
            robot.click_by_text("Settings")?;
            robot.validate_content("Settings Page")?;

            // Cleanup
            robot.exit()?;
            
            Ok::<(), String>(())
        })
        .run(|| {
            app::my_app();
        });
}
```

## Best Practices

### 1. Use Semantic Queries
**✅ Good:**
```rust
robot.click_by_text("Submit")?;
```

**❌ Avoid:**
```rust
robot.click(450.0, 320.0)?;  // Brittle - breaks if layout changes
```

### 2. Handle Timeouts Gracefully
```rust
match robot.wait_for_idle() {
    Ok(_) => {},  // Tab is idle
    Err(_) => {}, // Tab has animations - this is OK
}
```

### 3. Add Small Delays
Give the UI time to update between interactions:
```rust
robot.click_by_text("Next")?;
std::thread::sleep(Duration::from_millis(300));
robot.validate_content("Page 2")?;
```

### 4. Debug with print_semantics
When tests fail, print the tree to see what's available:
```rust
let semantics = robot.get_semantics()?;
Robot::print_semantics(&semantics, 0);
```

## Running Robot Tests

```bash
# Run a specific robot test
cargo run --package desktop-app --example robot_interactive --features robot-app

# Run with full logging
RUST_LOG=debug cargo run --package desktop-app --example robot_interactive --features robot-app
```

## Troubleshooting

### "Button not found"
- Use `print_semantics()` to see available elements
- Check if the button text matches exactly (case-sensitive)
- Verify the element is actually clickable

### "wait_for_idle timeout"
- This is normal for animated tabs
- Use `match` to handle timeouts gracefully
- Don't call `.expect()` on `wait_for_idle()`

### Clicks miss target
- Ensure the app has finished rendering
- Add a small delay before clicking
- Verify bounds using `print_semantics()`
