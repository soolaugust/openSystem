# openSystem

**AI를 전제로 하는 운영체제.**

> ⚠️ **실험적 프로젝트.** 본 프로젝트는 초기 연구 단계에 있으며, 프로덕션 환경에서의 사용은 권장하지 않습니다.
> API, 설정 형식, 아키텍처는 예고 없이 변경될 수 있습니다. 기여와 대담한 아이디어를 환영합니다.

**GitHub:** [soolaugust/openSystem](https://github.com/soolaugust/openSystem) · **v0.2.0-alpha** · 테스트 281개, 실패 0개

[English](README.md) | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | 한국어

오늘날 존재하는 모든 운영체제는 대형 언어 모델이 등장하기 전에 설계되었습니다.
Linux는 인간이 조작하도록 설계되었습니다. AIOS는 AI가 조작하도록 설계되었습니다——
그리고 인간이 *지시*합니다.

AIOS는 Linux 배포판이 아닙니다. 연구 프로토타입도 아닙니다.
이것은 명확한 베팅입니다: 5년 이내에 모든 의미 있는 OS 상호작용은 AI에 의해 매개될 것입니다.
우리는 그 전제에서 출발하는 OS를 구축하고 있습니다. 50년의 POSIX 레거시 위에 AI를 얹는 것이 아니라.

**당신이 다음을 믿는다면, 이 프로젝트는 당신을 불쾌하게 할 것입니다:**
- 결정론적 시스템은 항상 확률론적 시스템보다 안전하다
- 사용자는 OS가 무엇을 하는지 이해해야 한다
- POSIX 호환성은 제약이 아니라 기능이다

**당신이 다음을 믿는다면, 이 프로젝트는 당신을 위한 것입니다:**
- 1970년대 셸 비유는 이미 역할을 다했다
- AI 추론은 시스템 콜 경로에 들어갈 만큼 충분히 저렴해졌다
- 당신이 사용해 볼 최고의 OS는 아직 만들어지지 않았다

## 현재 사용 가능한 기능 (v0.2.0-alpha)

> 한 마디로 30초 안에 실행 중인 앱을 만들 수 있습니다.

```
opensystem> 포모도로 타이머 앱 만들어줘
  Classifying intent... CreateApp
  → Generating AppSpec from prompt...
  → App: "포모도로 타이머" — 25분 집중 타이머, 시작/정지 컨트롤 포함
  → Generating Rust/Wasm code (this may take ~30s)...
  ✓ App installed!
    UUID: 3f8a1c2d-...
    Package: /apps/3f8a1c2d-.../app.osp
    GUI layout: 847 chars of UIDL
    GUI preview: rendered 800×600 → 1920000 RGBA bytes ✓

opensystem> 포모도로 실행해줘
  → Running: 포모도로 타이머 (v0.1.0)
  → Executing WASM sandbox...
  ✓ App output:
    포모도로 타이머가 시작되었습니다. 25분간 집중하세요.
```

### 기능 현황

| 기능 | 상태 | 구현 |
|------|------|------|
| 자연어 → 앱 생성 | ✅ 동작 중 | `os-agent` 의도 파이프라인 + LLM 코드 생성 |
| WASM 샌드박스 실행 | ✅ 동작 중 | wasmtime 42 / WASIp1, `MemoryOutputPipe` 출력 캡처 |
| 앱 스토어 설치/검색 | ✅ 동작 중 | SQLite 레지스트리 + Ed25519 서명 `.osp` 패키지 |
| 소프트웨어 GUI 렌더링 | ✅ 동작 중 | tiny-skia 0.12 + fontdue 0.9 픽셀 래스터라이저 |
| UIDL → ECS 컴포넌트 트리 | ✅ 동작 중 | `build_ecs_tree()` 히트 테스트·레이아웃 엔진 포함 |
| UI 이벤트 → WASM 콜백 | ✅ 동작 중 | `EventBridge` 양방향 채널 |
| AI 생성 GUI 레이아웃 | ✅ 동작 중 | `UIDL_GEN_SYSTEM_PROMPT` few-shot 스키마 |
| AI 구동 리소스 스케줄링 | ✅ 동작 중 | eBPF 프로브 + cgroup v2 + LLM 결정 루프 |
| GPU 가속 렌더링 | 🔜 v2.1 | Bevy + wgpu (ECS 트리 연결 대기 중) |
| WASM 실행 시간 제한 | 🔜 v2.1 | epoch interrupt CPU 예산 |

### 앱 라이프사이클

```
사용자 의도
    ↓
os-agent 분류 → CreateApp
    ↓
LLM 병렬 생성:
  ┌─────────────────┐    ┌──────────────────────────┐
  │  Rust/WASM 코드  │    │  UIDL JSON (위젯 트리)    │
  │  cargo check    │    │  검증 후 패키지에 기록     │
  │  → app.wasm     │    │  → uidl.json in .osp      │
  └────────┬────────┘    └────────────┬─────────────┘
           └────────────┬─────────────┘
                        ↓
              .osp 패키지 → /apps/<uuid>/
                        ↓
        ┌───────────────┴───────────────┐
        │  wasmtime 샌드박스             │  ←── RunApp 의도
        │  app.wasm 실행                │
        │  stdout 캡처                  │
        └───────────────────────────────┘
```

## 아키텍처

```
┌──────────────────────────────────────────────────────────────┐
│                     사용자 인터랙션 레이어                   │
│   자연어 터미널 / Web UI / 음성（whisper.cpp）               │
├──────────────────────────────────────────────────────────────┤
│                   os-agent 데몬                              │
│  의도 인식 → 코드 생성 → UI 생성 → 리소스 결정 → 앱 스토어  │
├──────────────┬───────────────┬──────────────────────────────┤
│  앱 런타임   │  GUI 렌더러   │       시스템 서비스 버스      │
│  Wasmtime    │  Bevy + wgpu  │   (os-syscall-bindings)      │
├──────────────┴───────────────┴──────────────────────────────┤
│                     AI 런타임 레이어                         │
│    원격 LLM API（OpenAI 호환）+ whisper.cpp                 │
├──────────────────────────────────────────────────────────────┤
│               리소스 스케줄링（AI 구동）                     │
│    eBPF 프로브 + AI 결정 루프 + cgroup v2                   │
├──────────────────────────────────────────────────────────────┤
│                  Linux 6.x 최소 커널                         │
│    sched_ext + io_uring + eBPF + KMS/DRM + cgroup v2        │
└──────────────────────────────────────────────────────────────┘
```

## Linux와의 관계

> AIOS는 v1에서 Linux를 하드웨어 추상화 레이어로 사용하면서, 자체 커널을 병행 개발합니다.
> 하드웨어 지원을 위해 Linux를 참조하며, 30년간의 드라이버 작업에 감사드립니다.
> 하지만 우리의 프로세스 모델은 POSIX가 아니며, 우리의 셸은 셸이 아닙니다.
> Linux 호환성이 필요하다면: 이 프로젝트를 포크하여 호환 레이어를 구축하세요——링크는 걸겠지만, 절대 머지하지 않겠습니다.

## 시작하기

### 요구 사항
- Rust 1.75+
- `wasm32-wasip1` Rust 타겟: `rustup target add wasm32-wasip1`
- Python 3.10+（rom-builder 스크립트용）
- QEMU（테스트용）
- 원격 LLM API 엔드포인트（OpenAI 호환, 예: DeepSeek, Claude, Qwen）

### 빌드

```bash
cargo build --workspace
```

### QEMU에서 실행

```bash
# 시스템 이미지 빌드
python3 rom-builder/build.py --manifest hardware_manifest_qemu.json

# 권장 설정으로 QEMU 실행
qemu-system-x86_64 \
  -hda system.img \
  -m 8G \
  -smp 4 \
  -enable-kvm \
  -device virtio-net-pci,netdev=net0 \
  -netdev user,id=net0,hostfwd=tcp::8080-:8080 \
  -nographic
```

`-nographic`는 헤드리스 모드(시리얼 콘솔)로 실행합니다. 포트 8080은 앱 스토어 API용으로 포워딩됩니다. GUI 세션이 필요한 경우 `-nographic`를 virtio-gpu로 교체하세요:

```bash
qemu-system-x86_64 \
  -hda system.img -m 8G -smp 4 -enable-kvm \
  -device virtio-gpu -device virtio-keyboard-pci -device virtio-mouse-pci \
  -device virtio-net-pci,netdev=net0 \
  -netdev user,id=net0,hostfwd=tcp::8080-:8080
```

### AI 모델 설정

최초 부팅 시 설정 마법사가 대화형으로 모델 설정을 안내합니다.
언제든지 재설정하려면:

```bash
opensystem-setup
```

설정 파일은 `/etc/os-agent/model.conf`에 저장됩니다. 직접 편집할 수도 있습니다:

```toml
[api]
base_url = "https://api.deepseek.com/v1"   # OpenAI 호환 엔드포인트라면 모두 가능
api_key  = "<your-api-key>"
model    = "deepseek-chat"
# api_format = "anthropic"                 # Anthropic 네이티브 형식 사용 시 주석 해제

[network]
timeout_ms  = 10000
retry_count = 3

[fallback]                                 # 선택 사항: 폴백 엔드포인트
base_url = "https://api.anthropic.com/v1"
api_key  = "<your-api-key>"
model    = "claude-sonnet-4-6"
```

**지원되는 API 형식:**

| 형식 | `api_format` 값 | 인증 헤더 | 지원 제공업체 예시 |
|------|----------------|----------|-----------------|
| OpenAI 호환 (기본값) | `"openai"` 또는 생략 | `Authorization: Bearer` | DeepSeek, Qwen, vLLM, OpenAI |
| Anthropic 네이티브 | `"anthropic"` | `x-api-key` | Claude (api.anthropic.com) |

> URL에 `"anthropic"`이 포함되면 Anthropic 형식으로 자동 감지됩니다. `api_format`을 명시적으로 설정할 필요가 없습니다.

### 자연어 터미널

부팅 후 시스템은 `opensystem>` 프롬프트를 표시하며 자연어 입력을 받습니다:

```
opensystem> 시스템 메모리 상태 확인해줘
opensystem> 현재 디렉토리 파일 목록 보여줘
opensystem> 25분 작업, 5분 휴식 포모도로 타이머 앱 만들어줘
```

마지막 명령은 자동으로 Rust/WASM 코드를 생성·컴파일하여 `.osp` 앱 패키지로 설치합니다. 약 30초 소요됩니다.

## 논쟁적 입장

**시스템 콜 경로의 AI에 대해:**
> "AI 추론이 너무 느리지 않나요?" — 지금은 그렇습니다. 우리는 추론 지연이 1000ms가 아닌 10ms인 세계를 위해 최적화하고 있습니다.

**네트워크 의존성에 대해:**
> 오프라인 모드는 목표가 아닙니다. 이것은 iPhone이 iCloud에 대해 내린 것과 같은 결정입니다.

**POSIX에 대해:**
> AIOS에서 소프트웨어는 온디맨드로 생성됩니다. POSIX 호환성은 스트리밍 서비스에 VHS 지원을 요구하는 것과 같습니다.

## 컴포넌트 목록

| 크레이트 | 설명 | 테스트 수 |
|---------|------|---------|
| `os-agent` | 코어 데몬: NL 터미널, 의도 분류, 앱 생성, WASM 실행기 | 59 |
| `gui-renderer` | UIDL 레이아웃 엔진, 소프트웨어 래스터라이저, ECS 트리, 이벤트 브리지 | 64 |
| `app-store` | Ed25519 서명 `.osp` 레지스트리, HTTP API, `osctl` CLI | — |
| `resource-scheduler` | AI 구동 cgroup v2 관리, eBPF CPU/IO 프로브 | — |
| `rom-builder` | 하드웨어 매니페스트 리졸버, QEMU 보드 지원, 디스크 이미지 패키징 | — |
| `os-syscall-bindings` | WASI syscall API, 메모리 안전 IPC, 타이머 관리 | 58 |

## 라이선스

MIT
