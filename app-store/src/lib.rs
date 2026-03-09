//! openSystem App Store — package registry, signing, and HTTP API.
//!
//! # Crate layout
//! - [`manifest`] — `.osp` package manifest types
//! - [`osp`] — read/write `.osp` package archives
//! - [`registry`] — SQLite-backed [`registry::AppRegistry`]
//! - [`server`] — axum HTTP server ([`server::create_router`])
//! - [`signing`] — Ed25519 signing helpers

pub mod manifest;
pub mod osp;
pub mod registry;
pub mod server;
pub mod signing;
