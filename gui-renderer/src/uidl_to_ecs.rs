//! UIDL → ECS component mapping for openSystem.
//!
//! Converts a [`UidlDocument`] into a flat list of [`EcsNode`]s — an ECS-style
//! component table where each node holds:
//!   - its widget kind and data (components)
//!   - a parent/child relationship (entity hierarchy)
//!   - a resolved [`crate::widget_system::LayoutBox`] (spatial component)
//!
//! # Architecture
//!
//! ```text
//! UidlDocument  ──► build_ecs_tree()  ──► EcsTree
//!                                              │
//!                         ┌───────────────────┤
//!                         ▼                   ▼
//!                    EcsNode[]            Layout pass
//!                  (flat, indexed)   (widget_system::LayoutEngine)
//! ```
//!
//! `EcsTree` is the data source for:
//!   - Round 9: snapshot rendering via `widget_system::Painter`
//!   - Round 10: Bevy ECS entity spawning (1 Bevy entity per EcsNode)
//!   - Round 10: WASM event bridge (action strings → WASM callbacks)

use crate::uidl::{ButtonStyle, TextStyle, Theme, UidlDocument, Widget};
use crate::widget_system::{LayoutBox, LayoutEngine};
use anyhow::Result;

// ─── Entity ID ───────────────────────────────────────────────────────────────

/// Lightweight opaque entity identifier.
///
/// Corresponds 1:1 with a Bevy `Entity` when the Bevy backend is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntityId(pub u32);

// ─── Component data ───────────────────────────────────────────────────────────

/// The kind + payload of one widget, stored as a component on an [`EcsNode`].
#[derive(Debug, Clone, PartialEq)]
pub enum WidgetComponent {
    Text {
        content: String,
        style: Option<TextStyle>,
    },
    Button {
        label: String,
        /// Action identifier — forwarded to WASM on click.
        action: String,
        style: Option<ButtonStyle>,
    },
    Input {
        placeholder: Option<String>,
        value: Option<String>,
        /// WASM callback name for value changes.
        on_change: Option<String>,
    },
    VStack {
        gap: f32,
        padding: f32,
    },
    HStack {
        gap: f32,
    },
    Spacer {
        size: f32,
    },
}

/// Interaction state component — tracks whether the node is focused/hovered.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InteractionState {
    pub hovered: bool,
    pub pressed: bool,
    pub focused: bool,
}

/// A single ECS node: one widget, with its spatial and interaction components.
#[derive(Debug, Clone)]
pub struct EcsNode {
    pub id: EntityId,
    /// Optional string id from UIDL (for WASM event routing).
    pub uidl_id: Option<String>,
    pub component: WidgetComponent,
    pub layout: LayoutBox,
    pub interaction: InteractionState,
    /// Index of the parent node in `EcsTree::nodes`, or `None` for the root.
    pub parent: Option<usize>,
    /// Indices of child nodes in `EcsTree::nodes`.
    pub children: Vec<usize>,
}

impl EcsNode {
    /// Returns `true` if this node can receive pointer events.
    pub fn is_interactive(&self) -> bool {
        matches!(
            self.component,
            WidgetComponent::Button { .. } | WidgetComponent::Input { .. }
        )
    }

    /// Returns the WASM action string for this node (if any).
    pub fn action(&self) -> Option<&str> {
        match &self.component {
            WidgetComponent::Button { action, .. } => Some(action.as_str()),
            WidgetComponent::Input { on_change, .. } => on_change.as_deref(),
            _ => None,
        }
    }
}

// ─── EcsTree ─────────────────────────────────────────────────────────────────

/// Flat, indexed ECS component table for one rendered UIDL document.
///
/// Invariants:
/// - `nodes[0]` is always the root widget.
/// - Parent/child indices are valid indices into `nodes`.
/// - Every node has a resolved `LayoutBox`.
#[derive(Debug, Clone)]
pub struct EcsTree {
    pub nodes: Vec<EcsNode>,
    pub theme: Option<Theme>,
    pub canvas_width: f32,
    pub canvas_height: f32,
}

impl EcsTree {
    /// Returns the root node, or `None` if the tree is empty.
    pub fn root(&self) -> Option<&EcsNode> {
        self.nodes.first()
    }

    /// Look up a node by its [`EntityId`].
    pub fn get(&self, id: EntityId) -> Option<&EcsNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Mutable lookup by [`EntityId`].
    pub fn get_mut(&mut self, id: EntityId) -> Option<&mut EcsNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// Iterate over all interactive nodes (buttons, inputs).
    pub fn interactive_nodes(&self) -> impl Iterator<Item = &EcsNode> {
        self.nodes.iter().filter(|n| n.is_interactive())
    }

    /// Find the deepest interactive node whose layout box contains `(px, py)`.
    ///
    /// Used for hit-testing pointer events before routing to WASM.
    pub fn hit_test(&self, px: f32, py: f32) -> Option<&EcsNode> {
        // Walk in reverse order so later (visually on top) nodes win.
        self.nodes.iter().rev().find(|n| {
            n.is_interactive()
                && n.layout.x <= px
                && px <= n.layout.x + n.layout.width
                && n.layout.y <= py
                && py <= n.layout.y + n.layout.height
        })
    }

    /// Total count of nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns `true` if the tree contains no nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

// ─── Builder ─────────────────────────────────────────────────────────────────

/// Convert a [`UidlDocument`] into a fully-resolved [`EcsTree`].
///
/// Layout is computed by [`LayoutEngine`]; nodes are assigned sequential
/// [`EntityId`]s starting at 1.
pub fn build_ecs_tree(doc: &UidlDocument, canvas_width: u32, canvas_height: u32) -> Result<EcsTree> {
    let w = canvas_width as f32;
    let h = canvas_height as f32;

    let engine = LayoutEngine::new(canvas_width, canvas_height);
    let pairs = engine.layout_document(doc);

    let mut nodes: Vec<EcsNode> = Vec::with_capacity(pairs.len());
    let mut next_id: u32 = 1;

    // Build a flat list from the layout pairs (pre-order traversal order).
    // We need parent/child relationships — derive them by tracking a depth stack.
    let mut parent_stack: Vec<usize> = Vec::new();

    // LayoutEngine returns (widget_ref, layout_box) in pre-order.
    // We reconstruct hierarchy by checking whether each widget is a container.
    build_nodes_from_pairs(&pairs, &mut nodes, &mut next_id, &mut parent_stack);

    Ok(EcsTree {
        nodes,
        theme: doc.theme.clone(),
        canvas_width: w,
        canvas_height: h,
    })
}

/// Recursively build ECS nodes from layout pairs.
///
/// `pairs` is in pre-order: container appears before its children, and
/// consecutive children of the same container follow each other.
/// We track a stack of (node_index, children_remaining) to assign parents.
fn build_nodes_from_pairs(
    pairs: &[(&Widget, LayoutBox)],
    nodes: &mut Vec<EcsNode>,
    next_id: &mut u32,
    parent_stack: &mut Vec<usize>,
) {
    // LayoutEngine already flattened the tree in pre-order.
    // We need to reconstruct parent/child links.
    // Strategy: for each node, its parent is the last container on the stack
    // that still has children to assign.

    struct StackFrame {
        node_idx: usize,
        expected_children: usize,
        assigned_children: usize,
    }

    let mut stack: Vec<StackFrame> = Vec::new();

    for (widget, layout) in pairs {
        let node_idx = nodes.len();
        let id = EntityId(*next_id);
        *next_id += 1;

        // Determine parent from stack.
        let parent_idx = stack.last().map(|f| f.node_idx);

        let component = widget_to_component(widget);
        let child_count = widget_child_count(widget);

        let uidl_id = widget_id(widget).map(|s| s.to_string());

        let node = EcsNode {
            id,
            uidl_id,
            component,
            layout: *layout,
            interaction: InteractionState::default(),
            parent: parent_idx,
            children: Vec::new(),
        };
        nodes.push(node);

        // Register as child of parent.
        if let Some(pidx) = parent_idx {
            nodes[pidx].children.push(node_idx);
        }

        // Update parent stack counts.
        if let Some(top) = stack.last_mut() {
            top.assigned_children += 1;
        }
        // Pop fully-consumed frames.
        while stack.last().is_some_and(|f| f.assigned_children >= f.expected_children) {
            stack.pop();
        }

        // Push this node onto the stack if it's a container.
        if child_count > 0 {
            stack.push(StackFrame {
                node_idx,
                expected_children: child_count,
                assigned_children: 0,
            });
        }
    }

    // Suppress unused warning — parent_stack parameter kept for API symmetry.
    let _ = parent_stack;
}

// ─── Widget → Component helpers ──────────────────────────────────────────────

fn widget_to_component(widget: &Widget) -> WidgetComponent {
    match widget {
        Widget::Text { content, style, .. } => WidgetComponent::Text {
            content: content.clone(),
            style: style.clone(),
        },
        Widget::Button { label, action, style, .. } => WidgetComponent::Button {
            label: label.clone(),
            action: action.clone(),
            style: style.clone(),
        },
        Widget::Input { placeholder, value, on_change, .. } => WidgetComponent::Input {
            placeholder: placeholder.clone(),
            value: value.clone(),
            on_change: on_change.clone(),
        },
        Widget::VStack { gap, padding, .. } => WidgetComponent::VStack {
            gap: gap.map(|g| g as f32).unwrap_or(8.0),
            padding: padding.map(|p| p as f32).unwrap_or(0.0),
        },
        Widget::HStack { gap, .. } => WidgetComponent::HStack {
            gap: gap.map(|g| g as f32).unwrap_or(8.0),
        },
        Widget::Spacer { size } => WidgetComponent::Spacer {
            size: size.map(|s| s as f32).unwrap_or(16.0),
        },
    }
}

fn widget_child_count(widget: &Widget) -> usize {
    match widget {
        Widget::VStack { children, .. } | Widget::HStack { children, .. } => children.len(),
        _ => 0,
    }
}

fn widget_id(widget: &Widget) -> Option<&str> {
    match widget {
        Widget::Text { id, .. } => id.as_deref(),
        Widget::Button { id, .. } => id.as_deref(),
        Widget::Input { id, .. } => id.as_deref(),
        Widget::VStack { id, .. } => id.as_deref(),
        Widget::HStack { id, .. } => id.as_deref(),
        Widget::Spacer { .. } => None,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uidl::UidlDocument;

    fn parse(json: &str) -> UidlDocument {
        UidlDocument::parse(json).expect("parse failed")
    }

    #[test]
    fn test_build_single_text() {
        let doc = parse(r#"{"layout":{"type":"text","content":"hello"}}"#);
        let tree = build_ecs_tree(&doc, 400, 300).unwrap();
        assert_eq!(tree.len(), 1);
        let root = tree.root().unwrap();
        assert_eq!(root.id, EntityId(1));
        assert!(root.parent.is_none());
        assert!(root.children.is_empty());
        assert!(matches!(root.component, WidgetComponent::Text { .. }));
    }

    #[test]
    fn test_build_vstack_with_children() {
        let doc = parse(r#"{
            "layout": {
                "type": "vstack",
                "gap": 8,
                "children": [
                    {"type": "text", "content": "A"},
                    {"type": "button", "label": "B", "action": "click"}
                ]
            }
        }"#);
        let tree = build_ecs_tree(&doc, 400, 300).unwrap();
        // vstack + 2 children = 3 nodes
        assert_eq!(tree.len(), 3);

        let root = tree.root().unwrap();
        assert!(matches!(root.component, WidgetComponent::VStack { .. }));
        assert_eq!(root.children.len(), 2);
        assert!(root.parent.is_none());

        let child0 = &tree.nodes[root.children[0]];
        assert!(matches!(child0.component, WidgetComponent::Text { .. }));
        assert_eq!(child0.parent, Some(0));

        let child1 = &tree.nodes[root.children[1]];
        assert!(matches!(child1.component, WidgetComponent::Button { .. }));
        assert_eq!(child1.parent, Some(0));
    }

    #[test]
    fn test_layout_boxes_assigned() {
        let doc = parse(r#"{
            "layout": {
                "type": "vstack",
                "children": [
                    {"type": "text", "content": "Title"},
                    {"type": "input", "placeholder": "name"}
                ]
            }
        }"#);
        let tree = build_ecs_tree(&doc, 800, 600).unwrap();
        for node in &tree.nodes {
            // Every node should have non-zero width.
            assert!(node.layout.width > 0.0, "node {:?} has zero width", node.id);
        }
    }

    #[test]
    fn test_entity_ids_sequential() {
        let doc = parse(r#"{
            "layout": {
                "type": "hstack",
                "children": [
                    {"type": "button", "label": "A", "action": "a"},
                    {"type": "button", "label": "B", "action": "b"}
                ]
            }
        }"#);
        let tree = build_ecs_tree(&doc, 400, 300).unwrap();
        let ids: Vec<u32> = tree.nodes.iter().map(|n| n.id.0).collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn test_uidl_id_propagated() {
        let doc = parse(r#"{
            "layout": {
                "type": "button",
                "label": "Go",
                "action": "go",
                "id": "btn-go"
            }
        }"#);
        let tree = build_ecs_tree(&doc, 400, 300).unwrap();
        assert_eq!(tree.root().unwrap().uidl_id.as_deref(), Some("btn-go"));
    }

    #[test]
    fn test_interactive_nodes() {
        let doc = parse(r#"{
            "layout": {
                "type": "vstack",
                "children": [
                    {"type": "text", "content": "label"},
                    {"type": "button", "label": "OK", "action": "ok"},
                    {"type": "input", "placeholder": "type"}
                ]
            }
        }"#);
        let tree = build_ecs_tree(&doc, 400, 600).unwrap();
        let interactive: Vec<_> = tree.interactive_nodes().collect();
        assert_eq!(interactive.len(), 2);
    }

    #[test]
    fn test_hit_test_button() {
        let doc = parse(r#"{
            "layout": {
                "type": "button",
                "label": "Click",
                "action": "test_action"
            }
        }"#);
        let tree = build_ecs_tree(&doc, 400, 300).unwrap();
        // Button occupies the top of the canvas; hitting near (10, 10) should find it.
        let hit = tree.hit_test(10.0, 10.0);
        assert!(hit.is_some());
        assert!(matches!(hit.unwrap().component, WidgetComponent::Button { .. }));
    }

    #[test]
    fn test_hit_test_miss() {
        let doc = parse(r#"{"layout":{"type":"text","content":"no click"}}"#);
        let tree = build_ecs_tree(&doc, 400, 300).unwrap();
        // Text is not interactive, so hit test should return None.
        let hit = tree.hit_test(10.0, 10.0);
        assert!(hit.is_none());
    }

    #[test]
    fn test_action_routing() {
        let doc = parse(r#"{
            "layout": {
                "type": "button",
                "label": "Start",
                "action": "timer.start"
            }
        }"#);
        let tree = build_ecs_tree(&doc, 400, 300).unwrap();
        let root = tree.root().unwrap();
        assert_eq!(root.action(), Some("timer.start"));
    }

    #[test]
    fn test_theme_preserved() {
        let doc = parse(r##"{
            "layout": {"type": "text", "content": "hi"},
            "theme": {"primary_color": "#FF5500"}
        }"##);
        let tree = build_ecs_tree(&doc, 400, 300).unwrap();
        assert!(tree.theme.is_some());
        assert_eq!(
            tree.theme.unwrap().primary_color.as_deref(),
            Some("#FF5500")
        );
    }

    #[test]
    fn test_nested_hstack_in_vstack() {
        let doc = parse(r#"{
            "layout": {
                "type": "vstack",
                "children": [
                    {"type": "text", "content": "title"},
                    {
                        "type": "hstack",
                        "children": [
                            {"type": "button", "label": "A", "action": "a"},
                            {"type": "button", "label": "B", "action": "b"}
                        ]
                    }
                ]
            }
        }"#);
        let tree = build_ecs_tree(&doc, 800, 600).unwrap();
        // vstack + text + hstack + btn_a + btn_b = 5
        assert_eq!(tree.len(), 5);

        // root has 2 children (text + hstack)
        assert_eq!(tree.root().unwrap().children.len(), 2);

        // hstack is at index 2 and should have 2 children
        let hstack_idx = tree.root().unwrap().children[1];
        let hstack = &tree.nodes[hstack_idx];
        assert!(matches!(hstack.component, WidgetComponent::HStack { .. }));
        assert_eq!(hstack.children.len(), 2);
    }

    #[test]
    fn test_get_by_entity_id() {
        let doc = parse(r#"{"layout":{"type":"text","content":"lookup"}}"#);
        let tree = build_ecs_tree(&doc, 400, 300).unwrap();
        let found = tree.get(EntityId(1));
        assert!(found.is_some());
        let not_found = tree.get(EntityId(999));
        assert!(not_found.is_none());
    }

    #[test]
    fn test_spacer_component() {
        let doc = parse(r#"{
            "layout": {
                "type": "vstack",
                "children": [
                    {"type": "spacer", "size": 24}
                ]
            }
        }"#);
        let tree = build_ecs_tree(&doc, 400, 300).unwrap();
        let spacer = tree.nodes.iter().find(|n| matches!(n.component, WidgetComponent::Spacer { .. }));
        assert!(spacer.is_some());
        if let WidgetComponent::Spacer { size } = spacer.unwrap().component {
            assert_eq!(size, 24.0);
        }
    }

    #[test]
    fn test_empty_vstack() {
        let doc = parse(r#"{"layout":{"type":"vstack","children":[]}}"#);
        let tree = build_ecs_tree(&doc, 400, 300).unwrap();
        // VStack itself is one node, no children.
        assert_eq!(tree.len(), 1);
        assert!(tree.root().unwrap().children.is_empty());
    }

    #[test]
    fn test_is_empty_false() {
        let doc = parse(r#"{"layout":{"type":"text","content":"x"}}"#);
        let tree = build_ecs_tree(&doc, 400, 300).unwrap();
        assert!(!tree.is_empty());
    }
}
