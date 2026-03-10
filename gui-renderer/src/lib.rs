//! GPU-accelerated UI renderer for openSystem.
//!
//! Parses [`uidl::UidlDocument`] trees and renders them via
//! [`bevy_renderer::BevyRenderer`] (hardware) or [`bevy_renderer::SoftwareRenderer`] (fallback).

pub mod bevy_renderer;
pub mod cache;
pub mod event_bridge;
pub mod uidl;
pub mod uidl_to_ecs;
pub mod widget_system;

pub use bevy_renderer::{BevyRenderer, RenderHandle, Renderer, SoftwareRenderer};
pub use cache::UidlCache;
pub use uidl::{TextStyle, Theme, UidlDocument, Widget};
pub use event_bridge::{apply_patches, EventBridge, UidlPatch, UiEvent, WasmCallback};
pub use uidl_to_ecs::{build_ecs_tree, EcsNode, EcsTree, EntityId, WidgetComponent};
pub use widget_system::render_to_rgba;
