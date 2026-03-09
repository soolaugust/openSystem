//! GPU-accelerated UI renderer for openSystem.
//!
//! Parses [`uidl::UidlDocument`] trees and renders them via
//! [`bevy_renderer::BevyRenderer`] (hardware) or [`bevy_renderer::SoftwareRenderer`] (fallback).

pub mod bevy_renderer;
pub mod cache;
pub mod uidl;

pub use bevy_renderer::{BevyRenderer, RenderHandle, Renderer, SoftwareRenderer};
pub use cache::UidlCache;
pub use uidl::{TextStyle, Theme, UidlDocument, Widget};
