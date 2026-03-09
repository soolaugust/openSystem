//! AI-driven resource scheduler for openSystem.
//!
//! Collects per-cgroup metrics via [`monitor::CgroupMonitor`], feeds a
//! [`types::SystemSnapshot`] to [`ai_decision::AiDecisionLoop`], and applies
//! the resulting [`types::ResourceAction`]s through [`executor::CgroupExecutor`].

pub mod ai_decision;
pub mod executor;
pub mod monitor;
pub mod types;

pub use ai_decision::AiDecisionLoop;
pub use executor::CgroupExecutor;
pub use monitor::CgroupMonitor;
pub use types::{CgroupMetrics, DecisionResponse, ResourceAction, SystemSnapshot};
