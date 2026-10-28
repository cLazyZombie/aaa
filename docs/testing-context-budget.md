# coordinator·thread에 넣는 글 줄이기 검증(t-1225)

[기획 §8.533](plan.md#8533-coordinatorthread에-넣는-글-줄이기t-1225)의 변경을 지키는 시험과 크기 실측이다.

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
