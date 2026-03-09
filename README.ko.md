# openSystem

**AI를 전제로 하는 운영체제.**

> ⚠️ **실험적 프로젝트.** 본 프로젝트는 초기 연구 단계에 있으며, 프로덕션 환경에서의 사용은 권장하지 않습니다.
> API, 설정 형식, 아키텍처는 예고 없이 변경될 수 있습니다. 기여와 대담한 아이디어를 환영합니다.

**GitHub:** [soolaugust/openSystem](https://github.com/soolaugust/openSystem)

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
python3 rom-builder/build.py --manifest hardware_manifest_qemu.json
qemu-system-x86_64 -hda system.img -m 8G -enable-kvm
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

## 라이선스

MIT
