// SPDX-License-Identifier: GPL-2.0
/*
 * ai_scheduler.bpf.c -- AIOS AI-driven scheduler (sched_ext)
 *
 * Architecture:
 *   - Reads ai_weights BPF map (populated by ai_decision.rs every 5s)
 *   - Uses weights to influence task scheduling decisions
 *   - Minimal overhead: weights are pre-computed by AI, not inferred per-task
 *
 * Requirements: Linux 6.12+ with CONFIG_SCHED_CLASS_EXT=y
 *
 * TODO: Implement with libbpf + sched_ext when Linux 6.12 baseline is set.
 */

#include <vmlinux.h>
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_tracing.h>

/// AI-computed CPU weights per cgroup (written by ai_decision.rs)
/// Key: cgroup ID (u64), Value: CPU weight (u32, 1-10000)
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 1024);
    __type(key, u64);
    __type(value, u32);
} ai_weights SEC(".maps");

/// Statistics map for userspace monitoring
struct {
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __uint(max_entries, 1);
    __type(key, u32);
    __type(value, u64);
} scheduler_stats SEC(".maps");

// Minimal sched_ext_ops registration (PoC skeleton)
// Currently a no-op scheduler that defers all decisions to the default scheduler.
// The ai_weights map is populated by ai_decision.rs and will be read here
// once the full implementation is added.

#ifdef CONFIG_SCHED_CLASS_EXT

SEC("struct_ops/aios_enqueue")
void BPF_PROG(aios_enqueue, struct task_struct *p, u64 enq_flags)
{
    // TODO: Use ai_weights to influence task placement
    // For now, enqueue to default local DSQ
    scx_bpf_dispatch(p, SCX_DSQ_LOCAL, SCX_SLICE_DFL, enq_flags);
}

SEC("struct_ops/aios_dispatch")
void BPF_PROG(aios_dispatch, s32 cpu, struct task_struct *prev)
{
    scx_bpf_consume(SCX_DSQ_LOCAL);
}

SEC(".struct_ops")
struct sched_ext_ops aios_ops = {
    .enqueue    = (void *)aios_enqueue,
    .dispatch   = (void *)aios_dispatch,
    .name       = "aios_scheduler",
};

#endif /* CONFIG_SCHED_CLASS_EXT */

char LICENSE[] SEC("license") = "GPL";
