# 「main에 반영」 예약 시험(t-1955)

기획 「main에 반영」이 프로젝트 실행 한도에 걸리면 거절하지 않고 예약하고, 자리가 나면 host가 반영 thread를 띄우는지 본다.

## 재현(고치기 전)

`max_parallel_threads 1`과 열린 worker thread 하나가 있을 때 `aaa plan apply`가 `start_wait: 프로젝트 실행 한도 1. \`aaa task reserve\`로
예약한다`로 거절됐다(웹 창의 빨간 글과 같다). 반영 요청은 할 일이 아니라 할 일 예약에 넣을 길이 없었다.

## 시험

| 확인 | 위치 | 실행 |
| --- | --- | --- |
| 실제 host·agent·Git: 한도에서 `plan apply`가 올린 뒤 예약(`apply_wait.reason`에 한도), 같은 branch 거듭 예약 거부, `--cancel`로 거둠·다시 취소는 「없다」, 취소한 예약은 tick에도 띄우지 않음, 다시 예약 뒤 더 쓴 것·host 재시작에도 예약 남음·대화 사건(`apply_wait`·`apply_cancel`), 자리 난 뒤 tick이 기다리는 사이 쓴 판까지 올린 `origin/design/<주제>`에서 worktree thread를 한 번만 띄움 | `aaa` m13 `design_branch::apply_over_the_thread_limit_waits_for_a_slot_and_starts_itself` | `cargo test -p aaa --test m13 design_branch::apply_over` |
| 세션·읽기 기기·coordinator는 예약 취소도 못 함 | `aaa-host` `design_branch::tests::sessions_and_readers_do_not_write_design_branches`·`project_coordinator_opens_and_says_but_does_not_write` | `cargo test -p aaa-host design` |
| 대화 사건 문구(예약·취소·시작 못 함) | `web/test/designModel.test.mjs` 「기획 branch 화면」 | `cd web && node --test test/designModel.test.mjs` |
| 웹(실제 host·agent·git): 한도 1에서 「착지 창 요청」 → 창에 「예약됨 · 자리 나면 시작」과 까닭, 요청 단추 사라짐, 올린 판 표시 → 창의 「예약 취소」 → 다시 예약하고 닫으면 머리에 「예약됨 · 자리 나면 시작」, 누르면 같은 창 → 자리 thread를 끝내고 tick 하면 새로 읽지 않아도 「반영 대기」, worktree 반영 thread | E2E 136 「main에 반영: 실행 한도에 걸리면 예약하고…」 | `node web/e2e/web.e2e.mjs --spec 136` |

- 반영 예약은 할 일 예약보다 먼저 자리를 얻는다(ticker에서 `design_apply_pass`가 `queue_pass` 앞). 이 차례 자체를 겨루는 시험은 없다.
- 실행하지 않은 것: 다른 `start_wait`(머신 독점 예약·계정)로 기다리다 풀리는 길, 시작이 기다릴 까닭이 아닌 오류로 예약을 풀고
  `apply_fail` 사건을 남기는 길(실제 재현할 입력이 마땅치 않다, 문구는 웹 유닛 시험).
