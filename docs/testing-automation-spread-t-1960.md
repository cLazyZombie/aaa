# 자동화 저장소 펼치기 검증(t-1960)

thread 동작의 `repos = "design"`이 지키는 계약과 그것을 지키는 시험이다. 모두 실제 host·DB와 기존 agent frame fixture(`automation/start_tests.rs`)로 돈다.

| 계약 | 시험 |
| --- | --- |
| 일정 한 차례가 기획 저장소마다 입력 하나(`<차례>#<저장소>`)·thread 하나가 된다. 기획 폴더가 없는 저장소는 빠진다. 제목 `{repo}`는 저장소 이름이다 | `spread_starts_one_thread_per_design_repo_and_waits_per_repo` |
| 기본 상한 1은 저장소마다 센다. 기다림 까닭에 저장소를 적는다. 앞 저장소가 기다려도 뒤 저장소를 먼저 띄운다 | 같은 시험 |
| 실행 원장(`AutomationExecution.repo`)에 저장소가 남는다 | 같은 시험 |
| 저장소 추가는 넓힘: 규칙은 승인 필요, 새 저장소 입력만 기다리고 승인한 저장소는 계속 돈다. 저장소 빠짐은 좁힘 | 같은 시험 |
| 펼치지 않는 규칙의 범위 직렬화(승인 hash 입력)에 새 칸이 없다 | 같은 시험 |
| `repo`·`reuse = "open"`과 함께 쓰면 파일 오류 | 같은 시험 |
| 머신을 비우면 저장소마다 그 저장소 workspace 자동 배정을 허용한 머신에서만 고른다 | `spread_without_machines_uses_each_repo_workspace_permission` |
| 「지금 실행」도 저장소마다 입력이며, 대기 기록은 펼친 id로만 남는다 | 같은 시험 |
| 시작이 오류로 끝난 저장소 입력은 큐 뒤로 가서 다른 저장소를 막지 않고, 보존돼 다시 띄운다 | `spread_start_failure_does_not_block_other_repos` |
| 웹 폼 토글은 schema 2 파일에 `repos` 줄만 쓰고 지운다 | `automation::edit::tests::merge_writes_and_removes_repo_spread_in_v2_file` |
| 설정 보기의 저장소 칸·저장소마다 동시 상한 표시 | `web/test/automationChoices.test.mjs` |
| 실제 host·agent·Git: 범위 글에 기획 저장소만 → 사람 승인 → `automation run` → 저장소마다 worktree thread(제목 `{repo}`) → `automation runs`의 저장소 칸 | `m13` `automation_start::automation_spread_starts_one_thread_per_design_repo` |

머리 고르기와 실패 뒤 큐 뒤로 보내기, 「지금 실행」 대기 id는 각각 수리 부분을 끄면 위 시험이 실패하는 것을 확인했다.
