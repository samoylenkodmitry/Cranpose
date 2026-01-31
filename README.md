[https://codewiki.google/github.com/samoylenkodmitry/cranpose](https://codewiki.google/github.com/samoylenkodmitry/rs-compose)

[WIP.webm](https://github.com/user-attachments/assets/00533605-aa9c-4555-896c-c939195e3dce)
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

    Column(Modifier.fill_max_size().padding(20.dp), || {
        Text("My Todo List", Modifier.padding(10.dp).font_size(24.sp));

        // Input Row
        Row(Modifier.fill_max_width().padding(5.dp), || {
            BasicTextField(
                value = input_text.value(),
                on_value_change = move |new_text| input_text.set(new_text),
                Modifier.weight(1.0).padding(5.dp)
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
                        .padding(5.dp)
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
                        Spacer(Modifier.width(10.dp));
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

## Platform Support and Building

### Desktop
Supported on Linux, macOS, and Windows via `winit` and `wgpu`.
```bash
cargo run --bin desktop-app
```

### Android
Supported using `cargo-ndk` and `android-activity`.
```bash
# Prerequisites: cargo install cargo-ndk
cd apps/android-demo/android
./gradlew installDebug
```
Refer to [`apps/android-demo/README.md`](apps/android-demo/README.md) for full configuration details.

### Web (WASM)
Supported via `wasm-bindgen` and WebGL2.
```bash
# Prerequisites: cargo install wasm-pack
cd apps/desktop-demo
./build-web.sh
python3 -m http.server 8080
```
Refer to [`apps/desktop-demo/README.md`](apps/desktop-demo/README.md) for web build details.

## License
This project is available under the terms of the Apache License (Version 2.0). See [`LICENSE-APACHE`](LICENSE-APACHE) for the full license text.
