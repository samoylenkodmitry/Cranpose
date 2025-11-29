use super::test_renderer::TestRenderer;
use compose_app_shell::AppShell;
use compose_core::{location_key, Key, NodeId};
use compose_foundation::PointerEventKind;
use compose_ui::{LayoutTree, SemanticsNode, Rect};
use std::rc::Rc;

pub struct SemanticsMatcher {
    description: String,
    matcher: Box<dyn Fn(&SemanticsNode) -> bool>,
}

impl SemanticsMatcher {
    pub fn new(
        description: impl Into<String>,
        matcher: impl Fn(&SemanticsNode) -> bool + 'static,
    ) -> Self {
        Self {
            description: description.into(),
            matcher: Box::new(matcher),
        }
    }

    pub fn matches(&self, node: &SemanticsNode) -> bool {
        (self.matcher)(node)
    }
}

pub fn has_text(text: impl Into<String>) -> SemanticsMatcher {
    let text = text.into();
    SemanticsMatcher::new(
        format!("has_text({:?})", text),
        move |node| {
             if let compose_ui::SemanticsRole::Text { value } = &node.role {
                 value == &text
             } else {
                 false
             }
        }
    )
}

pub struct TestNode<'a> {
    rule: &'a mut ComposeTestRule,
    node_id: NodeId,
}

impl<'a> TestNode<'a> {
    pub fn perform_click(&mut self) {
        let bounds = self.get_bounds();
        let center_x = bounds.x + bounds.width / 2.0;
        let center_y = bounds.y + bounds.height / 2.0;
        
        self.rule.perform_touch_input(center_x, center_y, PointerEventKind::Down);
        self.rule.perform_touch_input(center_x, center_y, PointerEventKind::Up);
    }
    
    pub fn perform_touch_input(&mut self, block: impl FnOnce(&mut TouchInjectionScope)) {
        let bounds = self.get_bounds();
        let mut scope = TouchInjectionScope {
            rule: self.rule,
            current_x: bounds.x + bounds.width / 2.0,
            current_y: bounds.y + bounds.height / 2.0,
        };
        block(&mut scope);
    }

    pub fn get_bounds(&self) -> Rect {
        // We need to find the node in the layout tree to get its bounds.
        // Since SemanticsNode doesn't store bounds directly (it might, but let's check LayoutTree),
        // we might need to look up the LayoutBox corresponding to the node_id.
        // For now, let's assume we can traverse the layout tree to find the node.
        
        if let Some(tree) = self.rule.layout_tree() {
             if let Some(node) = find_layout_node(tree.root(), self.node_id) {
                 return node.rect;
             }
        }
        panic!("Node #{} not found in layout tree", self.node_id);
    }
    
    pub fn assert_exists(&self) {
        // If we created TestNode, it existed at that point. 
        // But we should verify it's still in the current tree.
        let exists = if let Some(tree) = self.rule.layout_tree() {
             find_layout_node(tree.root(), self.node_id).is_some()
        } else {
            false
        };
        assert!(exists, "Node #{} does not exist", self.node_id);
    }
}

pub struct TouchInjectionScope<'a> {
    rule: &'a mut ComposeTestRule,
    current_x: f32,
    current_y: f32,
}

impl<'a> TouchInjectionScope<'a> {
    pub fn down(&mut self, x: Option<f32>, y: Option<f32>) {
        if let Some(x) = x { self.current_x = x; }
        if let Some(y) = y { self.current_y = y; }
        self.rule.perform_touch_input(self.current_x, self.current_y, PointerEventKind::Down);
    }
    
    pub fn move_to(&mut self, x: f32, y: f32) {
        self.current_x = x;
        self.current_y = y;
        self.rule.perform_touch_input(self.current_x, self.current_y, PointerEventKind::Move);
    }
    
    pub fn up(&mut self) {
        self.rule.perform_touch_input(self.current_x, self.current_y, PointerEventKind::Up);
    }
    
    pub fn swipe_up(&mut self, distance: f32) {
        self.down(None, None);
        // Simulate a few move events
        let steps = 10;
        let start_y = self.current_y;
        for i in 1..=steps {
            let progress = i as f32 / steps as f32;
            let target_y = start_y - (distance * progress);
            self.move_to(self.current_x, target_y);
        }
        self.up();
    }
}

fn find_layout_node<'a>(root: &'a compose_ui::LayoutBox, id: NodeId) -> Option<&'a compose_ui::LayoutBox> {
    if root.node_id == id {
        return Some(root);
    }
    for child in &root.children {
        if let Some(found) = find_layout_node(child, id) {
            return Some(found);
        }
    }
    None
}

fn find_semantics_node<'a>(root: &'a SemanticsNode, matcher: &SemanticsMatcher) -> Option<&'a SemanticsNode> {
    if matcher.matches(root) {
        return Some(root);
    }
    for child in &root.children {
        if let Some(found) = find_semantics_node(child, matcher) {
            return Some(found);
        }
    }
    None
}

pub struct ComposeTestRule {
    shell: AppShell<TestRenderer>,
    root_key: Key,
}

impl ComposeTestRule {
    pub fn new() -> Self {
        let renderer = TestRenderer::new();
        let shell = AppShell::new(
            renderer,
            location_key(file!(), line!(), column!()),
            || {}, // Initial empty content
        );
        
        Self {
            shell,
            root_key: location_key(file!(), line!(), column!()),
        }
    }

    pub fn set_content(&mut self, content: impl FnMut() + 'static) {
        let renderer = TestRenderer::new();
        self.shell = AppShell::new(renderer, self.root_key, content);
        self.await_idle();
    }

    pub fn await_idle(&mut self) {
        let mut i = 0;
        while self.shell.should_render() || self.shell.needs_redraw() {
            self.shell.update();
            i += 1;
            if i > 100 {
                panic!("Composition failed to settle after 100 frames");
            }
        }
    }

    pub fn perform_touch_input(&mut self, x: f32, y: f32, kind: PointerEventKind) {
        self.shell.set_cursor(x, y);
        match kind {
            PointerEventKind::Down => { self.shell.pointer_pressed(); },
            PointerEventKind::Up => { self.shell.pointer_released(); },
            PointerEventKind::Move => { /* set_cursor already dispatched move */ },
            _ => {}
        }
        self.await_idle();
    }

    pub fn layout_tree(&self) -> Option<&LayoutTree> {
        self.shell.layout_tree()
    }

    pub fn on_node(&mut self, matcher: SemanticsMatcher) -> TestNode<'_> {
        // We need to find the node ID that matches.
        // Access the semantics tree from the app shell (we might need to expose it).
        // For now, let's assume we can get it or rebuild it.
        // Actually, AppShell doesn't expose semantics tree yet.
        // But LayoutTree has semantics information if we traverse it? 
        // No, SemanticsTree is separate but built from LayoutTree/Metadata.
        
        // Let's expose semantics_tree() on AppShell or build it here if possible.
        // Since we don't have easy access to SemanticsTree here without exposing it in AppShell,
        // let's assume we can traverse the LayoutTree and check metadata if we had it.
        
        // BETTER APPROACH: Expose semantics_tree on AppShell.
        // But for now, let's assume we can find it via a helper that traverses the layout tree 
        // and checks SemanticsRole if we can map LayoutBox -> SemanticsNode.
        
        // Wait, `compose-ui` exports `SemanticsTree` and `LayoutTree`.
        // Let's modify AppShell to expose `semantics_tree`.
        
        // For this step, I'll assume `self.shell.semantics_tree()` exists or I'll implement it.
        // If not, I will fail compilation and then fix AppShell.
        
        let node_id = {
            let semantics = self.shell.semantics_tree().expect("Semantics tree not available");
            let node = find_semantics_node(semantics.root(), &matcher)
                .unwrap_or_else(|| panic!("No node found matching {}", matcher.description));
            node.node_id
        };
            
        TestNode {
            rule: self,
            node_id,
        }
    }
}

impl Default for ComposeTestRule {
    fn default() -> Self {
        Self::new()
    }
}
