# 전체 ci-local 3분 예산(t-1310)

수정본의 캐시 실행은 174.4초였지만 첫 커밋 SHA에서는 222.1초였다. SHA 변경의 빌드 무효화까지 포함해 최종 후보를 다시 측정한다.

사용자가 전체 검증 시간을 3분 안으로 줄이도록 허용한 시험 삭제와 실행 자원 변경이다.
제품 쪽은 panic SHA의 주입 위치와 분할된 검증 결과의 수집만 바꾼다. release 최적화·시스템 설정·공용 브라우저 설치는 유지한다. 실패 재시도·시한 확대·기대값 완화는 없다.

## 실행 범위와 비용

인자 없는 `scripts/ci-local.sh`는 web·fmt·build·test·e2e·public을 실행한다.
web은 타입·번들·웹 unit 전체, test는 Python 전체·Rust workspace 전체·의존 경계·doctest를 포함한다.
현재 소스에 남긴 E2E 전체를 Chromium·WebKit에서 실행한다. `--spec`이나 환경 filter로 측정 범위를 줄이지 않았다.

네이티브 workspace strict Clippy의 기본 gate 중복은 삭제했다. 개발의 관련 Rust owner strict Clippy,
GitHub OS별 명시 `--only clippy`, 착지 full의 Linux/Windows strict cross는 유지한다.
따라서 아래 180초는 cross·Windows production·전송을 포함한 배포 시간이 아니다.
전체 gate와 착지 정책은 [verification.md](verification.md)에 있다.

| 실행 | 전체 초 | Rust 초 | E2E 초 | 결과·해석 |
| --- | ---: | ---: | ---: | --- |
| 기준 | 642 | 319 | 536 | 7단계 통과. target·웹 의존성이 없던 최초 준비, build 92·Clippy 81초 포함 |
| 수정 1 | 265 | 250 | 217 | E12 축소 때 빠뜨린 기준 변수로 실패 |
| 수정 2 | 248 | 232 | 210 | E100의 앞 시험 잔여 상태 의존으로 실패 |
| 수정 3 | 223 | 189 | 149 | 전체 통과, 아직 예산 초과 |
| 수정 4 | 278 | 182 | 142 | 전체 통과. 효과 없던 의존성 최적화의 재빌드 비용 포함 |
| 수정 5 | 205 | 181 | 148 | 6단계 전체 통과, 아직 예산 초과 |
| 수정 6 | 214.6 | 185 | 124 | 전체 통과. 외부 전체 종료 시각 실측, Rust 4·브라우저 6으로 Rust가 병목 |
| 수정 7 | 192.8 | 178 | 170 | 전체 통과. Rust 6·브라우저 4와 실측 우선순위, 아직 예산 초과 |
| 수정 8 | 191.7 | 168 | 145 | 전체 통과. 추가 웹 축소, build 9초 포함. 아직 예산 초과 |
| 수정 9 | 174.4 | 154 | 136 | 전체 통과. SHA 변경 없는 캐시 조건, web 10·fmt 2·build 6·public 1초 |
| 첫 커밋 5304098d | 222.1 | 154 | 136 | 전체 통과·예산 초과. 새 SHA가 공통 crate와 backend 전체를 무효화해 build 54초 |

측정 머신은 10 CPU·32 GiB Mac이다. 모든 무거운 명령은 `nice -n 15`로 실행했다.
Cargo의 컴파일 worker 수는 지정하지 않았다. worktree의 target과 기존 컴파일 캐시만 사용했다.
Playwright는 작업 target에 npm package 1.64.1만 준비했고 브라우저는 기존 공용 캐시를 사용했다.
첫 기준과 수정본은 캐시 조건이 같지 않다. 캐시가 없는 신규 머신의 최초 컴파일까지 180초라고 보장하지 않는다.

SHA 변경 전 수정 9의 외부 실측은 174.4428초, 종료 코드 0이다. 단계 선택 없이 `--logs`만 지정했다.
Rust 1,467개 통과·기존 ignored 7개, Python 85개·기존 skip 2개, 웹 unit 660개, E2E 22파일 전체가 통과했다.
Rust 323작업과 브라우저 31작업을 실행했다. 최초 준비와 이번 캐시 상태의 차이를 위 조건과 함께 해석한다.
수정 6부터는 바깥 `time.monotonic()`으로
ci-local 자식이 종료하고 임시 실행 사본 정리·metrics finish가 끝난 시점까지 잰다. 내부 요약의 정수 초와 구분한다.

## 실행기 변경

- web·fmt·build를 완료한 뒤 Rust와 브라우저가 같은 CPU를 나눈다. 10 CPU는 Rust 6·브라우저 4이다.
  1 CPU는 직렬 실행한다. 브라우저만 실행하면 CPU 둘마다 worker 하나·최대 6개다.
- 브라우저 단계의 실제 종료 결과 파일을 확인한 뒤 Rust에 전체 CPU를 돌려준다. 이전 실행의 표지는 먼저 지운다.
  `resources.tsv`와 Rust events/state에 초기 예산·반환을 남긴다. 선택하지 않은 단계에는 worker 0을 주입하지 않는다.
- 기존 m13 고정 shard 대신 독립 통합 owner를 각각 작업으로 넣는다. install/e2e/m12/deploy_start와 macOS restart도
  독립 owner로 나눈다. CLI/host/agent unit은 모듈로, 큰 CLI install 묶음은 namespace로 나눈다.
  목록의 중복·누락, 실제 실행 수 불일치는 실패다. 기존 ignored와 doctest 범위는 유지한다.
  준비와 Clippy가 끝난 뒤 doctest도 같은 예산으로 배치해 마지막에 남던 4.5초 대기를 줄인다.
- 시간 원장은 긴 작업의 우선순위에만 쓴다. 특정 owner를 생략하거나 시한을 정하는 자료가 아니다.
- macOS 전체는 verify의 실제 CLI를 고정 사본으로 웹에도 사용해 같은 CLI의 e2e profile 빌드를 중복하지 않는다.
  다른 OS와 단독 build의 기존 e2e 산출물은 보존한다. 실행 사본은 복사 후 `cmp`, 종료 시 정리한다.
- Unix 시험의 기본 Tokio runtime은 2 worker로 둔다. 격리 CLI/host/agent마다 전체 머신 코어를 예약하던 비용을 줄인다.
  Windows native 환경과 사용자가 명시한 worker/thread·Tokio 값은 유지한다.
- 줄 단위 fake만 쓰는 기존 14 owner는 기존 `report_shell_world` 설정을 재사용한다. 실제 TUI·paste와 제품 기본 간격은 그대로다.

## 커밋 SHA의 빌드 경계

`aaa-common`의 생성 소스에 SHA 상수를 넣던 구조는 커밋만 바뀌어도 session/doctor/host/agent와 모든 시험을 재컴파일했다.
깨끗한 첫 커밋에서 build 54초·전체 222.1초로 재현했다. SHA를 고정하거나 검사를 건너뛰어 덮지 않는다.
최종 `aaa` build script가 실제 HEAD를 검증해 compile-time 값으로 주고, main이 panic hook에 주입한다.
공통 crate는 tracked Rust 상대 경로 목록만 만들며 내용이 같으면 생성 파일을 다시 쓰지 않는다.
기존 panic 출력의 SHA·경로 가림·payload/이름 비공개·네 역할·반복 횟수는 유지한다.
CLI의 새 owner는 내장 SHA와 실제 checkout을 비교해 누락된 Git 감시나 오래된 산출물을 잡는다.

깨끗한 실행은 지표 archive의 기존 128작업 상한도 드러냈다. 독립 owner 분할은 300개 넘는 작업을 만든다.
producer와 agent parser가 4,096작업까지 받아 실제 350개 요약을 온전히 보관/집계하는 회귀를 둔다.
기존 8 MiB 읽기 예산·상대 로그 이름 검증·stdout 제외는 유지한다.

## 삭제와 보존

웹 E2E는 124개 파일에서 22개로, m13의 이 머신 실행 owner는 154개에서 89개로 줄였다.
E2E 102파일·515단계 삭제에 더해 남긴 파일의 기기·반복 행렬을 줄였다.
Rust 통합 owner 65개의 이름·삭제 이유·남긴 빠른 검사는 [삭제 원장](testing-ci-budget-t-1310-removed.md)에 있다.
웹 unit 660개는 유지했다. 새 ignore·필터·기존 기대값 하향으로 숫자를 맞추지 않았다.

r-5206의 실제 결함 사례에 해당하는 다음 경계를 계속 실행한다.

| 경계 | 남긴 owner |
| --- | --- |
| 상속/dup fd 뒤 백업 잠금의 실제 소유 수명 | aaa-host backup unit·OS별 CI |
| Windows 원격 경로 분기 | host router/원격 경로 unit·Windows CI |
| iPhone 첫 threads 응답이 늦을 때 전송 상태 | E23의 iPhone drawer·ACK 경합 |
| 대량 작업의 500/601 경계·500+101 묶음 | E113·114, 규모·원문/DB·부분 실패·수동 재개 |
| history 복원 뒤 첫 휠 SGR | E109의 두 엔진·6가지 복귀 |
| 실제 CLI 배포와 backend 재시작 | m13 coordinator_deployment_is_gated_and_survives_backend_restarts |

작업 중 측정이라 metrics archive는 미커밋 출처를 확인할 수 없어 보관하지 않았다.
로컬 원문은 `target/ci-local/candidate-9/`의 단계 로그·Rust state/plan/events·`wall-clock.json`에 남았다.
착지 gate는 최종 통합 SHA에서 별도로 한 번 실행한다.

삭제한 화면의 장치별 CSS·전체 흐름, 복합 다중 머신 rollout 복구, CLI 메타데이터 편의 흐름,
실제 webhook 송수신과 pane 채택 시작의 별도 통합 커버리지는 줄었다.
알림/inbox 편의 흐름, 구형 mission·advisor 시작, 이미지 API의 전체 프로세스 통합도 삭제했다.
인증/권한 unit, 호출자 위조 거부, 대량 inbox frame, 계정 unpark 왕복, 자동화 시작/기억 복구 통합은 유지한다.
추가로 E15 iOS 소리/알림·안전 영역, E42 실제 diff 맥락/대량 patch, E63 배포 화면,
E47 task 연결/보관 화면, E104 대화 수명의 장치별 LRU, E106 자동화 이관, E118 기록 검색, E119 자동 병합 표시, E120 의견/인용 화면의 통합 검사를 삭제했다.
E15의 과거 실기 회귀에 대한 실제 CSS 단언도 사라진다. 관련 unit과 E09 IME·E23 전송·E39 뒤로가기는 남는다. unit 통과가 이 범위를 모두 대신한다고 주장하지 않는다.

## 실패와 철회한 실험

E12 축소가 같은 함수 안 `pushFrom` 등 공유 기준 변수를 지웠다. 원래 기준 변수를 복원했다.
E100에서 지운 51개 paging 시험이 뒤 삭제 동기화 시험의 미확인 결정 목록도 준비하고 있었다.
뒤 시험이 필요한 자동 답 두 개를 자기 단계에서 실제 명령으로 만들도록 바꿨다. 기존 목록·화면 단언은 그대로다.
Rust owner 삭제 뒤 남은 HTTP/webhook helper는 strict Clippy의 dead-code 실패로 확인하고 함께 제거했다.
350개 집계 회귀의 첫 fixture에는 실제 libtest 줄 끝의 세미콜론이 빠졌다. parser가 그 잘못된 줄을 거부한 원인을 확인하고 실제 형식으로 고쳤다. agent 21개 owner와 producer 4개 owner가 통과했다.
실패 로그는 `target/ci-local/candidate-{1,2}/`와 `final-m13-clippy.log`에 남겼다.

의존성 opt-level 3, CLI package opt-level 1은 대표 onboarding owner의 전체 실행 시간을 줄이지 못해 철회했다.
각각 약 1.5초였고 추가 컴파일 비용도 있었다. 반 슬롯으로 프로세스 owner를 더 띄운 실험도 목표 달성을 입증하지 못해 철회했다.
짧게 실행되는 CLI 자식의 CPU가 순간 `ps` 관측에서 빠지므로 그 관측만으로 CPU가 비었다고 단정하지 않는다.

## 필요한 실행기 회귀

기존 실패 전파·중단·고정 사본·기존 바이너리 최신성 검사는 보존했다.
CPU 예산의 누락/중복 예약 검사, 실제 소켓 사건으로 E2E 종료 뒤 Rust 슬롯이 반환되는 검사,
분할된 원래 owner가 정확히 한 번씩 들어가는 검사가 필요하다. 잘못된 분할은 검사를 조용히 생략할 수 있고,
예산 오류는 병렬 과부하 또는 미선택 단계의 기동 실패를 만들기 때문이다. 고정 sleep을 추가하지 않았다. SHA freshness와 350개 결과 회귀는 각각 진단 판의 정확성과 분할 뒤 지표 유실을 지키기 위해 필요하다.

## 추가 확인

시험 삭제 후 m13의 macOS/Linux strict Clippy가 통과했다. SHA/수집 후속은 aaa-common·aaa-agent·aaa의 native, Linux/Windows all-targets strict Clippy가 통과했다.
기존 panic 세 owner, 실제 checkout SHA owner, agent 집계 21개와 producer 4개 owner도 확인했다.
cross는 cfg·공유 helper·unused 경계 확인이며 Linux/Windows 실기 실행 결과가 아니다.
실행기 회귀 32개와 웹 분할/sleep 회귀 14개도 통과했고, 전체 실행에서는 Python 85개와 웹 unit 660개에 포함된다.
삭제 원장은 Git의 실제 삭제 파일과 Cargo의 전후 m13 목록 차이 65개에 정확히 일치한다.
fmt·셸 문법·변경 공백·공개 저장소 검사도 통과했다.
전체 OS cross·Windows native/production·병합·push는 착지 창에서 최종 통합 SHA로 수행한다.

## 참고한 공개 구현

- [Playwright parallelism](https://playwright.dev/docs/test-parallel)와 [Apache-2.0 라이선스](https://github.com/microsoft/playwright/blob/main/LICENSE):
  독립 실험실을 worker로 나누는 접근을 참고했다. 기존 runner와 브라우저를 사용한다.
- [nextest test groups](https://nexte.st/docs/configuration/test-groups/)와
  [MIT](https://github.com/nextest-rs/nextest/blob/main/LICENSE-MIT)/[Apache-2.0](https://github.com/nextest-rs/nextest/blob/main/LICENSE-APACHE):
  프로세스별 자원 제한과 독립 작업 배치를 대조했다. 코드나 의존성을 가져오지 않았다.
