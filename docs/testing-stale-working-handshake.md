# 화면 idle 복구 시험의 Hello 경합 검증(t-1276)

## 원인

[CI run 40732077808의 macOS job](https://github.com/cLazyZombie/aaa/actions/runs/40732077808/job/122970761362)은
main `42299402`에서 `stale_working_left_by_hooks_turns_idle_from_the_screen`이 실패했다.
전체 job 로그와 check annotation을 확보했다. 잘린 요약과 달리 전체 로그에는
`crates/aaa/tests/e2e.rs:253:5`의 단언과 Hello의 harness 목록이 남아 있다.
목록의 유일한 상태는 `turn_start`가 아니라 화면으로 복구한 `idle`이었다.

제품 결함이 아닌 시험 fixture의 순서 경합이다. agent는 재개한 session pump를 띄운 뒤
host 연결을 시작한다. pump의 화면 감지는 Hello를 만들기 전에도 끝날 수 있다.
기존 중계는 Welcome만 보류하므로, Hello가 반드시 옛 `turn_start`를 싣는다는 단언의
전제까지 고정하지 못했다. CI에서 제품은 이미 화면 idle 복구를 마쳤다.

## 재현과 수리

- 수정 전 시험에 사건 순서만 주입했다. host agent socket의 새 listener를 만들기 전에
  `harness.json`에 새 화면 `idle`이 저장된 것을 기다렸다. 그 뒤 listener를 열자
  CI와 동일하게 Hello의 `turn_start` 단언이 실패했다(2.31초). 제품 코드는 바꾸지 않았다.
- 수리한 시험은 실험실의 session IPC도 중계한다. 초기 인증·복구 요청은 통과시키고
  첫 `Detect` 요청을 보류한다. Hello에 옛 `turn_start`가 실린 것을 확인하고 실제
  Detect 요청 도달도 확인한 뒤 요청을 푼다. 화면 idle 저장 뒤에만 Welcome을 푼다.
- `turn_start` Hello → 화면 idle 저장 → Welcome → host idle 상태 사건 순서를 고정한다.
  실제 session host 화면, host 상태, idle 저장 및 옛 turn_start 제거 단언을 유지한다.
  고정 sleep·재시도·시한 연장·기대값 완화는 없다. 제품 코드와 설정은 바뀌지 않는다.
- 실패 출력은 전체 AgentHello 대신 해당 harness 목록을 보여 준다.

기존 E2E를 유지해야 하는 이유는 실제 session host 화면으로 옛 hook 상태를 바로잡고,
Hello 이후 Welcome 이전의 변경분까지 실제 host에 전달하는 계약을 함께 검증하기 때문이다.
새 시험은 추가하지 않았다.

## 참고한 구현

- [Tokio oneshot](https://docs.rs/tokio/latest/tokio/sync/oneshot/index.html): 비동기 중계와
  시험 thread 사이 사건 해제에 기존 의존성을 사용한다. [라이선스는 MIT](https://github.com/tokio-rs/tokio/blob/master/LICENSE).
- [Rust UnixStream](https://doc.rust-lang.org/std/os/unix/net/struct.UnixStream.html):
  별도 중계 제품 대신 실험실의 Unix socket 경계를 사용한다.
  [Rust는 MIT/Apache-2.0 이중 라이선스](https://github.com/rust-lang/rust#license)다.
- 양방향 중계는 기존 [Tokio copy_bidirectional](https://docs.rs/tokio/latest/tokio/io/fn.copy_bidirectional.html)을
  유지한다. 외부 코드를 복사하거나 의존성을 추가하지 않았다.

## 검증

- 수리 직후 단독 실행: 통과, 2.30초.
- Welcome 뒤 harness 변경분을 버리는 결함을 제품에 임시 주입: 기존 「새 agent가 화면으로 idle」
  상태 사건 대기에서 실패, 22.19초. Hello의 working 확인과 파일의 idle 저장은 통과하므로
  실제 host 전달 누락을 잡는다. 결함은 검증 뒤 원상복구했다.
- 수리 후 E2E 묶음 7개 통과(전체 6.05초). 대상 시험은 평소 병렬 실행에서 2.554초다.
  바꾸지 않은 `update_replaces_keeps_and_rolls_back`은 6.049초였으며 이 수리의 대상은 아니다.
- 대상 시험을 고정한 실행 파일로 새 실험실에서 10회 실행: 10/10 통과,
  벽시계 1.992~2.228초. 실패하면 즉시 중단하는 검증이며 재시도가 아니다.
- `cargo test --profile verify --locked -p aaa --test e2e --test deps --no-run`으로 빌드한 뒤
  각 시험 실행 파일을 직접 실행했다. 개별 시간은 실행 파일에만 `RUSTC_BOOTSTRAP=1`과
  `-Z unstable-options --report-time`을 주어 측정했다. 의존 경계 시험 2개도 통과했다.
- aaa-agent 유닛 실행 파일의 `link::post_tests::harness_changes_while_waiting_for_welcome_reach_the_host`를 `--exact`로 실행: 1개 통과, 0.127초.
  실제 link의 Welcome 사이 변경분 전달·구판 replay·중복 방지 keeper를 함께 확인했다.
- `cargo clippy --profile verify --locked -p aaa --test e2e -- -D warnings` 통과.
- `cargo fmt --all --check`, `node web/test/fixed-sleeps.mjs`, `git diff --check` 통과.
- 새 검증 문서를 추적 목록에 넣은 뒤 `bash scripts/check-public.sh` 통과.
- 전체 ci-local은 실행하지 않았다. 착지 창의 통합 검증은 별도로 수행한다.
