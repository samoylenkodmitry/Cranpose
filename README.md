[https://codewiki.google/github.com/samoylenkodmitry/cranpose](https://codewiki.google/github.com/samoylenkodmitry/rs-compose)

[v0.0.40.webm](https://github.com/user-attachments/assets/df50209b-abfd-426a-b79c-a51a9543b385)

## 🌐 Live Demo

**[Try it in your browser!](https://samoylenkodmitry.github.io/Cranpose/)**

# Cranpose

<img width="1536" height="1024" alt="ChatGPT Image Jan 18, 2026, 10_53_13 AM" src="https://github.com/user-attachments/assets/2ce48dfe-a048-4b9d-8812-a0e4534691f8" />

Cranpose is a declarative UI framework for Rust, inspired by Jetpack Compose. It enables developers to build user interfaces for Desktop, Android, and Web (WASM) using a single Rust codebase.

## Quick Start via Isolated Demo

To get started, we recommend using the **Isolated Demo** template found in `apps/isolated-demo`. This project is pre-configured with the necessary dependencies and build scripts for all supported platforms.

```bash
# Clone the repository
git clone https://github.com/samoylenkodmitry/cranpose.git
cd cranpose/apps/isolated-demo

# Run on Desktop (Linux/macOS/Windows)
cargo run --features desktop,renderer-wgpu
```

## Example: Todo List Application

The following example demonstrates managing state, handling user input, and rendering a dynamic list.

```rust
use cranpose::prelude::*;

#[derive(Clone)]
struct TodoItem {
    id: usize,
    text: String,
    done: bool,
}

#[composable]
fn TodoApp() {
    // State management using useState
    let items = useState(|| vec![
        TodoItem { id: 0, text: "Buy milk".into(), done: false },
        TodoItem { id: 1, text: "Walk the dog".into(), done: true },
    ]);
    let input_text = useState(|| String::new());
    let next_id = useState(|| 2);

    Column(Modifier.fill_max_size().padding(20.0), || {
        Text("My Todo List", Modifier.padding(10.0).font_size(24.0));

        // Input Row
        Row(Modifier.fill_max_width().padding(5.0), || {
            BasicTextField(
                value = input_text.value(),
                on_value_change = move |new_text| input_text.set(new_text),
                Modifier.weight(1.0).padding(5.0)
            );
            
            Button(
                onClick = move || {
                    if !input_text.value().is_empty() {
                        let mut list = items.value();
                        list.push(TodoItem {
                            id: next_id.value(),
                            text: input_text.value(),
                            done: false,
                        });
                        items.set(list);
                        next_id.set(next_id.value() + 1);
                        input_text.set(String::new());
                    }
                }, 
                || Text("Add")
            );
        });
        
        // Dynamic List Rendering
        LazyColumn(Modifier.weight(1.0), || {
            items(items.value().len(), |i| {
                let item = items.value()[i].clone();
                
                Row(
                    Modifier
                        .fill_max_width()
                        .padding(5.0)
                        .clickable(move || {
                            // Toggle done status
                            let mut list = items.value();
                            if let Some(todo) = list.iter_mut().find(|t| t.id == item.id) {
                                todo.done = !todo.done;
                            }
                            items.set(list);
                        }),
                    || {
                        Text(if item.done { "[x]" } else { "[ ]" });
                        Spacer(Modifier.width(10.0));
                        Text(
                            item.text, 
                            Modifier.alpha(if item.done { 0.5 } else { 1.0 })
                        );
                    }
                );
            });
        });
    });
}
```

## Platform Support

| Platform | Backend | Status |
|---|---|---|
| Linux x86_64 | Vulkan via wgpu | Supported |
| macOS aarch64 | Metal via wgpu | Supported |
| Windows x86_64 | DX12/Vulkan via wgpu | Supported |
| Android | Vulkan/GLES via wgpu | Supported |
| iOS | Metal via wgpu | Supported |
| Web (WASM) | WebGPU/WebGL2 via wgpu | Supported |

Pre-built binaries for all platforms are available on the [Releases](https://github.com/samoylenkodmitry/Cranpose/releases) page.

## Building

### Desktop (Linux/macOS/Windows)
```bash
cargo run --bin desktop-app
```

### Android
```bash
# Prerequisites: cargo install cargo-ndk
cd apps/android-demo/android
./gradlew installDebug
```
See [`apps/android-demo/README.md`](apps/android-demo/README.md) for details.

### iOS
Open `apps/ios-demo/ios/CranposeDemo.xcodeproj` in Xcode, then build and run on a simulator or device. The Xcode project invokes `cargo build` via a build phase script.

### Web (WASM)
```bash
# Prerequisites: cargo install wasm-pack
cd apps/desktop-demo
./build-web.sh
python3 -m http.server 8080
```
See [`apps/desktop-demo/README.md`](apps/desktop-demo/README.md) for details.

## License
This project is available under the terms of the Apache License (Version 2.0). See [`LICENSE-APACHE`](LICENSE-APACHE) for the full license text.
