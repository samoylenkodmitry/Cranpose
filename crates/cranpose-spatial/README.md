# cranpose-spatial

Composable surfaces placed in a room and seen through a camera.

A [`Room`] starts as an ordinary screen: its north wall's content fills the
room's bounds. Once entered, the camera steps back and every wall of the
box-shaped room shows its own composable content in perspective. The
content stays live: it recomposes, animates and takes pointer and key input
where it lies on screen.

```rust,ignore
use cranpose_spatial::{Room, Wall, rememberRoomState};

let room = rememberRoomState();
Room(room, Modifier::empty().fill_max_size(), move |wall| match wall {
    Wall::North => Controls(room),
    Wall::East => Gallery(),
    _ => Plaster(),
});
```

W, A, S and D or the up and down arrows walk; Q and E, the left and right
arrows or a drag turn the camera. Each wall is a layer with a
[`GraphicsLayer::projection`](cranpose_ui_graphics::GraphicsLayer), the
camera's view of the wall's plane; the part of a wall behind the camera is cut.
