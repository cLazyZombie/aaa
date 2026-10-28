# coordinator·thread에 넣는 글 줄이기 검증(t-1225)

[기획 §8.535](plan.md#8535-coordinatorthread에-넣는-글-줄이기t-1225)의 변경을 지키는 시험과 크기 실측이다.

## 크기

소스의 글을 그대로 재었다(UTF-8 바이트). 토큰은 실측 한 점으로 어림했다: 55,245바이트 brief를 Claude Code가 26,326토큰으로 셌다(바이트 × 0.48).

| 글 | 전 | 후 |
| --- | --- | --- |
| coordinator AGENTS.md | 20,467B·186줄 | 16,092B·163줄 |
| thread 규칙(THREAD_GUIDE) | 10,011B | 5,677B |
| 진행 안내(brief·SessionStart) | 2,992B | thread 1,069B, coordinator 952B |
| coordinator 매 입력 hook | 1,030B(모든 입력) | 사람 입력 약 170B, inbox 알림 0B, auto 상태는 바뀐 입력에만 |
| 저장소 AGENTS.md | 4,396B | 2,787B |
| docs/testing.md(공개 검사 1MiB 상한) | 1,048,010B | 1,010,819B |

## 시험

- `cargo test -p aaa-host --lib -- guide:: brief_text::`: AGENTS.md 16KiB·200줄 상한과 주제 목록, 질문 post 안내, brief 순서·머리(이 머신 저장소 경로, 다른 머신 이름)·경로 한 번,
  `aaa context` 안내 없음, 카드 예시 파싱, memory 머리 표시(`audience`·`repos`·닫히지 않은 머리·상한 계산), 다시 시작한 brief의 Follow-ups 상한(처음 할 일·가장 최근 지시·보류 인계 보존).
  필요성: brief·AGENTS.md는 매 thread·매 compaction에 실리고, 줄인 뒤 다시 불거나 금지·승인 문장이 빠지면 바로 알아야 한다.
- `cargo test -p aaa-host --lib -- inbox::tests::reading_from_a_thread_pane_keeps_the_coordinator_nudge`: thread pane·worker actor가 `context`·`inbox list`를 읽어도
  새 항목이 「봤다」가 되지 않고, 세션 밖의 사람은 지금처럼 남긴다. 필요성: 「봤다」가 된 항목은 coordinator에 알리지 않는다.
- `cargo test -p aaa-agent --lib -- hooks::`: 역할별 진행 안내(thread만 `## 진행`, 둘 다 외부 푸시 없음), aaa가 친 입력에 매 입력 알림 없음과 기록 불변,
  auto mode 안내는 SessionStart·상태가 바뀐 입력에만(같은 상태의 inbox 알림은 빈 출력). 필요성: 매 입력 hook 글은 compaction 전까지 매 요청에 다시 실린다.
- `cargo test -p aaa --test codex_integration`: 실제 `aaa hook --agent codex`의 SessionStart·UserPromptSubmit 안내와 자동화 기억 재독.
- m13(Unix): `doctor_files_brings_the_coordinator_guide_to_existing_projects`(`GUIDE_SECTIONS` 문구·16KiB 상한), brief를 받는 가짜 agent·Claude hook 안내 시험.

## 결과(착지 전)

- macOS(LF 사본, `nice -n 15`): `cargo test -p aaa-proto -p aaa-agent -p aaa-host --lib --no-fail-fast`가 50·186·745개 통과(무시 2).
  m13 9개 통과: `doctor_files_brings_the_coordinator_guide_to_existing_projects`, `worktree_thread_round_trip`, `task_attachments_reach_a_remote_agent`,
  `coordinator_flow`, `memory_and_restart`, `claude_thread_states_questions_and_messages`, `auto_merge::coordinator_changes_auto_merge_only_with_user_request`,
  `automation_start::automation_memory_reentry_claude_after_compact_and_resume`·`…_codex_after_compact_and_resume`.
- Windows: `codex_integration` 9개, 바꾼 네 crate의 `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `scripts/check-public.sh` 통과.
  `core.autocrlf=true`인 작업 사본에서는 저장소의 LF 파일이 CRLF로 풀려, 그 파일과 바이트를 비교하는 시험 9개가 실패한다:
  aaa-agent detect 3개(fixture 줄 위치가 글자 경계에서 멈춤), aaa-host 4개(공개 스키마·ci·git 예시), aaa-proto 2개(설치 스키마·TypeScript 생성물).
  이 변경은 그 파일·코드를 건드리지 않으며 LF 사본(macOS)에서는 모두 통과한다. 그 사본의 단위 시험 결과는 판정에 쓰지 않았다.
- 전체 ci-local·Linux strict cross·실제 이름 목록을 포함한 공개 검사는 착지 회차에서 돈다.

## E2E12 응답 전 실패의 navigation 취소 구별(착지 수리)

- 착지 1회차 전체 ci-local(부하 36.6)에서 E2E12 「thread 목록 규모: WebKit 전체 페이지·재연결·실제 DOM」이 `HTTP 본문 읽기 실패: cancelled`로 실패했다.
  같은 spec의 Chromium 단계와 다른 단계는 통과했다. 보존한 WebKit 기록 순서(시작 기준 ms): 다음 쪽 요청 #32(+5502) → 시험의 reload
  `beforeunload`(+5605) → #32 `request-failed: cancelled`(+5606) → `pagehide`(+5631) → 새 문서(+5642).
- 원인: `web/e2e/lib/thread-page-trace.mjs`는 본문을 읽는 중의 실패만 실제 Document 교체(retire)로 navigation 취소를 구별했고, 응답 헤더 전
  `requestfailed`는 그 자리에서 오류로 끝냈다. 실패 사건이 새 문서 표지보다 먼저 오면 문서가 바뀐 뒤에도 이미 끝난 요청이라 취소로 구별되지 않았다.
- 수리: `requestfailed`는 실패 문구만 기억하고 끝내지 않는다. 문서가 바뀌면 retire가 navigation 취소로 끝낸다. `reads` 때 아직 남았으면 지금 문서를
  직접 확인해 같은 문서면 그 실패 그대로 오류로 둔다. 닫힌 page는 원래 실패 문구로 끝낸다. 오류 문구로 성공 처리하지 않으며 재시도·시한·기대값은 그대로다.
- 시험: `node --test web/test/threadPageTrace.test.mjs`에 위 순서(실패가 새 문서 표지보다 먼저)를 가짜 page로 그대로 재현하는 keeper와, 문서 표지 사건
  없이 지금 문서가 바뀐 경우(취소)·같은 문서(실패 그대로)·닫힌 page(실패 그대로)를 더했다. 수리 전 코드는 두 시험 모두 착지 때와 같은
  `HTTP 본문 읽기 실패: cancelled`로 실패하고 수리 뒤 6개가 통과한다. 필요성: 이 순서는 높은 부하에서만 드러나 E2E 실행만으로는 다시 볼 수 없다.
