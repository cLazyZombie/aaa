# deploy_start 재시작 준비 경계 수리 (t-1244)

## 원인과 범위

`disconnected_start_keeps_a_stopped_record_across_host_restart_without_installing`은
host를 재시작한 뒤 일반 `status` 성공만 보고 마지막 `deploy status --wait`를 실행했다.
일반 Front 연결 준비와 로컬 agent의 Hello 자료 준비는 별개다.
`aaa-host/src/front.rs`의 `deploy`·`deploy-wait` 연결은 호출자의 pane 권한을 판정하기 전에
`local_machine_connected()`와 `host.changed` 사건을 기다린다.
따라서 일반 `status`가 성공해도 배포 CLI가 아직 준비되지 않았을 수 있다.

t-1240의 전체 검증 기록에는 이 대기와 agent 연결 사이가 5.021초여서 시험 PTY의 5초 상한을
넘었다고 적혀 있다. 이 작업은 그 지연의 세부 원인이나 제품 재접속 성능을 확정하지 않는다.
잘못된 시험 준비 경계를 수리한다. 제품 코드·프로토콜·권한 준비 신호는 바꾸지 않는다.

## 수리와 회귀 조건

기존 시험 하나를 강화한다. host를 내린 뒤 agent도 명시적으로 종료해 연결을 보류한다.
새 host가 준비되면 새 `HostWatch`를 열고 `mac-a`의 `Disconnected` 상태 사건을 먼저 확인한다.
같은 상태 폴더로 agent를 다시 띄우고 `Connected` 사건을 받은 뒤 배포 상태 CLI를 실행한다.
옛 host에서 받은 `Connected` 상태는 재사용하지 않는다. 연결 실패 진단에는 새 host·agent 로그를 남긴다.

이 제어가 필요한 이유는 자연 재접속이 빠른 머신에서도 「host는 준비됐지만 agent는 아직 없다」는
회귀 조건을 매번 만들기 위해서다. 이 시험은 agent 프로세스가 살아 있는 동안의 backoff·socket 감시
성능 시험을 맡지 않는다. 기존 배포 시작 거절·실패 종료·설치 단계 미시작·같은 ID의 JSON 정본 보존·
잘못된 SHA 거절·원인 로그 단언은 유지한다. 새 시험·삭제·고정 sleep·재시도·시한 확대는 없다.

## 수리 전 재현

기준 `e321baab`의 원래 시험은 단독 실행에서 1.39초에 통과했다. 자연 재접속만으로는 결정적인 재현이 아니다.
같은 코드에 아래 한 줄만 넣어 agent 연결을 보류했다.

```diff
     lab.kill("host");
+    lab.kill("agent");
     lab.spawn("host-reopened", &h, &["host"]);
```

```sh
nice -n 15 cargo test --profile e2e --locked -p aaa --test deploy_start -- --nocapture
```

이 재현은 마지막 `deploy status <id> --wait`의 `PTY 출력이 5s 안에 끝나지 않았다`로 실패했다
(전체 시험 6.44초, 종료 코드 101). 새 host 로그에는 `배포 상태 조회: 로컬 agent 자료 기다림`이 있다.
실패 증거는 t-1244 보고의 `library/red.log`, `library/red-host-reopened.log`,
`library/reproduce-red.patch`에 보존한다. 이는 agent 준비 누락의 결정적 재현이며,
원래 5.021초 지연 자체를 그대로 재현했다고 해석하지 않는다.

수리판은 같은 agent 연결 보류를 유지하고 새 상태 구독→끊김 관측→agent 기동→연결 관측을 거친다.
동일 명령이 1.49초에 통과했다. PTY와 연결 사건의 실패 상한은 기존 5초다.

## 참고한 공개 구현

- [Playwright Page.waitForEvent](https://playwright.dev/docs/api/class-page#page-wait-for-event):
  사건을 일으키기 전에 관측을 등록하는 순서를 따른다.
  [라이선스 Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE).
- [Tokio Notify](https://docs.rs/tokio/latest/tokio/sync/struct.Notify.html):
  시간 경과 대신 통지로 진행하는 경계를 비교했다.
  [라이선스 MIT](https://github.com/tokio-rs/tokio/blob/master/LICENSE).

외부 코드는 복사하지 않고 저장소의 기존 `HostWatch`·Front 상태 사건을 사용한다.

## 전체 검증

- `nice -n 15 bash scripts/ci-local.sh --logs target/ci-local/t-1244`:
  web(유닛 651개), fmt, build, strict clippy, test(Rust 1,483개 통과·7개 무시,
  Python 72개 성공·그중 2개 건너뜀), public 통과. 의존 경계 시험 `deps`도 통과했다.
- 이 실행의 대상 시험은 5.36초였다. 당시 runner의 load average는 시작 29.36·끝 31.01이었고,
  Rust 6묶음(묶음당 thread 4개)과 웹 E2E worker 10개가 겹쳤다. 단독 1.49초와 고부하 수치를
  구분하며 보통 부하의 5초 기준 통과를 병렬 결과 하나로 단정하지 않는다.
- 최초 전체 명령은 전역 Playwright 1.59.1과 저장소 요구 1.64.1의 불일치 때문에
  E2E 시험을 시작하기 전에 종료 코드 1이었다. 전역 패키지·브라우저는 바꾸지 않았다.
  작업 폴더에 `playwright@1.64.1` 패키지만 설치하고 공용 캐시의 Chromium 1248·WebKit 2370을
  사용한다. 다음 명령으로 빠진 E2E 단계를 보완했다. 시험 실패를 재실행해 넘긴 것이 아니다.

```sh
NODE_PATH="$PWD/.aaa/projects-t-1244/tools/node_modules" \
  nice -n 15 bash scripts/ci-local.sh --only e2e --logs target/ci-local/t-1244-e2e
nice -n 15 bash scripts/ci-linux-clippy.sh
```

웹 E2E는 132개 작업이 실패 없이 완료됐고 E2E 단계는 272초에 통과했다.
Linux 전체 workspace·all-targets strict clippy도 33.06초에 통과했다.

전체 명령은 미커밋 작업 중 실행했으므로 workspace finish 출처 보관은 생성되지 않았다.
결과 원본은 `target/ci-local/t-1244`·`target/ci-local/t-1244-e2e`와 t-1244 보고의 library에 보존한다.
착지 창을 받기 전에는 병합·push하지 않는다.
