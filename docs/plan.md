# aaa 기획

2026-10-16 · **기획 승인(결정 8), 구현 진행 중.** 독립 검토(로컬 `.coordinator/review.md`), 사용자 결정 1~8, coordinator 운영 경험 10항목을 반영했다. 「정할 것」 1~8은 모두 추천안으로 정했다(§16). 개발자 도구 문제는 아직 해결되지 않았다(P0).

- 고정 버전의 소스 분석: [소스 분석](source-analysis.md)
- 실측과 미검증 조건: [실험 기록](experiments.md)
- 검토 지적별 반영 여부: [부록 B](#부록-b-검토-지적-반영표)

## 결론

1. **구조는 3층이다.** front(TUI·CLI·웹 브라우저), host backend(`aaa host`, 중심 macOS), agent backend(`aaa agent`, 각 머신의 Windows·macOS·Linux)로 나눈다. front는 상태 없는 얇은 client이며 host하고만 통신한다. host는 host 역할만 한다. host 머신에서 도는 agent도 별도 agent backend가 다룬다.
2. **연결은 설정 주소로 맺는다.** agent backend가 TOML에 적힌 host 주소:포트로 TCP+TLS 1.3 연결을 연다(상호 고정 키). SSH와 tailnet 기능에는 의존하지 않는다. Tailscale은 host 웹을 HTTPS로 공개하는 `tailscale serve`에만 쓴다.
3. **개발자 도구 등록은 한 번이다(결정 14).** 등록 대상은 **다시 빌드하지 않는 작은 launcher(`aaa-launcher.app`) 하나**이고, 제품 실행 파일은 `aaa` 하나다. 설치 때 launcher를 한 번 등록한다(개발자 도구·전체 디스크 접근). GUI 로그인 세션의 LaunchAgent가 launcher를 띄우고, launcher가 그때의 `aaa`를 자식으로 띄워 책임 프로세스가 된다. `aaa`는 자유롭게 다시 빌드·교체한다. host는 agent backend를 직접 띄우지 않고 OS에 기동을 요청한다. **아직 실측하지 않았다**(launcher 등록 한 번 뒤 재부팅 없이 확인한다).
4. **MVP는 지금 운영(세 Mac, herdr 0.9.5 + projects 0.2.35)을 대체하는 범위다.**
   - macOS host와 agent backend
   - 첫 단계부터 host 웹 HTTPS 공개
   - projects 0.2 핵심
   - 상태 사건과 확인창 원격 응답

   Windows·Linux는 MVP부터 계약 테스트·빌드·PTY smoke를 돌리고, 기능 합격은 M2에서 한다([정할 것 1](#16-위험과-정할-것)).
5. **스택**
   - Rust: Tokio, portable-pty, ghostty-vt adapter, Ratatui, rusqlite, rustls, toml/toml_edit
   - TypeScript: React, Vite, xterm.js
   - 웹 서버도 Rust(Axum)로 host 안에 둔다. Bun·Node는 배포 필수 요소가 아니다.

## 1. 범위·전제

### 사용자 결정(2026-10-16)

| # | 결정 | 반영 위치 |
|---|---|---|
| 1 | backend·frontend 분리가 가장 중요하다. front는 얇은 client이고 상태는 backend에 있다. 이미지 paste도 front가 받아 backend로 보낸다 | §4, §9 |
| 2 | 3층 구조: front ↔ host backend(중심 macOS) ↔ agent backend(각 머신). front는 host하고만 통신한다 | §4 |
| 3 | 아래 「결정 3 세부」 참조 | §4·§5·§10·§11·§13 |
| 4 | 구현은 mac-c worktree thread(Claude Opus high)가 한다 | 로드맵에 반영하지 않음 |
| 5 | backend 설정은 TOML이다. 위치·스키마 버전·마이그레이션·오류 표시를 정한다 | §7 |
| 6 | 아래 「결정 6 세부」 참조 | §6, §14 |
| 7 | 사후 평가자(workflow 감시) agent를 역할로 지정할 수 있다. 일반 agent 목록에 섞이지 않고 배경에서 돌며, 필요하면 열어 본다. 입력은 host가 자동으로 넘기는 사건, 출력은 정해진 평가 기록이다. 다른 agent에 직접 지시하지 않는다 | §8.3, §8.13, §14 |
| 8 | 기획 승인. 「정할 것」 1~8을 모두 추천안으로 정한다 | §16 |
| 9 | 유료 Apple Developer Program 회원이 아니므로 Developer ID를 쓰지 않는다(8-2 변경). 1순위 자체 서명 코드 서명 인증서(장기 유효, 세 Mac 공용), 2순위 기존 무료 Apple Development 인증서. P0 E1에서 둘 다 실측해 고른다. Gatekeeper·공증은 쓰지 않는다 | §10.5, §10.6, §16 |
| 10 | GUI가 필요한 사용자 작업(인증서·키체인·시스템 설정 화면)은 mac-a에서 한다. 명령만으로 되는 일은 mac-c에 ssh로 한다(ssh 세션은 login 키체인이 잠겨 있어 사용자가 직접 잠금을 푼다) | §10.6, `docs/p0-runbook.md` |
| 14 | 재부팅 실험은 하지 않는다(시험 Mac 불필요). 개발자 도구에는 한 번 만들고 다시 빌드하지 않는 launcher를 고정 경로에 두고 등록한다(LaunchAgent → launcher → `aaa` 자식). 목록이 일반 실행 파일을 받지 않으면 최소 `.app`으로 감싼다. 결정 9(자체 서명 인증서)는 보류한다. 전체 큰 그림(M1-2~M1-4 운영 대체)을 먼저 완성하고 P0 세부 실측에 시간을 쓰지 않는다 | §10, `docs/p0-runbook.md` |
| 15 | launcher 등록은 mac-b(coordinator Mac)에서 한다. L1 측정·운영 설치 확인도 mac-b를 첫 대상으로 ssh로 한다(결정 10의 GUI 머신 대신) | §10.6, `docs/p0-runbook.md` |
| 16 | UI를 참고 도구(herdr-web-ui·herdr TUI·herdr-mobile-relay)와 나란히 비교해 같거나 낫게 다듬는다. 웹(데스크톱·휴대폰)을 먼저, 그다음 TUI. 코드는 복사하지 않고 디자인만 참고한다(코드 복사 금지는 결정 19로 바뀌었다). 결과는 전후 비교로 사용자 검수를 받는다 | §8.23 |
| 17 | UI 검수 통과(10-17). 검수 때 후보로 남긴 휴대폰 drawer·명령 팔레트·머신 묶음 접기·TUI 왼쪽 목록·thread 줄 하나로 합치기는 이번에 하지 않고 나중 후보로만 둔다. 결정 14·16대로 M3 남은 범위를 이어서 한다 | §8.23 |
| 18 | 실행 파일·프로젝트 이름을 `aaa`(Any Agent, Anywhere)로 바꾼다(10-17). macOS 기본 Apple Archive 도구 `/usr/bin/aa`와 겹치지 않는다. binary·crate(`aaa-*`)·환경 변수(`AAA_*`)·launchd label(`dev.aaa.*`)·설정/상태/로그 폴더·launcher(`aaa-launcher.app`, 다시 등록 필요)·웹 제목·pairing cookie(`__Host-aaa`, 기존 pairing은 다시 한다)·작업 폴더(`.aaa/`)·branch 접두사(`aaa/`)를 함께 바꾼다. 옛 폴더는 읽지 않고 옮기는 명령을 안내한다. GitHub 저장소는 `cLazyZombie/aaa`다 | §11, `docs/install-runbook.md` |
| 19 | 참고 세 프로젝트(herdr·herdr-projects·herdr-web-ui)를 철저히 따른다(10-17). 동작은 새로 고안하지 않고 참고 소스를 먼저 보고 옮기며 코드도 그대로 옮긴다(출처·MIT 고지). 다르게 둘 때는 이유(3 OS·보안·사용자 결정)를 적는다. 결정 16의 「코드는 복사하지 않는다」를 바꾼다 | §8.36~§8.39, `library/diff-list.md`(thread 보고) |
| 20 | 동작 단위 차이 목록의 결정 묶음 D1~D12를 모두 참고대로 한다(10-17). D1 바로 입력 기본·입력줄은 기기별 선택, D2 머신·프로젝트 묶음·drawer·질문 카드(결정 17 보류 해제), D3 chat에서 보내기, D4 기기별 알림 종류(조용한 시간은 만들지 않음), D5 테마는 참고대로·다국어는 한국어 먼저, D6 웹 pairing(controller만), D7 터미널 화면, D8 harness 상태는 화면 감지(§3 「화면 긁기 안 함」을 바꾼다), D9 보고 형식은 THREAD.md, D10 brief 재시도·한도 경고·바로 입력·memory, D11 PR 병합·merged 뒤 resolve(`merge land`·대기열을 바꾼다), D12 herdr 키·재부팅 뒤 대화 자동 이어가기(결정 8을 바꾼다)·목록 기본 펼침·`--json` 기본·profile·safety CLI. 다만 D12의 prefix 동안 영문 입력기 전환·자모 해석은 사용자 요청이라 유지한다(설정으로 끌 수 있게, 기본 켬) | 차이 목록 순서대로 옮긴다 |

**결정 3 세부.** host는 host 역할만 한다(2안).
- 실행 파일은 하나이고 역할만 나눈다(`aaa host`/`aaa agent`). 경로와 서명은 고정이다.
- agent backend가 PTY와 agent 프로세스를 소유한다.
- host는 로컬 agent backend를 확인하고, 없으면 기동하거나 이미 떠 있는 것에 붙는다.
- 웹은 host만 연다. agent backend는 외부에 열지 않는다.
- 개발자 도구에 등록하는 대상은 agent backend뿐이며, 고정 경로로 띄운다.
- E2E에 한 머신에서 agent backend 여러 개를 띄우는 경우를 넣는다.
- MVP에서는 로컬 agent backend를 host의 자식으로 띄워도 된다. 프로토콜은 처음부터 원격과 같다.

**결정 6 세부.** aaa는 tailnet을 모른다.
- 연결은 설정의 주소로 일반 TCP를 쓴다. 인증·암호화는 aaa 자체 수준에서 한다.
- 유일한 예외는 host 웹의 HTTPS 공개다(`tailscale serve`). 이것은 MVP 첫 단계에 들어간다.

### 범위

| 항목 | 이번 설계 |
|---|---|
| 중심 머신 | macOS 한 대에서 host backend를 돌린다. 단일 사용자·단일 권위 상태 저장소이고, 프로젝트별로 coordinator agent가 있다 |
| agent 머신 | 대상: macOS arm64/x64, Linux x64/arm64, Windows x64 native. 중심 Mac도 로컬 agent backend로 agent를 돌린다. Windows arm64는 후속 검증 |
| Windows | ConPTY가 되는 지원 버전부터 시작하고, 우선 Windows 11 x64다. WSL로 Windows 지원을 대신하지 않는다 |
| macOS | SMAppService가 있는 macOS 13 이상을 후보로 둔다. 실제 하한은 서명·성능 gate에서 확정한다. 현재 운영 버전(27.x) 검증은 필수다 |
| Linux | glibc 환경이 우선이다. systemd user + linger를 쓰고, 없으면 명시적 foreground 감독으로 돈다. musl 확대는 별도로 검증한다 |
| 네트워크 | 머신은 지금 tailnet으로 묶여 있지만 aaa는 이를 모른다. TOML의 호스트:포트로 TCP+TLS 연결한다. tailnet 밖 접속은 이번 범위가 아니다 |
| 웹 | host만 연다. `127.0.0.1` HTTP로 열고 `tailscale serve`로 tailnet HTTPS 공개한다. desktop·모바일 모두 다루며, 보기와 조작 권한을 나눈다 |
| 복원 | front·네트워크·host·agent backend가 재접속해도 실행 중인 세션은 유지된다. OS 재부팅 뒤에는 layout과 agent 대화를 재개한다. 중단된 빌드를 성공으로 처리하지 않는다 |
| 이번 작업 | 문서·웹 보고서·검토만 했다. 제품 코드, 설치·권한 변경, 운영 서버·agent 설정 변경은 없다 |

초기 비목표:
- 다중 사용자 SaaS
- agent 모델 API 대체
- 임의 플러그인 marketplace
- 외부 gateway·WebRTC·QUIC
- 완전한 tmux 명령 호환
- tailnet 신원·ACL 활용

세 OS **agent** 지원은 비목표가 아니다. 프로토콜과 빌드는 첫 단계부터 세 OS를 대상으로 한다.

## 2. 세 도구에서 취할 것

| 도구·확보 버전 | 가져올 개념 | 줄이거나 버릴 점 | 라이선스·이용 방식 |
|---|---|---|---|
| herdr 0.9.5 `2ecf19a` | 실제 PTY, workspace/tab/pane, terminal identity, snapshot/event, 얇은 client(server VT + 화면 변경분), integration·resume, Windows native(ConPTY) 실행 경험 | 원격·웹이 먼저 서버를 띄우는 경합, GUI 터미널 수명에 의존한 권한, 내부 bincode wire 종속, 감지 manifest를 `herdr.dev`에서 자동 갱신하는 방식 | Apache-2.0. 선택적으로 재사용할 때 LICENSE와 수정 표시를 남긴다(루트 NOTICE 없음) |
| projects 0.2.35 `1f05c00` | 프로젝트 폴더·AGENTS/memory, coordinator의 판단과 binary의 절차 분리, profile, ticker/inbox/routine, worktree·report·sidebar, brief 전달 규칙, trust_screens, `owned.json` journal | 매 요청 CLI·SSH 실행, 원격 tab/checkout 제한, polling, POSIX 원격 스크립트 | MIT, 고지를 보존한다. 개념과 검증된 작은 로직 중심으로 가져온다. 세부 대응은 [부록 A](#부록-a-herdr-projects-0235-기능-대응표) |
| web-ui 0.4.10/v29 `aa624cb` | React/xterm terminal, 모바일 composer·IME, chat/terminal 전환, 장치 권한, ACK/backpressure, `tailscale serve` HTTPS 공개 절차, Web Push·PWA | Bun→Node→herdr 층, 웹이 실행 서버를 임의 기동(자동 재연결 포함), `Tailscale-User-Login` 헤더 자동 입장, vendor별 transcript 전부 | MIT, 고지를 보존한다. 번들 제3자 중 elkjs(EPL-2.0), DOMPurify(Apache/MPL), OFL 글꼴이 있어 component를 가져올 때 의존성별로 확인한다 |
| 보조 mobile-relay 0.21.3 `a58f1fc` | 장치별 취소, controller/reader 역할, 불안정한 연결 복구 | 외부 gateway·NAT·WebRTC는 범위가 아니다 | AGPL-3.0-or-later. 섞지 않고 일반 개념만 참고한다 |

원본 URL·전체 commit·코드 근거는 [소스 분석](source-analysis.md)에 있다. aaa 저장소의 MIT LICENSE는 유지한다.

## 3. 목표 기능과 사용자 흐름

| 영역 | 목표 기능 | 사용자가 확인할 결과 |
|---|---|---|
| 프로젝트·coordinator | 프로젝트 폴더(AGENTS.md·memory·TASKS.md·결정 기록), coordinator 세션, context, inbox cursor | 놓친 알림도 context에서 다시 읽는다. 결정은 한 곳에 남는다 |
| thread | 종류: worktree·tab·checkout·adopt·repo 없는 task dir. 그 밖에 profile, brief, 보고, 검토·ack·resolve | 로컬과 원격에서 기능이 같다. report가 검토 대기로 보존된다 |
| 여러 머신 | enrollment, 상태, 능력, profile, 단절·복귀, 머신 건강 | 단절 중 thread를 failed/done으로 오판하지 않는다. 이상이 바로 보인다 |
| 상태 사건·알림 | agent backend가 턴·질문·오류·종료를 사건으로 보낸다. coordinator·front는 구독한다 | 화면 긁기로 상태를 추정하지 않는다. 막히면 즉시 알림이 온다 |
| 확인창 원격 응답 | 폴더 신뢰·권한 확인 같은 확인창을 구조화된 질문으로 올린다 | 웹·TUI·coordinator가 원격에서 답한다. 자동 승인은 기본 금지 |
| 메시지 | 즉시 끼어들기와 다음 턴 대기열을 나눈다. 수신 확인과 기록을 남긴다 | 특수문자가 깨지지 않는다. 받았는지 화면을 다시 읽지 않는다 |
| 세션 | 이름 있는 session, workspace/tab/pane, split·resize·scrollback, 여러 front, detach/attach | front 종료·네트워크 끊김·host 재시작으로 pane이 죽지 않는다 |
| 복원·갱신 | 실행 중 pane 재attach, launch/resume spec, 재부팅 뒤 resume | model·effort·권한·daemon 금지 인자가 유지된다. 실행 중이던 build는 interrupted로 남는다 |
| 웹·모바일 | HTTPS 주소의 terminal·보고서·inbox·질문, 모바일 입력·한글 IME, push 알림 | 다른 기기에서 결과물을 바로 본다. 보기 권한으로는 조작할 수 없다 |
| 이미지 붙여넣기 | TUI·웹 Ctrl+V → front → host → 대상 agent 머신 파일 → agent 입력 | Claude·Codex가 **자기 머신의 실제 이미지 파일**을 읽는다 |
| 결과물 | report·library를 host가 받아 HTTPS로 보여 준다. 큰 결과물은 지연 전송한다 | 175MB·5GB 결과물도 잃지 않고 볼 수 있다 |
| 병합 | 병합 대기열(순번·사건·훅), 정리 | 병합 사이에 다시 통합하는 비용이 줄어든다 |
| 운영·진단 | `doctor`, 계보·서명·키체인·첫 실행 지연 자가 보고, 동시 작업 한도 | 빠르지만 키체인이 잠긴 상태도 실패로 보인다 |
| 사후 평가자 | 특정 agent에 「사후 평가자」 역할 지정. 배경 실행, 별도 상태 표시, 필요할 때 attach. 착지·완료·실패·재시도 사건을 host가 자동 전달 | 결과물 품질이 아니라 흐름의 효율(단계별 시간·재작업·막힌 곳·도구 문제)을 판정한 기록을 coordinator가 채택·보류한다 |

아래 명령은 **설계 예시이며 지금 실행되는 명령이 아니다**.

```text
aaa host / aaa agent                      역할 실행(보통 OS 감독자가 띄움)
aaa machine enroll --open 10m            host에서 등록 창 열기
aaa agent enroll <host:port>             agent 머신에서 등록
aaa session attach [--machine M] <name>  TUI front
aaa pane run|read|send                   셸 문자열과 구조화된 process start(argv)는 분리
aaa thread start --machine M --profile P --task-file F
aaa thread message <id> --queue|--interrupt --text-file F
aaa question list / aaa question answer <id> <선택지>
aaa decision add / aaa merge enqueue <thread>
aaa web publish / aaa web publish off
aaa evaluator assign --project P --machine M --profile P
aaa evaluator status / aaa evaluator open   배경의 사후 평가자 보기·attach
aaa evaluation list / aaa evaluation adopt|hold <id>
aaa doctor / aaa config check / aaa config migrate
```

명령마다 JSON 출력, 안정된 error code, exit code를 정의한다.

## 4. 3층 구조

### 4.1 역할과 프로세스

```mermaid
flowchart LR
  subgraph FRONT[front · 상태 없는 얇은 client]
    TUI[aaa TUI · CLI]
    BR[웹 브라우저 · React · xterm.js]
  end
  subgraph HOSTM[중심 머신 · macOS]
    TS[tailscale serve · HTTPS]
    H[aaa host · Front API · 웹 · ticker · 오케스트레이션]
    HDB[(host SQLite · 정본 목록 · spec · inbox)]
    LA[aaa agent · 로컬 agent backend]
    LS[aaa session-host · PTY · VT]
    TS --> H
    H --> HDB
    LA -- Agent link · UDS --> H
    LA --> LS
  end
  subgraph AGM[agent 머신 · Windows / macOS / Linux]
    RA[aaa agent]
    RS[aaa session-host · PTY / ConPTY · VT]
    RA --> RS
  end
  TUI -- Front API · UDS 또는 TCP+TLS --> H
  BR -- HTTPS --> TS
  RA -- Agent link · TCP+TLS 1.3 · agent가 연결 --> H
```

| 프로세스(역할) | 위치 | 소유하는 것 | 띄우는 주체 |
|---|---|---|---|
| `aaa` TUI·CLI (front) | 아무 기기 | 화면 viewport와 입력 중인 초안(저장 안 함) | 사용자 |
| 웹 (front) | 브라우저 | 같음 | 사용자 |
| `aaa host` | 중심 Mac 1대 | 정본 목록·spec·thread·inbox·routine·결정 기록, lease·권한, Front API, 웹 | macOS gui 도메인 LaunchAgent(KeepAlive) |
| `aaa agent` | 모든 agent 머신(중심 Mac 포함) | 머신 API, session host 감독, 파일·attachment 저장, 상태 사건, 머신 건강 | OS 감독자(macOS LaunchAgent, Windows 로그온 예약 작업, Linux systemd user) |
| `aaa session-host` | 세션마다 하나 | PTY master·child handle·VT 상태·출력 로그 | `aaa agent` |
| `aaa hook` | agent pane 안 | 없음(bounded 사건 한 건을 로컬 agent backend에 전달) | agent harness |

모두 같은 실행 파일 `aaa`의 역할이다. 설치·업데이트는 한 번에 하고, 경로와 서명은 고정이다(§10, §11).

**crate 구조(결정 13, M1-2 뒤 적용).** 실행 파일은 하나이고, 역할 경계는 crate 의존으로 컴파일 때 막는다.

| crate | 담는 것 | 의존할 수 있는 workspace crate |
|---|---|---|
| `aaa-proto` | 계약(메시지·frame·버전), layout 순수 함수, link heartbeat 주기 | 없음 |
| `aaa-config` | TOML 설정 | 없음 |
| `aaa-common` | 로컬 IPC·frame 연결·TLS 신원·OS 유틸·Front API 클라이언트 | proto |
| `aaa-vt` | VT adapter(vt100) | proto |
| `aaa-doctor` | 머신 진단(`aaa doctor`와 agent 자가 점검이 함께 쓴다) | common, config |
| `aaa-session` | session host·PTY·출력 ring·세션 파일 | proto, common, vt |
| `aaa-agent` | agent backend·Agent link agent 쪽 | proto, config, common, session, doctor |
| `aaa-host` | host backend·Front API 서버·웹 서버·Tailscale·SQLite | proto, config, common, vt |
| `aaa-tui` | TUI front(`aaa attach`) | proto, config, common |
| `aaa` | 진입점: CLI·역할 선택·OS 감독자 등록 | 모두 |

- front(`aaa-tui`, `web/`)는 `aaa-proto` 계약으로만 host와 말한다. `aaa-host`는 agent·session을 Agent link로만 다룬다.
- 공통 crate(`aaa-common`·`aaa-vt`·`aaa-doctor`)는 역할 crate에 의존하지 않는다.
- `crates/aaa/tests/deps.rs`가 `cargo metadata`로 금지 의존(간접 포함)을 찾는다.
- 결정 13 원안은 doctor를 진입점 `aaa`에 두었다. 그런데 agent 자가 점검도 같은 측정을 써서, 측정 라이브러리만 `aaa-doctor`로 뺐다. `aaa doctor` 명령은 그대로 `aaa`에 있다.

### 4.2 경계 원칙

1. **front는 host backend하고만 통신한다.** agent backend에 직접 붙지 않는다. agent 머신에서 사람이 쓰는 CLI도 host를 거친다.
2. **front는 backend를 띄우지 않는다.** host가 없으면 「host 없음」만 알린다.
3. **host는 자식 프로세스를 만들지 않는다.**
   - routine 명령·git·worktree·파일 작업은 해당 머신의 agent backend에 요청한다. host 머신의 작업이면 로컬 agent backend에 요청한다.
   - 예외는 `tailscale` CLI 호출(§6.3)과, MVP 개발 모드의 로컬 agent backend 자식 기동(아래 4번)이다.
4. **host가 로컬 agent backend를 「띄운다」는 것은 OS 감독자에게 기동을 요청한다는 뜻이다.**
   - 요청 방법: macOS는 `launchctl kickstart`(`gui/<uid>`), Windows는 예약 작업 실행, Linux는 `systemctl --user start`다.
   - 운영 모드에서는 host가 직접 fork하지 않는다. 자식으로 띄우면 host가 재시작될 때 책임 프로세스가 사라져 pane이 느려지기 때문이다(§10.3).
   - MVP 개발 모드에서는 결정 3에 따라 자식으로 띄울 수 있다. 다만 doctor가 그 상태를 「개발 모드」로 표시하고, 운영 모드에서는 시작을 거부한다.
5. **agent backend는 외부에 아무것도 listen하지 않는다.** 로컬 IPC(UDS/Named Pipe)는 `aaa hook`과 session host 전용이며 front 명령을 받지 않는다.
6. **이미지·파일은 front → host → agent로만 간다**(§9).
7. **aaa는 tailnet을 모른다.** 예외는 §6.3의 웹 HTTPS 공개다.

### 4.3 경계 계약

| 경계 | 계약 | transport | 인증 | 재연결 | 버전 |
|---|---|---|---|---|---|
| front ↔ host | Front API | 같은 머신 UDS, 다른 기기 TCP+TLS, 브라우저 HTTPS(serve) → `127.0.0.1` HTTP·WS | UDS peer UID + 장치 token, 장치 키 pairing, 웹 pairing cookie | cursor로 재구독, 보존 범위 밖이면 snapshot | major.minor, hello 협상 |
| host ↔ agent | Agent link | 로컬은 UDS, 원격은 TCP+TLS 1.3. 연결은 agent가 연다 | 상호 고정 키(자체 서명 인증서 fingerprint pinning) | backoff+jitter, outbox 재전송, command ID로 결과 조회 | major.minor, host가 agent N-1 minor 지원 |
| agent ↔ session host | Session IPC(내부) | 세션별 UDS/Named Pipe | peer UID/SID + 세션 token | agent 재시작 뒤 registry로 재접속 | 내부 버전. 이전 session host와 호환 |
| agent pane ↔ agent | Hook IPC | UDS/Named Pipe | 세션·boot ID에 한정한 token | 없음(bounded JSON 한 건) | hook 버전 |

Front API와 Agent link는 서로 다른 계약이다. 둘 다 다음 원칙을 따른다.

- **schema와 생성:** Rust typed request/event와 JSON Schema로 정의하고, TypeScript 타입을 생성한다. 웹의 `/api/v1`·`/ws`는 Front API를 생성으로 투영한 것이며 의미가 같다.
- **plane 분리:** 제어 plane은 길이 제한이 있는 versioned JSON이다. 터미널 plane은 seq·offset·ACK가 있는 binary frame이다. bincode enum 순서나 Rust 메모리 구조를 wire 계약으로 쓰지 않는다.
- **envelope:** protocol major·minor, request ID, machine ID·boot ID, object ID, command ID, deadline, revision을 둔다.
- **hello 협상:** hello에서 기능을 협상한다. 모르는 선택 필드는 무시하고, 필수 기능이 없으면 명시 오류를 낸다.
- **거부 대상:** boot가 다른 오래된 pane 입력, 모르는 major, 초과 frame, 잘못된 offset은 거부한다.
- **outbox와 receipt:** 상태 변경과 전달 상태를 원자적으로 저장한다. 같은 command ID로 재연결하면 결과를 조회한다. 터미널 타이핑과 외부 agent의 실제 접수까지 exactly-once라고 주장하지 않는다.

### 4.4 상태 정본

| 상태 | 정본 | 다른 쪽이 가진 것 |
|---|---|---|
| 프로젝트·thread·profile spec·inbox·routine·결정 기록·보고 receipt | host SQLite + 프로젝트 파일 | agent: thread 작업 폴더의 brief·report 원본 |
| 세션·workspace·tab·pane 목록과 layout, launch/resume spec | host | agent: 실행 중 세션 manifest(사본·journal) |
| 실행 사실(pid·시작 identity·exit·PTY 크기·VT 화면·scrollback·출력 offset) | agent(session host) | host: 마지막 snapshot·제한된 tail·last_seen |
| attachment 메타데이터(hash·대상·상태·참조) | host | agent: 파일 본문과 receipt |
| lease·권한·장치 | host | 없음 |
| host 단절 중의 사건·보고 | agent outbox | host: 재연결 때 받아 대조 |

상황별 동작:

- **agent ↔ host가 끊기면** 세션은 계속 돈다.
  - host는 해당 pane을 `disconnected`(읽기 전용)로 표시하고 마지막 snapshot과 tail을 보여 준다.
  - 입력·resize·paste·메시지는 대기열에 넣지 않고 `agent_disconnected` 오류를 낸다.
  - thread를 failed/done으로 바꾸지 않는다.
  - 재연결하면 agent가 실행 사실과 outbox를 보고하고, host가 boot ID로 대조한다. boot ID가 바뀌었으면(재부팅) `interrupted`다.
- **host가 재시작·업데이트되면** 세션은 유지된다. front와 agent가 다시 접속하고, host는 SQLite에서 목록을 다시 읽는다.
- **agent backend가 재시작·업데이트되면** session host가 살아 있는 한 registry로 재접속하고 세션을 유지한다. 그때 책임 계보가 유지되는지는 P0 E3이 정한다(§10.6).
- **머신이 재부팅되면** 프로세스는 사라진다. layout과 resume spec으로 재개를 제안하고, 진행 중이던 build는 `interrupted`로 남긴다.

상태 축은 분리한다.
- `launch_state`: planned·starting·running·exited·failed·interrupted
- `agent_state`: unknown·working·waiting_input·question·idle
- `review_state`: none·ready·acknowledged·resolved
- `connection_state`

오래된 관측은 last_seen/stale로 표시한다. agent idle이나 100% 진행만으로 검토·완료·merge를 자동으로 확정하지 않는다.

### 4.5 터미널 plane과 얇은 front

- **VT 정본은 session host에 있다**(VT adapter). M1-1 구현은 순수 Rust `vt100` 0.16을 adapter 뒤에 둔다. ghostty-vt는 빌드에 Zig 0.16이 필요해 구현 Mac·CI에 바로 쓸 수 없었다(§12, 위험 표).
- **화면 형식은 front 종류마다 hello에서 협상한다.**
  - TUI: cell grid snapshot + diff(herdr와 같은 얇은 방식)
  - 웹: raw byte stream + 재접속용 snapshot
  - 웹 snapshot은 VT 재생 문자열로 만든다. M1-1 확인: `vt100`의 상태 재생(`ESC[H ESC[2J` + 화면 상태)으로 만들고, 그 뒤 byte는 출력 offset으로 이어 붙인다. 느린 구독자·offset 틈은 snapshot을 다시 보낸다.
- **TUI가 갖는 것:** layout 합성, 입력, 로컬 viewport·scroll 위치뿐이다. 저장 상태는 없다.
  - M1-2 구현: `aaa attach [-w workspace]`(설계 예시의 `aaa session attach`를 줄였다). workspace layout(탭·나누기 트리)은 host가 SQLite에 두고 바뀌면 연결된 front에 알린다. TUI는 보이는 pane만 grid를 구독한다(33ms 간격, 바뀐 줄만).
  - prefix 키는 설정 `[tui] prefix`(기본 `C-b`)다. 그 뒤 `c` 새 탭, `%`·`"` 나누기, 화살표·`o` 이동, `n`·`p`·숫자 탭, `t` 이 화면 크기로(보통은 필요 없다, §8.33), `r` 재개, `x` 닫기, `d` 나가기, prefix 두 번은 prefix 글자를 그대로 보낸다. 3초 안에 명령 키가 없으면 취소한다. 한글 입력기면 prefix 동안 영문 입력기로 바꾼다(§8.34).
  - 다른 front가 지금 쓰는 pane은 PTY 크기 그대로 왼쪽 위에 맞춰 그리고 칸 밖은 자른다. 입력하거나 터미널 focus를 다시 얻으면 그 칸 크기로 맞추면서 이 화면이 지금 쓰는 화면이 된다(§8.33). lease가 빈 pane에 포커스해도 같다.
- **여러 front의 화면과 크기**
  - front마다 viewport는 독립이지만, 실제 PTY 크기는 「지금 쓰는 화면」(lease)이 정한다. lease는 host가 둔다.
  - **lease 규칙(M1-2 구현, 10-17 herdr 모델로 바꿈 §8.33).**
    - lease는 대화형 front 연결 하나(TUI의 Front 연결, 웹 터미널 WebSocket 하나)에 묶인다. 연결이 끝나거나 놓으면(`release`) 풀리고, host가 재시작해도 풀린다. 시간 만료는 없다.
    - ~~다른 연결이 갖고 있으면 `lease_held`로 거부하고 명시적으로 가져온다~~ → 대화형 입력·크기·그림은 거부하지 않는다. 보낸 연결이 지금 쓰는 화면이 된다(마지막 입력이 이긴다). 이전 화면에는 상태 변경으로 보인다.
    - 크기는 「쓰는 중인」 front만 건다: 지금 쓰는 화면이거나 아무도 쓰지 않을 때, focus를 다시 얻었을 때, 입력했을 때. 보이지 않거나 focus가 없는 front는 걸지 않는다.
    - 한 번 보내는 CLI(`pane resize`·`pane attach`)는 지금 쓰는 화면이 되지 않는다. `pane resize`는 아무도 쓰지 않을 때만 된다.
    - **한 front 연결에서 pane을 바꾸는 요청(입력·전송·크기·가져오기·놓기·종료)은 받은 순서대로 처리한다.** TUI는 키마다 응답을 기다리지 않고 보낸다. 그래서 host가 동시에 처리하면 키 순서가 뒤바뀌었다(M1-2 CI에서 발견). 조회 요청은 동시에 처리한다.
    - **한 번 보내는 API 입력(`pane send`)은 lease 대상이 아니다.** coordinator·스크립트의 전송이 사람의 attach에 막히지 않게 하려는 것이다. 감사 기록에는 남는다. M1-3 메시지 API(§8.5)와 질문 응답(§8.4)은 이 위에서 자기 규칙(interrupt·queue, generation·lease 재확인)을 더한다.
    - M1-1에서는 웹이 PTY 크기를 그대로 보여 주고 「화면에 맞추기」를 눌렀을 때만 resize했다. M1-2부터는 그것도 lease 보유자만 한다. 10-17부터 웹도 TUI와 같이 자동으로 맞추고(§8.30), 같은 날 명시적 가져오기를 없앴다(§8.33).
  - 보기(observe) front는 입력도 resize도 할 수 없다.
  - 느린 front는 bounded queue가 차면 snapshot으로 재동기화하며, 전체 agent를 무기한 backpressure하지 않는다.
  - TUI와 웹이 섞여도 같은 규칙이다.
  - 출력 로그·scrollback 상한과 frame 상한·rate limit은 설정 정책이다.

## 5. 세 OS agent backend

| 차이 | macOS | Linux | Windows native |
|---|---|---|---|
| 감독자 | gui 도메인 LaunchAgent(SMAppService.agent, `LimitLoadToSessionType=Aqua`, KeepAlive) | systemd user unit + linger. linger는 설치 때 사용자 승인. 없으면 명시 foreground 감독 | 로그온 시 예약 작업. 사용자 권한, 실패 시 재시작 |
| 터미널 | Unix PTY·SIGWINCH | Unix PTY·SIGWINCH | ConPTY·ResizePseudoConsole, 별도 input/output pump thread |
| 세션 수명 | session host 분리, 책임 계보(§10) | session host 분리. `KillUserProcesses`·linger 확인 | session host가 Job Object 소유. SSH job 밖에서 기동 |
| 종료 | PID+시작 identity, 자기 process group | 같음 | process handle+creation time. Cancel/CTRL event와 종료 fallback 구분 |
| 셸 | 사용자 zsh/bash, argv 실행 우선 | 사용자 shell, argv 우선 | PowerShell/pwsh/cmd 명시, CreateProcessW 인용, UTF-16 경로 |
| 경로·환경 | POSIX path·HOME, 외장 볼륨(전체 디스크 접근 필요) | POSIX bytes path·HOME·locale | drive/UNC·backslash·LOCALAPPDATA·대소문자·UTF-16 |
| 줄바꿈·문자 | LF·raw bytes | 같음, locale 검증 | CRLF·codepage/UTF-8. 단순 LF 치환 금지 |
| local IPC | user-only UDS·peer UID | 같음 | user SID 제한 Named Pipe·ACL(M1-1은 기본 DACL, 제한 ACL은 M2) |
| 키 저장 | 상태 디렉터리의 0600 파일(키체인 잠금에 의존하지 않음) | 0600 파일 | 사용자 DPAPI |
| 실행 검사·속도 | 필수 gate(§10) | 해당 없음 | Defender·SmartScreen 지연을 doctor에서 관측 |

- **portable-pty만으로는 부족하다.** API가 있다는 것만으로 수명 관리가 해결되지 않는다. Windows의 handle·job·ConPTY 입출력 deadlock, 부모 종료 뒤 생존은 native E2E로 확인한다.
- **ConPTY I/O는 별도 thread에서 한다.** [ConPTY 문서](https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session)는 동기 I/O를 별도 thread에서 처리하라고 한다. blocking reader/writer는 Tokio event loop 밖에 둔다.
- **herdr 0.9.5의 Windows native 지원을 참고한다.** herdr는 Windows native(ConPTY) 서버를 GA로 지원하고, kill-on-close Job은 WMI로 벗어난다. 이 처리를 참고 구현으로 검토한다(Apache-2.0).
- **Windows는 Session 0 시스템 서비스를 기본으로 쓰지 않는다.** 로그온 전 실행이 필요한지는 [정할 것 8](#16-위험과-정할-것)이다.
- **경로는 `(machine_id, os, native_path)`로 저장한다.** 중심 Mac의 PathBuf로 Windows 경로를 canonicalize하지 않는다. Unix 비UTF-8 경로는 명시 오류로 처리한다. 실행 env는 machine-local profile에서 해석하고, 중심의 HOME·PATH로 덮어쓰지 않는다. 파일 전송에서는 절대 경로·상위 이동·symlink/junction을 검증한다.

## 6. 연결·보안

### 6.1 Agent link

- **연결 방향과 주소**
  - agent가 `agent.toml`의 `host = "<이름 또는 주소>:<포트>"`로 연결한다.
  - 이름은 OS resolver로 푼다. tailnet MagicDNS든 LAN DNS든 aaa는 구분하지 않는다.
  - 로컬 agent backend는 같은 계약을 UDS로 쓴다.
- **암호화:** TLS 1.3(rustls)이 필수다. tailnet의 암호화에 기대지 않는다. 실제 LAN에서도 같아야 하기 때문이다.
- **상호 인증**
  - host와 agent가 각자 자체 서명 인증서(장치 키, rcgen)를 만든다.
  - enrollment 때 fingerprint를 교환해 서로 pinning한다.
  - Noise IK는 대안이다.
- **enrollment**
  - host에서 `aaa machine enroll --open 10m`으로 시간 제한 창을 연다.
  - agent에서 `aaa agent enroll <host:port>`를 실행한다.
  - 양쪽 화면에 키 교환에서 유도한 6자리 확인 코드(SAS)가 뜬다. 사용자가 host에서 일치를 확인한다.
  - 코드는 1회용이고 실패 시도는 제한한다. LAN의 다른 기기가 몰래 등록할 수 없다.
- **인증 전 제한:** frame 크기, 동시 연결 수, 시도 횟수를 제한한다.
- **재연결**
  - 지수 backoff+jitter로 재연결한다(상한은 TOML로 정하며 기본 60초).
  - 연결되면 hello(버전·boot ID·기능)를 보내고, outbox를 재전송한 뒤 command ID 결과를 조회한다.
  - 주소 변경은 TOML로만 한다.
- **키 관리:** host에서 `aaa machine revoke`로 폐기하고 rotate한다. agent는 다시 등록한다.
- **frame:** TLS 위에 길이 접두 frame을 쓴다. 브라우저가 아니므로 WebSocket이 필요 없다.
- **SSH는 필수가 아니다.** Windows 포함 세 OS 모두 SSH 없이 연결된다. SSH는 설치 부트스트랩과 진단에서만 선택적으로 쓴다. agent는 연결을 열기만 하므로(outbound) agent 쪽 방화벽 수신 허용이 필요 없다.

### 6.2 Front 연결

- **같은 머신:** 0700 디렉터리의 UDS를 쓰고 peer UID와 장치 token을 확인한다. agent pane 안에서 실행한 CLI는 세션·역할 범위 token으로 구분한다. 일반 thread의 범위 제한은 M3에서 검토하지만, 사후 평가자의 금지 동작(§8.13)은 이 token으로 강제하므로 평가자와 함께 넣는다.
- **다른 기기의 TUI:** TCP+TLS로 붙는다. 웹과 같은 장치 모델을 쓴다(장치 키 pairing, reader/controller 역할).
- **브라우저:** §6.3의 HTTPS 주소로 붙는다.

### 6.3 host 웹 HTTPS 공개 — aaa가 Tailscale을 아는 유일한 곳

herdr-web-ui 0.4.10의 방식(`server/tailscale.ts`, `scripts/plugin.ts`의 `phone`)을 그대로 가져오고, 아래 「aaa에서 정한 차이」만 바꾼다. 이 기능은 MVP 첫 단계(M1-1)에 넣는다.

| 항목 | 방식 |
|---|---|
| 웹 listen | host 웹은 `127.0.0.1:<web_port>`(TOML, 예: 7431)의 HTTP·WS로만 연다. TLS는 tailscale serve가 맡는다 |
| 상태 읽기 | `tailscale status --json`과 `tailscale serve status --json`을 읽는다(2.5초 timeout, 읽기 전용). CLI는 PATH의 `tailscale`을 쓰고, 없으면 `/Applications/Tailscale.app/Contents/MacOS/Tailscale`을 쓴다 |
| 공개 | `aaa web publish`가 실행한다. serve status에 이미 aaa 웹 포트를 proxy하는 HTTPS가 있으면 그 주소를 쓴다. 없으면 후보 포트(443·8443·7443·17443) 중 serve status의 TCP에 없는 첫 포트로 `tailscale serve --bg --yes --https=<포트> http://127.0.0.1:<web_port>`를 실행한다. 180초를 기다리는데, 처음이면 tailnet HTTPS 켜기 승인 링크가 나올 수 있다 |
| 주소 | MagicDNS 이름의 HTTPS 주소를 쓴다. 인증서가 이름에 대해 발급되므로 IP로 열면 경고가 난다. 주소는 QR로도 보여 준다 |
| 끄기 | `aaa web publish off`가 aaa가 만든 포트만 `tailscale serve --https=<포트> off`로 끈다 |
| 정리 | host 중지와 공개는 분리한다. host를 멈춰도 serve 설정은 남아 주소가 같다. `serve reset`은 금지한다. 다른 매핑(herdr-web-ui 등)을 덮지 않는다. 공개 상태는 host 상태에 기록한다 |
| Tailscale이 없거나 로그인 전·HTTPS가 꺼진 경우 | `missing`·`stopped`·`https 꺼짐` 상태와 할 일만 보여 준다. 웹은 `127.0.0.1`에서 계속 동작한다 |

**aaa에서 정한 차이**
- **입장은 aaa pairing·token으로만 한다.** web-ui의 `Tailscale-User-Login` 헤더 자동 입장은 쓰지 않는다. 결정 6이 tailnet 신원 사용을 뺐고, serve가 loopback으로 proxy하므로 같은 머신의 다른 프로세스가 헤더를 위조할 수 있다([정할 것 4](#16-위험과-정할-것)).
- **loopback을 인증 신호로 쓰지 않는다.** 웹 서버는 모든 요청을 loopback으로 받기 때문이다.
- **Origin 허용 목록:** 공개 주소(`https://<이름>.ts.net[:포트]`)와 `http://127.0.0.1:<web_port>`만 허용한다.

**HTTPS가 필요한 브라우저 기능(secure context, 표준 기준이며 실기기 확인은 M1-1)**
- 비동기 Clipboard API(복사 버튼)
- Service Worker·PWA 설치·Web Push
- Wake Lock
- `crypto.subtle`
- Secure·`__Host-` cookie

사용자가 일으킨 `paste` 이벤트의 `clipboardData`는 HTTP에서도 동작한다. 그래서 이미지 paste는 비동기 clipboard-read 권한을 필수로 두지 않는다. 그래도 다른 기기에서 알림·설치·복사를 쓰려면 HTTPS가 있어야 하므로 공개를 첫 단계에 둔다.

### 6.4 인증·권한

- **장치 역할과 범위:** 장치마다 reader(보기)와 controller(조작) 역할을 나눈다. 명령마다 대상 project·machine·session 권한을 검증한다.
- **pairing**
  - 6자리 코드나 QR로 한다. 코드는 host 화면이나 로컬 CLI에서만 보인다.
  - 틀린 시도는 backoff로 막는다(web-ui는 5회부터).
  - token은 URL·로그·HTML에 넣지 않는다.
- **cookie와 요청 검사:** cookie는 `HttpOnly`·`SameSite=Strict`, HTTPS에서는 `Secure`·`__Host-`로 한다. Origin과 CSRF를 검사하고 CSP를 둔다.
- **권한 취소:** 권한 취소와 lease 변경은 이미 열린 WebSocket과 대기 중인 입력에도 바로 반영한다. reader는 resize·키 입력·routine 실행·질문 응답·업로드를 할 수 없다.
- **업로드:** 이미지 업로드도 변경 권한이다. 전송이 끝나기 직전에 권한을 다시 확인한다.
- **감사 로그:** 입력, thread 시작, routine 실행, 질문 응답, 업로드, 공개 변경을 누가 언제 했는지 남긴다.
- **보고서 표시:** agent가 만든 보고서 HTML은 `allow-same-origin` 없는 sandbox iframe(opaque origin)과 CSP로 보여 준다. 제어 UI와 같은 origin에서 script를 실행하지 않는다.
- **agent pane 안의 CLI:** 세션 역할로 범위를 정한다(§8.25, 10-17). 일반 thread와 사후 평가자 세션 안의 CLI는 보기만 되고, coordinator와 사람의 셸은 그대로 조작한다.

### 6.5 신뢰 경계

- **강한 sandbox가 아니다.** agent와 coordinator는 같은 사용자 계정의 shell을 갖는다. 보안 격리가 필요하면 별도 OS 계정이나 VM이 필요하다.
- **원격 실행 권한:** agent backend는 등록된 host 키의 요청만 받는다. 그 요청도 agent.toml의 허용 root·profile 정책 안에서만 실행한다.
- **승인 묶기:** profile·routine 승인은 설정 revision과 정확한 명령 digest에 묶는다.
- **agent 산출물은 믿지 않는다.** HTML·Markdown, 터미널 OSC, 파일 링크가 대상이다. 파일 열기는 machine-local allow-root 검증을 거친다.

## 7. 상태·설정·저장

### 7.1 배치

| 구분 | macOS | Linux | Windows |
|---|---|---|---|
| 설정(TOML) | `~/.config/aaa/` | `$XDG_CONFIG_HOME/aaa/`(기본 `~/.config/aaa/`) | `%LOCALAPPDATA%\aaa\config\` |
| 상태(SQLite·로그·키) | `~/Library/Application Support/aaa/` | `$XDG_STATE_HOME/aaa/`(기본 `~/.local/state/aaa/`) | `%LOCALAPPDATA%\aaa\state\` |
| 실행 파일(고정) | `/Applications/aaa.app/Contents/MacOS/aaa`, CLI는 `~/.local/bin/aaa` 링크 | `~/.local/bin/aaa` | `%LOCALAPPDATA%\aaa\bin\aaa.exe` |
| 프로젝트(host) | `~/.aaa/projects/<project>/`(TOML로 변경 가능) | 없음 | 없음 |
| thread 작업 폴더(agent) | `<worktree 또는 task dir>/.aaa/<project>-<thread>/` 아래 brief.md·report.md·library/·attachments/ | 같음 | 같음 |

- **위치 덮어쓰기:** `AAA_CONFIG_DIR`·`AAA_STATE_DIR`로 바꿀 수 있다.
- **Windows는 roaming `%APPDATA%`를 쓰지 않는다.** 머신별 값이 다른 PC로 동기화되면 안 되기 때문이다.
- **macOS 설정을 `~/.config`에 두는 이유:** 운영에서 herdr 설정을 dotfiles 링크로 관리해 왔고, 사용자 CLI 관례이기 때문이다.
- **SQLite 파일은 SMB/NFS로 공유하지 않는다.**

host 프로젝트 폴더 구성:

| 파일·폴더 | 내용 |
|---|---|
| `AGENTS.md`, `PROJECT.md`, `MEMORY.md`, `memory/` | 사람이 읽는 문맥 |
| `project.toml` | 기계가 읽는 값(nudge, 자동 승인 정책, 결과물 상한 등) |
| `TASKS.md` | `(profile@machine)` 소유 표기 |
| `decisions.md` | 결정 기록(§8.6) |
| `routines/` | routine 정의 |
| `threads/<id>.md` | 보고 사본 |
| `library/<id>/` | 결과물 |
| `inbox/` | Markdown 투영(정본은 DB) |

### 7.2 TOML 설정

- **파일 구성**
  - `host.toml`은 host 머신에, `agent.toml`은 모든 agent 머신에 둔다.
  - 선택 사항으로 `host.local.toml`·`agent.local.toml`을 두면 머신별로 덮어쓴다. 공유 dotfile을 링크하고 머신별 값(worktree root 등)만 local에 둔다. 운영에서 「worktree 줄만 다른」 사본을 따로 둔 문제를 해결한다.
- **스키마 버전:** 파일 맨 위에 `schema = 1`을 둔다.
- **host.toml에 두는 값**
  - Agent link listen 주소·포트, 웹 포트, 공개 후보 포트
  - 프로젝트 root
  - profile 허용 목록
  - 알림 webhook
  - 업데이트 고정 버전
  - 백업 보관 수
  - 등록된 머신 키 같은 기계 상태는 TOML이 아니라 상태 DB에 둔다.
- **agent.toml에 두는 값**
  - host 주소, machine 이름
  - 작업 root·허용 root
  - profile 정의: harness, model, effort, args, env, resume 방식, OS별 shell, daemon 금지 인자
  - 동시 thread 한도·우선순위
  - toolchain 점검 명령
  - attachment quota
- **검증과 오류 표시**
  - 시작할 때 전체를 검증한다. 모르는 key는 경고한다.
  - 잘못된 값은 `파일:줄:열`과 이유를 보이고 시작을 거부한다. `aaa config check`로 미리 확인할 수 있다.
  - reload에 실패하면 마지막 정상 설정을 유지하고 doctor·front·inbox(`config-error`)에 표시한다.
- **마이그레이션**
  - 사용자 파일을 자동으로 다시 쓰지 않는다.
  - 새 버전은 이전 schema(N-1)를 읽고 경고한다. `aaa config migrate`가 백업(`.bak-<시각>`)을 만든 뒤 주석을 보존해 바꾼다(toml_edit).
  - 모르는 미래 schema는 명시 오류다.
  - 이전 프로세스가 아직 같은 파일을 읽는 기간(§11)에는 migrate를 미룬다.
- **비밀:** TOML에 넣지 않는다. 상태 디렉터리의 0600 키 파일이나 DPAPI에 둔다.
- **원격 편집:** host는 agent.toml을 몰래 고치지 않는다. `aaa machine config show|diff`로 보기만 한다. 변경은 그 머신에서 하거나 명시 명령으로 하고 감사 로그에 남긴다.
- **profile 해석:** profile은 agent.toml이 정의하고 host.toml이 허용한다. host는 thread spec에 해석 결과와 revision을 저장한다. restart 때는 저장된 spec을 그대로 쓰거나 명시적으로 갱신한다. 원격에 같은 설정을 미리 깔 필요가 없다.
- **front 설정:** front의 키 바인딩 같은 값도 TOML로 두되, 이것은 backend 상태가 아니다.

### 7.3 DB와 schema

- **구성:** rusqlite(bundled), WAL, 단일 write task로 쓴다. host DB와 agent DB는 따로다.
- **schema 변경:** schema 버전 테이블을 둔다. migration은 expand/contract 순서로 해서 이전 프로세스가 계속 읽을 수 있게 한다.
- **백업과 되돌리기:** migrate 전에 `VACUUM INTO`로 백업한다. 다운그레이드는 백업 복원으로 한다. host DB는 주기적으로 백업하고, 복원 절차는 E2E로 확인한다.
- **파일과 DB 분담:** Markdown·TOML은 사람이 읽는 원본이고, DB는 이를 파싱한 snapshot과 revision·hash를 저장한다. 같은 상태를 양쪽에서 임의로 고치지 않는다. 파일 감시가 실패해도 reconciliation이 hash를 다시 찾는다.

## 8. 오케스트레이션 (projects 0.2 계승)

### 8.1 프로젝트·coordinator

- **역할 분담:** 프로젝트 폴더가 곧 coordinator의 문맥이다. coordinator는 평범한 agent이고, binary는 결정적인 절차만 맡는다.
- **운영 철학:** **보고는 파일, 프롬프트는 알림**이다. 놓친 알림 때문에 결과를 잃지 않는다.
- **coordinator 판정:** 프로젝트 폴더를 cwd로 쓰는 agent를 coordinator로 본다.

### 8.2 profile과 launch/resume spec

- **profile 필드:** 이름, 설명, agent kind, model, effort, args, env 참조, resume 방식, OS별 shell, daemon 금지 인자·env.
- **daemon 금지 인자는 resume에도 적용한다.**
  - 예: codex `--no-daemon`, `CODEX_SSH_SKIP_APP_SERVER_BOOT=true`
  - 운영에서는 복원이 인자 없이 `codex resume <id>`를 띄워, yolo와 daemon 금지가 빠지고 managed app-server daemon이 떴다.
  - 같은 spec으로 resume하는지 유닛 테스트로 막는다.
- **권한 의도 보존:** 권한 관련 launch intent를 보존하되, agent CLI의 trust 절차를 우회하지 않는다.

### 8.3 상태 사건

- **사건 종류:** agent backend가 `session_start`, `turn_start`, `turn_end`, `question_shown`, `error`(용량 오류 포함), `progress`, `report_changed`, `exited`를 사건으로 보낸다.
- **출처 우선순위:** harness hook·notify(adapter별, 버전별 fixture) → 프로세스 상태 → 화면 heuristic 순이다. 화면 heuristic은 보조이며 신뢰도를 함께 표시한다.
- **adapter가 사건 출처를 직접 정한다.**
  - herdr 0.9.5의 claude·codex hook은 SessionStart에서 session ID만 보고한다. 그래서 herdr식 integration으로는 상태를 사건으로 만들 수 없다.
  - M1-3에서 harness별 사건 출처를 조사해 고정한다. Claude Code의 Stop·Notification류 hook, Codex의 hook·notify류가 후보이며, 현재 지원 범위는 아직 확인하지 않았다.
- **감시 대상:** 모든 agent 세션을 같은 방식으로 등록·감시한다(thread 밖 agent와 coordinator 포함). coordinator와 front는 구독하고 polling하지 않는다. 주기적 reconciliation은 보조다.
- **감지 규칙 고정:** 화면 감지 규칙은 버전을 pin하고 원격에서 자동 갱신하지 않는다.
- **흐름 사건(host가 낸다):** `thread_started`, `brief_delivered`, `report_ready`, `review_acked`, `thread_resolved`, `thread_failed`, `thread_restarted`(재시도·재brief), `question_answered`, `merge_step`(enqueue·push·landed·cleanup).
  - 모든 사건은 project별 순번(seq), host 시각 + boot ID, 관련 thread·role·profile·machine, 근거 경로(report·로그·시간표)를 가진 durable 기록이다.
  - 구독자는 cursor로 읽는다. 알림을 놓쳐도 cursor부터 다시 읽어 복구한다(보고는 파일, 프롬프트는 알림).
  - 사후 평가자(§8.13)는 이 사건 묶음만으로 단계별 시간을 계산할 수 있어야 한다. 그래서 사건 schema와 보존 기간은 M1-3에서 평가자 용도까지 고려해 정한다.

### 8.4 확인창 원격 응답

- **질문 사건:** adapter가 확인창을 `question` 사건으로 올린다. 사건에는 종류, 본문, 선택지, 기본값, 남은 시간 추정이 들어간다.
  - 확인창의 예: 폴더 신뢰, hook 신뢰, 권한 확인, 모드 전환 질문
- **답하기**
  - front나 coordinator가 `aaa question answer <id> <선택지>`로 답한다.
  - host가 대상 세션 generation·lease·권한을 다시 확인한다.
  - adapter가 답을 harness 키 입력으로 바꾸고, 결과 사건으로 확인한다.
- **자동 승인은 기본 금지다.** `project.toml`의 종류별 허용 목록으로만 허용하고 감사 로그에 남긴다.
- **시한이 있는 질문은 바로 알린다**(§8.7). 운영에서 권한 확인이 1분 32초 뒤 자동 거부된 일이 있었다.
- **재질문 원인을 줄인다.** hook 경로를 고정한다(§8.11, §10.9).
- **참고 구현:** 0.2.35의 `trust_screens`·`thread keys`, web-ui의 prompt card.

### 8.5 메시지 API와 재입력 규칙

- **보내기:** `aaa thread message <id> --interrupt|--queue`로 보낸다.
  - interrupt: 지금 입력한다. 작업 중이면 harness가 지원하는 끼어들기 방식을 쓴다.
  - queue: host 대기열에 순서대로 넣고, 다음 턴이 시작될 때 전달한다.
- **입력 방식:** 키를 하나씩 치지 않고 adapter 입력(bracketed paste + harness별 전송 키)으로 보낸다. 특수문자가 깨지지 않는다. 원문은 기록한다.
- **수신 확인:** deadline 안에 `turn_start` 사건(없으면 화면 변화)이 오면 `accepted`다. 오지 않으면 `delivery_unknown`이다.
- **brief 전달:** 0.2.35 규칙을 이어받는다. claim lease 60초, settle 3초, 최대 3회, 실패하면 inbox item을 남긴다.
- **재입력 규칙은 두 가지이며 trigger가 다르다.**
  - (a) **전달 여부를 모를 때**(`delivery_unknown`, 연결 단절, ACK 유실): prompt·keys·shell command를 **절대 자동으로 다시 보내지 않는다.** 전송 재접속과 작업 재실행은 다른 절차다.
  - (b) **자동 이어가기 정책:** 관측된 사건에만 적용한다(`turn_end` 뒤 harness가 다음 지시를 기다림, 일시적 용량 오류). 기본은 꺼져 있고 profile·project별로 켠다. 규칙(보낼 문구), 횟수 상한, backoff, 기록, front 표시를 갖춘다([정할 것 5](#16-위험과-정할-것)).
- **nudge:** 0.2.35 방식을 이어받는다. 같은 미확인 묶음은 hash당 한 번만 보낸다. 놓친 알림은 다시 보내지 않고 context를 다시 읽어 복구한다.

### 8.6 결정 기록

- **기록:** `projects/<p>/decisions.md`에 번호, 시각, 내용, 대체한 번호를 적는다. DB에 revision을 두고 `aaa decision add|list`로 다룬다.
- **전달:** brief와 context에 최신 revision을 자동으로 넣는다. thread가 시작할 때 읽은 revision을 기록해, 결정이 바뀌었는데 이전 결정으로 일하는 thread를 찾을 수 있다.

### 8.7 보고·결과물·알림

- **report:** report.md 원본은 thread 작업 폴더에 있다. hash가 바뀌면 host가 Agent link로 임시 경로에 받고, 크기·hash를 검증한 뒤 원자적으로 publish하고 review 사건을 낸다.
- **library**
  - 상대 경로·digest·길이를 담은 manifest로 다룬다.
  - 기본 복사 상한(예: 200MB, `project.toml`) 안에서만 복사한다. 그 넘는 파일은 agent에 두고, host 웹이 요청할 때 Agent link로 지연 스트리밍한다. host 캐시에는 상한을 둔다. 0.2.35의 50MB 상한 때문에 175MB·5GB 결과물을 잃을 뻔한 일을 막는다.
  - symlink와 junction은 따라가지 않는다.
- **보기:** host 웹이 보고서와 library를 HTTPS로 바로 보여 준다(§6.4 sandbox).
- **알림**
  - Web Push(HTTPS 필요)와 외부 알림 webhook(host.toml, HTTP POST)을 둔다. 명령 실행이 필요한 알림은 로컬 agent backend가 실행한다.
  - 대상 사건: 보고 준비, 질문, 막힘, 머신 이상.
  - 사용자는 이런 일을 텔레그램으로 받기를 원한다. webhook으로 연결한다.
- **resolve:** 최종 receipt와 dirty worktree 검사를 통과해야 한다. 실패하면 worktree를 보존한다. 0.2.35처럼 복사가 Partial이면 보존하고, Failed면 resolve를 거부한다.

### 8.8 병합 대기열과 정리

- **대기열:** `aaa merge enqueue <thread>`로 순번을 정한다.
- **단계마다 사건과 훅을 낸다.**
  1. 원격 일치·tree 같음 확인
  2. push
  3. 영향받는 thread에만 「main 이동」 알림(운영 규칙)
  4. 정리(중첩 worktree, worktree, branch, stash)
  5. 유휴 머신 사전 빌드 훅
- **단계 배치:** M1-3에서는 순번과 사건·훅만 둔다. 단계 자동 실행은 M3에서 한다(§8.24에서 했다).
- **참고:** 0.2.35의 `Landing` group, `on = "pr"` routine.

### 8.9 ticker·inbox·routine

- **ticker:** host 안의 task다. 사건을 구독하고 제한된 reconciliation timer를 돈다. 0.2.35는 15초마다 돌고 원격은 약 1분마다 polling했다.
- **inbox:** 잃지 않는 사건과 ack로 관리하고, Markdown은 투영이다. 특수 item을 둔다: `session`(재시작 감지), `outage`(장시간 단절), `config-error`, 머신 이상.
- **routine**
  - 일정은 `every`·`daily`·`on = "pr"`이다. 정확한 명령 digest로 승인하고, `routine_commands` 같은 전체 스위치를 둔다.
  - DST, missed-run 정책, 실행 ID를 다룬다. coordinator가 없으면 실행하지 않고 skipped로 기록한다.
  - **명령은 대상 머신의 agent backend가 실행한다.** host는 실행하지 않는다.

### 8.10 머신 건강과 자원 한도

- **건강 보고:** agent backend가 주기적으로 보고한다.
  - toolchain 버전(agent.toml에 선언한 명령, 예: CLT/Xcode)
  - 디스크 여유, 부하
  - boot ID와 재부팅
  - 계보·첫 실행 지연·키체인(§10)
  - 전원(AC), 시계
- **이상 처리:** 변화나 이상은 inbox와 알림으로 낸다. 점검에 실패한 머신에서는 새 세션 시작을 거부하고 사유를 보인다. 재부팅 중 작업이 조용히 실패하던 문제는 boot fencing으로 `interrupted`가 된다.
- **동시 작업 한도:** 머신별 동시 thread 한도를 둔다(운영값 예: 6).
- **우선순위:** nice, Windows priority class를 쓴다. macOS QoS는 P0에서 측정한 뒤 정한다.
- **빌드 병렬도는 제한하지 않는다.** 사용자 규칙이다.
- **종료:** session host가 자기 자식만 PID+시작 identity(Windows는 handle)로 종료한다.
- **안전 규칙의 위치:** rm·force push 같은 규칙은 agent가 지킬 정책이며 aaa가 대신 강제하지 않는다.

### 8.11 hook·integration

- **고정 명령:** `aaa hook`은 고정 명령 문자열로 등록한다(예: `~/.local/bin/aaa hook --agent claude`). 업그레이드로 경로나 인자가 바뀌지 않게 한다. 운영에서는 경로만 바뀌어도 codex가 hook 신뢰를 다시 물었다.
- **설치는 사용자가 한 번 한다.** `aaa integration install`로 설치한다(Claude Code는 실행마다 `--settings`로 주므로 설치가 필요 없다. §8.15). agent 설정 파일 변경은 auto mode가 막으므로 사용자가 직접 실행한다.
- **되돌리기:** 바꾼 내용을 journal로 남겨 정확히 되돌린다(0.2.35 `owned.json` 방식). 사용자가 지운 파일은 되살리지 않는다.
- **herdr와 공존:** aaa가 관리하지 않는 pane에서는 세션 token이 없으므로 hook이 아무것도 하지 않는다.

### 8.12 thread 종류와 배치

- **로컬·원격 기능이 같다.** 종류는 worktree, tab(같은 checkout), checkout, adopt(기존 pane 등록), repo 없는 task dir이다. 0.2.35는 원격에서 worktree만 허용했다.
- **기존 운영 규칙 유지:** worktree는 머신별 설정 root에만 만든다. thread 기록의 경로와 실제 디렉터리가 일치하는지 확인한다.

### 8.13 역할 모델과 사후 평가자 (결정 7)

참고 사례는 지금 운영의 t-0250(병합 재작업을 줄이는 도구 개선)이다. 착지마다 coordinator가 감시 기록(watch.md)에 손으로 사실을 적으면, 그 agent가 결과물 품질이 아니라 **흐름의 효율**을 판정하고 도구 개선안을 냈다. aaa는 이것을 기능으로 만든다.

**역할(role) 모델.** 모든 agent 세션은 spec에 역할을 가진다. 역할은 처음부터 schema·Front API·token에 들어가는 열거형이며, 새 역할을 더해도 기존 데이터가 깨지지 않게 둔다.

| 역할 | 하는 일 | 목록 표시 | 다른 agent에 지시 |
|---|---|---|---|
| `coordinator` | 프로젝트 판단, thread 시작·검토·resolve | 프로젝트 머리 | 가능(Front API 권한 안에서) |
| `worker` | 일반 thread 작업 | 왼쪽 agent 목록 | 불가(자기 보고만) |
| `evaluator`(사후 평가자) | 흐름 효율 판정·도구 개선안 | **목록에 넣지 않음.** 별도 표시 | **불가** |

**지정.**
- `aaa evaluator assign --project P --machine M --profile P`로 지정하고, `project.toml`의 `[evaluator]`에 기록한다. 프로젝트당 기본 하나이며 profile과 머신은 고른다.
- host가 평가자 세션을 일반 thread처럼 agent backend에 시작한다. 실행 사실·resume·재부팅 처리는 일반 세션과 같다. 동시 thread 한도에는 포함하고 우선순위는 낮게(nice) 둔다.
- 평가자 작업 폴더는 `projects/<p>/evaluator/`(host)와 대상 머신의 `.aaa/<project>-evaluator/`다.

**UI.**
- TUI 왼쪽 agent 목록과 웹 agent 목록에 섞지 않는다. 헷갈리지 않게 하기 위해서다. Front API의 목록 조회도 기본은 `coordinator`·`worker`만 돌려주고, 평가자는 `--role evaluator`로만 나온다.
- 상태는 따로 보인다. TUI는 상단 상태줄의 작은 표시(예: `평가 ● 2`, 새 판정 수와 working/idle), 웹은 header의 badge다.
- `aaa evaluator open`(TUI 단축키, 웹 badge 클릭)으로 평가자 pane을 열어 화면을 보고 직접 말을 건다(일반 attach와 lease 규칙이 같다). 닫으면 다시 배경으로 돌아가고 세션은 계속 돈다.

**입력: host가 사건을 자동으로 넘긴다.**
- 대상 사건: 착지(`merge_step` landed), 완료(`thread_resolved`), 실패(`thread_failed`), 재시도(`thread_restarted`), 그 밖에 §8.3 흐름 사건 중 project.toml에서 고른 것.
- 넘기는 내용: 사건 묶음(thread의 단계별 시작·끝 시간표, 재시도 횟수, 질문 대기 시간), 로그·보고서·검토 기록 경로. coordinator가 손으로 감시 기록을 적지 않아도 된다.
- 전달 방식: host가 평가자 cursor 뒤의 사건을 `evaluator/inbox/<seq>.json`에 쓰고, 메시지 API의 `--queue`로 「새 사건 n건」 알림만 보낸다. 사건이 몰리면 묶어서(debounce, 기본 예: 5분 또는 착지 1건마다) 보낸다. 알림이 `delivery_unknown`이어도 다시 보내지 않고, 평가자는 cursor부터 다시 읽는다(§8.5 a).

**출력: 정해진 평가 기록.**
- 평가자는 `aaa evaluation submit --file F`로 기록을 낸다. host가 `projects/<p>/evaluations/<id>.md`(사람이 읽는 본문)와 DB 구조화 기록을 저장한다.
- 구조화 기록 항목: 대상 사건·thread, 단계별 시간, 재작업(횟수·원인), 막힌 곳, 도구 문제, 결함을 누가 어느 단계에서 잡았나, 개선안 목록(id·내용·예상 효과·대상 도구).
- coordinator나 사용자가 `aaa evaluation adopt|hold <id>`로 개선안마다 채택·보류를 정한다. inbox에 `evaluation` item으로 뜨고 결정 기록(§8.6)과 연결된다.
- 채택된 개선의 구현은 coordinator가 정한다. 평가자 자신의 worktree에서 고쳐 일반 착지 절차를 밟게 할 수도 있고(t-0250 방식), 별도 thread를 시작할 수도 있다.

**금지.**
- 평가자는 다른 agent에 직접 지시하지 않는다. 평가자 세션의 token으로는 다른 thread에 대한 `thread message`·`thread start`·`question answer`·`pane send`가 거부된다(Front API가 role로 판정).
- 평가자는 사건·보고·로그를 읽고, 자기 작업 폴더와 평가 기록만 쓴다.

**단계.** MVP 필수는 아니다. 그래도 막히지 않게 아래는 M1-3에 넣는다.
- 역할 필드(schema·Front API·token)
- 목록의 기본 role 필터
- 흐름 사건과 cursor 구독
평가자 기능 자체(지정, 배경 실행, 별도 표시, 사건 자동 전달, 평가 기록, 채택·보류, role 금지 동작)는 M3에 둔다(§14). 구현하면서 입출력 경로와 지정 기록을 바꿨다(§8.22).

### 8.14 M1-3a 구현 결정(10-16)

- **자리 만들기는 agent가 한다.** `ThreadPrepare`를 받은 그 머신의 agent가 일을 맡는다.
  - worktree(`<work_root>/<repo 이름>/<project>-<thread>`, branch `aaa/<project>/<thread>-<slug>`)나 작업 폴더(`<work_root>/<project>/<thread>`)를 만든다.
  - brief.md를 쓰고 profile을 해석한다. host는 git을 실행하지 않고 해석 결과(`Launch`)를 thread 기록에 저장한다.
- **작업 폴더**는 `.aaa/<project>-<thread>/`(brief.md·report.md·library/)이고, repo의 `info/exclude`에 `.aaa/`를 넣는다.
- **profile → argv**
  - claude: `--model`·`--effort`·args 뒤에 `--session-id <uuid>`를 붙인다. 그래서 resume(`--resume <uuid>`)이 같은 대화를 잇는다.
  - codex: `-m`·`-c model_reasoning_effort=…`·args를 쓴다. 세션 ID는 M1-3b hook에서 받는다.
  - 두 harness 모두 resume argv가 launch와 같은 model·effort·args·env를 쓰는지 유닛 테스트로 막는다(§8.2).
- **brief 안내**
  - 화면이 3초 멈추면 `.aaa/<p>-<t>/brief.md를 읽고 그대로 하라.`를 한 번 입력하고 Enter를 보낸다.
  - 8초 안에 화면이 바뀌면 `delivered`, 아니면 `unknown`이다. unknown은 **자동으로 다시 보내지 않는다.** 사람이 `aaa thread brief`로만 다시 보낸다.
  - harness 확인창(폴더 신뢰 등)이 보이면 입력하지 않는다.
  - 수신 확인을 화면 변화 대신 harness 사건(`turn_start`)으로 바꾸는 것은 M1-3b에서 했다(§8.15).
- **보고 회수**
  - agent가 report.md(regular file만) hash를 1초마다 보고 바뀌면 알린다. host는 본문과 library(symlink 제외, `library_copy_mb` 상한)를 받는다.
  - 받은 것은 hash를 확인한 뒤 `threads/<id>.md`·`library/<id>/`에 원자적으로 둔다. 건너뛴 것이 있으면 사본이 `partial`이고, 실패하면 hash를 바꾸지 않는다.
  - agent가 다시 붙으면 host가 감시를 다시 걸고, agent는 곧바로 지금 hash를 보낸다. 그래서 host가 꺼진 동안 바뀐 보고도 받는다.
- **보고 형식(0.2.35에 없던 규칙)**
  - 첫 줄 상태 태그(`[진행 중]`·`[사용자 작업 필요]`·`[결정 필요]`·`[완료]`)와 `## Next` 줄을 읽는다. 지금 운영 brief가 쓰는 형식이다.
  - `[사용자 작업 필요]`·`[결정 필요]`면 목록 묶음이 「사람 차례」다(harness가 일하는 중이 아닐 때, §8.38).
- **resolve**
  - 마지막 사본이 실패하면 거부한다. 그다음 세션을 끝내고 worktree를 force 없이 지운다. 고친 파일이 있으면 남긴다.
  - branch는 기준 ref에 병합(조상)됐을 때만 지운다.
- **흐름 사건**
  - 프로젝트별 seq로 DB에 쌓는다. 지금 내는 사건: `thread_started`·`brief_delivered`·`brief_unknown`·`report_ready`·`copy_failed`·`review_acked`·`thread_resolved`·`thread_failed`.
  - 연결된 front에는 `Flow` 사건으로 알린다. `aaa events --after <seq>`는 cursor부터 다시 읽는다.
- **역할:** `SpawnSpec`·`SessionInfo`·`PaneView`에 role이 있다. pane 목록은 기본으로 평가자를 빼고 `--role`로 거른다.

### 8.15 M1-3b 구현 결정(10-16)

- **사건 출처(§8.3 확정)**
  - Claude Code는 실행마다 `--settings <작업 폴더>/.aaa/<p>-<t>/claude-settings.json`으로 hook을 준다. 사용자 설정 파일은 고치지 않는다. 그래서 Claude에는 `aaa integration install`(§8.11)이 필요 없다.
  - 쓰는 hook과 정규화 결과는 다음과 같다.
    - SessionStart → `session_start`, UserPromptSubmit → `turn_start`
    - PostToolUse → `working`, Stop → `turn_end`, StopFailure → `error`, SessionEnd → `exited`
    - PermissionRequest·Notification(permission_prompt) → `question`, Notification(idle_prompt) → `idle`
  - Codex는 notify(`agent-turn-complete` → `turn_end`)를 쓴다. 실행마다 `codex -c notify=[<aaa>,"hook","--agent","codex"]`로 준다(사용자 `~/.codex` 설정은 고치지 않는다). hook은 프로젝트 `.codex/`·hash 신뢰가 필요해 쓰지 않는다. 실측은 §8.21.
  - 세션 안의 `aaa hook --agent …`·`aaa report --percent --activity`가 그 머신 agent의 로컬 socket(`run/hook.sock`)으로 보낸다.
    - 세션 env의 `AAA_HOOK_TOKEN`은 세션 token에서 만든 값이라 따로 저장하지 않는다.
    - `aaa hook`은 무엇이 실패해도 출력 없이 0으로 끝난다(harness를 막지 않는다).
  - agent는 pane별 마지막 사건을 기억했다가 host에 다시 붙으면 다시 보낸다.
- **상태:** host가 pane별 `AgentStatus`(activity·since·harness·message·percent·progress·harness_session)를 사건으로만 바꾼다. `PaneView.agent`·`ThreadView.agent`로 보이며 polling하지 않는다.
- **세션 표지 env를 넘기지 않는다.**
  - agent를 Claude Code 세션 안에서 띄우면 `CLAUDECODE`·`CLAUDE_CODE_CHILD_SESSION`·메시징 token 같은 표지가 aaa 세션까지 이어진다.
  - 그러면 세션 안 Claude가 자기를 자식 세션으로 보고 기록 저장(=resume)을 끈다(실측). 그래서 session-host가 이 표지를 지운다. 사용자 설정 env는 남긴다.
- **질문(§8.4)**
  - hook 질문과, brief 안내 전에 화면에서 본 선택 창(폴더 신뢰 등)을 `question`으로 올린다. 한 pane에 열린 질문은 하나로 묶는다(Claude는 PermissionRequest와 Notification을 함께 낸다).
  - 다음 hook 사건(working·turn_end·idle·exited)이 오면 열린 질문은 `gone`이 된다.
  - 흐름 사건 `question_shown`·`question_answered`를 내고, front에는 `Question` 사건을 push한다.
- **답 키(adapter)**
  - 「아니오」는 Esc(취소)다. 「예」는 화면 선택 창을 읽어, 커서(❯)에서 Yes로 시작하는 첫 선택지까지 화살표로 옮긴 뒤 Enter를 보낸다.
  - Claude Code 2.1 폴더 신뢰 창은 **기본 선택이 「No, exit」**라 Enter만 보내면 세션이 끝난다(실측). 선택 창을 찾지 못하면 키를 보내지 않고 거부한다.
  - 답하기 전에 질문이 열려 있는지, 선택지에 있는지, 세션이 실행 중인지 다시 본다.
  - Claude는 권한을 거절하면 턴을 멈추고 입력을 기다리지만 Stop hook을 내지 않는다(실측). 그래서 거절 뒤 상태를 `idle`로, 허락 뒤 상태를 `working`으로 둔다.
- **선택 창은 화면 맨 아래에서만 찾는다.** 표지가 맨 아래 3줄(빈 줄 제외) 안에 있을 때만 선택 창으로 보고, 본문·선택지는 그 위 24줄에서 찾는다. 소스를 cat한 출력 등에 같은 글이 있어도 메시지를 막지 않는다.
- **interrupt 멈춤 판정은 새 「Interrupted」만 본다.** Esc 전후 화면의 개수를 비교한다. 지난 interrupt의 글이 남아 있으면 곧바로 입력해 사라지게 하던 문제를 막는다.
- **선택 창이 떠 있으면 아무것도 입력하지 않는다.** 표지는 「Enter to confirm」「Esc to cancel」, 신뢰 문구 등이다. brief 안내와 메시지 모두 해당한다. 질문이 열려 있는 동안 brief 안내는 최대 30분 기다린다.
- **입력과 수신 확인(§8.5)**
  - 한 줄은 그대로, 여러 줄은 bracketed paste로 보낸 뒤 Enter를 보낸다.
  - hook이 있는 세션은 10초 안에 `turn_start`가 오면 `accepted`, hook이 없으면 화면 변화로 본다. 확인 못 하면 `unknown`이고 다시 보내지 않는다.
- **메시지(§8.5 보충)**
  - queue: 입력 대기 중(idle·started, 상태를 모르면 포함)이면 바로 보낸다. 아니면 대기열에 두고, 그 thread의 다음 `turn_end`·`idle` 때 가장 오래된 것 하나를 보낸다. 원안의 「다음 턴이 시작될 때」를 「턴이 끝났을 때」로 고쳤다.
  - interrupt: 작업 중이면 Esc를 보내고, `turn_end`·`idle` 사건이나 화면의 「Interrupted」를 최대 15초 기다린 뒤 보낸다. Claude는 Esc로 멈춰도 Stop hook을 내지 않는다(실측, 약 1초에 멈춤).
  - 확인창이 열려 있으면 interrupt를 거부한다. Esc가 확인창을 「아니오」로 닫기 때문이다.
  - 보내기는 host에서 한 번에 하나씩 한다. 흐름 사건은 `message_queued`·`message_accepted`·`message_unknown`이다. 원문은 DB에 두고 감사 로그에는 길이만 적는다.
  - 선택 창이 떠 있을 때 보내려던 메시지는 입력하지 않고 `unknown`으로 끝난다(§8.5 a에 따라 다시 보내지 않는다). 답한 뒤 사람이 다시 보낸다.
- **front 표시(M1-3c-1)**
  - 웹 events WS는 목록(state)과 질문(question) 사건을 보낸다. 웹은 연결할 때마다 `questions`로 한 번 읽고, 이후는 사건으로 따라간다. 사건을 놓치면(broadcast lag) host가 WS를 끊어 다시 읽게 한다.
  - 웹 목록 위에 「답을 기다리는 질문」 카드가 있고, 고른 pane 위에도 그 pane의 질문 카드가 있다. reader에게는 답 버튼이 없다. pane마다 harness 상태 badge가 있다.
  - TUI는 상태 줄에 `실행 · 작업 중` 같은 상태를 보이고, 질문이 열린 pane에는 「질문 — aaa question」 badge를 그린다.
- **pane 등록 전에 온 hook 사건:** 세션이 시작하자마자 보낸 hook이 세션 시작 사건보다 먼저 host에 닿으면 버려지던 문제를 고쳤다. host는 모르는 pane의 사건을 60초(최대 32개) 보관했다가 pane이 등록되면 순서대로 적용한다.
- **계약:** AGENT_LINK·FRONT_API를 1.3으로 올렸다(`AgentEvent::Harness`, 질문·메시지 요청, `PaneView.agent`·`ThreadView.agent`). 1.2 메시지를 읽는 시험이 있다.
- **확인 방법**
  - E2E `m13 hooked_thread_questions_and_messages`: 가짜 Claude가 실제처럼 신뢰 창(기본 No)·권한 창을 그리고 hook을 보낸다. 거절과 Esc 뒤에는 Stop을 내지 않는다.
  - 실제 Claude Code 2.1(haiku) smoke: 신뢰 창 → brief → 권한 거절·허락 → 메시지 → 보고 회수 → interrupt를 한 바퀴 돌렸다.

### 8.16 M1-3c 구현 결정(10-16): 결정 기록, thread stop·restart

- **결정 기록(§8.6):** `aaa decision add <p> --text|--text-file [--replaces N]`, `aaa decision list <p>`
  - `decisions.md`가 원본이다. `## <번호> · <UTC 시각>[ · <번호>번 대체]` 묶음으로 덧붙인다. 사람이 고친 파일도 `## <번호>` 제목으로 읽는다.
  - 흐름 사건은 `decision_added`(thread 없는 프로젝트 사건)다.
  - thread는 시작·다시 시작 때 brief에 넣은 가장 큰 번호를 `decisions_rev`로 기록한다. `decisions_now`보다 작으면 CLI가 「결정 N번까지 보고 시작했다」를 보인다. 별도 DB revision 표는 두지 않았다(파일 번호가 revision이다).
- **thread stop:** 0.2.35처럼 Esc 한 번이다. 세션·작업 폴더는 그대로다. 흐름 사건은 `thread_stopped`다.
- **thread restart**
  - 세션이 실행 중이면 거부한다(`aaa pane kill` 뒤에 한다). 열린 thread인데 보고 없이 세션이 끝났으면 목록 묶음이 「사람 차례」다. 보고를 쓴 뒤 끝난 세션은 끝난 일이다(새 보고면 「검토 대기」, 확인했으면 「쉼」, §8.38).
  - 기본은 task(Follow-ups 포함)·지침·결정·memory를 다시 읽어 brief를 새로 쓴다. brief 맨 앞에는 「다시 시작했다, 이전 시도를 먼저 확인한다」를 붙이고, 새 대화로 띄운 뒤 안내한다. claude는 새 `--session-id`를 받는다(agent `ThreadRebrief`).
  - `--resume`은 저장된 launch의 resume argv로 같은 대화를 잇고, 안내하지 않는다. `{session}`이 필요한데 harness 세션 ID를 모르면 거부한다.
  - profile은 다시 해석하지 않는다(저장된 launch를 쓴다). profile 바꾸기는 하지 않았다.
- **아직:** `thread next`, TASKS.md 소유, `start_threads` 정책은 하지 않았다.

### 8.17 M1-3c 구현 결정(10-16): inbox·ticker·nudge

- **inbox(§8.9):** host DB가 원본이고 `projects/<p>/inbox/<id>.md`(0.2.35와 같은 `+++` TOML front matter)는 투영이다.
  - ID는 `<UTC>-<종류>-<대상>-<순번>`이다. 끝내면 파일을 `inbox/done/`으로 옮기고, `inbox_done_days`(기본 30일)가 지나면 지운다.
  - `aaa inbox list <p> [--all] [--peek]`, `aaa inbox done <p> <id…>|--all`. `--peek`이 아니면 보인 항목을 「봤다」로 표시한다.
- **항목 출처**
  - 흐름 사건: `report_ready`(report), `question_shown`(question), `brief_unknown`·`message_unknown`(delivery), `thread_failed`·`copy_failed`(thread).
  - `session`: 열린 thread의 세션이 끝났다. **세션(pane)마다 한 번만** 만든다. 끝낸 뒤에도 같은 세션으로는 다시 만들지 않는다(E2E가 잡은 결함).
  - `outage`: 그 머신에 열린 thread가 있는 프로젝트에, `outage_secs`(기본 600초) 넘게 끊기면 한 번, 돌아오면 한 번. 끊긴 시간은 이 host 프로세스가 처음 끊김을 본 때부터 잰다(§8.38).
  - `config-error`: project.toml을 TOML로 읽지 못하면 파일 내용 hash당 한 번.
- **ticker:** host 안의 task다. `tick_secs`(기본 15초)마다 돌고, inbox 항목이 생기거나 harness가 입력 대기(idle)가 되면 바로 돈다. 원격 polling은 없다(상태는 사건으로 온다).
- **nudge:** 0.2.35를 이어받는다.
  - 처음 보는(unseen) 끝내지 않은 항목 ID 묶음의 hash가 지난번과 다를 때만 보낸다. 문구는 `[aaa inbox] <대상> <사건>, …; …(aaa inbox list <p>)` 한 줄이고, 대상은 5개까지 적고 나머지는 「N more」로 줄인다.
  - coordinator는 host 머신에서 cwd가 프로젝트 폴더인 실행 중 pane이다. **coordinator가 없으면 보내지 않고 hash도 남기지 않는다.** 생기면 그때 알린다(0.2.35는 기록만 하고 넘어갔다).
  - 보내는 때: hook 상태가 있으면 idle·started로 `nudge_idle_secs`(기본 60초, herdr-projects `NUDGE_IDLE_SECS`) 넘게 머물 때, 없으면 화면이 그만큼 그대로일 때다. 입력창(맨 아래 `❯`·`›` 줄)이 이번 검사와 `nudge_quiet_secs`(기본 10초) 전 검사에서 모두 비어 보일 때만 보낸다(herdr-projects `box_quiet`, 사람이 치는 글과 섞이지 않게). 쓰던 글이 있거나 입력 줄이 없으면(일반 셸 등, 글을 쳐 넣으면 명령으로 실행된다) 보내지 않는다.
  - 안 본 항목이 남아 있으면 새 항목이 올 때 다시 나열된다(herdr-projects와 같다). 「봤다」는 `aaa inbox list`(`--peek` 아님)와 `aaa context`(`--peek` 아님)가 보인 항목에 단다(10-17, 차이 목록 G3·F8).
  - 전달은 메시지와 같은 send_prompt다(선택 창이 열려 있으면 입력하지 않음). 받았는지 몰라도(unknown) 같은 묶음은 다시 보내지 않는다(§8.5 a). 흐름 사건은 `nudge_sent`다.
- **닫힌 질문(실제 시험 뒤 결정):** 질문에 답했거나 harness가 넘어가 질문이 닫히면, 그 질문의 `question` 항목은 끝낸 것으로 둔다(기록은 `inbox/done/`에 남는다). 실제 시험에서 이미 답한 질문이 알림마다 「question」으로 섞였기 때문이다.
- **아직:** 웹·TUI의 inbox 화면, 머신 이상(건강) 항목.

### 8.18 M1-3d 구현 결정(10-16): 병합 대기열 순번, 동시 thread 한도, 미룬 것

- **병합 대기열(§8.8):** `aaa merge enqueue|list|done|drop <p> <thread>`
  - 순번은 host DB에 둔다. 열린 thread만 넣는다. 이미 있으면 그 자리를 지킨다.
  - 흐름 사건: `merge_enqueued`(순번), `merge_head`(맨 앞이 바뀜), `merge_landed`(`done`), `merge_dropped`(`drop`·resolve).
  - 단계 실행과 훅은 M3에서 한다(원래 배치). 맨 앞이 된 thread에 무엇을 보낼지는 coordinator가 `merge_head` 사건을 보고 정한다.
- **동시 thread 한도(§8.10):** `[projects] max_threads_per_machine`(기본 6, 0이면 제한 없음)
  - 시작 중이거나 세션이 실행 중인 열린 thread를 머신별로 센다. `thread start`·`restart`가 한도에 닿으면 거부하고, 어느 thread가 도는지 보인다.
- **미룬 것(이유: 결정 14의 「큰 그림 우선」, M1-4와 운영 대체 경로가 먼저다)**
  - 머신 건강 보고(디스크·부하·toolchain·전원·시계)와 이상 시 새 세션 거부 → M1 마무리(M1-4 뒤). boot fencing과 단절(`outage`)은 이미 있다. **M1 마무리에서 했다(§8.21).**
  - `thread next`, TASKS.md `(profile@machine)` 소유, `start_threads` propose/auto → M1 마무리. coordinator는 지금도 `thread start`로 같은 일을 한다. **M1 마무리에서 했다(§8.21).**
  - routine은 M1-3d의 다음 착지에서 한다(명령 실행이 있어 보안 경계를 따로 본다).

### 8.19 M1-3d 구현 결정(10-16): routine

- **파일:** `projects/<p>/routines/<name>.md`. `+++` TOML front matter(`schedule`·`command`·`machine`·`enabled`)와 본문(coordinator 안내)이다. 모르는 키는 오류로 본다(오타로 명령이 빠지지 않게).
  - 읽지 못한 파일은 내용 hash당 한 번 `config-error` 항목이 된다. `on = "pr"`은 M3까지 오류로 둔다(조용히 무시하지 않는다).
- **일정(0.2.35 의미 그대로)**
  - `every <N>s|m|h|d`, `daily HH:MM`. `s`는 aaa가 더한 단위다(E2E가 분 단위로 기다리지 않게). 처음 본 때를 지난 실행으로 친다.
  - daily는 host 머신의 지역 시각이다. 각 HH:MM의 offset을 그 시각 기준으로 구한다(`localtime_r`). 서머타임이 바뀐 다음 날 두 번 돌던 계산을 단위 시험이 잡아 고쳤다. unix가 아닌 host는 UTC로 본다.
  - 여러 번 놓쳐도 한 번만 돈다.
- **coordinator 없음:** 돌지 않고 `no_coordinator`만 센다. 지난 실행은 앞으로 옮긴다(밀린 실행을 쌓지 않는다).
- **안내 routine(명령 없음):** inbox `routine`·`due` 항목을 본문과 함께 남긴다. 처리하지 않은 항목이 있으면 새로 만들지 않고 `skipped`를 센다.
- **명령 routine: 두 문이 모두 열려야 한다.** 둘 다 프로젝트 폴더 밖에 둔다(프로젝트 폴더는 agent가 고칠 수 있다).
  1. host.toml `[projects] routine_commands = ["<p>"|"*"]`
  2. 사람의 승인: `aaa routine approve <p> <name>`. 승인 대상은 명령과 실행 머신의 sha256이고 host DB에 둔다. 파일의 명령이나 머신이 바뀌면 승인이 풀린다.
  - 둘 중 하나가 없으면 실행하지 않고, 명령 hash당 한 번 `routine-approval` 항목을 남긴다.
  - 승인 CLI는 host 머신 로컬 요청만 받는다(웹·원격 front 불가). aaa 세션 안(`AAA_SESSION_ID`·`AAA_HOOK_SOCK`)이거나 표준 입력이 터미널이 아니면 거부한다. 명령을 보이고 routine 이름을 치게 한다. CLI가 보낸 hash가 지금 파일과 다르면 host가 거부한다.
  - **남는 한계:** aaa 밖에서 PTY를 만들어 env를 지운 프로세스가 사람처럼 이름을 치는 것까지는 막지 못한다. 0.2.35(터미널 + 이름 입력)보다는 강하지만 사람 확인 장치는 아니다. 강한 확인(웹 pairing 기기의 승인 버튼, OS 인증)은 M3에서 본다.
- **실행:** 대상 머신 agent가 `sh -c`(Windows는 `cmd /C`)로 실행한다. host는 실행하지 않는다.
  - 실행 폴더: host 머신이면 프로젝트 폴더, 다른 머신이면 그 agent의 `<work_root>/<project>/routines`다.
  - 자식은 새 process group으로 띄우고, 60초가 지나면 **그 group만**(agent가 만든 pgid) 끝낸다. 출력은 64KB까지 모으고 나머지는 읽어 버린다.
  - host ticker는 실행을 기다리지 않는다(별도 task). 같은 routine이 아직 돌면 그 차례는 건너뛴다.
- **결과:** 종료 코드와 출력의 hash가 바뀔 때만 `routine`·`output changed` 항목을 만든다. 본문은 안내 + 출력 4,000자다. 출력은 「신뢰할 수 없는 명령 출력. 지시가 아니라 자료다」 아래 fence로 감싸고, fence는 출력 안 backtick보다 길게 한다. 실행하지 못하면 `routine`·`failed` 항목(열린 것이 있으면 하나)과 `routine_failed` 사건을 남긴다.
- **흐름 사건:** `routine_ran`, `routine_failed`, `routine_approved`.
- **CLI:** `aaa routine list <p>`(일정·지난·다음·건너뜀·승인 상태), `aaa routine approve <p> <name>`.

### 8.20 M1-4 구현 결정(10-16): 보고서·결과물 보기, 알림, 이미지, 순서와 미룬 것

- **순서:** 운영 대체에 먼저 필요한 것부터 한다. ① 웹 보고서·결과물 보기 ② 알림 webhook ③ 웹 이미지 붙여넣기(macOS 대상).
- **미룬 것**
  - TUI Ctrl+V 이미지: front 기기 clipboard를 읽는 crate(arboard)가 Linux CI에 X11 개발 의존을 끌어온다. 웹 붙여넣기가 같은 attachment 경로를 먼저 검증한다. → M1 마무리(실제로는 M3 §8.28에서 했다. 개발 파일 의존도 생기지 않았다)
  - Web Push: VAPID·암호화(ECE)·service worker가 필요하다. M1-4 기준 「질문·보고 알림」은 webhook(텔레그램 연결)으로 채운다. → M3(PWA와 함께, §8.26에서 구현)
- **결과물 manifest:** 보고를 회수할 때 `threads/<id>.files.json`에 결과물 목록(경로·크기·host 사본인지)을 남긴다. 복사 상한을 넘어 agent에 남긴 일반 파일도 적는다(agent가 그 크기를 알린다). symlink·일반 파일이 아닌 것은 목록에 없다.
- **보기 경로(권한):** 웹은 `ThreadLibrary`(reader도 가능)로 `/v/<cap>/`를 받는다.
  - cap은 무작위 24바이트이고 host 메모리에만 있다. 2시간 뒤 만료되고 host가 다시 시작하면 사라진다. 그 thread 결과물 읽기만 된다.
  - **URL에 권한을 넣는 이유:** 결과물 HTML은 CSP `sandbox`로 opaque origin이다. 그 문서의 그림·CSS 요청에는 SameSite=Strict cookie가 붙지 않는다. 그래서 장치 cookie 대신 짧은 읽기 경로를 쓴다. §6.4의 「token을 URL에 넣지 않는다」는 장치 token 규칙이고 그대로 지킨다. `Referrer-Policy: no-referrer`로 경로가 밖으로 새지 않게 한다.
- **보내는 방식:** host 사본이 있으면 그것을, 없으면 agent에서 512KB 조각으로 읽어 그대로 흘려보낸다. host에 캐시하지 않는다. 경로는 library 안 상대 경로만 받고, agent도 `library/` 밖과 symlink를 거부한다. agent 쪽 파일을 읽지 못하면 응답 전에 502와 이유를 돌려준다.
- **결과물 HTML 격리:** CSP `sandbox allow-scripts allow-popups …`(allow-same-origin 없음), `frame-ancestors 'self'`, `X-Frame-Options: SAMEORIGIN`. 웹 화면은 같은 sandbox 속성의 iframe으로 `index.html`을 보인다. 웹 E2E가 결과물 script에서 부모 문서·cookie·제어 API에 닿지 못하는 것을 확인한다.
- **웹 화면:** 목록 위에 끝나지 않은 thread(묶음 표시), 고르면 보고(Markdown을 HTML로 해석하지 않고 그린다)·결과물 iframe·파일 목록(agent에서 읽는 파일 표시)·확인(ack) 버튼. thread 사건이 events WS로 와서 polling 없이 바뀐다.
- **알림 webhook:** host.toml `[notify]`
  - 대상은 inbox 항목이다(종류는 `kinds`로 고른다. 기본: report·question·delivery·thread·session·outage·routine-approval). 알림이 inbox와 같은 사건을 본다.
  - `debounce_secs`(기본 15초) 동안 모아 프로젝트마다 한 통으로 보낸다. 실제 시험에서 한 thread가 몇 초 안에 질문을 서너 번 냈기 때문이다.
  - 실패하면 다시 보내지 않는다. 흐름 사건 `notify_failed`에 이유만 남기고, 실패가 이어지는 동안 경고 로그는 한 번만 낸다. 성공은 `notify_sent`다.
  - 형식: `json`(항목 목록과 글) 또는 `telegram`(Bot API `sendMessage` 본문, `telegram_chat_id`). 텔레그램은 `webhook = "https://api.telegram.org/bot<token>/sendMessage"`로 쓴다.
  - 주소에는 봇 token이 들어갈 수 있다. `host.local.toml`(커밋하지 않는 머신별 파일)에 두고, 로그·흐름 사건·오류 글에는 주소를 넣지 않는다.
  - https만 받는다. 루트 인증서는 webpki 목록을 쓴다(OS 키체인을 읽지 않는다). http는 이 머신 loopback만 허용한다(시험용).
  - HTTP client는 tokio-rustls 위에 직접 쓴 POST 한 번이다. 큰 HTTP crate를 더하지 않았다.
- **웹 이미지 붙여넣기(§9를 이렇게 줄여 구현했다)**
  - 수집: 터미널 영역 붙여넣기(xterm보다 먼저 가로챈다)·입력 막대 붙여넣기·끌어 놓기·「그림」 버튼. 글 붙여넣기는 그대로 xterm으로 간다.
  - 전송: **pane WebSocket(대화형 front)**으로 512KiB 조각을 base64 JSON(`attach`)으로 보낸다. 별도 POST 경로를 두지 않은 이유: 조작 lease가 그 WebSocket 연결(front)에 묶여 있어, 조각마다 같은 권한·lease 판정을 그대로 쓸 수 있다. 조각은 Agent link frame(1MiB) 안에 든다. 메시지 상한도 1MiB로 둔다.
  - host는 모으지 않고 조각마다 agent로 넘긴다(그 사이 화면 출력이 끼어든다). 첫 조각의 signature로 PNG·JPEG·GIF·WebP만 받고(브라우저가 알린 MIME은 믿지 않는다), 이름은 host가 무작위로 정한다. 상한 16MiB, offset이 맞지 않으면 거부한다.
  - 대상 경로: 모든 pane이 **세션 범위** `<agent state>/sessions/<id>/attachments/<hex>.<ext>`다(§9 표의 thread `.aaa/…/attachments` 대신). thread 작업 폴더(worktree)에 두면 그림이 git에 섞일 수 있고, 세션 폴더는 이미 0700이며 세션 기록을 지울 때 함께 지워진다. 폴더 0700·파일 0600, 세션당 64개까지. `.part`에 이어 쓰고 마지막 조각에서 sha256을 대조한 뒤 이름을 바꾼다.
  - 입력: 다 받은 뒤 **다시 확인**한다(실행 중, lease가 이 연결, hook 상태가 있는 harness pane, 열린 질문 없음, 화면에 선택 창 없음). 모두 맞으면 절대 경로를 bracketed paste로 넣고 Enter는 보내지 않는다. 아니면 저장한 경로와 이유만 돌려준다(일반 셸·시작 전·확인창).
  - 실제 확인(Claude Code 2.1, macOS): 넣은 경로가 `[Image #1]`로 바뀌고, 글을 덧붙여 보내자 그림 내용을 읽고 답했다. 신뢰 창이 떠 있을 때(hook 전)는 넣지 않았다. Codex(codex-cli 0.163)도 `[Image #1]`로 받아 그림 색을 답했다(§8.21).
  - 미룬 것: 끊긴 전송의 offset 재개·receipt 조회(지금은 다시 붙여 넣는다), 업로드 중 Send 보류(경로가 들어간 뒤에야 사람이 보낼 수 있다), 보존 기한 정리(지금은 세션 기록을 지울 때). → M1 마무리

### 8.21 M1 마무리 구현 결정(10-16): 머신 건강, coordinator 일 흐름

- **보고:** agent가 `[health] every_s`(기본 60초)마다 재서 `AgentEvent::Health`로 보낸다. host가 `health.v1`을 알렸을 때만 보낸다(이전 host와 호환). 다시 붙으면 마지막 보고를 바로 보낸다.
  - 디스크: 작업 root가 있는 디스크의 여유·전체(`statvfs`). 작업 root가 아직 없으면 있는 위 폴더를 본다.
  - 부하: 1분 평균(`getloadavg`)과 CPU 수. 판정에는 쓰지 않고 보이기만 한다.
  - 전원: macOS `pmset -g batt`, Linux `/sys/class/power_supply`. 배터리 전원은 주의(warning)로만 보인다.
  - toolchain: agent.toml `[health.toolchain]`의 명령(이름 = argv)을 10초 상한으로 실행하고 출력 첫 줄을 버전으로 보인다. agent.toml은 그 머신 사용자가 쓰는 파일이라 profile 명령과 같은 신뢰로 본다.
  - 시계: host가 보고의 agent 시각과 자기 시각의 차이를 잰다. 60초 넘게 어긋나면 이상이다.
  - 계보·첫 실행 지연·키체인은 시작 때 자가 점검(selfcheck)이 이미 보낸다. boot ID·재부팅은 boot fencing이 맡는다.
- **이상(새 세션 거부):** 디스크 여유가 `min_free_gb`(기본 5GB)보다 적다, toolchain 명령 실패, 시계 차이. 연결 중인 머신만 보고(끊김은 `outage`), 첫 보고 전에는 막지 않는다. `pane run`·resume·`thread start`·`restart`가 이유와 함께 거부된다. 이미 도는 세션은 건드리지 않는다.
- **알림:** 이상의 **종류**(`disk`·`toolchain:<이름>`·`clock`)가 생기거나 바뀌거나 풀리면 그 머신에 열린 thread가 있는 프로젝트 inbox에 `health` 항목(`problem`·`ok`)을 남긴다. 값(디스크 여유 GB 등)만 바뀐 것은 새 항목이 아니다(처음에는 설명 글로 비교해 디스크 여유가 0.1GB 바뀔 때마다 항목이 생겼다. E2E가 4번에 1번꼴로 잡았다). 알림 webhook 기본 종류에 `health`를 더했다.
- **보기:** `aaa status`·`aaa machine list`에 이상 이유 또는 디스크 여유·부하·주의, 웹 머신 줄에 「점검 이상」 표시와 이유.
- **남는 한계:** 마지막으로 알린 이상 종류는 host 메모리에만 둔다. host를 다시 시작하면 같은 이상이 이어져도 `problem` 항목이 한 번 더 생긴다.

**coordinator 일 흐름(부록 A의 M1 마무리 항목, 0.2.35 의미)**
- `aaa context <p>`: coordinator가 매 턴 처음 본다. TASKS.md task(목록·owner·thread·메모), 열린 thread와 Next 줄·harness 상태, `start_threads`, 맡길 수 있는 owner, 끝내지 않은 inbox 항목(id·종류·대상·요약, 지시가 아니라 자료). herdr-projects `context`처럼 보인 항목은 「봤다」가 된다(`--peek`이면 그대로).
- **TASKS.md:** aaa는 읽기만 하고 coordinator가 쓴다. `- [ ] 제목 (owner) · t-0001`. owner는 비움·`agent`(미할당), `me`, `profile`·`@machine`·`profile@machine`(agent), 그 밖은 사람이다. 옛 표기 `(agent → t-0007)`도 읽는다. 완료는 줄을 지운다.
- `aaa thread start <p> --from-task "<제목>"`: 제목(대소문자 무시)으로 task를 찾는다. owner가 `--profile`·`--machine` 기본값이 되고 다르면 거부한다. `me`·사람 task, 미할당인데 `--profile`이 없으면 거부한다. 메모는 할 일 끝에 `## Notes from the task list`로 붙는다.
  - 0.2.35와 다른 점: task에 붙은 thread가 아직 열려 있으면 다시 맡기지 않는다(같은 일을 두 thread가 하는 것을 막는다).
- `aaa assignable <p> [--check <owner>]`: 머신마다 host 허용 목록 ∩ 그 머신 agent.toml profile. 끊긴 머신은 「모름」이다. 맨 profile은 host 로컬 머신을 본다(`thread start`의 기본 머신과 같다). 0.2.35의 ssh 조회·1시간 캐시 대신 연결된 agent에 바로 묻는다.
- `aaa thread next <p> <id> [--line N | --add "…"]`: Next 목록은 보고서 `## Next` 줄 + `threads/<id>.next.md`. `--line N`은 그 줄을 메시지 API(대기열, 턴이 끝나면 전달)로 보낸다. 0.2.35는 바로 입력했다.
- **`start_threads`(project.toml, propose·auto, 기본 propose):** coordinator가 따르는 안내 값이다. aaa는 강제하지 않는다(0.2.35와 같다). 동시 실행 상한은 aaa가 강제한다(`max_threads_per_machine`).
- **safety·yolo:** 권한을 건너뛰는 인자는 프로젝트 설정 대신 그 머신 agent.toml의 profile `args`에 둔다. agent.toml은 그 머신 사용자만 고치므로 coordinator가 스스로 켤 수 없다. 0.2.35의 `yolo`처럼 한 번에 켜는 스위치는 두지 않는다.
- **Codex 실측(codex-cli 0.163, macOS, 10-16)**
  - 실행마다 `-c notify=…`를 주면 턴이 끝날 때 `turn_end`(마지막 답)와 `thread-id`가 온다. pane 상태가 입력 대기가 되고, `thread-id`를 harness 세션으로 써서 `thread restart --resume`이 `codex … resume <id>`로 이전 대화를 이었다.
  - `-c notify=…`는 aaa가 띄운 세션에서만 사용자 `~/.codex/config.toml`의 `notify`를 덮는다. 그래서 그 세션에서는 herdr-projects 쪽 notify가 불리지 않는다(의도한 분리다). 다른 Codex 실행은 그대로다.
  - `-c`를 주면 Codex가 공유 background server 없이(embedded) 돈다는 경고를 한 번 낸다. aaa는 daemon 없는 실행을 원하므로 그대로 둔다.
  - 그림: 웹으로 붙여 넣은 경로가 `[Image #1]`이 되고, 보내자 그림 색을 답했다.
  - **글과 Enter를 한 덩어리로 보내면 Codex는 붙여넣기로 보고 Enter를 줄바꿈으로 넣는다(제출되지 않는다).** 웹 입력 막대 「보내기」와 `aaa pane send --text … --enter`가 이렇게 보내고 있었다. 글을 보낸 뒤 300ms 뒤에 Enter를 따로 보내게 고쳤다(host의 brief·메시지 전달은 처음부터 이렇게 했다). 웹 E2E에 이 동작을 흉내 내는 가짜 입력창을 두었다.
- **운영 감독자 설치(결정 14):** `aaa service show|install|uninstall --role host|agent`. 사용자 LaunchAgent(`~/Library/LaunchAgents/dev.aaa.<role>.plist`)가 `aaa-launcher <aaa> <role>`을 띄운다(RunAtLoad·KeepAlive, Aqua 세션, `ProcessType=Interactive`). launchd 기본 PATH로는 `claude`·`codex`를 못 찾으므로 설치하는 셸의 `PATH`·`LANG`을 plist에 적는다. 설치·해제는 launchd를 바꾸므로 사람이 터미널에서 `yes`를 입력해야 하고, aaa 세션 안에서는 거부한다. 순서와 확인은 [운영 설치 점검표](install-runbook.md)다. 이전 SMAppService(`aaa.app`) 경로(`plan`·`register`)는 보류로 남긴다.
- **새 프로젝트 AGENTS.md:** coordinator 안내(매 턴 `aaa context`, thread 시작 규칙, TASKS.md 규칙, 결정·병합 명령)를 쓴다. 있는 파일은 덮지 않는다.

### 8.22 M3 사후 평가자 구현 결정(10-17)

§8.13을 실제 구조에 맞춰 바꿨다. 바꾼 이유는 Front API가 **host 머신의 UDS뿐**이라는 점이다. 다른 머신에서 도는 평가자는 `aaa evaluation submit`을 부를 수 없고 host의 `evaluator/inbox/`도 읽지 못한다. 그래서 입출력을 모두 Agent link로 옮겼다.

- **평가자 = 역할이 `evaluator`인 thread.** `aaa evaluator assign <p> --profile P [--machine M] [--repo R] [--note …]`가 일반 thread와 같은 방식으로 시작한다(작업 폴더·resume·보고 회수·세션 끝남 알림·resolve가 같다). 프로젝트당 하나다.
  - thread ID는 일반 순번(`t-NNNN`)이고 작업 폴더는 `.aaa/<p>-<t>/`다. §8.13의 `.aaa/<project>-evaluator/` 대신이다.
  - 지정 기록은 thread 기록(역할)이 정본이다. host는 project.toml을 쓰지 않는다. `[evaluator]`에는 사용자가 쓰는 설정만 둔다: `events`(대상 사건, 기본 `merge_landed`·`thread_resolved`·`thread_failed`·`thread_restarted`), `batch_s`(기본 300초).
  - 끝내기는 `aaa evaluator unassign <p>`(평가자 thread resolve)다. 동시 thread 한도에 들어간다. 낮은 우선순위는 그 머신 agent.toml profile의 command에 `nice`를 넣어 정한다(aaa가 따로 두지 않는다).
- **목록에 섞지 않는다.** `aaa thread list`·`aaa context`의 thread·`aaa pane list`·웹 thread·머신 목록은 기본으로 평가자를 뺀다. `--role evaluator`로만 보인다. `aaa context`에는 따로 `evaluator` 요약이 있다.
- **입력.** 대상 사건이 생기면 착지는 바로, 나머지는 `batch_s` 뒤에 host가 cursor 뒤 사건 묶음을 만든다.
  - 묶음은 Agent link(`WorkFileWrite`)로 평가자 작업 폴더 `inbox/<첫 seq>-<끝 seq>.json`에 쓴다. agent는 이 이름 형식 말고는 쓰지 않는다.
  - 묶음에는 이번 사건, 관련 thread마다 **전체 사건 이력·시간 요약·host가 받은 마지막 보고 본문(64KiB까지)**을 넣는다. 원격 평가자가 host 파일 없이도 단계별 시간을 계산할 수 있어야 해서다.
  - 평가자 자신의 사건은 넣지 않고 대상 사건으로도 보지 않는다(평가 → 사건 → 묶음 → 평가로 도는 고리를 막는다).
  - 묶음 하나는 사건 200건까지이고 JSON 512KiB까지다(Agent link frame 1MiB에 base64로 실려야 한다). 넘으면 보고 본문을 8KiB로, 다음은 빼고, 다음은 thread 이력을 마지막 20건으로 줄이며 줄인 까닭을 묶음에 적는다. 사건이 200건으로 찼으면 대상 사건이 없어도 보내고 바로 다음 묶음으로 잇는다. 그래서 큰 묶음 하나가 전달을 영영 막지 않는다.
  - thread 기록은 JSON 한 덩어리다. 읽은 뒤 agent 응답을 기다린 쪽(보고 사본·다시 시작·resolve·묶음)은 저장 직전에 다시 읽어 자기 필드만 고친다(`update_rec`). 그래야 기다리는 동안 옮긴 cursor를 옛 값으로 덮지 않는다.
  - cursor는 thread 기록에 durable하게 둔다. 쓴 뒤 옮기고, 메시지 대기열로 「새 사건 n건」 한 줄을 보낸다. 받았는지 몰라도 다시 보내지 않는다. 평가자는 `inbox/`에서 처리하지 않은 파일을 다시 읽는다(§8.5 a). 머신이 끊겨 쓰지 못하면 cursor를 그대로 두고, 다시 연결될 때 확인한다.
- **출력.** 평가자는 `library/evaluations/<이름>.json`(같은 이름의 `.md`는 선택)을 쓰고 report.md를 다시 쓴다. host는 보고 사본을 받은 뒤 처음 보는 기록을 검사해 받는다.
  - 받은 기록은 DB와 `projects/<p>/evaluations/<이름>.md`에 둔다. inbox에는 `evaluation` 항목으로 뜬다.
  - 형식이 틀리거나 이미 받은 이름의 내용이 바뀌었으면 받지 않는다. 같은 내용당 한 번 `evaluation`·`invalid` 항목으로 알린다.
  - 평가자의 보고 자체는 `report` 항목으로 알리지 않는다.
- **채택·보류.** `aaa evaluation adopt|hold <p> <id> <개선안…> [--note]`. 채택은 결정 기록(§8.6)에 한 줄로 남기고 그 번호를 붙인다. 보류한 것은 나중에 채택할 수 있고, 채택한 것은 바꾸지 않는다. 모두 정하면 inbox 항목이 끝난다.
- **금지(부분 thread 범위 token).** CLI는 aaa 세션 안이면 `AAA_SESSION_ID`를 Hello에 담는다. 그 세션의 역할이 평가자면 host가 그 연결을 **보기 권한(Reader)**으로 다룬다. 그래서 다른 thread에 대한 메시지·시작·질문 응답·pane 입력·채택이 거부된다.
  - 남는 한계: env를 지운 프로세스는 사람의 CLI와 구별되지 않는다(routine 승인과 같은 수준, §8.21). 원격 머신의 평가자는 Front API에 닿지 않는다.
- **표시.** 웹 header의 「평가 ● n」 badge(n = 정하지 않은 개선안, 점 색 = 평가 중·대기·세션 끝남)를 누르면 평가자 pane이 열린다. TUI 상태 줄에는 `평가 ● <p> <상태>`가 보인다. `aaa evaluator open <p>`는 평가자 pane을 workspace `eval-<p>`에 두고 TUI로 연다. 나가도 평가자는 배경에서 계속 돈다. `aaa evaluator status`는 세션이 끝난 평가자를 「세션 끝남」과 다시 띄우는 명령으로 보인다.
- **시험.** m13 `evaluator_flow`: 가짜 평가자가 착지·완료 묶음을 받아 평가 기록(정상 1, 잘못된 1)을 낸다. 확인하는 것은 다음과 같다.
  - 목록·context·pane 목록에 섞이지 않는다.
  - 평가자 세션의 지시·채택은 거부되고 보기는 된다.
  - 묶음에 평가자 사건이 없고, seq가 겹치지 않고, 이력·보고 본문을 담는다.
  - 채택이 결정 기록에 남고, 두 번 채택은 거부된다. 보류까지 하면 inbox가 정리된다.
  - 평가자를 끝내고 다시 띄워도 앞 사건을 다시 넘기지 않는다.
  - 단위 시험: 기록 검사(요약 필수, 개선안 id 중복·형식, 소수 분 반올림, 평가자가 쓴 decision 무시), thread 시간 요약.

### 8.23 UI 다듬기(결정 16, 10-17)

웹(데스크톱·휴대폰)과 TUI의 생김새를 참고 도구(herdr-web-ui·herdr TUI·herdr-mobile-relay)와 나란히 띄워 비교하고 다시 짰다. 참고 도구의 코드는 가져오지 않았고 화면 구성 원칙만 참고했다. 색과 아이콘은 aaa가 따로 정했다.

- **색 token.** `web/src/style.css` `:root`에 둔다.
  - 어두움은 차가운 흑연(`--bg #0f1115`, 단계 면 `--bg-panel`·`--bg-elevated`·`--bg-hover`·`--bg-active`), 밝음은 따뜻한 종이색(`#f8f8f6`)이다.
  - 강조색은 iris 하나(어두움 `#9d8fff`, 밝음 `#5b4bd6`)다. 고른 것·주 버튼·초점에만 쓴다.
  - 상태색은 작업 중 파랑·막힘(질문·사람 차례) 빨강·완료 초록·주의 호박이다. 늘 글과 함께 보인다.
  - 터미널(xterm) 색도 같은 token에서 읽는다. 테마마다 ANSI 16색이 따로 있다.
- **테마.** 기본은 시스템 설정을 따른다. header의 테마 단추로 시스템 → 밝음/어두움을 고르며, 고른 것은 그 기기에만 남는다(`localStorage aaa:theme`, `<html data-theme>`).
- **글꼴·간격.**
  - UI 글꼴은 Pretendard(설치돼 있으면)와 시스템 한글 글꼴, 코드 글꼴은 JetBrains Mono·SF Mono·Menlo다. 외부 글꼴은 받지 않는다(tailnet 안 화면이고 공개 저장소다).
  - 간격은 4px 단위, 모서리는 6/8/12/16px, 움직임은 120/180ms다. 「움직임 줄이기」 설정이면 숨쉬는 점을 멈춘다.
- **화면.**
  - sidebar 320px에 질문(답을 기다림) → Thread → 머신별 pane 순으로 둔다. 한 줄은 앞 표시(harness·thread 아이콘) + 제목/부제 두 줄 + 뒤 상태 글이다.
  - thread pane은 thread 제목으로 보인다. 평소 상태인 「실행」은 적지 않고, 끝났거나 끊긴 것만 적는다.
  - 질문 카드는 「Bash {json}」을 도구 이름과 명령 상자로 풀어 보인다. 터미널 아래(입력 위)에도 번호 붙은 선택지로 붙는다.
  - 입력은 바로 입력이나 입력줄(그림·글·보내기, §8.44)과 키 막대(§8.43)다. thread 보기는 가운데 860px 문서 모양(제목·meta·보고 카드·결과물 카드)이다.
  - 휴대폰은 목록과 보기를 화면 전체로 바꿔 보인다. 줄 높이는 56px, 키 줄은 40px 높이, header는 반투명이다. 터치 기기에서는 hover 색을 쓰지 않는다.
  - **입력 방식(§8.44, herdr-web-ui terminalInputMode, 결정 20 D1).** 기본은 바로 입력(터미널에 직접 친다)이다. 입력줄은 기기마다 고른다: 터치 기기는 키 막대 맨 앞 ⌨로 바꾼다. 데스크톱에서 입력줄을 고르는 설정 화면은 아직 없다.
    - 입력줄은 터미널 마지막 줄 위에 떠서 4줄까지 자란다. 흐름에는 한 줄 칸만 두어 자라도 PTY 크기를 바꾸지 않는다.
    - 모든 기기에서 Enter가 보내기, Shift+Enter가 줄바꿈이다. 한글 등 IME 조합 중의 Enter는 글자 확정만 한다(`isComposing`·keyCode 229). 빈 줄 보내기는 Enter만 누른다(유지 키가 붙는다).
    - 여러 줄은 터미널 붙여넣기로 보낸다. 프로그램이 bracketed paste를 켰으면 감싸고, 줄바꿈은 CR로 바꾼다. 그래서 Claude·Codex 입력창이 줄마다 제출하지 않는다. Enter는 따로 조금 뒤에 보낸다.
    - 쓰다 만 글은 머신·pane마다 이 기기(localStorage, 32,768자 상한)에 남는다. 보내는 동안 친 글은 남는다.
    - 연결이 끊겨도 입력줄은 쓸 수 있고 보내기만 막힌다.
  - 키 막대(§8.43, herdr-web-ui KeyBar): Esc·Tab·^C·Ctrl·Alt·Shift·Enter·↑↓←→. 터치 기기나 폭 768px 이하에서만 보인다. Ctrl·Alt·Shift는 유지 키다. 다시 누르거나 연결이 끊길 때까지 켜져 있다(래치). 휴대폰에서는 가로로 밀어 본다.
- **TUI.** 칸 크기는 그대로 둔다. 테두리를 두르지 않아 PTY 크기가 줄지 않는다.
  - 탭 줄은 이름표·workspace·탭(지금 탭은 강조색 바탕)으로 짜고, 오른쪽에 열린 질문 수를 둔다.
  - 포커스 pane과 맞닿은 구분선만 강조색으로 그린다.
  - 상태 줄은 색 조각(머신·pane·상태색 글·조작 중·평가자)으로 짜고, 오른쪽에 prefix 도움말을 둔다.
  - pane 표시는 상태 색(질문 빨강, 끊김·중단 호박, 종료 회색)을 따른다.
  - 색은 256색 번호이고, 색 바탕 위 글자는 굵게 써도 밝아지지 않게 256색 검정(16)을 쓴다.
- **견본 도구.** `web/e2e/showcase.mjs`는 host·agent 둘(로컬·원격 등록)과 셸·가짜 Claude(작업 중·입력 대기·질문)·thread 셋·평가자가 있는 상태를 만든다. 웹 화면 8가지(데스크톱 1920×1080·휴대폰 390×844 × 어두움·밝음)와 TUI(`aaa attach`를 웹 터미널로)를 찍는다. 디자인 검수와 전후 비교에 쓴다. `AAA_BIN`으로 옛 binary를 줄 수 있다.
- **PWA(M3, 10-17).** 휴대폰 홈 화면에 앱으로 둔다.
  - `web/public/manifest.webmanifest`(standalone, 시작 `/`)와 아이콘(192·512·maskable·apple-touch·favicon PNG)을 둔다. 아이콘은 견본 도구처럼 headless Chromium으로 한 번 그려 저장소에 넣었다.
  - manifest와 아이콘은 인증 없이 받는다(브라우저가 cookie 없이 가져간다). 비밀이 없다.
  - Chrome·Android가 설치 신호를 주면 목록 맨 아래에 「홈 화면에 설치」가 보인다. iOS는 공유 → 「홈 화면에 추가」다.
  - service worker(`/sw.js`)는 Web Push와 함께 넣었다(§8.26). 앱을 열 때마다 등록한다. 화면·자산을 캐시하지 않아 업그레이드 뒤 옛 화면이 남지 않는다. host에 닿지 않을 때 화면 이동만 「연결 없음」 화면(`/offline.html`)으로 바꾼다.
- **검수(결정 17).** 사용자 검수를 통과했다. 나중 후보(이번에 하지 않음): 휴대폰 drawer, 명령 팔레트, 머신 묶음 접기, TUI 왼쪽 목록, thread 줄 하나로 합치기.
- **남은 한계.** 한글 폭이 두 칸에 차지 않는 글꼴이면 터미널 한글 사이가 벌어져 보인다. 고정폭 한글 글꼴(D2Coding 등)이 있는 기기에서는 이 문제가 없다.

### 8.24 M3 병합 단계 자동 실행(10-17)

§8.8의 단계를 `aaa merge land <p> [<thread>]` 한 번으로 돈다. git은 그 thread 머신의 agent가 실행하고, host는 순번·사건·알림만 맡는다.

- **조건:** 대기열 맨 앞이고(`thread`를 주면 그것이 맨 앞이어야 한다), 열린 worktree thread이며, 기준이 `<원격>/<branch>`(예: `origin/main`)여야 한다.
  - 한 프로젝트에서 동시에 하나만 돈다.
  - 이 머신의 CLI(host UDS)에서만 부른다. 웹 controller에서는 부를 수 없다. main에 push하는 일이라 기기 token이 새어도 못 하게 했다.
  - 평가자 세션은 보기 권한이라 부를 수 없다.
- **단계(agent)**
  1. **확인:** thread worktree에 커밋하지 않은 추적 파일 변경이 없다.
  2. **fetch:** 원격을 받는다.
  3. **원격 일치:** branch가 `origin/main`을 포함하고 새 커밋이 있다. 포함하지 않으면 멈추고 「thread에서 `git merge origin/main`으로 합치고 시험한 뒤 다시」를 돌려준다.
  4. **병합:** 임시 detached worktree(`origin/main`, 시스템 임시 폴더)에서 `merge --no-ff -m "병합: <제목> (<p>/<t>)"`를 한다. 이름·이메일은 사용자 repo 설정을 따른다.
  5. **tree 같음:** 병합 tree가 branch tree와 같은지 본다.
     - branch가 `origin/main`을 포함하므로 같아야 정상이다. 같으면 thread가 시험한 내용 그대로다.
     - 그래서 **병합 뒤 시험 명령을 다시 돌리지 않는다.** 처음 초안의 「승인된 check 명령」은 넣지 않았다. 명령 실행 승인이라는 보안 경계를 하나 줄인다.
  6. **push:** `git push origin HEAD:refs/heads/main`(force 없음). 그 사이 원격이 움직였으면 거부되고 멈춘다.
  7. **사용자 checkout:** repo가 지금 main이고 고친 추적 파일이 없으면 `merge --ff-only origin/main`으로 맞춘다. 아니면 그대로 두고 까닭을 적는다. 실패로 치지 않는다.
  8. **정리:** 임시 worktree를 지운다. aaa가 만든 것이라 force로 지운다. 실패해도 이 단계는 돈다.
  - git은 `GIT_TERMINAL_PROMPT=0`으로 돌린다. 인증을 묻는 창이 뜨면 멈추지 않고 실패한다.
- **host**
  - 단계마다 `merge_step` 사건(시작 + 단계별 ok·실패와 첫 줄)을 낸다.
  - push까지 됐으면(병합 commit이 있으면) `merge_landed`로 대기열에서 빼고 `merge_head`를 낸다. 사후 평가자가 이 사건을 받는다.
  - 같은 프로젝트에서 같은 기준을 쓰는 다른 열린 worktree thread에 「main 이동」을 대기열 메시지로 보낸다. 「`git fetch && git merge origin/main`으로 합치고 다시 시험」이라는 내용이다.
  - 멈추면 대기열을 그대로 두고 단계 결과를 돌려준다. CLI는 종료 코드 1로 끝난다.
- **미룬 것**
  - thread worktree·branch 정리는 지금처럼 `thread resolve`가 한다. 병합 뒤라 branch도 지워진다. land가 thread를 끝내지 않는 것은 thread가 보고를 마무리할 수 있게 하려는 것이다.
  - 유휴 머신 사전 빌드 훅은 미룬다.
  - 맨 앞이 되면 자동으로 land하는 설정도 미룬다. 지금은 coordinator가 `merge_head` 사건을 보고 부른다.
- **시험:** m13 `merge_land_steps`. 확인하는 것은 다음과 같다.
  - 순번 거부
  - 커밋하지 않은 변경 → 확인 단계에서 멈춤
  - 그 사이 main이 앞서 나감 → 원격 일치에서 멈추고 대기열 유지
  - 합친 뒤 land: 8단계 ok, 원격 main = 병합 commit(부모 둘, 메시지), 병합 tree = branch tree, 사용자 main ff-only, 임시 worktree 없음, 「main 이동」 알림, 대기열 다음, `merge_step` 9건·`merge_landed`·`merge_head`
  - 새 커밋 없는 thread는 멈춤

### 8.25 M3 thread 범위(10-17)

§6의 「agent pane 안의 CLI 범위 token」을 세션 역할로 정했다.

- CLI는 aaa 세션 안이면 `AAA_SESSION_ID`를 Hello에 담는다. host는 그 세션 pane의 역할을 본다.
  - **일반 thread(worker)와 사후 평가자:** 그 연결을 **보기 권한(Reader)**으로 다룬다. 보기(`thread show`·`context`·`events` 등)는 되고, 메시지·시작·결정·병합·pane 입력·질문 응답은 거부된다. host 머신 전용 요청(`merge land`·`routine approve`·`web pair` 등)도 거부된다.
  - **coordinator와 thread가 아닌 pane(사람의 셸):** 그대로 조작한다.
  - 감사 기록의 actor는 `worker:<세션>`·`evaluator:<세션>`이다.
- thread는 진행률을 Front API가 아닌 hook(`aaa report`)으로 알리므로 이 제한에 걸리지 않는다. thread 규칙(brief)에 「이 세션 안의 `aaa` CLI는 보기만 된다」를 적었다.
- **남는 한계(§8.22와 같다):** env를 지운 프로세스는 사람의 CLI와 구별되지 않는다. 같은 사용자 권한으로 도는 agent를 완전히 막는 것은 OS 사용자 분리가 필요하다. 지금은 실수로 다른 thread에 지시하는 일을 막는 수준이다.
- **시험:** m13 `worker_session_cli_is_read_only`. worker 세션의 결정·메시지·pane 입력·병합 대기열은 거부되고 보기는 된다. 사람의 셸 pane 안에서는 결정이 된다.

### 8.26 M3 Web Push와 PWA 동작(10-17)

질문·보고 준비 같은 inbox 알림을 휴대폰·데스크톱 브라우저 알림으로 받는다. 웹은 홈 화면 앱(PWA)으로도 동작한다.

- **켜기.** 웹 목록 맨 아래 「알림 켜기」를 누른다. 브라우저 알림 권한을 묻고 구독을 만들어 host에 맡긴 뒤 시험 알림을 보낸다. 보기 권한 기기도 알림은 받는다. 「알림 끄기」는 구독을 지운다.
  - 단추는 브라우저가 Web Push를 할 수 있고 host 설정이 켜져 있을 때만 보인다. 권한을 막았으면 「알림이 브라우저 설정에서 막혀 있다」로 보인다.
  - iPhone·iPad는 홈 화면 앱(공유 → 「홈 화면에 추가」, iOS 16.4 이상)에서만 켤 수 있다. Safari 탭에서는 그 안내를 보인다. 홈 화면 앱은 Safari와 cookie가 따로라 처음 열 때 다시 pairing한다.
- **보내기.** webhook과 같은 대기열(`notify.kinds`, `debounce_secs`)을 쓴다. 프로젝트마다 한 통이고, 한 thread 항목만이면 누를 때 그 thread 보기(`/#thread=<p>/<t>`)로 연다. 열린 창이 있으면 그 창으로 간다.
- **보안.**
  - 내용은 기기 키로 암호화한다(RFC 8291, aes128gcm). 중간의 push 서비스(Google·Apple·Mozilla·Microsoft)는 읽지 못한다.
  - host는 VAPID(RFC 8292, ES256) 키를 상태 폴더 `push/vapid.p8`(0600)에 한 번 만들고 요청마다 서명한다. 새 의존성 없이 rustls가 이미 쓰는 ring으로 구현했다.
  - 구독 주소는 기기가 준다. host가 아무 주소로나 요청하지 않게 `notify.push_hosts`(fcm.googleapis.com·push.apple.com·push.services.mozilla.com·notify.windows.com과 그 아래 이름)의 https만 받는다. loopback http는 시험용으로 목록에 적었을 때만 받는다.
  - 구독 주소는 그 기기에 보내는 열쇠라 로그·흐름 사건·감사 기록에 넣지 않는다. 취소한 장치의 구독에는 보내지 않는다. 404·410이면 브라우저가 버린 구독이라 지운다. 다른 실패는 다시 보내지 않는다(webhook과 같다).
- **설정(`[notify]`).** `web_push = true`(기본), `push_hosts`, `push_subject`(비우면 공개 주소 `https://<이름>.ts.net`, 없으면 `mailto:aaa@example.com`).
- **service worker.** 설치·Web Push 알림·알림 누르기를 맡는다. 캐시는 「연결 없음」 화면 하나뿐이다.
- **API.** `GET /api/v1/push`(키·구독 수), `POST /api/v1/push/subscribe`·`unsubscribe`·`test`. 장치 cookie와 Origin 검사는 다른 API와 같다.
- **시험(웹 E2E).**
  - PWA: Chromium이 설치할 수 있다고 본다(`Page.getInstallabilityErrors` 빈 목록). service worker가 화면을 맡는다. host를 내린 동안 새로 열면 「연결 없음」 화면이 나온다.
  - Web Push: 가짜 push 서비스가 host 요청을 받는다. 본문은 Node crypto로 따로 구현한 RFC 8291 복호화로 풀고, VAPID JWT는 공개키로 검증한다(aud·exp·sub).
  - 허용하지 않은 주소·목록에 없는 loopback 이름·틀린 키는 거부한다. 해제하면 지워진다. 보고 준비 알림은 그 thread 주소로 온다. 410 구독은 지워진다. 흐름 사건에 구독 주소가 없다.
  - 알림 표시: 정식 Chromium(새 headless)에서 CDP로 push를 넣으면 service worker가 알림을 띄운다. headless shell은 알림을 지원하지 않는다.
- **남는 한계.**
  - 실제 push 서비스(FCM·Apple)로 가는 왕복은 자동 시험에 없다. headless 브라우저는 실제 구독을 만들지 못한다. 운영 휴대폰에서 「알림 켜기」의 시험 알림으로 확인한다.
  - **고친 것(10-17 shadow에서 찾음).** Apple push 서버는 `Connection: close`를 보내도 응답 뒤 연결을 닫지 않는다(실측: 0.75초에 응답, 15초 넘게 열림. FCM은 바로 닫는다). 그런데 보낼 때 연결이 닫힐 때까지 읽었다. 그래서 Apple로 간 알림은 전달돼도 매번 10초 timeout 「실패」로 기록됐다. 410(버려진 구독) 판정도 하지 못했다.
    - 이제 응답 머리(빈 줄까지)만 읽어 상태 코드를 쓴다. webhook도 같은 함수를 쓴다.
    - 시험: 응답 뒤 연결을 열어 두는 가짜 서버로 유닛 시험을 둔다. 고치기 전 코드에서는 실패한다. 네트워크 시험(`--ignored`)에는 실제 Apple 서버에 잘못된 경로로 보내 5초 안에 4xx를 받는지 넣었다.
  - 알림을 눌렀을 때 thread로 가는 것은 자동 시험이 없다(CDP로 알림을 누를 수 없다).

### 8.27 M3 chat read-only(10-17)

Claude·Codex pane을 터미널 대신 대화 기록으로도 본다. 휴대폰에서 긴 작업의 흐름을 읽기 쉽게 하려는 것이다. 보기만 하고, 입력은 지금처럼 터미널로 간다.

- **보기.** 웹 터미널 머리의 「대화」 단추(harness가 claude·codex인 pane만)로 바꾼다. 보이는 동안 3초마다 다시 읽고, 맨 아래를 보고 있으면 따라 내려간다. CLI는 `aaa pane chat <pane>`이다. 보기 권한 기기·worker 세션도 볼 수 있다(보이는 범위는 pane 보기와 같다).
- **어디서 읽나.** agent가 그 머신의 기록 파일을 읽는다.
  - Claude: hook 사건의 `transcript_path`. host는 pane에만 기억하고 웹·로그·흐름 사건으로 내보내지 않는다(그 머신의 홈 경로다).
  - Codex: notify의 thread-id로 `sessions/YYYY/MM/DD/rollout-…-<thread-id>.jsonl`을 찾는다.
- **무엇을 보이나.** 사람이 쓴 글, harness의 답, 도구 호출 한 줄(`Bash: cargo test`)이다. 생각(thinking)·도구 결과 본문·하위 agent(sidechain)·시스템이 끼운 글(`<system-reminder>` 등)은 뺀다. 터미널보다 많이 보이지 않게 하려는 것이다. 끝에서 150줄, 한 줄 4000자까지다.
- **보안.** hook은 세션 안에서 누구나 보낼 수 있어 경로를 믿지 않는다.
  - agent 자기 환경으로 정한 기록 폴더(`$CLAUDE_CONFIG_DIR`|`~/.claude`의 `projects/`, `$CODEX_HOME`|`~/.codex`의 `sessions/`) 안의 일반 `.jsonl` 파일만 읽는다. symlink는 실제 위치로 풀어 보고, FIFO·폴더는 거부한다.
  - Codex thread-id는 형식(36자 hex·`-`)을 먼저 본다.
  - 오류 글에 경로를 넣지 않는다.
- **큰 기록.** Claude 기록에는 요청 원문 같은 큰 줄이 많다. 뒤에서부터 1MiB씩 읽어 필요한 만큼만 고르고 16MiB에서 멈춘다.
- **호환.** hook 사건의 `transcript`는 없어도 되는 필드다. 옛 host는 무시하고, 옛 agent는 보내지 않는다(그때는 「아직 기록 위치를 알리지 않았다」).
- **시험.**
  - agent 유닛: 합성 기록으로 거르기(3MiB 줄·sidechain·thinking·system-reminder·tool_result)와 끝부분 자르기, Codex 줄, 경로(밖·상대·`..`·확장자·밖을 가리키는 symlink·폴더·FIFO), thread-id를 본다.
  - 웹 E2E: 실험실 안 `CLAUDE_CONFIG_DIR`에 기록을 두고 hook으로 알린다. 사람·답·도구 줄만 보이고, 빼야 할 글은 없다. 새 줄을 따라가고 터미널로 돌아간다. CLI도 같다. 기록 폴더 밖을 가리키는 hook은 「기록 폴더 밖」으로 거부하고 내용을 내보내지 않는다.
- **남는 한계.** Claude·Codex 기록 형식은 공개 규격이 아니다. 바뀌면 모르는 줄은 조용히 빠진다(오류로 멈추지 않는다). 실제 Codex 기록 왕복은 사용량 때문에 자동 시험이 없다.

### 8.28 M3 TUI Ctrl+V와 front clipboard(10-17)

로드맵의 「Windows·Linux front clipboard」는 TUI의 Ctrl+V 그림 붙여넣기를 뜻한다. 그런데 TUI에는 macOS를 포함해 clipboard 읽기가 아예 없었다(M1-4에서 미룸, §8.20). 그래서 세 OS를 한 번에 만든다.

- **누르는 것.** `aaa attach`에서 prefix 없이 Ctrl+V를 누른다. 그림만 든 clipboard를 붙이면 빈 붙여넣기(bracketed paste)를 보내는 터미널이 있어 그것도 받는다.
  - clipboard에 그림이 없거나 clipboard를 열지 못하면(ssh 셸·desktop 없음) 원래 키 0x16을 그대로 보낸다. 이때 상태 줄에는 아무것도 띄우지 않는다. Claude Code 같은 harness가 Ctrl+V를 직접 쓰기 때문이다.
- **읽는 것.** arboard 3.7을 쓴다.
  - macOS는 NSPasteboard, Windows는 clipboard(PNG·CF_DIBV5·CF_DIB), Linux는 X11이다.
  - 순수 Rust(X11은 x11rb)다. 새로 들어온 crate 가운데 C 라이브러리를 링크하는 것(`links`)은 없다. 그래서 Linux 빌드에 X11·Wayland 개발 파일이 필요 없다. §8.20에서 걱정한 점은 생기지 않았다.
  - arboard는 RGBA를 돌려준다. 그래서 front가 PNG로 다시 만든다. 원래 형식과 색 profile은 남지 않는다.
- **보내는 길.** Front API에 `PaneAttach{pane, offset, total, sha256, data}` 조각 요청을 더했다. 응답은 `Attached{path, inserted, note}`다. 웹 터미널 WebSocket의 `Attach`와 규칙이 같다.
  - 512KiB 조각(`ATTACH_CHUNK`)으로 나누고, 마지막 조각에서 sha256을 대조한다.
  - host의 `attach_chunk`가 그대로 처리한다. 그 안에서 서명으로 형식을 보고(PNG·JPEG·GIF·WebP), 16MiB 상한과 lease를 확인한다. harness 입력창이면 경로를 넣고(Enter 없음), 아니면 저장만 한다.
  - 받는 중인 첨부는 front 연결마다 하나다. 키 입력과 같은 순서 큐로 처리해 조각과 키가 섞이지 않는다.
  - 앞 조각이 실패하면 남은 조각은 거부한다. TUI는 그 거부를 한 번만 알린다.
- **raw ssh 셸.** 사용자 PC의 clipboard가 없다. 그래서 `aaa pane attach <pane> <파일>`로 파일을 올린다. 같은 요청을 쓴다.
- **시험.**
  - 유닛: PNG 만들기와 풀기 왕복, 크기 불일치 거부.
  - TUI E2E(`tui.rs`): 다음을 본다.
    - 그림이 없으면 Ctrl+V가 원래 키다(lnext를 끈 `cat -v`가 `^V`를 보인다).
    - 그림이 있으면 대상 세션 폴더에 PNG가 생기고, 풀어 본 pixel이 같다. 셸 pane에는 경로가 들어가지 않는다.
    - `aaa pane attach`: 1.2MB 여러 조각이 bytes 그대로 저장된다. 그림이 아니면 거부한다. ~~TUI가 조작 중이면 lease로 거부한다~~ → 10-17부터 한 번 보내는 CLI는 지금 쓰는 화면을 바꾸지 않고 그대로 저장한다(§8.33).
  - **clipboard 바꿔치기.** 개발 Mac에서 사람의 clipboard를 덮어쓰지 않으려고, E2E는 `AAA_TEST_CLIPBOARD_PNG` 파일을 clipboard 대신 읽는다. 실제 clipboard는 CI 단계 「clipboard(실제)」가 `AAA_CLIP_NATIVE=1`로 확인한다.
    - 세 OS 모두 arboard로 시스템 clipboard에 그림을 쓰고 읽는다. Linux는 Xvfb 위에서 한다.
    - macOS·Linux는 같은 TUI E2E를 실제 clipboard로 한 번 더 돈다.
    - 이 값을 켰는데 clipboard를 열지 못하면 실패다(건너뛰지 않는다).
- **남는 한계.**
  - Windows의 TUI E2E는 없다. `tui.rs`가 Unix PTY 전용이라 Windows는 clipboard 읽기 왕복만 본다.
  - Wayland 전용 세션(XWayland 없음)은 읽지 못한다. arboard의 Wayland 기능을 켜지 않았다. 그때는 Ctrl+V가 원래 키로 가고, 파일 첨부 명령이나 웹을 쓴다.
  - 외부 터미널이 그림 파일 경로를 글로 붙이는 경우(§9 「입력 호환」)는 아직 올리지 않는다. 경로를 일부러 붙인 것과 구분할 수 없어 글 그대로 보낸다.
  - 실제 Claude·Codex 입력창에 넣는 길은 웹과 같은 host 코드다. 웹 E2E의 harness pane 단계가 그것을 본다.

### 8.29 다시 띄워도 harness 상태 유지(10-17, 운영 업그레이드에서 찾음)

- **찾은 것.** 세 Mac의 감독자를 다시 띄우는 업그레이드(점검표 9)에서 일이 있었다. 운영 host Mac은 host와 agent를 함께 다시 띄웠는데, 그 뒤 쉬고 있던 shadow Claude pane의 harness 상태가 비었다. 상태 badge가 없어졌고, 「대화」 보기도 「Claude·Codex pane만」으로 거부됐다. 세션 자체는 이어졌다.
- **원인.** harness 상태는 hook 사건으로만 생긴다. host가 다시 뜨면 agent가 세션별 마지막 사건을 다시 알려 주는데, agent는 그 사건을 메모리에만 두었다. 그래서 agent도 같이 다시 뜨면 다음 hook이 올 때까지 상태가 없다. 쉬는 harness에는 다음 hook이 오지 않는다.
  - 또 하나: 다시 알리는 사건이 세션마다 마지막 하나뿐이었다. Claude Stop 뒤에 진행 보고(`aaa report`)가 오면, 다시 알릴 때 Claude 상태와 기록 위치가 빠졌다.
- **고친 것.** agent가 세션마다 두 칸을 기억한다. harness(Claude·Codex) 마지막 사건 하나와 진행 보고 마지막 사건 하나다.
  - 같은 harness의 다음 사건에 기록 위치나 harness 세션 ID가 없으면 앞 사건 것을 이어받는다.
  - 두 칸을 세션 폴더 `harness.json`(0600)에 적는다. agent가 다시 뜨면 세션을 다시 붙일 때 읽고, host에 붙으면 시각 순으로 다시 알린다. 세션을 지우면 함께 지워진다.
  - 기록 경로는 그 머신의 세션 폴더에만 남는다(§8.27의 「host는 내보내지 않는다」와 같다). host DB에 두는 방법도 해 봤지만 버렸다. agent가 이미 다시 알리는 구조가 있어서, 그것을 디스크에 남기는 쪽이 고칠 곳이 하나다.
- **시험.** `host_restart_keeps_sessions_and_disconnect_is_readonly` E2E에 단계를 더했다.
  - Stop hook(기록 위치 포함)과 진행 보고를 보낸 쉬는 Claude pane을 만든다.
  - host만 다시 띄웠을 때, 그리고 host와 agent를 함께 다시 띄웠을 때 모두 같은 harness 상태(때·진행 포함)와 대화 보기가 돌아오는지 본다.
  - 다음 두 경우에 이 시험이 실패하는 것을 확인했다.
    - agent가 `harness.json`을 읽지 않을 때
    - 진행 보고가 harness 칸을 덮을 때
- **남는 한계.**
  - 내려가 있는 동안 harness가 상태를 바꾸면, 다음 hook까지는 마지막으로 알던 상태다. 감독자가 내려가 있는 동안 온 hook은 다시 보낼 곳이 없다.
  - 이 수정 전에 시작한 세션에는 `harness.json`이 없다. 그래서 그 세션은 다음 hook이 와야 상태가 생긴다.

### 8.30 웹 터미널 크기와 글꼴(10-17, 사용자 보고)

사용자 보고는 이렇다. 넓은 데스크톱 창에서 thread pane 터미널이 header에 `120×32`로 보이고, 창 왼쪽 위 일부만 차지했다. 한글은 글자 사이가 벌어져 보였다.

- **(1) PTY가 창을 따라가지 않은 이유.**
  - 웹만 수동이었다. TUI는 이 화면이 lease를 가졌거나 lease가 비었으면 칸 크기로 맞춘다(§4.5). 웹은 M1-1부터 PTY 크기를 그대로 보이고 「화면에 맞추기」를 눌렀을 때만 바꿨다.
  - 그래서 thread가 시작할 때 host가 준 기본 크기 120×32가 남았다.
  - 그 화면은 「제어 권한」이었고 다른 front가 조작 중이지도 않았다. 권한이나 크기 공유 때문이 아니다.
- **고친 것.** 웹도 TUI와 같은 규칙으로 자동으로 맞춘다.
  - 이 화면이 조작 중이거나 아무도 조작하지 않으면 PTY를 칸 크기로 바꾼다. 처음 열 때, lease가 바뀔 때, 창 크기가 바뀔 때(150ms 묶음) 한다. 같은 크기면 보내지 않는다.
  - lease가 비어 있으면 크기를 보낸 이 연결이 lease를 갖는다(host 규칙 그대로).
  - 다른 front가 조작 중이거나, 보기 권한이거나, 끊겼거나, 대화 보기 중이면 바꾸지 않는다. 그때는 PTY 크기 그대로 왼쪽 위에 그린다. 「화면에 맞추기」 단추는 남긴다.
- **함께 고친 것: 맞춘 크기가 몇 칸 컸다.**
  - FitAddon은 터미널 부모(`.term-box`)의 계산된 width·height를 칸 크기로 쓴다. 그런데 `box-sizing: border-box`에서 부모의 padding이 그 값에 들어갔다.
  - 그래서 1280×800에서 칸은 118열인데 120열로 잡아 글자가 칸 밖으로 넘쳤다. 수동 「화면에 맞추기」도 같았다.
  - 여백을 바깥 `.term-scroll`로 옮겼다.
- **(2) 글꼴·cell 크기.** 데스크톱은 13px(좁은 화면 11px), 줄 높이 1.15다. 이 Mac의 Chromium에서 영문 cell은 7.83px다. cell 자체가 작게 잡힌 것은 아니다. 작아 보인 직접 원인은 (1)이다.
- **(3) 한글 간격.**
  - 글꼴 목록(`JetBrains Mono, SF Mono, ui-monospace, Menlo, D2Coding, …`)의 앞쪽 고정폭 글꼴에는 한글이 없다. 그래서 한글은 시스템의 비례폭 한글 글꼴로 그려진다.
  - 13px에서 한글 글자는 11.24px이다. 터미널은 넓은 글자에 2칸(15.66px)을 준다. 그래서 글자마다 약 4.4px(28%)가 빈다.
  - D2Coding을 대체 글꼴로 설치해도 다 풀리지 않는다. cell 너비는 영문 글꼴(0.6em)이 정하므로, 한글(1em)과 2칸(1.2em) 사이가 여전히 빈다.
  - 풀려면 한글이 영문의 정확히 2배인 고정폭 글꼴(D2Coding·Sarasa Mono K 등)을 **첫 글꼴로** 써야 한다. 그러려면 웹 자산(약 1.5~2MB, OFL)을 저장소와 실행 파일에 넣어야 해서 결정으로 남긴다.
- **시험(웹 E2E 22단계).**
  - 「넓은 창」 단계: 1280×800에서 터미널 글자 영역이 칸을 한 cell(과 scrollbar 자리) 안쪽까지 채운다. header 크기와 host PTY 크기가 같다. 1900×1150으로 넓히면 열·줄이 늘고, 되돌리면 같은 크기로 돌아온다. 자동 맞추기를 끄면 이 단계가 120×32에서 실패한다.
  - 「두 controller」 단계: 보기만 하는 쪽은 창 크기가 달라도 PTY 크기를 바꾸지 않는다. 가져오면 그 화면을 따른다. 연결이 끝나 lease가 비면 남은 화면이 다시 맞춘다.

### 8.31 웹 터미널 글꼴 Meslotendard(10-17, 사용자 결정)

§8.30 (3)의 한글 간격을 풀려고, 사용자 결정에 따라 사용자의 공개 글꼴 Meslotendard(MesloLGM Nerd Font Mono 영문·아이콘 + Pretendard 한글, OFL 1.1)를 터미널 첫 글꼴로 넣었다. 한글 한 글자가 영문 정확히 두 칸(units 2466:1233)이다.

- **글꼴 목록.** `--font-term: Meslotendard, "Meslotendard Web", <기존 목록>`.
  - 기기에 깔린 글꼴을 먼저 쓴다.
  - 없으면(휴대폰 등) 함께 넣은 WOFF2를 받는다. `@font-face`의 src도 `local()`을 먼저 보므로, 깔린 기기에서는 받지 않는다.
  - xterm은 처음 연 글꼴로 cell을 잰다. 그래서 WOFF2가 아직 없으면 그것을 뺀 목록으로 열고, 받아지면 원래 목록으로 바꿔 다시 잰다. 그 뒤 자동 맞추기(§8.30)가 다시 돈다.
- **넣은 파일.** `web/public/fonts/meslotendard-0.3.1/`
  - 원본: release `v0.3.1`의 `Meslotendard.zip`. 이 zip의 TTF 넷은 release `SHA256SUMS.txt`·운영 host Mac 설치본·개발 Mac 설치본과 sha256이 같았다.
  - Regular·Bold만 WOFF2로 바꿨다(fontTools, glyf 변환 없음).
    - 다시 풀어 비교하면 10개 표가 원본과 bytes가 같다. `head`만 WOFF2 규격의 「손실 없이 변환함」 표시(flags bit 11)와 checksum이 다르다.
    - subset·이름 변경은 하지 않았다. Pretendard가 Reserved Font Name이라 변경판을 만들지 않는다.
  - 크기: TTF 4.98MB·4.95MB → WOFF2 2.14MB·2.16MB. release 실행 파일이 28.25MB에서 32.63MB가 됐다(+4.38MB).
  - Italic·BoldItalic은 넣지 않았다. 운영 Claude pane 두 화면을 읽기 전용 화면 구독으로 세어 보니 굵게 35·88자, 기울임 0자였다. 기울임은 깔린 기기에서는 그 기울임꼴을, 아니면 브라우저가 Regular를 기울여 그린다. 넣으면 약 4.3MB가 더 는다.
  - 고지: `LICENSE`(OFL 전문), `licenses/MESLO_APACHE-2.0.txt`, `licenses/NERD_FONTS_LICENSE.txt`, `licenses/THIRD_PARTY_NOTICES.md`, `FONTLOG.md`, 그리고 출처·버전·sha256을 적은 `README.md`. 저장소 README에도 적었다.
- **보내기.** WOFF2와 고지는 실행 파일에 들어가 `/fonts/meslotendard-0.3.1/`로 나간다. 폴더에 버전이 있으므로 host는 `fonts/`를 `assets/`처럼 1년 immutable로 보낸다(버전을 바꾸면 폴더도 바꾼다). CSP `font-src 'self'` 안이다.
- **시험(웹 E2E 23단계).**
  - 「터미널 글꼴」 단계(1280×800, 13px): host가 WOFF2(bytes 같음·`font/woff2`·immutable)와 고지 네 파일을 보낸다. 터미널 글꼴 목록이 바뀐 뒤 xterm cell 너비 7.828px = 영문 7.827px, 한글 15.653px = 영문의 정확히 2배다. 넣은 WOFF2만으로 잰 값도 같다.
  - 휴대폰 폭(390×844, 11px): cell 6.62px, 한글 13.25px.
  - 개발 Mac에는 Meslotendard가 깔려 있어 `local()`로 그린다. 그래도 터미널은 「WOFF2를 뺀 목록으로 열고 → 받아지면 바꿔 다시 잼」 길을 지난다. 처음 열 때 `"Meslotendard Web"` face가 아직 불러지지 않았기 때문이다. 넣은 WOFF2 자체(받기·bytes·2:1 너비)는 `FontFace(url)`로 따로 잰다.
  - **웹 E2E는 CI에 없다**(개발 Mac에서만 돈다). 그래서 글꼴이 없는 기기에서 브라우저가 `url()`로 받는 것은 자동 시험이 없다. 운영 휴대폰에서 눈으로 확인한다. 처음 착지(`dcb0cea`) 커밋 메시지의 「CI가 지난다」는 틀린 말이었다.
- **남는 것.** 웹 UI의 다른 고정폭 글자(코드·질문 카드 등)는 바꾸지 않았다(`--font-mono`).
- **운영 확인(사용자).** Mac Safari는 깔린 글꼴을 숨겨도 넣은 WOFF2로 그린다. 한글이 두 칸 안에서 띄어 보이는 것은 같은 글꼴의 네이티브 터미널과 같은 모양이다. iPad(글꼴 없음)도 사용자 확인으로 통과했다.
  - 기록: Safari 27은 페이지가 열린 직후 넣은 face를 `error`로 두고 `document.fonts.check()`가 true다. 1초 안에 스스로 `loaded`가 된다. 그 순간 터미널이 열리면 cell을 대체 글꼴로 잴 수 있다. 실제 화면 문제가 없어 고치지 않았다. 다시 보이면 `check()` 대신 face 상태로 판단한다.

### 8.32 웹 터미널 한글 IME 조합(10-17, 사용자 보고)

- **증상.** 웹 터미널(xterm)에 「상태」를 치면 pane에 「ㅅㅌ」만 들어갔다. 아래 입력 막대(composer)는 정상이었다(브라우저 textarea가 조합을 끝낸 글을 보낸다).
- **원인(xterm 6.0.0 CompositionHelper).** 조합 밖에서 keydown(229)이 오면 xterm은 `setTimeout(0)` 뒤 숨은 textarea에 늘어난 글을 「조합이 아닌 글」로 보낸다. 브라우저가 첫 자모를 textarea에 넣고 다음 task에 `compositionstart`를 보내면 다음이 일어난다.
  - 첫 자모(ㅅ)가 먼저 나간다.
  - 조합 시작 위치가 그 자모 뒤로 잡힌다. 보낸 길이(`_dataAlreadySent`)만큼 또 건너뛰어 완성 음절(상)은 잘린다.
  - keydown과 조합 사건이 같은 task에 오면 문제가 없다. 그래서 브라우저·부하에 따라 보인다.
- **고침(`web/src/ime.ts`).** 그 터미널의 CompositionHelper 두 곳만 바꾼다.
  - keydown(229) 뒤 늘어난 글은 keyup(늦어도 200ms)까지 미룬다. 그 사이 조합이 시작되면 보내지 않고, 조합 시작 위치를 keydown 때 끝으로 되돌린다.
  - 조합이 없으면 xterm과 같은 판정으로 보낸다(한글 입력기의 숫자·문장부호).
  - xterm 내부 이름을 쓰므로 판을 `6.0.0`으로 고정했다. 이름이 없으면 걸지 않고 경고만 남긴다(웹 E2E가 잡는다).
- **시험(웹 E2E 24단계, 「터미널 한글 IME」).** 가짜 pane(받은 바이트를 그대로 보임)에 Chromium·WebKit으로 「상태」를 조합한다.

| 입력 | 고치기 전 | 고친 뒤 |
|---|---|---|
| 조합 사건이 keydown과 같은 task | 상태 | 상태 |
| `compositionstart`가 다음 task(첫 자모가 먼저 textarea에) | **ㅅㅌ**(사용자 증상) | 상태 |
| 조합 없는 keydown(229) 숫자·문장부호 | 7. | 7. |
| Chromium 실제 IME 사건(CDP `Input.imeSetComposition`·`insertText`) | 상태 | 상태 |

  - 실제 macOS 입력기로 치는 자동 시험은 없다(OS 입력을 흉내 내려면 손쉬운 사용 권한이 필요하다). 사건 순서는 합성 사건으로 재현했고, 운영 화면에서 사용자가 확인한다.

- **바뀜(10-17 11:30, §8.36).** 위 `ime.ts`는 실제 입력기에서 충분하지 않았다(사용자 보고: 초성·중성·종성이 분리된다, iPhone 자모 분리). 사용자 지시로 herdr-web-ui 방식(xterm 5.5.0 + IME patch)으로 바꿨고 `ime.ts`는 지웠다. 「다음 task의 compositionstart」 순서는 기록된 사건이 아니라 추측으로 만든 합성 순서였다. 그 줄은 herdr-web-ui가 실제 브라우저에서 기록한 순서(§8.36)로 바꿨다.

### 8.33 명시적 가져오기 없애기(10-17, 사용자 결정: herdr 모델)

- **사용자 보고.** TUI에서 입력이 안 됐다. 웹이 lease를 갖고 있어 TUI는 보기만 했고 `Ctrl+B t`로 가져와야 했다. 사용자 결정: 「명시적으로 가져오는게 불편하다. herdr·herdr-web-ui처럼 한다.」
- **herdr의 규칙(research 확인).**
  - herdr 0.9.5: lease가 없다. 탭마다 「geometry controller」 client 하나의 크기를 쓴다. 터미널 focus를 얻을 때, 실제 입력 때, 탐색 때 controller를 가져간다. 처음 attach는 controller가 없을 때만 가진다. controller가 떠나면 그 탭을 보는 client가 이어받는다.
  - herdr-web-ui: 조작 연결이면 언제나 입력된다. 보이고 focus가 있을 때(`inUse`)만 크기를 건다. focus·보임을 얻거나, 크기가 어긋난 채 클릭·입력하면 다시 맞춘다. 가려지면 1초 뒤 놓는다. observe 연결은 입력·크기 불가다.
- **aaa 규칙.**
  - host: 대화형 front의 입력·크기·그림은 거부하지 않고, 보낸 연결을 「지금 쓰는 화면」(lease)으로 둔다. 한 번 보내는 CLI(`cli` 연결)는 바꾸지 않는다. `lease_held`는 CLI `pane resize`(누가 쓰는 중)에만 남는다.
  - TUI: 입력은 그대로 보낸다. 터미널 focus 사건을 켜고, FocusGained면 보이는 pane 모두 크기를 다시 건다. FocusLost 동안은 크기를 걸지 않는다(focus 사건을 안 보내는 터미널은 늘 focus로 본다). 비정상 종료에도 focus 사건 보고를 끈다. 상태 줄은 「이 화면 크기」·「<화면> 크기」다. `prefix t`는 남겼다.
  - 웹: 입력 막대·키 줄·그림을 막지 않는다. 보이고 focus가 있을 때만 크기를 건다. window focus·다시 보임·터미널 누름이면 크기를 다시 건다. 1초 넘게 가려지면 `release`한다. 「가져오기」 단추를 없애고 「<화면>이 크기를 정하는 중 · 입력하거나 터미널을 누르면 이 화면 크기로」만 보인다.
  - 보기 권한(reader)은 그대로 입력·크기 불가다.
  - 두 화면이 번갈아 입력하면 크기가 그때마다 바뀐다(herdr와 같다).
- **시험.**
  - m12 `lease_between_fronts`: 다른 front 입력이 바로 되고 lease가 옮겨 간다. CLI resize는 쓰는 중 거부, `release`는 자기 것만 놓는다, 연결이 끝나면 풀린다.
  - TUI harness `lease_conflict_with_another_front`: 다른 front가 쓰던 pane에 바로 입력되고 칸 크기로 맞춘다. 다른 front가 다시 쓴 뒤 `ESC [ I`(focus 얻음)면 크기를 다시 건다. `ESC [ O` 동안은 lease가 비어도 크기를 걸지 않는다. `prefix t`도 된다.
  - 웹 E2E 「두 controller」: 열기만 해서는 바꾸지 않는다. 입력하면 바로 들어가고 그 화면 크기가 된다. 번갈아 입력하면 크기가 바뀐다. window focus면 다시 건다. 가려진 화면은 1초 뒤 놓고 남은 화면이 맞춘다.

### 8.34 TUI prefix 뒤 명령 키를 영문 입력기로(10-17, 사용자 요청)

- **요청.** 한글 입력기로 `Ctrl+B` 다음 명령 키(`c` 등)를 누르면 자모가 들어가 명령이 안 된다. prefix를 받으면 영문 입력기로 바꾸고, 끝나면 되돌린다.
- **구현(`aaa-tui::input_source`, macOS Text Input Sources).**
  - prefix를 받으면, 지금 입력기가 영문 자판 배치(`TISTypeKeyboardLayout`이면서 ASCII 가능)가 아닐 때 `TISCopyCurrentASCIICapableKeyboardLayoutInputSource`(ABC 등 사용자의 영문 배치)를 고른다.
    - 두벌식 같은 입력기도 macOS는 ASCII 가능으로 보고한다. 그래서 종류(자판 배치인지)까지 본다.
  - 명령 키를 처리한 뒤, 취소, 시간 초과(3초), TUI 종료·오류·panic에 원래 입력기로 되돌린다(값이 사라질 때 되돌리는 guard).
  - 지속 설정이 아니라 지금 입력기만 바꾼다. ssh 세션(`SSH_CONNECTION`·`SSH_CLIENT`·`SSH_TTY`)에서는 바꾸지 않는다(그 Mac의 입력기는 앞에 앉은 기기의 것이 아니다).
  - 설정 `[tui] prefix_ascii`(기본 켬)로 끈다.
- **보조: 자모 해석.** prefix 뒤 두벌식 자모는 같은 자리 영문 키로 읽는다(ㅊ=c, ㅅ=t, ㅌ=x, ㅇ=d, ㄱ=r, ㅜ=n, ㅔ=p, ㅐ=o, 쌍자음·ㅒ·ㅖ는 Shift 자리). 바꾸기 전에 들어온 키, ssh·다른 OS를 위한 것이다.
- **시험.**
  - 유닛: 자모 → 키, ssh 판정.
  - TUI harness: prefix 뒤 `ㅊ`로 새 탭. 시험 설정은 `prefix_ascii = false`라 사용자 입력기를 건드리지 않는다.
  - macOS 실제 입력기(`AAA_IME_NATIVE=1`, 개발 Mac에서 짧게, 단언이 실패해도 처음 입력기로 돌려 둔다): ABC → 두벌식 → prefix 영문 `com.apple.keylayout.ABC` → 되돌림 두벌식 → 처음 ABC. 약 0.4초다.

### 8.35 TUI 왼쪽 목록(10-17, 사용자 요청)

- **요청.** TUI에서도 웹처럼 프로젝트 thread와 머신별 pane을 보고 연다. 웹 왼쪽 목록과 같은 모양, 접기·펴기, 목록과 pane 사이 포커스 이동, 좁은 터미널은 접힌 채 시작.
- **구현(`aaa-tui::list`).**
  - 순서는 웹과 같다: 끝나지 않은 thread(평가자 제외, 최근 바뀐 순, 상태 묶음 배지·%) → 머신마다 연결 표시, 「+ 새 셸」(연결된 머신만), pane(평가자 제외, 상태 배지).
  - 자료는 host의 `State`(머신·pane 순서)와 시작할 때 읽는 `Threads`, 그 뒤 `Thread` 사건뿐이다. TUI는 저장하지 않는다(새 thread는 사건으로 따라온다).
  - 폭 32칸 + 구분선. 터미널 폭이 100칸 이상이면 펼친 채, 그보다 좁으면 접힌 채 시작한다. 목록이 보이면 pane 칸은 그만큼 좁아지고 크기도 그 폭으로 건다.
  - 키: `prefix s` 펴기·접기(펴면 목록으로 포커스), `prefix Tab` 목록 ↔ pane. 목록에서는 `↑↓`·`j/k` 고르기, `Enter` 열기, `Esc`·`Tab` pane으로. prefix는 목록에서도 그대로다.
  - 열기: pane·thread 줄은 그 pane이 있는 탭으로 가고, 탭에 없으면 새 탭으로 넣는다(`AddTab`). 「+ 새 셸」은 그 머신 기본 셸을 새 탭으로 띄운다.
  - 고른 줄은 색과 함께 `▸`로 표시한다(색이 없는 터미널).
- **시험(TUI harness `side_list_like_web`).** 130칸에서 펼침과 pane 폭(130−33), 두 번째 thread가 사건으로 보임, 탭에 없는 pane·thread·이미 있는 pane·새 셸 열기, Esc, `prefix s` 접으면 전체 폭, 90칸은 접힌 채 시작하고 `prefix s`로 펼침. 다른 TUI 시험은 90칸(접힘)으로 돈다.

### 8.36 웹 한글 IME를 herdr-web-ui 방식으로(10-17, 사용자 지시)

- **지시.** 「새로 만들지 말고 herdr-web-ui를 따라한다.」 iPhone에서 「입력」이 「ㅇㅣㅂㄹㅕㄱ」로, 데스크톱에서도 초성·중성·종성이 분리됐다.
- **옮긴 것(herdr-web-ui `aa624cb`).**
  - xterm을 6.0.0에서 herdr-web-ui와 같은 5.5.0(addon-fit 0.10.0)으로 내리고, 그 IME patch를 소스에 붙여 빌드한다(`web/patches/`, `web/xterm-source.mjs`, `scripts/build-xterm.ts`와 같은 방식). npm에는 bun의 patchedDependencies가 없어 빌드 때 cache에 복사해 붙인다. `patch(1)` 결과와 같은지 확인했다.
  - 터미널 조합 상태(`PaneTerminal.tsx` 588~605줄): 조합 중이거나 방금 끝난 조합의 확정 글에는 키 막대 유지 키(Ctrl·Alt·Shift)를 붙이지 않는다.
  - 입력 막대(`TerminalInput.tsx`): 조합 중 Enter·keyCode 229 Enter는 보내지 않는다(이미 같았다). 조합 중에 누른 보내기는 blur·focus로 확정시키고 compositionend 뒤 보낸다. 누른 때부터 300ms(`COMMIT_WAIT_MS`) 안에 확정되지 않으면 「글자를 마친 뒤 보낸다」. 보내기 단추는 focus를 가져가지 않고, 보낸 뒤에도 입력칸이 focus를 지킨다(화면 키보드가 그대로).
- **시험(웹 E2E 24단계).** herdr-web-ui 재생 시험의 기록 순서를 옮겼다.

| 입력 | 바꾸기 전(`a39b5c62`, xterm 6 + ime.ts) | 바꾼 뒤 |
|---|---|---|
| Chromium·WebKit 같은 task 조합, 조합 없는 229 숫자, Chromium CDP 실제 IME | 상태 · 7. · 상태 | 같음 |
| macOS Safari 26.6.2 기록: abc + 한글 | **abcㅎㄱ** | abc한글 |
| macOS Safari: Enter 앞 확정 · 받침 넘김 · 확정 전 지우기 | **ㅎ⏎ · ㄱ사 · ㅎa** | 한⏎ · 갑사 · a |
| iPad 하드웨어 키보드 기록(platform MacIntel·iPad): abc + 한글 · 마아 · Enter | **abcㅎㅏㄴㄱㅡㄹ · ㅁㅏㅇㅏ · ㅎㅏㄴ⏎** | abc한글 · 마아 · 한⏎ |
| 입력 막대 조합 중 보내기 네 경우(herdr-web-ui terminal-input-regression) | (없던 동작) | 통과 |
| 유지 키 Ctrl + 조합 확정 「한」 + b | 한b(Ctrl을 「한」이 씀) | 한^B |

- **iPhone은 아직 모른다.** herdr-web-ui patch는 `navigator.platform`이 Mac·iPad일 때만 바꿔 넣기 길을 켠다. iPhone(`iPhone`)에서 iPad와 같은 순서가 오면 바꾼 뒤에도 자모가 갈린다(E2E로 확인: platform iPhone으로 iPad 순서를 재생하면 「abcㅎㅏㄴㄱㅡㄹ」). herdr-web-ui에는 iPhone 기록이 없다. 실제 iPhone 확인이 필요하다.

### 8.37 조합 상자 모양과 색(10-17, 사용자 보고·지시)

- **보고.** iPad 화면 키보드로 입력줄에 「상태 ㅅ」을 치면 조합 중인 ㅅ이 검은 사각 상자로 보이고, 앞 글자들과 크기·위치가 어긋났다. 지시: herdr-web-ui `terminalGlyphs.ts`의 조합 상자 맞추기(`fitView`)를 옮기고, 색은 그 자리 칸과 맞춘다(배경 = 그 칸 배경, 글자 = 그 칸 글자색, 테두리 없음).
- **옮긴 것(herdr-web-ui `aa624cb`, 바꾸지 않음).** `src/lib/terminalGlyphs.ts`·`terminalBoxGlyphs.ts`를 `web/src/herdr/`에 두고 `adjustTerminalGlyphs(term)`를 건다(herdr-web-ui `PaneTerminal.tsx` 609줄과 같은 자리).
  - 줄: 칸보다 좁게 그린 두 칸 글자(Meslotendard가 없는 기기의 한글 등)를 최대 1.2배 키우고 칸 가운데 둔다. 선·블록 문자는 칸 크기 상자로 그린다. 분해된 글자는 NFC로.
  - 조합 상자: 그 줄이 줄 크기·자간·여백(`fitView`). xterm 자신의 폭 측정을 쓴다(macOS WebKit 한글이 canvas 13px 대 xterm 11.2px로 다르게 재지는 문제).
  - herdr-web-ui의 조합 상자 관련 CSS는 따로 없다. xterm.css(5.5.0, aaa도 같은 파일)를 그대로 쓴다.
- **다르게 둔 것(이유: 사용자 지시).** herdr-web-ui는 조합 상자 색을 정하지 않아 xterm.css의 검은 배경·흰 글자 그대로다. aaa는 `web/src/compositionView.ts`가 상자 아래 칸(커서 칸이면 옆 칸)의 배경·글자색을 읽어 맞추고 테두리·외곽선을 없앤다.
- **세로 자리(12:05 사용자 보고: 조합 중 「가」 위 짧은 가로줄).** xterm(5.5.0 `CompositionHelper`)은 상자 top을 `줄 번호 × cell 높이`로 계산한다. cell 높이가 1/64px로 나눠지지 않으면(DPR 3의 14.6667px 등) 브라우저가 쌓은 실제 줄은 14.65625px씩 내려가, 아래 줄일수록 상자가 그 줄보다 낮아진다. 그러면 상자 위로 커서 블록이 1 device px 줄로 보이고 상자 밑은 아래 줄로 넘친다(iPhone 크기 맨 아래 줄 0.45px).
  - herdr-web-ui도 같은 xterm 계산·DOM renderer·블록 커서를 쓰고 따로 고치지 않는다. 같은 자리에 같은 줄이 생길 구조다.
  - aaa는 `compositionView.ts`가 상자 top·높이·줄 높이를 실제 커서 줄 요소에 맞춘다(이유: 사용자 보고). IME 후보 창 자리(helper textarea)는 xterm 그대로 둔다.
- **시험(웹 E2E 「조합 상자」, WebKit 820×1180 2배, 어두운·밝은 테마 × 배경 없는 줄·배경 있는 입력줄).** 조합 상자 배경·글자색이 그 칸과 같고, 테두리가 없고, 폭이 두 칸이다. 고치기 전(`26f305f1`)은 네 경우 모두 검은 배경·흰 글자였다.
  - iPad 세로·가로(2배), iPhone 390×844(3배, 11px 글꼴) × 배경 없는 줄·배경 줄·화면 맨 아래 배경 줄(「› 가」)에서 상자 세로가 커서 줄과 같고, 캡처의 상자 둘레 3px에 커서색 픽셀이 없다. 고치기 전에는 iPhone 맨 아래 줄에서 상자가 0.45px 낮고 커서색 21 device px 줄이 보였다.
  - headless는 Meslotendard가 있어 한글이 정확히 두 칸이다. 그래서 크기 어긋남(글꼴이 없을 때)은 headless로 재현하지 못했다. iPad 실제 화면으로 확인한다.

### 8.38 thread 묶음과 outage를 herdr-projects대로(10-17, 차이 목록 1)

- **묶음(C2·F4).** herdr-projects `thread::group()`(src/thread.rs 565~606)을 `orch.rs` `thread_group`으로 옮겼다. 위 줄부터 처음 맞는 것이 이긴다.
  1. 끝남.
  2. 시작 중: 5분 안이면 작업 중, 넘으면 사람 차례.
  3. 사람 차례: 시작 실패, 보고 없이 닫힌 pane, brief가 안 간 채 harness가 입력을 받을 상태가 아닌 것이 60초 넘음, brief 전달 불명.
  4. 사람 차례: 확인창이 30초 넘게 떠 있음, 진행 보고 `Waiting for you`(harness가 일하는 중이 아닐 때).
  5. 새 보고: harness가 일하는 중이 아니고 brief를 보냈으면 검토 대기. 일하는 중에 쓴 보고는 결과가 아니다.
  6. 작업 중: harness 작업 중·확인창, brief 대기, 5분 안의 100% 아래 진행 보고.
  7. 쉼(`idle`, 새 묶음).
  - 대응: prompt_pending = brief `pending`, brief_stuck = `unknown`, blocked = `question`, 준비 상태 = `started`·`idle`. 진행 보고 시각(`progress_at_ms`)을 새로 기록한다. 새 harness 세션은 진행 보고를 지우고, 사용자 입력(turn_start)은 `Waiting for you`를 지운다(herdr-projects progress `respond`).
  - 다르게 둔 것: PR Landing 묶음 없음(aaa는 merge 대기열, D11). 보고 사본 실패와 `[사용자 작업 필요]`·`[결정 필요]` 태그는 사람 차례(aaa는 보고를 host로 복사하고 태그로 사람 일을 알린다, D9).
  - hook이 없는 harness나 지금의 Codex(턴 끝 notify만, D8)는 일하는지 몰라 진행 보고가 없으면 「쉼」으로 보인다.
- **outage(F7).** herdr-projects steps.rs `Outage`를 옮겼다. 끊긴 시간은 이 host 프로세스가 처음 끊김을 본 때부터 재고(저장하지 않음), 기준을 넘으면 한 번, 돌아오면 한 번 알린다. 옛 코드는 연결할 때만 바뀌는 `last_seen`으로 재서 host를 다시 띄우면 오래 붙어 있던 머신이 곧바로 「34분째 끊김」이었다(운영 11:22).
- **시험.** host 단위 시험(묶음 9개는 herdr-projects 시험을 옮김, outage 1개). m13 재시작 시험: 머신이 끊긴 채 host를 다시 띄우면 기준(5초) 전에는 outage 없음 → 기준 뒤 down 한 번 → 다시 붙으면 recovered 한 번. 옛 코드로 돌리면 실패한다.

### 8.39 화면 키보드 위로 입력하는 곳을 올린다(10-17, 사용자 보고·지시)

- **보고(12:26, iPad 화면 키보드).** 키보드가 올라오면 터미널 아래쪽(프롬프트 줄·커서·입력 막대·키 막대)이 키보드 뒤에 가린다. herdr-web-ui에서는 입력하는 곳이 키보드 위로 올라간다. 지시: herdr-web-ui를 그대로 따른다.
- **원인.** iOS Safari는 화면 키보드가 올라와도 레이아웃 높이(100%·100dvh)를 그대로 두고 보이는 높이(`visualViewport`)만 줄인다. aaa 화면은 레이아웃 높이 그대로라 아래쪽이 키보드 뒤에 남았다.
- **옮긴 것(herdr-web-ui `aa624cb`, 파일을 바꾸지 않음, `web/src/herdr/`).**
  - `lib/viewport.ts`·`viewportResume.ts`·`keyboard.ts`. 키보드가 보이는 동안 보이는 높이를 `--app-height`로 알리고 `data-keyboard`를 달고 페이지를 (0, 0)에 둔다. `.app` 높이는 `var(--app-height, 100dvh)`(herdr-web-ui styles.css)다. 그래서 머리·터미널·입력 막대·키 막대가 키보드 위에 남는다.
  - viewport meta에 `interactive-widget=resizes-content`(Chrome은 레이아웃 높이를 스스로 줄인다).
  - PaneTerminal 705~752줄 `keyboardHolds`: 키보드는 화면 아래를 가릴 뿐이라 높이만 줄면 PTY 크기를 바꾸지 않는다(다른 화면이 키보드마다 두 번 줄바꿈하지 않게). 대신 터미널을 커서 줄이 보이게 옮기고, 새 출력에도 따라간다. 손으로 옮기면 따라가지 않는다. 폭이 바뀌면(회전) 키보드 중에도 맞춘다. 키보드가 내려가 터미널이 커지면 붙잡아 둔 맞추기를 한다.
- **다르게 둔 것과 이유.** aaa 터미널은 늘 바로 입력이라(입력줄 모드 없음, D1에서 참고대로 바꾼다) 터미널 칸에 `data-direct-typing`을 늘 단다. 그래서 xterm 입력칸 focus도 키보드로 센다.
- **시험(웹 E2E 「화면 키보드」).** herdr-web-ui `scripts/keyboard-viewport-regression.ts`처럼 visualViewport·innerHeight·100lvh 자만 가짜로 둔다. WebKit iPad 820×1180(키보드 400)·iPhone 390×844(키보드 336)에서 터미널을 누른 경우와 입력 막대를 누른 경우를 본다.
  - 커서 줄이 터미널 영역 안에, 입력 막대·키 막대가 보이는 높이 안에 있다. PTY 크기는 그대로다.
  - 고치기 전에는 `.app`이 1180·844 그대로였고, 입력 막대(아래 1091·787)와 키 막대(1168·836)가 키보드 뒤에 있었다.

### 8.40 thread 흐름 나머지를 herdr-projects대로(10-17, 차이 목록 1)

- **B7 다른 세션·하위 agent 사건 거르기.** herdr-projects progress.rs `eligible`·`foreign_session`을 옮겼다.
  - `agent_id`·`subagent_id`·`agent_transcript_path`가 있거나 기록이 `/subagents/` 아래인 사건은 버린다.
  - SessionStart 때 적은 harness 세션과 다른 session_id의 사건도 버린다(중첩 Codex thread 등). 적은 ID는 agent 메모리에만 있다(agent를 다시 띄우면 다음 SessionStart까지 거르지 않는다).
  - 다르게 둔 것: 확인창(question) 사건은 하위 agent 것도 둔다. aaa는 hook으로 질문을 알고, 하위 agent의 확인창도 세션을 막는다(herdr는 화면으로 본다).
- **E5 보낸 메시지 기록.** 입력한 메시지는(받았는지 몰라도) `threads/<id>.task.md`의 `## Follow-ups` 아래에 시각과 함께 덧붙인다(herdr-projects `append_follow_up`). 다시 시작하면 task와 함께 brief에 들어간다. 입력 전에 거부되면 남기지 않는다.
- **A11 `thread brief` 두 번 입력 막기(herdr-projects threads.rs `brief`).**
  - brief를 받았으면 거부한다(더 보낼 것은 `thread message`). 보내는 중이면 거부한다.
  - 전달 불명인데 입력창에 그 줄이 남아 있으면 다시 치지 않고 Enter만 보낸다. 전에는 줄이 입력창에 두 번 이어 붙었다(시험으로 재현).
- **E2 brief 전 메시지 거부.** brief를 받기 전(`pending`·`unknown`)에는 메시지를 거부한다(herdr-projects `prompt`: prompt_pending이면 거부). 전에는 대기열이 brief보다 먼저 보낼 수 있었다.
  - 아직: 입력창에 사람이 쓴 글이 있으면 거부(`draft_in_box`)는 herdr-projects `prompt_box`(ANSI 화면 해석)를 옮길 때 한다.
- **A12 시작 시한.** 묶음(§8.38)이 시작 5분을 넘으면 사람 차례로 보인다. herdr-projects ticker는 상태를 failed로 바꾸지만, aaa는 시작 작업이 아직 끝날 수 있어 상태는 그대로 둔다.
- **시험(m13).** 하위 agent·중첩 세션 Stop을 보내도 「작업 중」이 남는다. 메시지 둘이 Follow-ups에 순서대로 남는다. 입력창에 남은 brief는 Enter만으로 받고 줄이 한 번만 보인다. 받은 뒤 `thread brief`와 brief 전 메시지는 거부한다. 앞 셋은 옛 코드로 돌리면 실패한다.

### 8.41 화면 키보드에서 한글이 자모로 갈리는 퇴행 조사(10-17, 사용자 보고)

- **보고.** 화면 키보드 고침(§8.39) 뒤 iPad 화면 키보드로 터미널에 치면 한글이 초성·중성·종성으로 갈린다. 그 전 판(§8.37)에서는 조합이 됐다.
- **원인 commit.** 두 판 사이에 웹을 바꾼 것은 §8.39 하나다. 의심: 출력마다 `.term-scroll`을 커서 줄로 옮기는 것, `herdr/viewport.ts`가 viewport 사건마다 `scrollTo(0, 0)`하는 것, `.app` 높이, `data-direct-typing`.
- **headless 재현 안 됨.** 웹 E2E IME 단계에 「화면 키보드가 올라온 iPad」 판을 더했다.
  - WebKit, iPad UA, 터치, 가짜 visualViewport로 키보드를 올려 grid를 붙잡고 터미널을 옮긴 상태다.
  - 100ms마다 상태 줄을 다시 그리는 pane에서, 30ms마다 visualViewport scroll을 보내며 iPad·Safari 기록을 재생한다.
  - 지금 코드로 모두 통과한다. 이 재생은 하드웨어 키보드·macOS Safari 기록이라 iOS 화면 키보드의 사건 순서는 들어 있지 않다. herdr-web-ui도 iPadOS 화면 키보드 기록이 없다(docs/terminal-input.md).
- **실기 사건 기록(herdr-web-ui `terminal-ime-diagnostic` 목적).**
  - 목록 아래 「입력 기록 켜기」(터치 기기) 또는 `?trace=input`으로 켠다.
  - 모으는 것: 터미널 helper textarea의 키·beforeinput·input·조합·focus 사건, 그때의 textarea 값과 선택, visualViewport, `data-keyboard`, 터미널 옮김, 보낸 글, 출력.
  - 「입력 기록 보내기」가 host 상태 폴더 `host/diag/input-<ms>.json`에 남긴다(`POST /api/v1/diag/input`, pairing 필요, 1MiB, 20개).
  - 기록을 E2E 재생으로 옮겨 고치기 전 실패를 보인 뒤 고친다.
- **IME 재생을 매 commit에.** CI에 `web-e2e (macos-15)` job을 더했다. 웹 E2E 전체(IME 기록 재생 포함)를 headless Chromium·WebKit(Playwright 1.59.1)으로 돌린다. binary를 바꾸기 전에도 로컬에서 웹 E2E를 돌리고 결과를 report.md에 적는다.
- **E2E 고침.** composer 단계의 「휴대폰 입력 막대」 검사가 연결 전에 봐서 타이밍에 따라 실패했다. 화면이 온 뒤에 본다(기대값은 그대로).
- **실기 기록과 원인(10-17 13:30).** 사용자 기록은 iPhone(iOS 18.7, Safari 27.0.1, 홈 화면 앱) 화면 키보드였다(처음에 iPad로 적은 것을 고친다).
  - 자모마다 keydown(key는 자모, code `Unidentified`, keyCode 0, 조합 사건 없음)이 오고, 편집기가 새 음절은 insertText, 바뀐 음절은 deleteContentBackward 뒤 insertText로 고친다. iPad 하드웨어 키보드와 같은 꼴이다.
  - herdr-web-ui xterm patch는 이 경로(`passHangulKey`·`beforeinput` 바꿔 넣기)를 `isMac || isIpad`에서만 켠다. iPhone(`navigator.platform` `iPhone`)에서는 keydown이 자모를 바로 보냈다(기록의 send: ㅎ ㅏ ㄴ ㄱ ㅡ ㄹ).
  - 화면 키보드 고침(§8.39)과는 상관이 없다. 갈림은 keydown에서 정해지고 그 조건은 플랫폼뿐이다.
- **고침(참고와 다름, 사용자 지시 「직접 고쳐」).** patch의 두 조건에 `isIphone`을 더했다. 사용자 확인으로는 herdr-web-ui도 같은 결함이 있어, 참고에서 고침을 찾지 않고 aaa에서 고쳤다. `web/patches/README.md`와 차이 목록에도 적었다.
  - E2E 「iPhone 화면 키보드 사용자 기록 재생」: 기록 순서 그대로(iPhone UA·platform·430×932, WebKit은 화면 키보드를 올린 상태). 고치기 전 `"ㅎㅏㄴㄱㅡㄹ"`, 고친 뒤 `"한글"`. 기존 IME 재생 24가지도 그대로 통과한다.
- **참고 iPad 회귀 시험 옮김(사용자 지시).** herdr-web-ui `scripts/terminal-ipad-ime-regression.ts`를 그대로 옮겼다. 참고처럼 pane WebSocket을 가로채 보낸 프레임을 본다.
  - 판: Chromium MacIntel·iPad, 화면 키보드를 올린 WebKit iPad.
  - 경우: 영문 뒤 한글, 조합 중 자모를 보내지 않음, 조합 글자 크기, 두 음절 바꿔 넣기, Enter·Ctrl+C·Tab 앞 확정, blur 확정, 조합 중 pane 바꾸기.
  - 찾은 결함: 조합 중 pane을 바꾸면 Chromium에서 조합 중 글자가 옛 pane으로 갔다. 지워지는 textarea의 blur가 effect 정리보다 먼저 온다. 참고 `presentedRef`처럼 화면에서 지우기 전(layout effect 정리)에 내리는 `presentRef`로 막았다.
- **시험 도구 고침.** m13 가짜 harness `sticky`가 한 바이트씩 `dd`로 되돌려 찍어, 느린 CI에서 Enter 뒤에도 찍는 중이라 받았다고 판정됐다(`bf056ae` macOS CI 실패). 받은 조각을 한 번에 찍는다.

### 8.42 웹 터미널 키 처리를 herdr-web-ui대로(10-17, 차이 목록 2)

- **옮긴 것(PaneTerminal 539~562·585·620~664·1053~1076·1284~1335줄).** `herdr/keys.ts`·`osc52.ts`·`terminalFileLinks.ts`는 그대로 복사했다.
  - 블록 커서 깜빡임. macOS Option+누름은 마우스 보고 중에도 선택한다.
  - 웹 링크는 http(s)만 새 탭으로 연다(`isWebLink`).
  - Ctrl+V는 브라우저 붙여넣기에 맡긴다(xterm이 ^V를 보내지 않는다). 선택이 있을 때 Ctrl+C는 복사하고 ^C를 보내지 않는다.
  - Shift+Enter는 `ESC CR`(줄바꿈)로 보낸다. Ctrl+Enter는 프로그램이 modifyOtherKeys를 요청했으면 `CSI 27;5;13~`(Claude Code 「지금 보내기」)로 보내고, 아니면 Enter다.
  - macOS Cmd+⌫는 `^U`, Cmd+←/→는 `^A`/`^E`로 보낸다(조합 글 뒤, bubble keydown).
  - OSC 52(vim·tmux·Claude Code의 클립보드 쓰기)를 클립보드에 쓴다. 질의는 답하지 않는다.
- **aaa만의 것: replay가 modifyOtherKeys를 다시 말한다.** vt100은 이 모드를 화면 상태에 담지 않는다. 그래서 프로그램이 모드를 켠 뒤에 붙은 화면은 Ctrl+Enter를 그냥 Enter로 보냈다. `aaa-vt`가 `CSI > 4 ; n m`을 추적해 replay 끝에 붙인다. herdr도 replay에 키보드 모드를 담는다.
- **시험.** 웹 E2E 「키 처리」 단계(macOS platform)로 키마다 pane이 받은 bytes를 본다. 늦게 붙은 화면의 Ctrl+Enter도 본다.
  - 고치기 전 웹: Shift+Enter가 `\r`로 갔다.
  - 새 웹과 옛 vt: Ctrl+Enter가 `\r`로 갔다.
  - `aaa-vt` 유닛 시험 `replay_says_modify_other_keys_again`.

### 8.43 키 막대를 herdr-web-ui KeyBar대로(10-17, 차이 목록 2 C)

- **옮긴 것.** `herdr/keyBar.ts`·`keyRepeat.ts`는 그대로 복사했다. `web/src/KeyBar.tsx`는 참고 `KeyBar.tsx`를 옮긴 것이다. 문구는 한국어로, lucide 아이콘은 글자로 바꿨다.
  - 기본 항목과 순서: Esc, Tab, ^C, Ctrl, Alt, Shift, Enter, ↑, ↓, ←, →.
  - 터치(`pointer: coarse`)나 폭 768px 이하에서만 보인다(KeyBar.css와 같은 media query). 데스크톱 마우스 화면에는 없다.
  - 유지 키 Ctrl·Alt·Shift는 래치다. 다시 누를 때까지 켜져 있고, 연결이 끊기면 풀린다. pane이 바뀌면 화면이 다시 열려 풀린다.
  - 화살표·⌫·Del·Home·End·PgUp·PgDn은 꾹 누르면 400ms 뒤 50ms마다 되풀이한다. 10px 넘게 밀면 취소한다.
  - 키를 눌러도 focus를 가져가지 않는다(화면 키보드가 내려가지 않는다). 터미널 조합 중에는 회색이고 눌러도 가지 않는다.
  - 붙여 넣은 글과 조합으로 확정된 글에는 유지 키를 붙이지 않는다. 유지 키는 다음 키를 기다린다.
  - 화살표 키는 프로그램 화살표 모드(DECCKM)를 따른다. 항목 목록은 기기별 `localStorage`(`aaa:keybar-items`)에서 읽고 `sanitizeKeyBarItems`로 거른다.
- **다른 곳 1: chord 인코딩을 front가 한다.** 참고는 유지 키 + 키를 chord(`ctrl+left` 등)로 herdr에 보내고, herdr가 pane 프로그램의 키보드 프로토콜로 인코딩한다. aaa host에는 그 인코더가 없다. 그래서 `web/src/keyChord.ts`가 front에서 bytes로 바꾼다.
  - 기준: 같은 수식을 하드웨어 키로 눌렀을 때 이 웹 터미널(xterm.js 기본 인코더 + §8.42 재매핑)이 보내는 bytes와 같다.
  - 그래서 xterm.js `Keyboard.ts`의 Alt+화살표 규칙도 따른다. Alt+←→는 macOS면 `ESC b`·`ESC f`, 그 밖은 Ctrl+←→다. Alt+↑↓는 macOS가 아니면 Ctrl+↑↓다.
  - Shift+Enter는 `ESC CR`, Ctrl+Enter는 modifyOtherKeys대로(§8.42), Shift+Tab은 `CSI Z`다.
- **다른 곳 2: 항목 설정 화면은 아직 없다.** 참고 KeyBarSettings(추가·삭제·순서)는 설정 화면을 만들 때 옮긴다. 저장값을 읽고 거르는 것만 지금 옮겼다.
- **다른 곳 3: lens·active.** 참고는 lens나 active pane이 바뀌어도 유지 키를 푼다. aaa에는 lens가 없고 화면마다 pane이 하나라 pane이 바뀌면 화면이 다시 열린다.
- **시험.** 웹 E2E 「키 막대」 단계(터치 820×1180, platform Linux).
  - 데스크톱에서는 숨김, 키 순서, focus 유지, 래치를 본다.
  - 유지 키 + 키 막대 키(또는 친 키)가 같은 수식의 하드웨어 키와 같은 bytes인지 키마다 견준다.
  - DECCKM, 꾹 누름 반복, 밀어서 취소, 조합 중 회색, CDP IME 확정 뒤 Ctrl도 본다.
  - 처음 만든 encoder는 Alt+←·Alt+↑가 하드웨어와 달랐다(`CSI 1;3D` ≠ `CSI 1;5D`). xterm.js 규칙을 따르게 고쳤다.
  - 이 단계가 옛 「키 줄」 단계를 대신한다. 옛 단계가 t1에 입력해 두던 것에 「두 controller」 단계가 기대고 있었다. 이제 그 단계가 처음에 직접 t1에 입력한다.

### 8.44 입력 방식을 herdr-web-ui대로: 바로 입력 기본, 입력줄은 기기별(10-17, 결정 20 D1, 차이 목록 2 D)

- **옮긴 것.** `herdr/terminalDraft.ts`는 그대로 복사했다. `web/src/TerminalInput.tsx`는 참고 `TerminalInput.tsx`를 옮겼다(문구 한국어). PaneTerminal 358~361·2218~2245·2318~2393·3008~3020줄을 따랐다.
  - 입력 방식은 기기별 설정(`aaa:terminal-input-mode`, `auto`=바로 입력)이다. 터치 기기는 키 막대 맨 앞 ⌨로 바꾼다. 입력줄 조합 중에는 ⌨만 회색이다.
  - 입력줄이면 xterm 칸에 `inputmode=none`을 둔다(터미널을 눌러도 화면 키보드가 안 올라온다). 바로 입력을 켤 때만 터미널에 focus를 준다. 키 막대 키는 입력줄의 키보드를 터미널로 옮기지 않는다.
  - 입력줄은 그리드 위에 뜨고 4줄까지 자란다. 흐름에는 한 줄 칸만 둔다. 모드를 바꾸면 그 한 줄만큼 PTY가 한 번 바뀐다(참고와 같다).
  - Enter는 모든 기기에서 보내기다(`enterKeyHint="send"`). 빈 줄 보내기는 Enter이고 단추 그림이 ⏎로 바뀐다. 조합 중 누른 보내기는 확정을 300ms 기다린다.
  - 초안은 머신·pane마다 localStorage에 둔다. 보내는 동안 친 글은 지우지 않는다(보낸 앞부분만 지운다). 끊겨도 입력줄은 그대로고 보내기만 거부한다.
- **다른 곳 1: 입력 확인 답.** 참고는 server의 submit 답을 기다려 「보냄」으로 친다. aaa 입력 protocol에는 답이 없다. Enter까지 보낸 때(300ms 뒤)를 「보냄」으로 친다. 답을 받으려면 protocol을 바꿔야 해 따로 한다.
- **다른 곳 2: 「그림」 단추.** 참고 터미널 화면에는 없고 chat 화면 composer에 있다. aaa chat 화면은 아직 읽기 전용이라(D3) 휴대폰에서 그림을 올릴 곳이 입력줄뿐이다. 그래서 입력줄 앞에 둔다. 붙여넣기·끌어 놓기는 바로 입력에서도 된다.
- **다른 곳 3: 받아쓰기(VoiceInput)** 는 aaa에 없다. 설정 화면도 아직 없어 데스크톱에서는 입력줄을 고를 수 없다(설정 화면을 만들 때 넣는다).
- **바뀐 사용자 규칙.** 데스크톱 기본 화면에서 입력 막대가 없어졌다. 휴대폰 Enter가 줄바꿈에서 보내기로 바뀌었다(§8.23 규칙을 결정 20 D1로 바꾼다).
- **시험.** 웹 E2E 「입력 방식」 단계: 데스크톱·터치 기본에 입력줄 없음, ⌨ 맨 앞, 입력줄 켜면 `inputmode=none`·저장, 글+Enter·빈 Enter·Alt+빈 Enter bytes, 키 막대가 focus를 안 옮김, 6줄을 써도 PTY·그리드 높이 그대로(rows 4), 조합 중 ⌨만 회색, 다시 바로 입력이면 터미널 focus.
  - 입력줄을 쓰는 단계는 그 화면을 입력줄로 두고 돈다(`pairPage` `line`). 휴대폰 Enter·끊김 단언은 참고 규칙으로 바꿨다(위 「바뀐 사용자 규칙」).
  - 화면 키보드 단계는 터미널을 바로 입력에서, 입력줄을 ⌨로 켠 뒤에 본다. PTY 기준은 모드를 바꾼 뒤에 잰다.

### 8.45 구독 사용량 막대와 설정 톱니를 herdr-web-ui대로(10-17, 사용자 요청)

- **요청.** 목록 왼쪽 아래에 설정 톱니와 구독별 남은 양(주간) 막대를 herdr-web-ui처럼 넣는다(사용자 캡처: 휴대폰 sidebar 바닥 줄).
- **옮긴 것(herdr-web-ui aa624cb).**
  - host `crates/aaa-host/src/usage.rs` ← `server/usage.ts`. Claude·Codex·Cursor·Copilot·Grok·Antigravity·OpenCode의 CLI 로그인으로 한도를 읽는다.
    - 읽기만 한다(token refresh 안 함, 끝났으면 `expired`). FRESH 5분, 다시 읽기 30초, 실패 뒤 1분, 429는 Retry-After까지 계정마다 기다린다.
    - 뒤에서 혼자 돌지 않는다: 웹이 물을 때만 읽는다. 계정을 말하지 않는 로그인의 key는 찾은 곳의 hash다(경로가 브라우저로 가지 않는다).
  - `GET /api/v1/usage[?refresh=1]` → `UsageReport`(aaa-proto, 참고 `shared/protocol.ts` Usage 형식 그대로). 보기 권한 기기도 본다(참고: 들어온 client 누구나). `cache-control: no-store`.
  - 웹 `usage.ts` ← `lib/usage.ts`(import만 바꾸고 문구 한국어), `UsageMeters.tsx` ← `UsageMeters.tsx`, 바닥 줄 ← `MachineSidebar` `sidebar-footer-row`, 설정 창의 「입력 방식」·「구독 사용량」·「계정」 묶음 ← `SettingsDialog`(Terminal·Usage 쪽).
- **다른 곳 1: keychain을 로그인 세션 밖에서 다시 읽지 않는다.** 참고는 `security`가 36(잠김)을 내면 launchd 일회 job(`launchctl bootstrap gui/<uid>`)으로 다시 읽는다. launchd job을 만드는 일은 사용자 승인이 필요한 시스템 동작이다. aaa host는 로그인 세션의 LaunchAgent로 돈다. 그때는 `locked`로 알린다.
- **다른 곳 2: 설정은 이 기기에만.** 참고는 구독 사용량 설정을 server에 두어 기기끼리 나눈다(shared). aaa에는 설정 저장소가 아직 없어 localStorage에 둔다.
- **다른 곳 3: 막대가 기본으로 보인다.** 참고 기본은 끔이다. 사용자가 보이라고 해서 켬으로 둔다. 쓴 양·주간은 참고 기본 그대로다(캡처의 Claude 95% 빨강은 쓴 양이다).
- **다른 곳 4: 설정 창의 쪽.** 이번에는 aaa가 뒷받침하는 묶음만 넣었다(터미널 입력 방식·구독 사용량·계정 순서·숨기기). 데스크톱에서도 입력줄을 고를 수 있게 됐다(§8.44의 빈 곳). 모양·알림·기기·키 막대 편집 등은 그 기능을 옮길 때 넣는다. 구독 표시는 참고 AgentMark 대신 aaa 아이콘(Claude·Codex)과 이름 첫 글자다.
- **HTTP.** `crates/aaa-host/src/http.rs`: 본문(Content-Length·chunked·닫힘)을 1MiB까지 읽는 작은 client. 이미 쓰는 rustls·webpki 위에 두어 새 crate를 넣지 않았다(Retry-After 날짜만 `httpdate`, 이미 lock에 있던 판).
- **시험 고리.** `AAA_USAGE_HOME`(집, keychain·gh·환경 변수 안 씀), `AAA_USAGE_ORIGIN`(구독 주소의 scheme·host를 가짜 서버로). 웹 E2E와 유닛 시험이 실제 로그인·keychain에 닿지 않는다.
- **시험.**
  - 유닛(`usage::tests`, 가짜 집·keychain·gh·loopback 서버): 일곱 구독 읽기, 5분 캐시와 30초 다시 읽기, 429 Retry-After, 끝난 token은 묻지 않음, keychain과 파일 중 늦은 것, 잠긴 keychain, 이름 없는 계정 번호와 경로 없는 key. `http::tests`: Content-Length·chunked(작은 chunk 5000개)·닫힘·상한.
  - 웹 E2E 「구독 사용량·설정」: 칩(95% 빨강, 429는 「—」 흐림), 펼친 한도·초기화까지·plan·계정, 여러 화면을 열어도 구독마다 한 번만 물음, 남은 양·세션·숨기기·순서, 설정 창에서 입력줄, 다시 열어도 남음, 끄면 사라짐.
  - 실제로 읽어 본 구독은 없다. Cursor·Copilot·Grok·Antigravity·OpenCode는 지어낸 답으로만 시험했다(§8.49에서 그 다섯은 지웠다). 운영 host 머신(mac-a)의 Claude·Codex는 host를 다시 띄운 뒤 사용자 화면에서 처음 읽는다. 이때 macOS keychain 허용 창이 그 머신에 뜰 수 있다.

### 8.46 끊긴 동안 친 글을 붙잡는다(10-17, 차이 목록 2 E)

- **전.** host나 agent가 끊긴 동안 터미널에 친 글은 읽기 전용이라 조용히 버려졌다.
- **옮긴 것(herdr-web-ui PaneTerminal 363~379·1353~1424·2257~2268·2777~2800줄).** `herdr/draft.ts`는 그대로 복사했다.
  - 입력할 수 없는 동안(host 끊김·agent 끊김·붙는 중) 친 글·붙여 넣은 글은 「붙잡은 입력」에 모은다. 줄을 세워 다시 보내지 않는다.
  - Enter·줄바꿈은 붙잡지 않고 그 자리를 빈칸으로 둔다. 「보내기」는 Enter를 누르지 않는다(띠가 그렇다고 알린다). ⌫는 붙잡은 글을 지운다. 1024자 상한.
  - 끊김 띠에 붙잡은 글이 보인다. 다시 붙으면 따로 띠가 되어 사람이 「보내기」·「버리기」를 고른다.
  - 붙잡은 입력이 있는 동안 글·⌫·Enter는 거기에 더하고, chord(유지 키·Shift·Ctrl+Enter)와 입력줄 보내기·빈 보내기(Enter)는 「먼저 보내거나 버린다」로 거절한다. ^C·Esc·Tab·화살표는 그대로 간다.
  - 입력할 수 없을 때 chord는 붙잡지 않고 「아직 키를 받을 수 없다」고 알린다(닫기로 지운다).
  - 이 기기 localStorage `herdr-web-ui:terminal-draft:<머신>:<pane>`에 24시간 남는다(끊긴 동안 다시 열어도 잃지 않는다). 세션이 끝났으면 띠가 「붙잡은 입력은 버렸다」고 알린다.
- **aaa에 맞춘 곳.** 참고는 socket이 입력을 못 받으면 붙잡는다. aaa는 host가 알리는 읽기 전용(끊김·붙는 중)일 때 붙잡는다. 보기 권한 기기는 붙잡지 않는다(참고의 observe와 같다). 키 막대 키도 같은 길로 간다(참고는 키 막대를 xterm 입력으로 흘려 같은 길을 탄다). 비밀번호 줄(secret) 감지는 aaa에 없어 그 규칙은 없다.
- **시험.** 웹 E2E 「agent 끊김」·「agent 복귀」 단계: 끊긴 동안 친 `echo held-$((2+3))`와 Enter가 띠와 저장에 남는다(Enter 자리 빈칸, `leftOut`). 다시 붙어도 저절로 가지 않는다. 입력줄 보내기는 거절되고 그 글은 남는다. 「보내기」 뒤 사람이 Enter를 눌러 `held-5`가 나오고 저장이 지워진다.

### 8.47 글자 크기 설정과 두 손가락 핀치(10-17, 차이 목록 2 G)

- **전.** 폭 700px 이하면 11px, 아니면 13px로 고정이었다.
- **옮긴 것(herdr-web-ui settings.ts `terminalFontSize`, SettingsDialog 395~397줄 `Stepper`, PaneTerminal 1528~1640·1996~2025줄, `lib/pinchFontSize.ts`).**
  - 글자 크기는 이 기기 설정이다(참고도 `device` 범위). 기본 13px, 10~22px. 설정 창 「터미널 → 글자 크기」에서 한 칸씩 바꾼다. 모든 기기가 이 크기로 연다(좁은 화면 11px는 없앴다).
  - 크기가 바뀌면 쓰는 화면은 그 글자로 PTY 크기를 다시 건다.
  - 터미널에서 두 손가락을 벌리거나 오므리면 글자가 손가락을 따라 바로 바뀌고 크기(「20px」)를 띄운다. 손을 떼면 설정에 저장하고 PTY를 한 번 다시 건다. 취소(touchcancel)면 처음 크기로 돌아간다. Safari 확대(gesture)는 막는다.
  - `herdr/pinchFontSize.ts`는 import 경로만 바꿔 옮겼다.
- **다른 곳: 핀치 중에는 grid를 이 화면에서만 다시 맞추지 않는다.** 참고는 핀치 중 xterm grid를 그 화면에서 맞춘다. aaa 터미널 grid는 늘 host가 알리는 PTY 크기를 따른다(이 화면에서만 맞추는 일이 없다). 핀치 중에 grid를 이 화면에서만 맞추면, 그 사이 온 출력이 PTY에 없는 grid에 그려진다. 그래서 글자만 손가락을 따르고, grid는 손을 뗄 때 PTY와 함께 한 번 바뀐다. 화면 키보드가 grid를 붙잡고 있으면 키보드가 내려갈 때 맞춘다(§8.39).
- **아직.** 한 손가락 끌기를 휠 보고로 바꾸는 참고 동작은 스크롤 묶음(차이 목록 J, 결정 필요 줄)과 함께 옮긴다.
- **시험.** 웹 E2E 「글자 크기」: 설정에서 16px이면 PTY 93→76열, 13px로 돌리면 93열, 기본값은 저장하지 않음. 휴대폰(CDP 실제 터치)에서 100→150px로 벌리면 20px와 「20px」 표시, 손을 떼기 전에는 PTY·저장 그대로, 뗀 뒤 저장 20과 PTY 45→29열.

### 8.48 그냥 끌어 고르기·제자리 클릭·보내기 전에 크기 되찾기(10-17, 차이 목록 2 H·J)

- **옮긴 것(herdr-web-ui PaneTerminal 760~1046·1890~1918·2231·2342·2362줄).**
  - 프로그램이 마우스 보고를 켜도 왼쪽 끌기는 고른다(Shift, macOS는 Option을 누른 것처럼). 놓으면 클립보드에 복사하고 「클립보드에 복사했다」를 2.5초 띄운다. OSC 52도 같은 알림을 띄운다.
  - 끌지 않고 제자리에서 놓은 누름은 클릭이다. 고르기 키 없이 다시 보내 프로그램이 요청한 보고 형식(SGR 등)으로 간다(Claude Code diff의 닫기 단추, TUI 메뉴). 선택을 지우는 누름과 링크 누름은 보내지 않는다.
  - 크기 되찾기는 보내기 **전에** 한다. 다른 화면이 크기를 가져간 뒤 이 화면에서 터미널 키를 치면, 그 키보다 먼저 이 화면 크기를 건다(반복 키·조합 중 키는 아니다). 키 막대 키, 입력줄 보내기·빈 보내기(Enter)도 그렇다. 전에는 host가 입력을 받고 「지금 쓰는 화면」을 바꾼 뒤에 맞춰, 첫 글의 답이 옛 크기로 그려질 수 있었다.
  - 터미널을 눌러 크기를 거는 것은 왼쪽 버튼만이다(오른쪽 클릭은 아니다).
  - 붙잡은 입력·보내지 못함·복사 알림 띠는 터미널 위 오른쪽에 뜬다(참고 `.terminal-banners`). 흐름에 넣으면 터미널 칸이 줄어 PTY 크기가 바뀐다. 다른 띠(끊김·권한·지금 쓰는 화면)는 아직 흐름에 있다(차이 목록 2장 레이아웃 줄과 함께 옮긴다).
- **옮기지 않은 것.** 참고 xterm은 scrollback이 없어 화면을 넘는 끌기를 herdr 기록 API로 따라가고 herdr 원문을 복사한다. aaa xterm은 자기 scrollback이 있어 끌기가 가장자리를 넘으면 xterm이 굴리고, 복사도 xterm 선택 글이다. scroll 묶음(차이 목록 J 「scrollback과 휠」, 결정 필요)을 정하면 그때 맞춘다.
- **시험.** 웹 E2E 「그냥 끌어 고르기·제자리 클릭·크기 되찾기」: 마우스 보고(SGR)를 켠 시험 프로그램에서
  - 제자리 클릭은 `ESC[<0;3;1M`·`m`으로 간다.
  - 끌기는 프로그램에 아무것도 보내지 않고 `pick-this-word`를 복사하며 알린다.
  - 다른 화면이 크기를 가져간 뒤 친 `z`는 이 화면이 WebSocket에 `resize`를 먼저 쓰고 `input`을 쓴다. 프로그램 쪽 순서는 SIGWINCH와 입력 읽기 순서가 정해지지 않아 보지 않는다.
  - keydown 되찾기를 뺀 판으로 돌리면 `[input z]`만 가서 실패한다(웹 번들을 다시 지은 판으로 확인).

### 8.49 구독 사용량은 Claude·Codex만, 표시는 herdr-web-ui AgentMark(10-17, 사용자 요청)

- **요청(원문: 「사용량은 claude codex만. 사용량 아이콘은 herdr web ui 같은거 사용」).**
- **Claude·Codex만 묻고 보인다.** host는 그 둘의 로그인만 찾고 그 둘의 주소에만 묻는다. 참고가 함께 읽는 Cursor·Copilot·Grok·Antigravity·OpenCode 읽기는 지웠다. proto `UsageProviderId`도 둘이다. 그래서 그 구독들의 keychain 항목·`gh` 명령·Cursor 앱 DB도 열지 않는다.
- **표시는 참고와 같다.** `web/src/herdr/AgentMark.tsx`의 `ClaudeMark`(주황 타일의 별)·`CodexMark`(글자색 매듭)·`FallbackMark`와 `AgentMark.css`·`agentMark.ts`는 참고 그대로다. `PROVIDER_MARK`(`usage.ts`)도 참고와 같다(둘만 남음). 칩 14px, 펼친 창·설정 계정 16px도 참고 크기다.
- **다른 곳: 참고의 다른 agent 표시는 옮기지 않았다.** 참고 `AgentMark.tsx`는 omp·cursor·opencode·gjc·omo도 그리고, `agentSvgMarks.ts`에서 LobeHub Icons(MIT)와 출처 favicon(droid·maki) 표시를 가져온다. aaa는 Claude·Codex만 보인다. 라이선스가 적히지 않은 favicon 표시를 공개 저장소에 들이지 않으려고 그 둘의 표시만 남겼다.
- **출처·라이선스.** 표시 코드는 herdr-web-ui(MIT, Copyright (c) 2026 devswha)의 것이다(`web/src/herdr/README.md`에 그 MIT 고지가 있다). 참고는 이 표시를 같은 저자의 chatmux llm-logo-provider 묶음에서 옮겼다고 적는다. Claude와 OpenAI(Codex) 로고는 각 상표권자(Anthropic·OpenAI)의 것이다. aaa는 구독 이름을 가리키는 데만 쓰고 그들과 관계를 뜻하지 않는다.
- **시험.** 유닛: 다른 구독 읽기 시험 둘을 지웠다(Claude·Codex 시험 여섯은 그대로). 웹 E2E 「구독 사용량·설정」: 가짜 집에 OpenCode 로그인이 있어도 칩은 Claude·Codex 둘이고 OpenCode 주소에는 묻지 않는다(0번). 칩 표시는 `agent-mark claude is-brand`(svg 14px, 타일 `#D77655`)와 `agent-mark codex`(`currentColor`). 숨기기는 Claude로 본다.

## 9. 이미지 붙여넣기 (front → host → agent)

herdr 0.9.5는 원격 endpoint에서 client의 clipboard 이미지 bytes를 대상 서버 temp 파일로 보내고, 그 절대 경로를 pane에 붙인다. 웹 UI는 브라우저 file을 대상 머신 `<cwd>/.herdr-web-ui/`에 저장하고 `@path`를 넣는다. 확인한 코드와 차이는 [소스 분석](source-analysis.md#이미지-붙여넣기-소스-확인)에 있다.

aaa의 원칙:
- local과 remote를 같은 attachment API로 다룬다.
- **이미지 bytes를 front에서 host를 거쳐 실제 agent 머신으로 옮긴다.** 그 머신에서 읽을 수 있는 파일을 만든 뒤 입력을 전달한다.
- agent 머신에는 GUI·clipboard·X11/Wayland가 필요 없다.
- 원격 agent의 clipboard를 읽지 않는다. front 기기의 경로 문자열만 보내지도 않는다.

```mermaid
flowchart LR
  T[TUI front · Ctrl+V · front 기기 clipboard] --> H[host · Front API attachment stream]
  B[웹 front · paste event · Blob] --> S[tailscale serve HTTPS] --> H
  H --> A[대상 agent backend · Agent link chunks]
  A --> F[검증 · 원자적 저장 · 대상 머신 파일]
  F --> R[길이 · SHA256 · native path receipt]
  R --> H
  H --> P[대상 재검증 · Claude/Codex 입력 adapter]
  P --> I[agent가 파일을 읽어 이미지로 소비]
```

| 단계 | 동작·계약 |
|---|---|
| TUI 수집 | 명시적 Ctrl+V(또는 빈 bracketed paste)에서 front 자신의 clipboard를 읽는다. macOS는 NSPasteboard 이미지 → PNG, Windows는 PNG/CF_DIBV5/CF_DIB, Linux desktop은 X11/Wayland다. arboard로 한다(§8.28, Wayland 전용 세션은 아직 아님) |
| 입력 호환 | 이미지가 있으면 중복 Ctrl+V byte와 텍스트 paste를 소비하고 image intent 하나를 만든다. 없으면 기존 text·raw-key 동작을 보존한다. 외부 터미널이 이미지 파일 경로를 붙이면 그 로컬 파일을 업로드한다 |
| raw SSH 셸의 한계 | 단순 `ssh host` 셸은 사용자 PC의 clipboard를 제공하지 않는다. 기본 경로는 사용자 기기에서 실행한 aaa TUI front다. raw 셸에서는 파일 첨부 명령이나 웹을 안내한다 |
| 웹 수집 | terminal 영역과 composer의 user paste event에서 `clipboardData.items/files`의 Blob을 받는다. 이미지와 plain text를 함께 draft에 보존하고 중복 paste를 막는다. 파일 선택·drop도 같은 API로 받는다 |
| 대상 고정 | paste 순간 machine·session·terminal generation·agent identity·lease·front 권한을 intent에 고정한다. 업로드 중에 pane을 바꿔도 새 pane에 넣지 않는다. 실제 삽입 직전에 다시 검증한다 |
| 전송 | front → host: Front API의 bounded binary chunk(웹은 streaming Blob body). host → agent: Agent link chunk. host는 **저장하지 않고 통과 전송**한다(bounded buffer). agent가 끊겨 있으면 바로 실패하고 draft는 유지한다. base64 거대 JSON과 scp 문자열은 쓰지 않는다. 길이·SHA256·offset을 검증한 뒤 agent가 원자적으로 commit하고 receipt를 반환한다 |
| 대상 경로 | `.aaa/<project>-<thread>/attachments/<attachment_id>.png`. 일반 세션은 session 범위 경로다. Windows agent는 Windows 절대 경로를, Unix agent는 그 머신의 POSIX 경로를 돌려준다. cwd를 모르거나 쓸 수 없으면 명시 오류를 낸다 |
| agent 전달 | adapter가 harness별로 검증한 이미지 참조 형식으로 바꾼다. 모든 agent에 `@path`나 POSIX quote 하나를 강제하지 않는다. 업로드 완료와 실제 입력 접수는 따로 본다. 일반 shell과 확인창에는 경로를 자동으로 넣지 않는다 |
| 전송 의도 | Ctrl+V는 첨부일 뿐 자동 Enter가 아니다. 업로드가 끝나지 않았으면 Send를 보류한다. pane별 순서와 paste barrier로 경로보다 Enter가 먼저 가지 않게 한다 |
| 재연결 | 미완료 전송은 offset으로 재개할 수 있다. commit 뒤 응답이 유실되면 같은 ID로 receipt를 조회한다. 입력 직후 유실은 `delivery_unknown` 규칙을 따른다(§8.5 a) |
| 보관 | 파일 수명을 front 연결에 묶지 않는다. session·thread가 참조하는 동안 보존하고, 참조가 끝나고 보존 기한이 지난 것만 정리한다. 이미지 내용은 로그·보고·git에 넣지 않는다 |

추가 규칙:
- **크기:** 기본 상한은 파일당 16MiB다. 넘으면 연결을 끊지 않고 명시 오류를 내며 draft를 유지한다. herdr는 이 경우 client 연결을 끊는데, 그 동작은 가져오지 않는다.
- **형식:** PNG/JPEG가 필수다. 실제 signature·decode·pixel·memory 상한·quota를 확인한다.
- **권한:** Unix는 0700/0600, Windows는 사용자 SID ACL이다. symlink·junction·path traversal은 거부한다.
- **책임:** 파일 생성·경로 인용·정리 책임은 대상 agent backend에 있다. front는 목적지 절대 경로를 지정할 수 없다.

## 10. 개발자 도구·속도 해결 계획

### 10.1 합격 기준과 「한 번 등록」 목록

사용자 합격 기준은 다섯 가지다. 이를 완화한 우회책은 채택하지 않는다.
- 설치 때 한 번 등록까지만 허용
- 업그레이드 뒤 재등록 없음
- 매번 GUI 터미널을 띄울 필요 없음
- 재부팅 직후 ssh·웹 우선 접속에도 빠름
- 키체인 잠김 없음

여기에 검토에서 찾은 처리량과 하위 daemon 조건을 더했다.

| 기준 | 설계 | P0 검증 | 현재 |
|---|---|---|---|
| 설치 때 한 번 등록 | 아래 목록을 설치 절차에서 한 번 받는다 | L1 | 가설 |
| 업그레이드 뒤 재등록 없음 | 바뀌지 않는 launcher(결정 14), 고정 hook 문자열, quarantine 없는 updater. `aaa`는 launcher의 자식이라 다시 빌드해도 등록 대상이 그대로다 | L1·E7 | 가설. adhoc 재빌드 실행 파일 자체는 요구 사항이 바뀜(실측). 같은 소스로 다시 만든 launcher는 cdhash가 같다(실측) |
| GUI 터미널 불필요 | agent backend는 gui 도메인 LaunchAgent(KeepAlive) | E2·E4 | 가설. 근거: WezTerm(등록된 GUI 앱) 자손은 빠름 |
| 재부팅 직후 ssh·웹 우선에도 빠름 | GUI 로그인 뒤 LaunchAgent만 기동한다. 로그인 전에는 대기 상태를 명시한다(§10.7) | 실측하지 않는다(결정 14) | 설계·문서로만 다룬다. 로그인 전은 미해결([정할 것 3](#16-위험과-정할-것)) |
| 키체인 잠김 없음 | Aqua 세션 프로세스만 agent를 띄운다 | L1(키체인 풀림) | Aqua job·LS 앱 unlocked 실측. 실제 항목 접근은 미측정 |
| 처리량이 줄지 않음(추가) | App Nap·자동 종료 끄기, QoS 확인 | E2 | **미등록 GUI 앱 계보에서 warm이 약 3배 느렸다(실측, 원인 모름)** |
| 하위 daemon도 같은 계보(추가) | profile 금지 인자, doctor 점검 | E6 | 운영에서 codex app-server 문제를 겪었다 |

설치 때 한 번 받는 항목:

| 항목 | 대상 | 근거 | 업그레이드 뒤 유지 조건 |
|---|---|---|---|
| 개발자 도구 | `~/Applications/aaa-launcher.app`(결정 14, 다시 빌드하지 않음) | 미등록 계보는 첫 실행 +221.609~236.280ms(실측) | launcher를 바꾸지 않는다 |
| 전체 디스크 접근 | 같음 | 외장 볼륨 worktree. Background 세션은 허용 창을 띄우지 못한다(운영 t-0087) | 같음 |
| 로그인 항목·백그라운드 항목 승인 | SMAppService LaunchAgent | macOS가 백그라운드 항목 승인을 요구한다 | bundle id·서명 유지 |
| 방화벽 수신 허용(방화벽을 켠 경우) | host(Agent link·Front TLS listener) | macOS 애플리케이션 방화벽 | 서명 identity |
| 로컬 네트워크(실제 LAN 사설 주소에 연결할 때, macOS 15 이후) | macOS agent backend | 미검증 | 서명 identity |
| harness hook 신뢰 | Claude·Codex 설정 | 경로가 바뀌면 다시 묻는다(운영) | 고정 hook 문자열 |
| Apple Events(필요한 agent만) | 대상 앱별 | 처음 쓸 때 GUI 프롬프트 | 서명 identity |

### 10.2 실측 요약(n=6/조건)

| 실험 | 첫 실행 평균 ms | warm 평균 ms | 추가 지연 ms | 키체인 | 의미 |
|---|---:|---:|---:|---|---|
| WezTerm 책임 상속 | 1.962 | 1.824 | 0.138 | unlocked | 빠른 기준선. 운영 해결안으로는 탈락 |
| setsid만 적용 | 1.749 | 1.638 | 0.111 | unlocked | SID 분리는 책임 계보를 바꾸지 않는다 |
| disclaim=true | 225.950 | 1.966 | 223.984 | unlocked | 미등록 child가 자기 자신의 책임자가 되어 느리다 |
| 미등록 GUI app | 227.043 | **5.434** | 221.609 | unlocked | GUI 형태만으로 면제되지 않는다. warm 약 3배 |
| 등록된 sshd-session 아래 | 1.767 | 1.677 | 0.090 | **locked** | 빠르지만 전체 합격은 아니다 |
| SSH → LaunchServices 미등록 app | 241.976 | **5.696** | 236.280 | unlocked | GUI 로그인 뒤 키체인 경로. 속도는 미합격 |

- **warm 저하:** GUI 앱 두 조건의 warm(표본 3.995~7.542ms)은 다른 8조건(표본 1.447~2.398ms)보다 약 3배 느렸다. GUI 앱은 LaunchServices가 띄우므로 측정 script의 nice 15를 물려받지 않았을 것이다. 그래서 nice로는 설명되지 않는다(실제 priority는 측정하지 않았다).
- **원인은 모른다.** App Nap, QoS, task role은 가설이다.
- **지표를 바꾼다.** 「첫 실행 − warm」만 보면 계보 전체가 고르게 느려지는 경우를 놓치므로 warm 절대값과 처리량을 gate에 넣는다.
- 세부와 60개 표본은 [실험 기록](experiments.md)에 있다.

### 10.3 후보 재평가

| 후보 | 근거 | 판정 |
|---|---|---|
| WezTerm 계보에 의존 | 빠르고 unlocked이지만 매번 GUI 터미널 기동·생존에 의존한다. WezTerm을 닫으면 살아남은 서버도 면제를 잃는다(운영: ppid 1, SLOW 360ms) | 탈락 |
| sshd 아래 서버 | +0.090ms이지만 키체인 locked(실측) | 탈락 |
| setsid·disclaim만 | 등록 없이는 +223.984ms | 단독 탈락 |
| adhoc 재빌드 launcher | 같은 경로 재빌드 때 designated requirement(cdhash)가 바뀐다(실측) | 탈락 |
| **불변 launcher + 자식 `aaa`(결정 14)** | 한 번 만들고 다시 빌드하지 않는 C launcher(`p0/launcher/aaa-launcher.c`, 최소 `.app`, adhoc). 등록 대상이 바뀌지 않는다. 미등록 상태에서 launcher를 LaunchServices로 띄우면 책임 프로세스가 `aaa-launcher`로 잡힌다(mac-c 실측, 음성 대조: +220ms·FDA denied) | **채택, 등록 뒤 실측 대기**(L1) |
| **고정 서명 `aaa` + gui 도메인 LaunchAgent(SMAppService.agent) 아래 agent backend** | 운영 t-0087: 등록된 본체의 gui/501·Aqua LaunchAgent 아래 +0.04ms, 키체인 풀림. Developer ID인 kache는 업그레이드해도 권한이 유지됐고, Developer ID인 WezTerm이 책임자일 때는 herdr 업그레이드에 재등록이 필요 없었다. 탈락 사유였던 detached=false·bridge 경합은 herdr 고유 문제다 | **1순위 후보, 미합격**(aaa identity·SMAppService 귀속·warm 미측정) |
| 고정 서명 `aaa.app` login item(LS 앱) 아래 | LS 앱 unlocked(실측). 비정상 종료 뒤 자동 재시작이 없다(일반 동작). warm 3배 원인 미규명 | 2순위 비교 대상 |
| 절대 갱신하지 않는 adhoc broker + 버전 디렉터리 core(이전 추천) | 결정 3(단일 실행 파일)과 어긋난다 | 서명 identity를 확보하지 못할 때의 대안 |

### 10.4 추천 구조

1. **등록 대상(결정 14):** `~/Applications/aaa-launcher.app`(bundle id `dev.aaa.launcher`, adhoc, 다시 빌드하지 않음)을 개발자 도구·전체 디스크 접근에 한 번 등록한다. 제품 실행 파일 `aaa`는 고정 경로(예: `~/.local/bin/aaa`)에 두고 자유롭게 교체한다. 서명 identity(결정 9)는 보류한다.
2. **기동:** host와 agent를 각각 gui 도메인 LaunchAgent(Aqua, KeepAlive)로 띄운다. LaunchAgent의 ProgramArguments는 `aaa-launcher <aaa 경로> agent …`이다. launcher가 책임 프로세스이고 `aaa`는 그 자식이다. launcher는 신호를 자식에게 전달하고 자식의 종료 코드를 돌려준다. `AbandonProcessGroup` 등 정리 동작은 설치 작업(M1-4)에서 정한다.
3. **session host:** agent backend가 spawn한다. agent backend가 재시작될 때 session host와 그 자손이 면제를 유지하는지는 **E3의 질문**이며, 결과로 둘 중 하나를 고른다.
   - (a) disclaim spawn으로 session host를 독립 책임 프로세스로 만든다. 비공개 API이며 LLDB가 쓰는 예가 있다.
   - (b) agent backend 재시작을 drain(세션 종료 뒤 재시작)으로 처리한다.
4. **처리량 설정:** agent backend는 App Nap과 자동 종료를 끈다. QoS는 E2 결과로 정한다.
5. **host의 역할:** host는 OS 감독자에게 요청만 한다(§4.2). GUI 로그인 전에는 띄우지 않는다(§10.7).
6. **자가 보고**
   - agent backend가 시작·재연결 때와 주기적으로 다음을 사건으로 보고한다. 실패하면 새 세션을 거부한다. 10-16 사고처럼 첫 실행 probe만 OK이고 키체인은 잠긴 상태를 잡는다.
     - 계보(responsible path·서명)
     - 보안 세션
     - 키체인 실제 항목 읽기
     - 첫 실행 지연 probe
     - warm 지표
   - 비공개 API(`responsibility_get_pid_responsible_for_pid`, `TCCAccessPreflight`)는 진단 보조로만 쓰고, 실패하면 timing probe로 판정한다.

### 10.5 서명 identity

| 선택지 | 장점 | 단점·위험 |
|---|---|---|
| 자체 서명 code-signing 인증서(결정 9, **결정 14로 보류**) | 무료, Apple 계정·연 갱신에 기대지 않는다. 한 번 만들어(10년) 세 Mac이 같은 인증서를 쓴다. designated requirement가 `identifier "dev.aaa" and certificate root = H"<인증서 SHA-1>"`이 되어 같은 인증서로 다시 서명하면 그대로일 것이라는 가설 | 개발자 도구·전체 디스크 접근 유지 여부 미검증(E1). 인증서 반입이 키체인 변경(사용자 작업). 신뢰 설정이 필요한지 미확인 |
| 무료 Apple Development 인증서(결정 9, **결정 14로 보류**) | mac-a·mac-b에 이미 있다. Apple 발급이라 요구 사항이 인증서 이름(CN)·Apple 체인에 묶인다 | 1년마다 갱신, 머신마다 다른 인증서. 갱신 뒤 이름이 같은지·등록이 유지되는지 미검증. mac-c에는 없다 |
| Developer ID(결정 9로 제외) | 근거가 가장 강했다(Developer ID인 kache가 업그레이드 뒤 권한 유지, 운영 관찰) | 유료 회원이 아니라 만들 수 없다 |
| **불변 launcher(결정 14 채택)** | 서명 identity 불필요. launcher는 인자를 받아 자식을 띄우는 일만 해서 바꿀 일이 거의 없다 | launcher를 고치면 재등록이 필요하다. 그래서 기능을 더하지 않는다 |

### 10.6 P0 실험

결정 14로 범위를 줄였다. **재부팅 실험(E5)은 하지 않고 시험 Mac도 쓰지 않는다.** 자체 서명 인증서 E1(결정 9)은 보류하고, launcher 방식이 실측에서 실패할 때만 다시 꺼낸다. P0 세부 실측보다 M1-2~M1-4 완성을 먼저 한다.

필요 조건:
- 사용자 작업: mac-b에서 `aaa-launcher.app`을 개발자 도구·전체 디스크 접근에 한 번 등록(결정 15, 화면 필요)
- 측정은 thread가 ssh로 한다. 재부팅·LaunchAgent 등록·시스템 설정 변경은 하지 않는다

| 실험 | 방법 | 합격 gate |
|---|---|---|
| **L1 launcher 등록 유지** | ① `p0/launcher/install.sh`로 launcher를 한 번 만들어 둔다. ② 사용자가 한 번 등록한다. ③ `p0/launcher/measure.sh before --aaa <aaa>`: launcher를 LaunchServices로 띄우고 자식 `aaa`가 새 binary 30개·FDA·책임 프로세스를 잰다. ④ `aaa`를 다시 빌드(`AAA_BUILD_SALT`)해 같은 경로에 교체한다. ⑤ 재등록 없이 `measure.sh after`. launcher cdhash가 그대로인지 스크립트가 확인한다. 절차는 `docs/p0-runbook.md` | median 추가 지연 ≤10ms, p95 ≤20ms, 100ms 초과 0. 전체 디스크 접근 ok. 책임 프로세스 `aaa-launcher`. 재등록 0 |
| E2 기동 방식·처리량 | LaunchAgent(Aqua) → launcher → `aaa`에서 warm 절대값과 작은 cargo build 시간을 WezTerm 기준선과 비교한다(설치 작업 M1-4와 함께) | 위 지연 기준 + warm·처리량이 기준선 대비 +10% 이내(제안) |
| E3 책임 프로세스 수명 | agent backend만 재시작했을 때 기존 session host 자손의 첫 실행 지연을 본다 | 세션 유지 + 지연 기준 유지. 안 되면 §10.4 3(b) |
| E4 기동 경합 | GUI 로그인 뒤 CLI·웹·Agent link를 동시에 먼저 연결한다 | agent backend는 LaunchAgent 하나만 뜬다. host fork 0, fallback 기동 0 |
| ~~E5 재부팅~~ | 하지 않는다(결정 14). 재부팅 뒤 동작은 §10.7 설계로만 다룬다 | — |
| E6 하위 daemon | codex app-server, `--no-daemon`, 빌드 cache daemon이 처음 뜬 문맥을 doctor가 탐지하는지 본다 | 잘못된 계보 탐지 100% |
| E7 업그레이드 부수 효과 | 교체 뒤 hook 신뢰 재질문, 방화벽 프롬프트, quarantine·Gatekeeper 창을 본다 | 셋 다 0 |

- **gate 성격:** 위 기준은 앞으로 적용할 제안 gate다. 과거 probe 임계값(100ms)을 낮춰 이전 결과를 통과시킨 것이 아니다.
- **실패 처리:** 실패하면 원인을 조사한다. 반복 실행이나 timeout 연장으로 통과시키지 않는다.
- **현재 상태:** **완전 합격한 해결안은 없다.** P0을 넘기 전에는 운영 전환을 하지 않는다. M1-1 개발은 개발 모드로 병행할 수 있다.

### 10.7 재부팅·로그인 상태 기계

```text
부팅 → FileVault 해제(사람 또는 fdesetup authrestart)
     → 로그인 창: GUI 세션이 없어 aaa는 돌지 않는다.
       ssh에서 `aaa status`는 「GUI 로그인 대기」를 보고하고, 웹 주소는 응답이 없다.
     → GUI 로그인(현장 또는 화면 공유)
     → LaunchAgent가 aaa host와 aaa agent를 기동하고 자가 점검을 통과한다
     → ready. front와 원격 agent가 재접속한다
```

- 로그인 전에도 빨라야 하는지는 [정할 것 3](#16-위험과-정할-것)이다.
- 로그인 전에는 login keychain이 풀려 있다고 보장할 수 없다. 암호를 저장해 자동으로 unlock하는 방식은 쓰지 않는다.
- 운영 Mac은 FileVault가 켜져 있고 자동 로그인이 없다. 계획 재부팅은 `fdesetup authrestart`로 한다.

### 10.8 하위 daemon

- **profile:** launch/resume spec에 daemon 금지 인자와 env를 넣는다(§8.2).
- **doctor 점검:** pane 자손이 아닌 곳에서 agent 일을 하는 장수 프로세스의 책임 프로세스와 보안 세션을 점검한다. 대상은 codex app-server, 빌드 캐시 서버, gradle·bazel 같은 첫 사용 daemon이다.
- **E2E:** 「agent가 daemon을 띄우고 다른 pane이 재사용」하는 경우를 시험한다.

### 10.9 업그레이드 뒤 「재등록과 같은 일」

- **hook 신뢰:** hook 명령 문자열을 고정한다(§8.11).
- **quarantine:** updater가 직접 내려받은 파일에는 quarantine을 붙이지 않는다. 이를 `xattr`로 확인하는 테스트를 둔다. 공증은 쓰지 않는다(결정 9). 그래서 quarantine이 붙은 사본은 Gatekeeper 창이 뜨므로 설치·교체 도구가 반드시 quarantine 없이 옮긴다(`ditto --noqtn`, `p0/install-app.sh`).
- **첫 실행 승인 창:** 승인 창이 GUI에 뜨면 ssh 실행이 그 자리에서 멈춘다. 운영에서도 `_dyld_start`에서 멈춘 일이 있다. 그래서 E7에서 0건임을 확인한다.

## 11. 설치·업데이트·버전 호환

- **설치**
  - macOS:
    1. `aaa.app`을 배치한다.
    2. §10.1 목록을 사용자가 한 번 등록한다.
    3. `aaa agent install`로 LaunchAgent를 등록한다. host 머신이면 `aaa host install`도 한다.
    4. enrollment와 `aaa integration install`을 한다.
  - Windows: per-user로 `%LOCALAPPDATA%\aaa\bin`에 설치하고 예약 작업을 등록한 뒤 enrollment한다. 관리자 권한은 필요 없다.
  - Linux: `~/.local/bin/aaa`, systemd user unit, linger 승인 뒤 enrollment한다.
- **버전 고정:** `host.toml`의 `[update] version = "x.y.z"`로 고정한다. major를 자동으로 올리지 않는다. 운영에서는 update 스크립트가 원치 않는 1.0으로 올리려 한 일이 있다.
- **산출물과 배포:** OS/arch별 서명 package와 서명 manifest(ed25519)다. host가 내려받아 검증하고, agent에는 Agent link로 보낸다. agent 머신에는 인터넷과 SSH가 필요 없다.
- **교체**
  - 같은 볼륨에 staging한 뒤 self-check를 하고 rename으로 교체한다. 제자리 쓰기는 금지한다.
  - macOS는 번들을 통째로 rename한다.
  - Windows는 실행 중인 exe를 rename한 뒤 교체한다. 예약 작업은 고정 경로를 가리킨다.
  - 실행 중인 프로세스는 이전 이미지로 계속 돈다.
- **순서:** host → 로컬 agent → 원격 agent를 롤링한다. agent backend 재시작은 E3 결과에 따라 무중단이거나 drain이다. 실행 중인 session host를 끊는 자동 update는 하지 않는다.
- **호환**
  - 같은 major 안에서 host는 agent minor N-1까지 지원한다. agent는 더 새 host와 hello로 협상하고, 지원하지 않는 기능은 플래그로 끈다.
  - 실행 파일이 하나라서 갱신 직후 같은 머신에서 새 host와 이전 agent backend가 함께 도는 것이 정상 상태다.
  - session host 내부 프로토콜은 이전 session host와 호환해야 한다. 세션은 몇 주 이어질 수 있다.
- **DB·설정:** DB는 expand/contract로 바꾼다. 이전 프로세스가 남아 있는 동안 설정 migrate를 미룬다.
- **롤백:** 이전 산출물을 보관한다(개수는 TOML). DB 백업을 복원하고 `aaa update rollback`을 실행한다. 서명이나 다운로드가 실패하면 fallback 설치를 하지 않는다.

## 12. 기술 스택 추천

도입 버전은 구현 승인 뒤 lockfile로 고정하고 세 OS build·test로 확인한다. 라이브러리가 지원한다고 밝힌 플랫폼과 aaa가 직접 검증한 결과를 구분한다.

| 계층 | 추천·이유 | 세 OS·빌드 고려 | 대안·선택 조건 |
|---|---|---|---|
| 비동기 | Tokio. 연결·사건·timer 생태계와 Axum 통합 | blocking PTY·DB 작업은 전용 thread | async-std를 섞지 않는다 |
| PTY·spawn | portable-pty(herdr가 vendor한 0.9.0과 ConPTY 패치를 참고) + 얇은 OS adapter | Windows job·종료·수명, macOS spawn 속성은 직접 검증한다 | 한계가 확인되면 openpty·Windows API를 직접 쓴다 |
| VT | adapter 뒤 `vt100` 0.16(M1-1 구현). ghostty-vt는 후보로 남긴다 | ghostty-vt는 Zig 0.16.0 추가 빌드 도구가 필요하다(구현 Mac·CI에 없음). vt100은 순수 Rust라 세 OS CI에서 바로 빌드된다 | ghostty-vt(정확도·성능 비교 뒤 교체), alacritty_terminal. vte는 parser뿐이다 |
| TUI front | Ratatui+Crossterm | cell grid diff를 받는 얇은 client | 순수 ANSI UI는 부담이 크다 |
| Front API·Agent link | serde JSON + schemars JSON Schema → TS 타입 생성, binary terminal frame | UDS/Named Pipe·TCP+TLS adapter | 공개 다언어 SDK가 커지면 Protobuf. bincode를 공개 계약으로 쓰지 않는다 |
| Agent link 보안 | rustls TLS 1.3 + rcgen 자체 서명 인증서 pinning | 세 OS 공통 Rust | Noise IK(snow) |
| 웹 서버 | Axum+Tower, host 프로세스 안 | 중심 macOS | Bun/Hono는 runtime·타입 중복이 늘어난다 |
| 웹 front | TypeScript+React+Vite+xterm.js | 모바일 IME·WebKit은 따로 검증 | Svelte. web-ui component 개념을 쓰려면 React가 유리 |
| 저장 | rusqlite(bundled) + Markdown | DB I/O 전용 worker | 순수 파일+lock은 사건·outbox 정합성 비용이 크다 |
| 설정 | toml(serde) + toml_edit(주석 보존 migrate) | 세 OS 경로 규칙(§7.1) | 없음(결정 5) |
| clipboard·이미지 | front: arboard 후보 + 제한된 PNG/JPEG decoder. 웹: paste·Blob streaming | agent 머신에는 clipboard가 필요 없다 | osascript·wl-paste·xclip은 선택 adapter |
| macOS 앱·SMAppService | objc2-service-management(Rust, `aaa service`) | 자체 서명 pipeline(`p0/build-app.sh`), 공증 없음 | 작은 Swift 부분 |
| ID·관측 | ULID 또는 UUID, tracing 구조화 로그 | wall-clock 대신 boot ID+sequence, secret redact | OpenTelemetry는 초기 필수가 아니다 |
| 웹 도구 | Node LTS + lockfile | TS build·Playwright에만 쓴다 | Bun은 선택 |
| 배포 | OS/arch release + ed25519 서명 manifest, macOS `aaa.app` | 고정 경로·rename 교체 | Homebrew는 배포 선택지. Cellar 경로를 등록 대상으로 쓰지 않는다 |

primary 근거:
- [Tokio](https://tokio.rs/)
- [portable-pty](https://docs.rs/portable-pty/0.9.0/portable_pty/)
- [ConPTY 구현](https://github.com/wezterm/wezterm/blob/main/pty/src/win/conpty.rs)
- [Axum](https://docs.rs/axum/latest/axum/)
- [VT API](https://ghostty.org/docs/vt)
- [vte](https://github.com/alacritty/vte)
- [rusqlite](https://github.com/rusqlite/rusqlite)
- [xterm flow control](https://xtermjs.org/docs/guides/flowcontrol/)
- [SMAppService](https://developer.apple.com/documentation/servicemanagement/smappservice)

## 13. 테스트 전략과 직접 만들 E2E 도구

**100% 커버리지를 목표로 하지 않는다.** 대상은 상태·권한·전달·수명·복원처럼 틀리면 결과를 잃는 경계다. 외부 도구가 관측을 제공하지 않으면 작은 harness를 직접 만든다. production에 테스트용 bypass endpoint를 만들지 않는다.

### 유닛·계약 테스트

| 대상 | 단위 검증 | 잡으려는 실패 |
|---|---|---|
| thread 상태 reducer | delivery_unknown·stale·question·review·resolve 전이, 재시작 뒤 복구 | 끊김을 done으로 분류, 중복 resolve |
| 재입력 규칙 | delivery_unknown에서는 재전송 0, 이어가기 정책의 상한·기록 | 같은 prompt 재입력, 무한 이어가기 |
| profile·resume | OS별 argv·effort·model·daemon 금지 인자·revision 합성 | resume 때 옵션 소실, Windows quote 손상 |
| Front API·Agent link | ID dedupe·receipt·boot fencing·bounded frame·snapshot cursor·hello 버전 협상 | 재접속 때 재입력, 오래된 pane 입력, 버전 skew |
| 설정(TOML) | schema 버전, 모르는 key, 잘못된 값 위치, local 겹치기, migrate 백업·주석 보존 | 조용한 기본값, 사용자 파일 손상 |
| 보고·파일 | hash·부분 복사·지연 스트리밍·symlink/junction·path traversal | 회수 전 삭제, 다른 머신 파일 접근 |
| 이미지 attachment | 실제 포맷·상한·digest·receipt·target generation·adapter 입력·참조 수명 | bytes 도착 전 경로·Enter 삽입, 중복 붙여넣기 |
| question·message | 선택지 매핑, lease 재확인, interrupt·queue 순서 | 잘못된 pane에 답 입력 |
| routine·inbox | 가상 clock의 schedule·DST·missed-run·ack·중복 억제 | 재시작 뒤 중복 실행 |
| 권한 | reader·controller·lease·취소·Origin/CSRF | 읽기 front의 입력, 취소 뒤 pending write |
| 역할·사후 평가자 | 목록 기본 role 필터, 평가자 token의 금지 동작 거부, 사건 cursor·debounce, 평가 기록 schema·채택·보류 | 평가자가 일반 목록에 섞임, 다른 agent에 지시, 알림 유실로 사건 누락 |
| adapter | Claude·Codex hook·화면 fixture를 버전별로 decode | 사건 오판, UI 문구 변화로 잘못된 키 |
| Tailscale 공개 | `status --json`·`serve status --json` fixture로 포트 선택·상태 판정 | 기존 매핑 덮어쓰기, 잘못된 주소 |

단순 wrapper·getter의 mock 호출 횟수를 고정하는 테스트는 만들지 않는다. schema 호환은 실제로 저장한 구버전 fixture로 검증한다. fuzz·property 검증은 frame·path·state처럼 입력 공간이 큰 경계에 집중한다.

### 직접 만들 도구

| 도구 | 구성 | 관측·단언 |
|---|---|---|
| `aaa-e2e` 실행 harness | Rust PTY·ConPTY driver, 임시 state root·포트·socket, 소유 PID/handle registry | 명시 deadline·사건 대기, 잔여 프로세스·파일 receipt. 실패하면 transcript·버전·seed를 저장 |
| `aaa-lab` 다중 머신 재현 | 한 머신에 agent backend 여러 개(인스턴스별 state root·포트·machine ID·TOML) + Agent link fault proxy | 지연·단절·재순서·느린 링크·인증 실패·version skew(이전·새 binary 혼합)·live 세션 중 host/agent 재시작. prompt 자동 재전송 0 |
| 터미널 fixture executable | UTF-8·한글·emoji·ANSI·alternate screen·resize·Ctrl-C 시나리오 | 실제 키 입력·grid·scrollback·reconnect. 유료 API 불필요 |
| 화면 단언기 | production ghostty와 독립된 vt100/vte 기반 관측 parser | raw stream과 TUI front 출력에서 visible text·cursor·cell region |
| TUI front harness | harness PTY 안에서 aaa TUI를 실행 | layout·attach/detach·lease·stale 표시를 화면으로 단언 |
| OS lifecycle adapter | Mac LaunchAgent, Linux user unit, Windows 예약 작업을 sandbox root에서 실행 | 부모 shell·SSH·웹 종료 뒤 생존, Windows job 탈출·ConPTY deadlock |
| Mac 성능 probe | fresh-binary 방식을 doctor와 P0 harness로 정제 | 책임자·서명·키체인 실제 접근·first/warm·처리량. 결과 기록 |
| 웹 headless driver | Playwright + 실제 host + fixture agent | pairing·revoke·reader 금지·attach·lease·보고 sandbox·모바일 composer |
| 이미지 clipboard·agent fixture | 격리 runner의 native clipboard writer, PTY driver, 받은 경로의 파일을 여는 fixture agent | 대상 머신에서 decode·pixel hash·draft 보존·Enter 순서 |
| soak | 24시간 이상 | 메모리·scrollback 상한·재연결 반복 |

- **독립 관측:** screen fixture와 production 엔진이 같은 버그를 공유하지 않게 관측을 독립시킨다.
- **가짜 LLM과 실제 smoke:** 가짜 LLM은 사건을 결정적으로 재현한다. 실제 Claude·Codex의 hook·resume·이미지 소비는 release 전에 소수의 명시적 smoke로만 확인한다.
- **미실행은 합격이 아니다.** TCC·책임 프로세스·재부팅은 hosted CI에서 검증할 수 없다. P0 harness를 수동으로 돌려 결과를 기록하고, CI에서는 「미실행」으로 표시하며 합격으로 세지 않는다.

### E2E 배치

| 위치 | 매 변경 검증 | release·nightly 검증 |
|---|---|---|
| macOS native runner(예: macos-26) | Rust 유닛·계약, Unix PTY, host+agent+fixture, `aaa-lab`, Playwright Chromium | 승인된 시험 Mac의 P0 항목(등록·키체인·재부팅·처리량) |
| Linux native runner(예: ubuntu-24.04) | agent binary·PTY·경로·Agent link·부모 종료 생존 | systemd user·linger 유무, arm64 |
| Windows native runner(예: windows-2025) | agent·ConPTY·PowerShell/cmd·Unicode 경로·CRLF·Named Pipe ACL·Job handle·Agent link | 표준 사용자·예약 작업 logon·logoff/reboot·DPAPI |
| 격리 세 OS lab + 중심 Mac | 해당 변경에서 전체 시나리오 | 실제 세 OS thread·보고 회수·단절·복귀·업데이트·rollback |

- **runner 이름:** [GitHub 지원 목록](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)을 따른 예시다. 이름은 pin해서 쓴다.
- **hosted job의 한계:** hosted job끼리 직접 접속된다고 가정하지 않는다.
- **실패 처리:** 실패에 자동 재실행을 붙이지 않고 첫 실패의 증거를 보존한다. timeout은 미리 정한 failure bound이며 늘려서 통과시키지 않는다.
- **측정 전 rebuild:** 성능 비교와 화면 캡처 전에는 현재 소스로 산출물을 다시 만든다.

### 이미지 붙여넣기 E2E 합격 기준

| 시나리오 | 실제 경계의 단언 |
|---|---|
| Mac TUI front Ctrl+V → local·세 OS 원격 | 격리한 PNG를 native clipboard에 넣고 실제 TUI에 Ctrl+V를 보낸다. 대상 fixture agent가 자기 native path를 열어 decode한 pixel checksum을 출력해야 한다. 경로 출력이나 API 응답만으로는 합격이 아니다 |
| Windows·Linux client adapter | PNG·CF_DIB(V5), 격리 X11/Wayland에서 native writer → Ctrl+V → 대상까지. desktop이 없는 runner에서 미실행이면 합격이 아니다 |
| 웹 Ctrl+V → 세 OS | HTTPS 공개 주소(시험 tailnet 또는 localhost)의 실제 host와 fixture agent로 검증한다. synthetic 이벤트는 component 계약일 뿐이다 |
| 입력·대상 경합 | 한글 draft+이미지, 이미지 직후 Enter, 여러 이미지 순서, 느린 업로드 중 pane 전환·lease 상실을 본다. 텍스트 손실·중복·자동 Enter·잘못된 pane 입력이 없어야 한다 |
| 전송·수명·권한 | offset 재개, ACK 유실, agent 단절 시 즉시 실패·draft 유지, detach 뒤 지연 읽기, quota·disk full·MIME 위장·Windows 한글 경로·junction·reader/revoke를 본다 |
| 실제 Claude·Codex smoke | 지원을 선언한 CLI 버전·OS에서 이미지 인식 경로와 draft를 확인한다. 모델 설명 문장을 기대값으로 쓰지 않는다 |

## 14. 단계별 로드맵

기간은 집중 개발 기준의 **작업일 추정**이며 약속이 아니다. M1 중간에 실제 진행 속도로 다시 산정한다. 시험 환경·서명 identity·권한 실험 승인이 선행 조건이다.

참고 구현 규모(이번에 셈):
- herdr `src`: Rust 285,077줄
- herdr-projects: 25,031줄
- herdr-web-ui: server 75,659줄 + src 59,940줄

이 규모를 보면 이전 추정(28~46일)은 비현실적이었다.

| 단계 | 기간 추정 | 내용 | 완료 기준·검증 |
|---|---|---|---|
| P0. 개발자 도구 gate | 3~6일 + 승인·시험기 대기(M1-1과 병행 가능) | E1~E7, 서명 identity 결정, 프로토콜·수명 ADR | §10.6의 gate 실측. 실패하면 설계를 수정. **P0 합격 전에는 운영 투입 금지** |
| M1-1. 골격 + 웹 HTTPS | 8~12일 | 단일 실행 파일 역할, TOML, Agent link(TLS·enrollment), 로컬 agent backend(개발 모드는 자식, 운영 모드는 LaunchAgent), session host·PTY·VT, `aaa pane run/read/send`, host 웹 최소판(pairing·세션 목록·terminal 보기·입력), `aaa web publish` | 다른 기기에서 Tailscale HTTPS 주소로 Mac 3대의 pane을 보고 입력한다. host 재시작 뒤에도 세션 유지. agent 단절 시 읽기 전용 표시. Windows·Linux는 Agent link 계약 테스트·CI build·PTY smoke |
| M1-2. TUI front·세션 관리 | 6~10일 | workspace/tab/pane layout, attach/detach, 여러 front lease, 재부팅 뒤 resume spec | TUI harness로 화면 단언, 여러 front 동시, 재부팅 뒤 interrupted 표시 |
| M1-3. 오케스트레이션 | 10~15일 | projects 0.2 핵심(부록 A의 M1 항목), 상태 사건(Claude·Codex adapter), 흐름 사건·cursor 구독과 역할 필드(평가자 대비, §8.13), 확인창 원격 응답, 메시지 API, 결정 기록, 병합 대기열 순번·사건, 머신 건강·자원 한도, routine을 agent에서 실행 | Mac 중심 → Mac 3대 thread 시작·보고 회수·ack·resolve. 질문 원격 응답. polling 없는 상태 표시 |
| M1-4. 이미지·결과물·알림 | 4~7일 | 웹 이미지 paste(macOS 대상), 결과물 HTTPS 보기·지연 스트리밍, webhook. TUI paste는 M1 마무리, Web Push는 M3(§8.20) | 이미지 E2E(macOS), 200MB 넘는 library 보기, 질문·보고 알림 |
| → **M1 완료 = macOS 운영 대체 가능** | 합계 28~44일 | | P0 합격 + M1 기준 + shadow 비교 |
| M2. Windows·Linux agent 기능 합격 | 10~16일 | 감독자, ConPTY, 경로·ACL, linger, 이미지 대상, 세 OS lab | 세 OS thread·보고·이미지·단절·업데이트 E2E |
| M3. 확장 | 11~19일 | 사후 평가자 기능(§8.13·§8.22, **10-17 구현**), 병합 단계 자동 실행(§8.24, **10-17 구현**), 모바일 composer·IME(§8.23, **10-17 구현**), chat read-only(§8.27, **10-17 구현**), PWA·Web Push(§8.23·§8.26, **10-17 구현**), TUI Ctrl+V·세 OS front clipboard(§8.28, **10-17 구현**), thread 범위 token(§8.25, **10-17 세션 역할로 구현**) | 웹 headless + native, 병합 E2E. 평가자: 가짜 LLM 평가자가 착지 사건 묶음을 받아 평가 기록을 내고, 목록에 섞이지 않으며, 다른 thread 지시가 거부됨 |
| M4. 이전·전환 | 4~7일 + 운영 대기 | inventory, shadow, pilot, 전환 gate, rollback rehearsal | §15, 기존 작업 보존 |

- **합계:** P0 3~6일 + M1 28~44일 + M2~M4 25~42일 = **약 56~92 작업일**에 승인·환경 대기가 더해진다.
- **사후 평가자 배치:** MVP 필수가 아니므로 M3에 두었다. 역할 필드·흐름 사건은 M1-3에 있으므로, 원하면 M1 직후 M2와 독립적으로 먼저 할 수 있다.
- **MVP 범위:** M1 범위를 macOS로 둔 것은 [정할 것 1](#16-위험과-정할-것)이다. 3 OS 기능 합격을 MVP에 넣으면 M2가 M1 안으로 들어와 첫 사용 시점이 늦어진다.
- **세 OS 프로토콜·빌드는 M1-1부터 함께 간다.** 바뀌는 것은 기능 완성 시점뿐이다.
- **실제 속도로 다시 잡은 기간(10-16, M1-1 실측).**
  - M1-1은 구현 agent 한 thread가 약 40분(18:10~18:47)에 macOS 기준을 채웠다. 범위는 backend·웹·E2E·실제 Mac 3대 HTTPS이고, Windows CI smoke는 그 뒤 고치는 중이다.
  - 위 「일」 추정은 사람 작업 기준이라 구현 시간에는 맞지 않는다. 남은 단계는 구현 시간으로 M1-2 2~4시간, M1-3 6~10시간, M1-4 3~5시간으로 다시 잡는다.
  - 실제 일정을 정하는 것은 구현 시간보다 사용자 작업·승인 대기다. launcher 등록(결정 14로 시험 Mac 불필요), 실제 Claude·Codex smoke, 휴대폰 확인, 운영 이전 shadow 기간이 여기에 든다. 그래서 위 표의 「일」 단위는 대기와 검증을 포함한 상한으로 남긴다.

## 15. 현재 운영에서 이전

1. **읽기 전용 inventory.**
   - herdr 0.9.5와 projects 0.2.35의 machine·session·pane·thread·profile·report·library를 export하고 hash를 보존한다.
   - 토큰과 비밀은 가져오지 않는다.
2. **별도 namespace의 shadow.**
   - aaa의 state root·포트·worktree 접두사·hook namespace를 분리한다.
   - 기존 thread는 제어하지 않고 상태·보고만 비교한다.
3. **새 작업부터 pilot.**
   - 기존 herdr thread는 기존 도구로 마무리한다.
   - 새 low-impact thread만 aaa에서 시작한다. live takeover는 약속하지 않는다.
4. **결과·문맥 이전.**
   - 완료된 report·library·memory는 검증한 사본으로 가져온다.
   - 진행 중인 agent는 사용자가 정한 idle 시점에 session ID와 resume spec으로 명시적으로 재개한다.
5. **전환 gate와 rollback.**
   - active thread마다 소유 도구를 하나만 둔다.
   - aaa가 실패하면 새 시작을 멈추고, 보고와 문맥을 기존 coordinator가 읽을 수 있게 export한다.
   - 기존 설치는 검증 기간 동안 유지한다.

공존 기간에 생기는 충돌과 대책:
- **Tailscale serve:** herdr-web-ui가 7317과 serve 후보 포트(443·8443·7317·17317)를 쓴다. aaa는 다른 웹 포트를 쓰고, serve status를 읽어 빈 포트만 쓰며 기존 매핑을 덮지 않는다.
- **hook:** aaa hook은 aaa 세션 token이 없으면 아무것도 하지 않는다. 설치하면 codex가 신뢰를 한 번 다시 물을 수 있으므로 미리 알린다.
- **herdr-web-ui 자동 재연결:** 승인 없이 bridge와 herdr를 띄운다. 이전 기간에 해당 머신을 web-ui 원격 목록에서 빼거나 자동 재연결을 끌지는 사용자가 정한다. aaa는 herdr 쪽 설정을 바꾸지 않는다.
- **계보 점검:** 같은 Mac에서 herdr 서버(WezTerm 계보)와 aaa agent backend(LaunchAgent)가 각자 올바른 계보인지 doctor로 함께 확인한다.
- **버전 고정:** `herdr-projects update`처럼 지정하지 않은 최신판(1.0)으로 넘어가는 경로는 쓰지 않는다. source pin·import schema를 명시하고 원본 0.2 자료를 덮어쓰지 않는다.

## 16. 위험과 정할 것

### 정할 것(사용자 결정)

2026-10-16 사용자 「진행해」「추천대로 해」로 1~8 모두 **추천안으로 결정**했다(결정 8). 6번의 문장은 AGENTS.md에 넣었다. 2번은 같은 날 결정 9로 바뀌었다(유료 회원이 아니라 Developer ID 불가).

| # | 질문 | 선택지 | 추천 |
|---|---|---|---|
| 1 | MVP 범위 | macOS 운영 대체 먼저(Windows·Linux는 계약·빌드·smoke, 기능 합격은 M2) / 3 OS 기능 합격을 MVP에 포함 | **macOS 운영 대체 먼저.** 지금 운영이 Mac 3대이고 프로토콜·빌드는 처음부터 3 OS다 |
| 2 | 서명 identity | Developer ID / 자체 서명 인증서 / 불변 adhoc broker | ~~Developer ID~~ → 결정 9(자체 서명) → **결정 14로 변경:** 불변 launcher를 등록하고 서명 identity는 보류 |
| 3 | 「재부팅 직후」 범위 | GUI 로그인 뒤(터미널 없이 ssh·웹 우선) / FileVault·GUI 로그인 전까지 포함 | **GUI 로그인 뒤.** 로그인 전은 키체인 무잠김을 보장할 수 없고 자동 unlock도 쓰지 않는다 |
| 4 | 웹 입장 | aaa pairing·token만 / Tailscale-User-Login 헤더로 본인 기기 자동 입장 허용 | **aaa pairing만**(결정 6, 헤더 위조 위험) |
| 5 | 자동 이어가기 기본값 | 꺼짐 + profile·project별 opt-in / 켜짐 | **꺼짐 + opt-in** |
| 6 | AGENTS.md에 구조 원칙 한 줄 추가 | 추가 / 안 함 | **추가 권장.** 초안: 「backend(Rust)와 frontend(TUI·웹)를 분리한다. frontend는 상태를 갖지 않고 host backend하고만 통신하며, host backend가 각 머신의 agent backend와 통신한다.」 사용자가 정한 파일이라 승인 전에는 바꾸지 않았다 |
| 7 | Linux linger | 설치 때 `loginctl enable-linger` 승인 / foreground 감독만 | **linger 승인**(SSH 로그아웃 뒤에도 세션 유지) |
| 8 | Windows 로그온 전 agent | 사용자 로그온 예약 작업 / 시스템 서비스 | **사용자 로그온.** Session 0은 사용자 자격 증명과 다르다 |

**나중 후보(사용자 결정 대기).**

- ~~실행 파일 이름 `aa`(Apple Archive 도구 `/usr/bin/aa`와 겹침)~~ → 결정 18로 `aaa`로 바꿨다.

### 위험

| 위험 | 영향 | 대응 |
|---|---|---|
| 불변 launcher 자식도 업그레이드 뒤 느려짐(TCC가 자식 binary를 따로 봄) | 업그레이드마다 재등록 | L1에서 실측. 실패하면 결정 9(자체 서명 인증서)를 다시 꺼낸다 |
| SMAppService.agent 자식의 책임 귀속이 예상과 다름 | 느림·키체인 | E2에서 login item과 비교 |
| GUI 계보 warm 저하 | build 전체가 느려짐 | E2 원인 조사, App Nap·QoS 설정 |
| agent backend 재시작 때 session host가 면제를 잃음 | 업데이트 뒤 느림 | E3. 결과에 따라 drain |
| VT 정확도(vt100) | 복잡한 TUI 화면 재생이 어긋남 | M1-1은 vt100으로 시작했다. M1-2 TUI harness는 셸 화면(넓은 글자 포함)까지 독립 parser로 단언했다. Claude·Codex 화면 단언은 그 agent를 실제로 띄우는 M1-3에서 하고, 어긋나면 ghostty-vt(Zig 빌드)나 alacritty_terminal로 adapter 뒤에서 교체 |
| harness hook·notify 지원 범위 | 사건화가 불완전 | adapter별 fixture, 화면 보조와 신뢰도 표시 |
| Tailscale serve CLI 변경 | 공개 실패 | 읽기 전용 상태 판정 fixture, 실패 시 할 일 표시 |
| 비공개 macOS API 변경 | 진단 실패 | timing probe fallback |
| 로드맵 초과 | 전환 지연 | M1 중간 재산정, 범위는 부록 A 기준으로 조정 |
| 사후 평가자가 사건마다 깨어남 | 토큰 비용·잡음 | 사건 묶음 debounce, 대상 사건 선택(project.toml), 판정은 기록으로만 |

중심 머신의 Linux·Windows 지원은 후속 범위의 열린 질문이다. agent 머신의 세 OS 지원과 혼동하지 않는다.

## 17. 이번 작업 검증·전달 상태

- **첫 조사:** 세 소스를 `research/<tool>/`에 확보하고 commit을 고정했다. 10조건×6개 fresh executable을 측정했다. 설정·개발자 도구·키체인·launchd·운영 herdr 서버는 바꾸지 않았다.
- **이번 개정:** 독립 검토 지적 24개, 운영 경험 10항목, 사용자 결정 1~7을 반영했다. 반영 여부는 부록 B에 있다. 실험을 다시 돌리지 않았고, 등록·재부팅이 필요한 검증은 P0으로 남겼다.
- **함께 고친 문서:** [소스 분석](source-analysis.md)의 사실 오류와 [실험 기록](experiments.md)의 재해석을 같이 고쳤다.
- **웹 보고서:** 한국어 웹 보고서는 로컬 `.coordinator/library/index.html`에 있다. 검증 결과는 `.coordinator/report.md`에 적는다.
- **다음 단계:** 구현은 사용자 승인 뒤 별도 thread가 한다.

## 부록 A. herdr-projects 0.2.35 기능 대응표

| 0.2.35 기능 | aaa 대응 | 단계 |
|---|---|---|
| 프로젝트 폴더(PROJECT/AGENTS/MEMORY/memory/TASKS/routines), coordinator = 프로젝트 cwd agent | 그대로(§8.1). 결정 기록·`project.toml` 추가 | M1-3 |
| profile(agent·model·effort·args·description), 허용 목록(thread/coordinator) | agent.toml 정의 + host.toml 허용, 해석 결과 저장(§7.2, §8.2) | M1-3 |
| thread 종류 worktree·tab·checkout·adopt, 원격은 worktree만 | 로컬·원격 기능 같음 + repo 없는 task dir(§8.12) | M1-3 |
| `thread start/restart/prompt/next/stop/brief/read/keys/list/show/ack/resolve` | Front API 명령으로 대응. read·keys는 pane read/send와 question 응답. `next`는 M1 마무리에서 했다(§8.21) | M1-3 |
| `trust_screens`, 확인창 키 응답 | question 사건 + 원격 응답(§8.4) | M1-3 |
| brief 전달(claim 60초·settle 3초·3회·`--wait --until working`) | 그대로 계승(§8.5) | M1-3 |
| `prompt_box` draft 감지 | adapter 입력 전 draft 감지 | M1-3 |
| report hash 복사, library 50MB 상한, symlink 불복사 | receipt·manifest·지연 스트리밍(§8.7) | M1-3·M1-4 |
| resolve 규칙(복사 Failed는 거부, Partial은 보존, merge된 branch만 삭제) | 그대로 + receipt(§8.7) | M1-3 |
| inbox(timestamp·kind·subject·counter, done 30일), 특수 item session·outage·config-error | DB 사건 + Markdown 투영, 특수 item 유지(§8.9) | M1-3 |
| ticker(15초, 원격 1분, 5분 유휴 종료, `.ticker.lock`) | host 안 task, 사건 구독 + reconciliation | M1-3 |
| routine(every·daily·`on = "pr"`, 명령 digest 승인, `routine_commands`, coordinator 없으면 skipped, `sh -c` 60초) | 그대로 의미 유지, 실행은 agent backend(§8.9). `on = "pr"`은 M3 | M1-3 |
| nudge(미확인 묶음 hash당 1회, `herdr agent prompt`, 입력창 비었을 때) | 그대로(§8.5) | M1-3 |
| progress 보고(`report --percent`), hook 5개 harness | 상태 사건 `progress` + adapter(Claude·Codex 먼저, 나머지 후속) | M1-3 / M3 |
| sidebar·overview·focus·needs-you·popup | TUI·웹 표시 | M1-2 / M3 |
| TASKS.md `(profile@machine)` 소유, `assignable`, `context` | 그대로 + `--from-task` 중복 방지(§8.21) | M1 마무리(완료) |
| safety·yolo `launch_args`, `start_threads` propose/auto | profile `args`(agent.toml)·project.toml `start_threads`(§8.21) | M1 마무리(완료) |
| `threads/<id>.task.md` Follow-ups, `<id>.next.md` | thread 기록에 포함, restart 때 다시 읽기 | M1-3 |
| `owned.json` configure journal | integration journal(§8.11) | M1-3 |
| `launch_attempts` 3회, starting 5분 timeout | launch_state 전이 | M1-3 |
| memory 32,000자 상한 | brief 조립 상한(project.toml) | M1-3 |
| `Landing` group·PR 우선순위 | review_state·병합 대기열(§8.8) | M1-3 / M3 |
| rename·sweep·archive·pause·resume·delete | 같은 의미의 명령 | M3 |
| `update` 명령(1.0으로 올라감) | 쓰지 않음. 버전 고정(§11) | 없음 |
| uploads·scratch | 프로젝트 폴더에 유지 | M1-3 |
| 1.0의 「보드」 | 범위 밖 | 없음 |

## 부록 B. 검토 지적 반영표

출처: 로컬 `.coordinator/review.md`(지적 1~24, 운영-1~10), `.coordinator/decisions.md`.

| 지적 | 요지 | 반영 | 위치 | 미반영·부분 이유 |
|---|---|---|---|---|
| 1 | 3층 경계 부재, front→agent 직결 경로 | 반영 | §4.1~4.3, §9 | |
| 2 | TUI 얇음·화면 형식·정본·단절 시 동작 | 반영 | §4.4~4.5 | |
| 3 | 단일 실행 파일과 불변 broker 충돌, 고정 서명 identity 필요 | 반영 | §10.3~10.5, §11 | 서명 identity 선택은 정할 것 2 |
| 4 | 로컬 agent backend 자식 기동의 계보 위험 | 부분 | §4.2, §5, §10.4, §10.7 | session host 독립성(disclaim이냐 drain이냐)은 P0 E3 결과 전까지 보류 |
| 5 | launchd 후보를 herdr 고유 문제로 탈락 | 반영 | §10.3, §10.6 | |
| 6 | Tailscale HTTPS 공개 누락·P4 배치 | 반영 | §6.3, §14 M1-1 | 헤더 자동 입장 여부는 정할 것 4 |
| 7 | 전송 이원화(SSH/WSS) → 설정 주소 TCP | 반영 | §6.1 | |
| 8 | GUI 계보 warm 3배 저하 누락 | 반영 | §10.1, §10.2, §10.6 E2 | 원인은 미측정(승인 필요) |
| 9 | 등록 목록에 전체 디스크 접근 등 누락 | 반영 | §10.1 | |
| 10 | 하위 daemon(codex app-server) | 반영 | §8.2, §10.8, E6 | |
| 11 | 업그레이드 뒤 hook 재신뢰·quarantine | 반영 | §8.11, §10.9, E7 | |
| 12 | host·agent 버전 호환·업데이트 순서 | 반영 | §11 | |
| 13 | TOML 위치·스키마·마이그레이션·오류 | 반영 | §7.1~7.2 | |
| 14 | host의 직접 실행(routine 등) | 반영 | §4.2, §8.9 | |
| 15 | 재부팅 제품 동작, Linux linger | 반영 | §10.7, §5 | 로그인 전 범위는 정할 것 3, linger는 정할 것 7 |
| 16 | projects 0.2 세부 동작 누락, nudge 방식 | 반영 | 부록 A, §8.5 | nudge는 0.2.35 방식(재전송 없이 재독)을 채택 |
| 17 | 로드맵 비현실, MVP 재정의 | 반영 | §14 | MVP 범위는 정할 것 1 |
| 18 | 테스트 보완(다중 agent backend 등) | 반영 | §13 | |
| 19 | 참고 소스 재사용(Windows native, manifest pin, web-ui 기능) | 반영 | §2, §5, §8.3 | |
| 20 | 소스 분석 사실 오류 | 반영 | source-analysis.md | |
| 21 | 라이선스 세부 | 반영 | §2, source-analysis.md | web-ui npm 의존성(node_modules 없음)은 구현 때 lockfile로 확인 |
| 22 | 보안 세부(감사 로그·시도 제한·CSP·thread token·16MiB 동작) | 반영 | §6.4, §9, §8.25 | thread 범위는 세션 역할로(10-17) |
| 23 | AGENTS.md에 구조 원칙 추가 여부 | 반영 | AGENTS.md | 결정 8(정할 것 6 추천안)으로 추가 |
| 24 | 이전 경로 공존 충돌 | 반영 | §15 | |
| 운영-1 | 상태·알림 사건화, 자동 이어가기 | 반영 | §8.3, §8.5 | 이어가기 기본값은 정할 것 5 |
| 운영-2 | 확인창 원격 응답 | 반영 | §8.4 | |
| 운영-3 | 로컬·원격 같은 경로 | 반영 | §4, §8.12 | |
| 운영-4 | 계보 자가 점검 | 반영 | §10.4(6), §8.10 | |
| 운영-5 | 업그레이드(고정 경로·버전 고정·되살리기 금지) | 반영 | §11, §8.11 | |
| 운영-6 | 머신 건강 | 반영 | §8.10 | |
| 운영-7 | 메시지 API·결정 기록 | 반영 | §8.5, §8.6 | |
| 운영-8 | 결과물 HTTPS·크기·알림 | 반영 | §8.7, §6.3 | |
| 운영-9 | 병합 대기열·정리 | 부분 | §8.8 | M1-3은 순번·사건·훅까지, 단계 자동 실행은 M3 |
| 운영-10 | 안전·자원 한도 | 반영 | §8.10 | |
| 결정 1 | front·backend 분리 | 반영 | §4 | |
| 결정 2 | 3층 구조 | 반영 | §4 | |
| 결정 3 | host 전용·단일 실행 파일·로컬 agent backend | 반영 | §4, §5, §10, §11, §13 | MVP 자식 기동은 개발 모드로 한정(§4.2) |
| 결정 4 | 구현 진행 방식 | 해당 없음 | 없음 | 사용자 지시대로 로드맵에 넣지 않음 |
| 결정 5 | TOML | 반영 | §7 | |
| 결정 6 | tailnet 모름, 설정 주소 TCP | 반영 | §6.1 | |
| 결정 6 예외 | Tailscale HTTPS 공개, MVP 첫 단계 | 반영 | §6.3, §14 | |
| 결정 7 | 사후 평가자 역할(배경 실행·별도 표시·attach·사건 자동 입력·평가 기록·직접 지시 금지) | 반영 | §3, §6.2, §6.4, §8.3, §8.13, §13, §14 | 기능은 M3, 역할 필드·흐름 사건은 M1-3 |
