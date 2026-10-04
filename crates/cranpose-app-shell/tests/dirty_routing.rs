use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::{AppShell, RootId};
use cranpose_core::{MutableState, location_key, rememberMutableStateOf};
use cranpose_macros::composable;
use cranpose_render_common::{
    RenderScene, Renderer, graph, graph_scene::Scene, hit_graph::collect_hits_from_graph,
};
use cranpose_ui::{Box, BoxSpec, Modifier, WindowRootDescriptor, WindowRootEntry};
use cranpose_ui_graphics::Size;

#[derive(PartialEq)]
struct TestWindow;

impl WindowRootDescriptor for TestWindow {
    fn layout_size(&self) -> Size {
        Size::new(200.0, 100.0)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

struct HitRenderer {
    scene: Scene,
}

impl Default for HitRenderer {
    fn default() -> Self {
        Self {
            scene: Scene::new(),
        }
    }
}

impl Renderer for HitRenderer {
    type Scene = Scene;
    type Error = std::convert::Infallible;

    fn scene(&self) -> &Self::Scene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.scene
    }

    fn rebuild_scene(
        &mut self,
        layout_tree: &cranpose_ui::LayoutTree,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.scene.clear();
        let graph = cranpose_render_common::scene_builder::build_graph_from_layout_tree(
            layout_tree.root(),
            1.0,
        );
        collect_hits_from_graph(
            &graph.root,
            graph::ProjectiveTransform::identity(),
            &mut self.scene,
            None,
        );
        self.scene.replace_graph(graph);
        Ok(())
    }

    fn rebuild_scene_from_applier(
        &mut self,
        applier: &mut cranpose_core::MemoryApplier,
        root: cranpose_core::NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.scene.clear();
        if let Some(graph) =
            cranpose_render_common::scene_builder::build_graph_from_applier(applier, root, 1.0)
        {
            collect_hits_from_graph(
                &graph.root,
                graph::ProjectiveTransform::identity(),
                &mut self.scene,
                None,
            );
            self.scene.replace_graph(graph);
        }
        Ok(())
    }
}

#[composable]
fn Pane() {
    Box(
        Modifier::empty().size_points(90.0, 30.0).clickable(|_| {}),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn Screen(show_window: Rc<RefCell<Option<MutableState<bool>>>>, window: Rc<TestWindow>) {
    let show = rememberMutableStateOf(|| true);
    *show_window.borrow_mut() = Some(show);
    let modifier = if show.get() {
        Modifier::empty().window_root(window)
    } else {
        Modifier::empty()
    };
    Box(modifier, BoxSpec::default(), || Pane());
}

fn first_window(entries: Vec<WindowRootEntry>) -> u64 {
    entries.first().expect("the window root is attached").node as u64
}

#[test]
fn dirty_routing_tracks_a_window_root_that_detaches_and_attaches_again() {
    let show_window: Rc<RefCell<Option<MutableState<bool>>>> = Rc::new(RefCell::new(None));
    let window = Rc::new(TestWindow);
    let mut shell = AppShell::new(
        HitRenderer::default(),
        location_key(file!(), line!(), column!()),
        {
            let show_window = Rc::clone(&show_window);
            let window = Rc::clone(&window);
            move || Screen(Rc::clone(&show_window), Rc::clone(&window))
        },
    );
    let root = first_window(shell.window_roots());
    shell.add_window_surface(root, HitRenderer::default(), (200, 100), (200.0, 100.0));
    shell.update();
    assert!(
        !shell
            .surface(RootId::Window(root))
            .expect("window surface")
            .scene()
            .hit_test(20.0, 15.0)
            .is_empty()
    );

    let show = (*show_window.borrow()).expect("the composition remembered the state");
    show.set(false);
    shell.update();
    assert!(shell.window_roots().is_empty());
    assert!(
        !shell.primary().scene().hit_test(20.0, 15.0).is_empty(),
        "the pane now belongs to the primary surface"
    );
    assert!(
        shell
            .surface(RootId::Window(root))
            .expect("window surface")
            .scene()
            .hit_test(20.0, 15.0)
            .is_empty(),
        "the detached window drops its old hit target"
    );

    show.set(true);
    shell.update();
    assert_eq!(first_window(shell.window_roots()), root);
    assert!(
        !shell
            .surface(RootId::Window(root))
            .expect("window surface")
            .scene()
            .hit_test(20.0, 15.0)
            .is_empty(),
        "the next dirty batch routes the pane back into the window"
    );
}
