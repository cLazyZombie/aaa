# 구조 단순화와 시험 정리 검증 (t-2142)

동작을 바꾸지 않는 구조 변경이다. 큰 파일을 항목 이동만으로 나누고, 호출자가 없거나 시험뿐인 코드와 그 시험을 지웠다.

## 큰 파일 나누기

항목(함수·타입·`impl` 멤버)을 통째로 옮겼고 본문은 바꾸지 않았다. 옮긴 항목에는 부모와 그 자손에게 보이던 범위를 유지하는
`pub(super)`만 붙였다. `crate::…` 경로와 시험 이름은 그대로다.

| 파일 | 전 → 후(줄) | 새 파일 |
| --- | --- | --- |
| `crates/aaa/src/cli.rs` | 6,969 → 1,728 | `cli/args` · `task` · `thread` · `pane` · `agent` · `project` · `doctor_cmd` · `service_cmd` · `events` · `render` · `tests` |
| `crates/aaa-proto/src/msg.rs` | 5,508 → 636 | `msg/link` · `view` · `front` · `term` · `tests` |
| `crates/aaa-host/src/lib.rs` | 4,636 → 1,062 | `agent_conn` · `agent_sync` · `views` · `dispatch` · `pane_ops` · `workspaces` · `enroll` · `pairing` · `pairing_tests` · `exit_tests` |
| `crates/aaa-host/src/orch.rs` | 3,865 → 1,118 | `orch/projects` · `profiles` · `start` · `restart` · `report` · `resolve` · `tests` |
| `crates/aaa-host/src/harness.rs` | 3,580 → 660 | `harness/questions` · `input` · `tests` |
| `crates/aaa-host/src/web.rs` | 3,478 → 533 | `web/files` · `push_api` · `events` · `pane_ws` · `static_files` · 시험 셋 |
| `crates/aaa-proto/src/orch.rs` | 3,340 → 401 | `orch/library` · `agent_status` · `question` · `thread` · `brief` · `routine` · `task` · `tests` |
| `crates/aaa-agent/src/lib.rs` | 3,292 → 392 | `session_pump` · `hook_intake` · `harness_memory` · `chat_watch` · `spawn` · `dispatch` · `tests` |
| `crates/aaa-host/src/db.rs` | 2,694 → 309 | `db/library` · `schema` · `machines` · `devices` · `threads` · `messages` · `inbox` · `task_log` · `audit` · `projects` · `posts` · 시험 다섯 |

300줄을 넘는 inline 시험 모듈 16개도 `<모듈>/tests.rs`로 뗐다(host `post`·`library_links`·`task_edit`·`asks`·`inbox`·`link`·`front`·
`web_publication`·`transitions`, agent `plan`·`hooks`·`chat`·`diff`·`link`·`deploy/guard`, `aaa-vt`).

확인한 것:

- 나누기 직전 커밋과 나눈 뒤의 시험 이름 목록(`--list`)이 같다: 1,757개, 차이 0.
- `web/src/gen/proto.ts`와 schema 생성물이 그대로다(`typescript_bindings_are_current` 등).
- strict Clippy: macOS 네이티브, Linux·Windows cross.

## 지운 것

- 쓰이지 않는 의존(`aaa-common`의 `futures-util`·`webpki-roots`)과 호출자 없는 항목.
- 시험만 부르던 production wrapper. 시험이 실제 입구를 부르게 했고, 남긴 것은 `#[cfg(test)]`로 표시했다.
- 시험 전용 옛 구현 `automation::convert::apply`. 변환 시험 다섯은 production의 `resume`을 본다.
- TUI 삭제와 기획 직접 쓰기 폐지 뒤 호출자가 없던 proto 함수(`layout::rects`, `design_put_id` 등).
- 웹: gate 뿌리에서 닿지 않는 `web/e2e/lib` helper 22개, harness의 호출자 없는 선언, 폐지된 기획 대화의 model·CSS,
  `Chat`의 늘 비어 있던 `marks`·`contextOf` 경로.
- 위 코드만 보던 시험과, 더 강한 경계가 같은 실패를 잡는 중복 시험 39개.

옮기거나 강화한 단언은 production을 한 곳 망가뜨려 실패하는 것을 본 뒤 바이트 단위로 되돌렸다.

| 단언 | 변형 | 실패한 시험 |
| --- | --- | --- |
| 변환의 편집 감지·보존 사본 충돌 | 두 검사를 끔 | `converter_refuses_an_edit_after_the_plan_…`, `converter_revalidates_archive_choices_…` |
| image 실패 표지 거부 | 검사를 끔 | `output_must_be_a_bounded_image_inside_generated_home` |
| `layout::swap`이 비율을 보존 | swap이 비율을 뒤집음 | `swap_exchanges_leaves_and_keeps_cells` |
| 없는 플랫폼 거부 문구 | 문구를 바꿈 | `manifest_selects_platform_from_http` |
| 깨진 자동화 파일은 꺼져 있어도 오류 | 꺼짐 판정을 먼저 | 자동화 카드 표지 시험 |

## 전후

| 항목 | 전 | 후 |
| --- | --- | --- |
| Rust 시험(macOS) | 1,763 | 1,757 |
| Python 시험 | 105 | 104 |
| 웹 유닛 | 748 | 718 |
| 웹 E2E | spec 47 · 실행 단위 58 | 같음 |
| 1,500줄 넘는 `.rs` | 24 | 14 |
| 전체 gate | 306초 | 299초 |
