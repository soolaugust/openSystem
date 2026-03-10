# app-store

openSystem 的应用分发服务 — Ed25519 签名的 `.osp` 包注册表，带 HTTP API 和 `osctl` CLI。

## 概述

app-store 负责 openSystem 应用的发布、搜索与安装。每个应用以 `.osp`（openSystem Package）格式打包，包含 WASM 字节码、UIDL GUI 描述、清单文件和 Ed25519 签名，保证分发完整性。

```
开发者打包 .osp
      ↓
POST /api/apps/upload  →  签名验证 → SQLite 注册
      ↓
用户 "install calculator"
      ↓
GET /api/apps/search?q=calculator  →  GET /api/apps/:id/download
      ↓
os-agent 提取 .osp → /apps/<uuid>/
```

## .osp 包格式

`.osp` 是标准 `.tar.gz` 归档，内部结构：

```
app.wasm          # 编译好的 WASM 字节码（wasm32-wasip1）
manifest.json     # 应用元数据
uidl.json         # GUI 布局描述（可选，AI 生成）
icon.png          # 应用图标（可选）
signature.json    # Ed25519 签名（wasm + manifest 的 SHA256）
```

`manifest.json` 示例：

```json
{
  "name": "Pomodoro Timer",
  "version": "0.1.0",
  "description": "25-minute focus timer",
  "app_id": "pomodoro-timer",
  "has_uidl": true
}
```

## HTTP API

服务默认监听 `:8080`，通过 `OPENSYSTEM_STORE_URL` 环境变量配置客户端地址。

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/api/apps/upload` | 上传 `.osp` 包（multipart/form-data，字段名 `file`） |
| `GET` | `/api/apps/search?q=<keyword>` | 按关键词搜索应用，返回 JSON 数组 |
| `GET` | `/api/apps/:id` | 获取单个应用元数据 |
| `GET` | `/api/apps/:id/download` | 下载 `.osp` 包 |

### 认证

设置 `OPENSYSTEM_STORE_API_KEY` 环境变量启用上传认证。客户端需在请求头中携带：

```
X-Api-Key: <your-secret-key>
```

未设置时跳过认证（适合本地开发）。

## 启动服务

```bash
# 使用默认路径（./store_data）
cargo run -p app-store

# 指定数据目录
STORE_DIR=/var/lib/opensystem/store cargo run -p app-store

# 启用上传认证
OPENSYSTEM_STORE_API_KEY=mysecret cargo run -p app-store
```

## osctl CLI

`osctl` 是应用包管理工具：

```bash
# 上传应用到商店
osctl publish ./my-app.osp

# 搜索应用
osctl search calculator

# 查看应用详情
osctl info <app-id>

# 列出所有应用
osctl list
```

## Ed25519 签名机制

上传时自动对 `app.wasm + manifest.json` 的 SHA256 摘要进行 Ed25519 签名：

```rust
// 签名流程（signing.rs）
let digest = sha256(wasm_bytes || manifest_bytes);
let signature = signing_key.sign(&digest);
// 签名写入 signature.json，公钥存入注册表
```

下载时可离线验证签名，无需联网。v2.1 计划强制验证。

## 技术栈

| 组件 | 依赖 |
|------|------|
| HTTP 框架 | axum 0.7 |
| 数据库 | SQLite（rusqlite 0.31，bundled） |
| 签名 | ed25519-dalek 2 |
| 哈希 | sha2 0.10 |
| 包格式 | tar + flate2（.tar.gz） |
| 异步运行时 | tokio 1 |

## 开发

```bash
# 运行所有测试
cargo test -p app-store

# 运行基准测试
cargo bench -p app-store

# 集成测试（含认证、签名端到端）
cargo test -p app-store -- --test-output immediate
```

测试覆盖：注册表 CRUD、签名验证、HTTP 路由（tower test client）、认证中间件、签名端到端。

## 安全说明

- 认证为简单 bearer token 比对，无用户隔离和 key rotation
- 无内置限流，公网部署需在 nginx 等反向代理层配置
- HTTPS 由上层终止，服务本身不强制 TLS
- app ID 用于文件路径时的正则校验计划在 v2.1 加入

详见根目录 [SECURITY.md](../SECURITY.md)。
