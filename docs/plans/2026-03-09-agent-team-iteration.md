# Agent Team Iteration System Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 创建一个持续迭代的 agent team，自主推进 openSystem 项目走向商用就绪，通过飞书通知进度。

**Architecture:** team-lead 作为协调中枢，驱动 analyst → optimizer → coder → reviewer → tester 的循环。每轮状态写入 `docs/iteration/` 本地目录（不提交 git）。里程碑时自动打 release tag 并推送 GitHub，同时通过飞书发送进度通知。

**Tech Stack:** Claude Code Agent Teams, Feishu MCP, git, cargo, Rust

---

## 里程碑定义文件

### Task 1: 创建迭代状态目录和里程碑定义

**Files:**
- Create: `docs/iteration/milestones.json`
- Create: `docs/iteration/current.json`
- Create: `.gitignore` (追加 docs/iteration/)

**Step 1: 追加 .gitignore**

```bash
echo "docs/iteration/" >> .gitignore
```

**Step 2: 创建里程碑定义**

```json
// docs/iteration/milestones.json
{
  "milestones": [
    {
      "version": "v0.1.0",
      "description": "核心链路跑通",
      "criteria": [
        "os-agent 能完整走通 intent → app生成 → 安装流程",
        "cargo clippy --workspace -- -D warnings 零警告",
        "cargo test --workspace 全部通过",
        "基础测试覆盖率 > 20%"
      ]
    },
    {
      "version": "v0.2.0",
      "description": "app-store 完整可用",
      "criteria": [
        "app-store HTTP API 完整实现并有集成测试",
        "osctl publish/install/list 完整可用",
        "Ed25519 签名验证端到端测试",
        "测试覆盖率 > 40%"
      ]
    },
    {
      "version": "v0.3.0",
      "description": "resource-scheduler 真实调度",
      "criteria": [
        "eBPF scheduler 真实编译运行",
        "AI decision loop 有完整集成测试",
        "稳定性测试：连续运行 1h 无 panic",
        "文档覆盖所有公开 API"
      ]
    },
    {
      "version": "v0.5.0",
      "description": "完整集成演示",
      "criteria": [
        "所有子系统集成联调",
        "测试覆盖率 > 60%",
        "CI badge 绿色",
        "QEMU 完整演示可复现"
      ]
    },
    {
      "version": "v1.0.0",
      "description": "生产就绪",
      "criteria": [
        "安全审计完成",
        "完整用户文档",
        "所有 panic 路径有错误处理",
        "性能基准测试建立"
      ]
    }
  ]
}
```

**Step 3: 创建初始迭代状态**

```json
// docs/iteration/current.json
{
  "round": 0,
  "current_version": "0.0.1",
  "next_target": "v0.1.0",
  "status": "initializing",
  "last_updated": "",
  "backlog": [],
  "completed_this_round": [],
  "metrics": {
    "test_count": 10,
    "clippy_warnings": 0,
    "test_coverage_pct": 0
  }
}
```

**Step 4: 确认 docs/iteration/ 不被 git 追踪**

```bash
git status docs/iteration/
# 应该显示 ignored
```

---

## Agent Team 启动

### Task 2: 创建 Agent Team 并启动循环

**核心设计原则：**
- team-lead 是唯一的"大脑"，负责读写 `docs/iteration/current.json`
- 每个 agent 只做自己职责内的事，完成后汇报给 team-lead
- 飞书通知 open_id: `ou_e2482d6084d87ba70767c024e40ee0c6`
- git push 使用已配置的 `origin`（git@github-soolaugust:soolaugust/openSystem.git）

**Step 1: 创建 team**

调用 TeamCreate，team_name = "opensystem-iteration"

**Step 2: 创建任务**

按顺序创建以下 TaskCreate：
1. `[Round N] analyst: 评估商用就绪度` — analyst 负责
2. `[Round N] optimizer: 生成 backlog` — optimizer 负责（blocked by analyst）
3. `[Round N] coder: 实现 backlog` — coder 负责（blocked by optimizer）
4. `[Round N] reviewer: code review` — reviewer 负责（blocked by coder）
5. `[Round N] tester: 测试验证` — tester 负责（blocked by reviewer）
6. `[Round N] team-lead: 里程碑判断 + 通知` — team-lead 负责（blocked by tester）

**Step 3: 启动 6 个 agent**

- `analyst` (general-purpose): 分析商用就绪度
- `optimizer` (general-purpose): 生成迭代 backlog
- `coder` (general-purpose): 功能开发
- `reviewer` (superpowers:code-reviewer): 代码 review
- `tester` (general-purpose): 测试验证
- `team-lead` 由主会话 (当前 Claude) 担任

**Step 4: team-lead 循环逻辑**

```
loop:
  等待 tester 完成信号
  读取 docs/iteration/current.json
  评估是否达到 next_target 里程碑
  if 达到:
    git tag vX.Y.Z
    git push origin vX.Y.Z
    更新 current_version, next_target
    飞书通知: "🎉 openSystem vX.Y.Z 已发布！[里程碑描述]"
  else:
    飞书通知: "🔄 Round N 完成，当前进度: [摘要]"
  更新 round += 1
  重新分配下一轮任务
```

---

## Agent 职责详细说明

### analyst 职责

每轮读取：
- `cargo test --workspace` 输出
- `cargo clippy --workspace` 输出
- `docs/iteration/current.json` 的 metrics
- 代码结构（哪些模块功能缺失）

输出到 `docs/iteration/analysis_round_N.json`：
```json
{
  "round": N,
  "readiness_score": 0-100,
  "milestone_progress": {
    "v0.1.0": { "met": [...], "unmet": [...] }
  },
  "top_gaps": ["gap1", "gap2", "gap3"]
}
```

### optimizer 职责

读取 analyst 输出，生成优先级排序的 backlog，写入 `docs/iteration/backlog_round_N.json`：
```json
{
  "round": N,
  "items": [
    {
      "priority": 1,
      "title": "修复 X",
      "scope": "os-agent/src/xxx.rs",
      "rationale": "阻塞里程碑 v0.1.0 的 criterion Y",
      "estimated_complexity": "small|medium|large"
    }
  ]
}
```

### coder 职责

按 backlog 优先级逐项实现，每完成一项：
- `cargo build --workspace` 确认编译
- `cargo clippy --workspace -- -D warnings` 确认无警告
- `git commit -m "feat/fix: ..."`

完成后更新 `docs/iteration/current.json` 的 `completed_this_round`。

### reviewer 职责

对本轮所有 commit 执行 code review：
- 安全性、正确性、可维护性
- 输出 `docs/iteration/review_round_N.md`
- 严重问题交回 coder 修复，minor 问题记录

### tester 职责

- `cargo test --workspace` 并记录结果
- 为本轮新增功能补充测试用例
- 统计测试数量变化
- 输出 `docs/iteration/test_round_N.json`：
```json
{
  "round": N,
  "tests_total": N,
  "tests_passed": N,
  "new_tests_added": N,
  "coverage_estimate": "X%",
  "failures": []
}
```

---

## 飞书通知格式

### 进度通知（每轮完成）
```
🔄 openSystem 迭代进度 - Round N

目标版本: vX.Y.Z
商用就绪度: XX/100

本轮完成:
• [item1]
• [item2]

测试: N个通过 (+M个新增)
下轮重点: [top priority]
```

### 里程碑通知
```
🎉 openSystem vX.Y.Z 发布！

[里程碑描述]

达成标准:
✅ [criterion1]
✅ [criterion2]

GitHub: https://github.com/soolaugust/openSystem/releases/tag/vX.Y.Z
```
