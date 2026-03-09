# openSystem

**AIを前提とするOS。**

> ⚠️ **実験的プロジェクト。** 本プロジェクトは初期研究段階にあり、本番環境での使用は推奨されません。
> API、設定形式、アーキテクチャは予告なく変更される場合があります。コントリビューションや大胆なアイデアを歓迎します。

**GitHub:** [soolaugust/openSystem](https://github.com/soolaugust/openSystem)

[English](README.md) | [简体中文](README.zh-CN.md) | 日本語 | [한국어](README.ko.md)

今日存在するすべてのオペレーティングシステムは、大規模言語モデルが登場する前に設計されました。
Linuxは人間が操作するために設計されました。AIOSはAIが操作するために設計されています——
そして人間が*指示*する。

AIOSはLinuxディストリビューションではありません。研究プロトタイプでもありません。
これは明確な賭けです：5年以内に、あらゆる意味のあるOS操作がAIによって仲介されるでしょう。
私たちはその前提から出発したOSを構築しています。50年のPOSIXレガシーの上にAIを乗せるのではなく。

**あなたが以下を信じるなら、このプロジェクトはあなたを怒らせるでしょう：**
- 決定論的システムは常に確率的システムより安全だ
- ユーザーはOSが何をしているか理解すべきだ
- POSIX互換性は制約ではなく機能だ

**あなたが以下を信じるなら、このプロジェクトはあなたのためにあります：**
- 1970年代のシェルの比喩はとっくに役目を終えている
- AI推論はシステムコールパスに組み込むほど安価になっている
- あなたがこれまで使った中で最高のOSはまだ構築されていない

## アーキテクチャ

```
┌──────────────────────────────────────────────────────────────┐
│                     ユーザーインタラクション層               │
│   自然言語ターミナル / Web UI / 音声（whisper.cpp）          │
├──────────────────────────────────────────────────────────────┤
│                   os-agent デーモン                          │
│  意図認識 → コード生成 → UI生成 → リソース決定 → Appストア  │
├──────────────┬───────────────┬──────────────────────────────┤
│  App ランタイム│  GUIレンダラー│     システムサービスバス      │
│  Wasmtime    │  Bevy + wgpu  │   (os-syscall-bindings)      │
├──────────────┴───────────────┴──────────────────────────────┤
│                     AI ランタイム層                          │
│    リモート LLM API（OpenAI互換）+ whisper.cpp              │
├──────────────────────────────────────────────────────────────┤
│                リソーススケジューリング（AI駆動）             │
│    eBPFプローブ + AI決定ループ + cgroup v2                  │
├──────────────────────────────────────────────────────────────┤
│                  Linux 6.x 最小カーネル                      │
│    sched_ext + io_uring + eBPF + KMS/DRM + cgroup v2        │
└──────────────────────────────────────────────────────────────┘
```

## Linuxとの関係

> AIOSはv1においてLinuxをハードウェア抽象化レイヤーとして使用しながら、独自カーネルを並行開発しています。
> ハードウェアサポートについてLinuxを参考にし、30年分のドライバー開発に感謝します。
> しかし私たちのプロセスモデルはPOSIXではなく、私たちのシェルはシェルではありません。
> Linuxの互換性が必要な場合：このプロジェクトをフォークして互換レイヤーを構築してください——リンクはしますが、マージはしません。

## はじめに

### 必要環境
- Rust 1.75+
- `wasm32-wasip1` Rustターゲット：`rustup target add wasm32-wasip1`
- Python 3.10+（rom-builderスクリプト用）
- QEMU（テスト用）
- リモートLLM APIエンドポイント（OpenAI互換、例：DeepSeek、Claude、Qwen）

### ビルド

```bash
cargo build --workspace
```

### QEMUで実行

```bash
# システムイメージをビルド
python3 rom-builder/build.py --manifest hardware_manifest_qemu.json

# 推奨設定でQEMUを起動
qemu-system-x86_64 \
  -hda system.img \
  -m 8G \
  -smp 4 \
  -enable-kvm \
  -device virtio-net-pci,netdev=net0 \
  -netdev user,id=net0,hostfwd=tcp::8080-:8080 \
  -nographic
```

`-nographic` はヘッドレスモード（シリアルコンソール）で動作します。ポート8080はapp-store API用に転送されます。GUIセッションの場合は `-nographic` をvirtio-gpuに置き換えてください：

```bash
qemu-system-x86_64 \
  -hda system.img -m 8G -smp 4 -enable-kvm \
  -device virtio-gpu -device virtio-keyboard-pci -device virtio-mouse-pci \
  -device virtio-net-pci,netdev=net0 \
  -netdev user,id=net0,hostfwd=tcp::8080-:8080
```

### AIモデルの設定

初回起動時、セットアップウィザードが対話形式でモデル設定を案内します。
再設定する場合：

```bash
opensystem-setup
```

設定ファイルは `/etc/os-agent/model.conf` に保存されます。直接編集することも可能です：

```toml
[api]
base_url = "https://api.deepseek.com/v1"   # OpenAI互換エンドポイントであれば何でも可
api_key  = "<your-api-key>"
model    = "deepseek-chat"
# api_format = "anthropic"                 # Anthropicネイティブ形式の場合はコメントを外す

[network]
timeout_ms  = 10000
retry_count = 3

[fallback]                                 # 任意：フォールバックエンドポイント
base_url = "https://api.anthropic.com/v1"
api_key  = "<your-api-key>"
model    = "claude-sonnet-4-6"
```

**サポートされるAPIフォーマット：**

| フォーマット | `api_format` の値 | 認証ヘッダー | 対応プロバイダー例 |
|------------|------------------|------------|-----------------|
| OpenAI互換（デフォルト）| `"openai"` または省略 | `Authorization: Bearer` | DeepSeek、Qwen、vLLM、OpenAI |
| Anthropicネイティブ | `"anthropic"` | `x-api-key` | Claude (api.anthropic.com) |

> URLに `"anthropic"` が含まれる場合、Anthropicフォーマットとして自動検出されます。`api_format` の明示的な設定は不要です。

### 自然言語ターミナル

起動後、システムは `opensystem>` プロンプトを表示し、自然言語入力を受け付けます：

```
opensystem> システムのメモリ状態を確認して
opensystem> 現在のディレクトリのファイルを一覧表示して
opensystem> 25分作業・5分休憩のポモドーロタイマーアプリを作って
```

最後のコマンドは自動的にRust/WASMコードを生成・コンパイルし、`.osp`アプリパッケージとしてインストールします。所要時間は約30秒です。

## 論争的な立場

**システムコールパスにおけるAIについて：**
> 「AI推論は遅すぎないか？」— 今はそうです。私たちは推論レイテンシが1000msではなく10msの世界に向けて最適化しています。

**ネットワーク依存について：**
> オフラインモードは目標ではありません。これはiPhoneがiCloudについて下したのと同じ決断です。

**POSIXについて：**
> AIOSではソフトウェアはオンデマンドで生成されます。POSIX互換性は、ストリーミングサービスにVHSサポートを要求するようなものです。

## ライセンス

MIT
