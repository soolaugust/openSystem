//! Stability tests for the resource scheduler.
//!
//! These are long-running tests marked with #[ignore] so they don't run in
//! normal CI. Run them explicitly with:
//!   cargo test -p resource-scheduler --test stability -- --ignored

use resource_scheduler::types::{CgroupMetrics, DecisionResponse, ResourceAction, SystemSnapshot};

/// Simulate a single decision tick: build snapshot, serialize, parse response,
/// validate actions. Returns the number of actions produced.
fn simulate_decision_tick(
    tick: u64,
    metrics: &[CgroupMetrics],
) -> usize {
    // 1. Build snapshot (same as AiDecisionLoop.decision_tick)
    let snapshot = SystemSnapshot {
        metrics: metrics.to_vec(),
        total_memory_mb: 16384,
        total_cpu_cores: 8,
        timestamp_ms: 1_700_000_000_000 + tick * 5000,
    };

    // 2. Serialize (same as ask_llm builds the request)
    let snapshot_json = serde_json::to_string_pretty(&snapshot).unwrap();
    assert!(!snapshot_json.is_empty());

    // 3. Simulate LLM response based on metrics
    let decision = simulate_llm_decision(tick, metrics);

    // 4. Validate each action can be serialized/deserialized
    for action in &decision.actions {
        let json = serde_json::to_string(action).unwrap();
        let parsed: ResourceAction = serde_json::from_str(&json).unwrap();
        assert_eq!(*action, parsed);
    }

    decision.actions.len()
}

/// Simulate what an LLM might return for given metrics.
/// Deterministic based on tick number to test various action patterns.
fn simulate_llm_decision(tick: u64, metrics: &[CgroupMetrics]) -> DecisionResponse {
    let mut actions = Vec::new();

    for m in metrics {
        match tick % 6 {
            0 => {
                // No-op tick
                actions.push(ResourceAction::NoOp);
            }
            1 => {
                // CPU adjustment
                let weight = if m.cpu_usage_percent > 50.0 { 500 } else { 2048 };
                actions.push(ResourceAction::SetCpuWeight {
                    app: m.app_id.clone(),
                    weight,
                });
            }
            2 => {
                // Memory limit adjustment
                let limit = if m.memory_used_mb > m.memory_limit_mb / 2 {
                    m.memory_limit_mb + 128
                } else {
                    m.memory_limit_mb
                };
                actions.push(ResourceAction::SetMemoryLimit {
                    app: m.app_id.clone(),
                    limit_mb: limit,
                });
            }
            3 => {
                // IO weight adjustment
                actions.push(ResourceAction::SetIoWeight {
                    app: m.app_id.clone(),
                    weight: 100 + (tick % 500) as u32,
                });
            }
            4 => {
                // Mixed actions
                actions.push(ResourceAction::SetCpuWeight {
                    app: m.app_id.clone(),
                    weight: 1024,
                });
                actions.push(ResourceAction::SetIoWeight {
                    app: m.app_id.clone(),
                    weight: 500,
                });
            }
            5 => {
                // Extreme: kill app if CPU > 90 (simulated)
                if m.cpu_usage_percent > 90.0 {
                    actions.push(ResourceAction::KillApp {
                        app: m.app_id.clone(),
                        reason: "sustained high CPU".to_string(),
                    });
                } else {
                    actions.push(ResourceAction::NoOp);
                }
            }
            _ => unreachable!(),
        }
    }

    DecisionResponse {
        actions,
        reasoning: Some(format!("tick {tick}: automated stability test")),
    }
}

fn make_test_metrics(num_apps: usize, tick: u64) -> Vec<CgroupMetrics> {
    (0..num_apps)
        .map(|i| CgroupMetrics {
            app_id: format!("stability-app-{i}"),
            cpu_usage_percent: ((tick * 7 + i as u64 * 13) % 100) as f32,
            memory_used_mb: 64 + (i as u64 * 32 + tick) % 512,
            memory_limit_mb: 1024,
            io_read_kb_s: (tick + i as u64) % 1000,
            io_write_kb_s: (tick * 3 + i as u64) % 500,
            net_rx_kb_s: 0,
            net_tx_kb_s: 0,
            pid_count: 1 + (i as u32 % 10),
            timestamp_ms: 1_700_000_000_000 + tick * 5000,
        })
        .collect()
}

#[test]
#[ignore]
fn stability_720_ticks_single_app() {
    let mut total_actions = 0;
    for tick in 0..720 {
        let metrics = make_test_metrics(1, tick);
        total_actions += simulate_decision_tick(tick, &metrics);
    }
    // 720 ticks × at least 1 action each = at least 720
    assert!(total_actions >= 720, "Expected >= 720 actions, got {total_actions}");
}

#[test]
#[ignore]
fn stability_720_ticks_multiple_apps() {
    let mut total_actions = 0;
    for tick in 0..720 {
        let num_apps = 1 + (tick as usize % 5); // 1-5 apps, varying
        let metrics = make_test_metrics(num_apps, tick);
        total_actions += simulate_decision_tick(tick, &metrics);
    }
    // At least 720 ticks with at least 1 app each
    assert!(total_actions >= 720, "Expected >= 720 actions, got {total_actions}");
}

#[test]
#[ignore]
fn stability_snapshot_serialization_pressure() {
    // Test that repeated serialization/deserialization doesn't leak or panic
    for tick in 0..720 {
        let metrics = make_test_metrics(10, tick); // 10 apps
        let snapshot = SystemSnapshot {
            metrics,
            total_memory_mb: 32768,
            total_cpu_cores: 16,
            timestamp_ms: 1_700_000_000_000 + tick * 5000,
        };

        let json = serde_json::to_string(&snapshot).unwrap();
        let parsed: SystemSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.metrics.len(), 10);
    }
}

// This non-ignored test verifies the stability framework itself works (quick sanity check)
#[test]
fn stability_framework_sanity_check() {
    let metrics = make_test_metrics(3, 42);
    let count = simulate_decision_tick(42, &metrics);
    assert!(count > 0);
}
