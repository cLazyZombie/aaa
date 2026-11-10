# 자동 답 처리 결과의 크기와 성공 표시(t-1692)

자동 답 전달 뒤 전체 답변 이력을 붙여 보내던 경로를 작은 처리 결과로 바꾼다.
성공은 기존 `ok` 봉투와 `question_answered { id, complete }`, 실제 처리 실패는 `err`로 반환한다.
`complete: false`는 해당 질문만 답했고 같은 묶음의 나머지를 기다림, `true`는 묶음 답 처리 완료다.
실행 중인 harness의 작업 완료를 뜻하지 않는다.

## 변경 경계

- 자동 답(`question_auto_answer`)만 새 결과를 쓴다. post 답 저장과 목록 조회를 분리해 처리 성공 뒤의 조회 실패가 자동 결정 실패가 되지 않게 한다.
- FRONT_API 1.166 이전 CLI에는 기존 `done` 성공을 보낸다. 새 CLI도 구판 host의 기존 응답을 읽는다.
- 웹의 `ask_answer`·사람의 `question_reply`·`asks`와 이력 페이지 형식은 유지한다.
- frame 상한·부분 답 저장·중복 답 거절·전달 여부가 불명확한 답의 자동 재전송 금지는 유지한다.

## 필요한 owner

| 시험 | 지키는 동작 |
| --- | --- |
| host `automatic_reply_returns_a_small_ack_with_large_answer_history` | 약 2MiB 이력, 실제 frame 직렬화, 부분·완료 답, 한 번의 알림, 이력 보존, 페이지 조회 |
| host `automatic_reply_preserves_delivery_failure_and_releases_the_draft` | 실제 전달 거절과 실패 원장, 답 예약 복원, 재전송 차단 |
| host `automatic_reply_ack_is_compatible_with_old_cli` | 구판 성공 응답과 실제 오류 보존 |
| 기존 host `asks`·`question_ids`·`auto_mode` owner | 웹 답 형식·부분 답·원장·권한 |
| 기존 웹 E2E 100 | 실제 coordinator CLI 결과, 별도 원장 조회, Chromium·WebKit 데스크톱·휴대폰의 결정 갱신 |
| proto 타입 생성·CLI 의존 경계 | Rust/웹 계약 일치와 crate 분리 |

새 시험은 고정 sleep 없이 처리 반환·DB 상태·frame 결과를 직접 확인한다. 별도 E2E 시나리오는 추가하지 않고 기존 자동 답 시나리오의 응답 단언을 바꿨다.

## 재현과 검증

수정 전 같은 회귀 시험은 `asks 응답이 2025 KiB라 frame 상한 1024 KiB를 넘는다`로 실패했다.
실제 답 처리 뒤의 응답 실패를 재현한 것이며 상한을 바꿔 통과시키지 않았다.

개발 검증은 `aaa verify -- <명령>`으로 실행했다.

- host: 877 통과·실패 0·기존 제외 2, proto: 54 통과·실패 0.
- host·proto의 `--all-targets` strict Clippy와 공개 검사 통과.
- 웹 타입·번들·유닛 717개, E2E 100의 Chromium·WebKit 데스크톱·휴대폰 전체 단계 통과.
- CLI 의존 경계 2개, CLI strict Clippy와 최종 형식 검사 통과. 공개 검사는 커밋 전 최종 변경 전체에 수행한다.

첫 host 전수 검사는 웹 빌드와 병행해 `embedded_assets_restore_exact_build_input` 하나가 실패했다.
웹 미빌드 상태에서 빈 자산으로 컴파일한 시험과 실행 도중 생성된 `web/dist`가 달라진 것이 원인이다.
웹 빌드 완료 뒤 host를 새 입력으로 다시 빌드해 전수 owner와 strict Clippy를 통과했다.
상한·기대값·시험 조건을 완화하지 않았다.

최종 통합 SHA의 전체 gate·OS cross·배포 검증은 착지 승인 뒤 수행한다.

참고 구현은 `docs/source-analysis.md`의 고정본을 읽었다. herdr(Apache-2.0)의 `handle_pane_send_text`,
projects(MIT)의 `threads::keys`, web-ui(MIT)의 `paneSendText`·`paneSendKeys`처럼 조작 결과를 작게 반환하고
목록 조회와 분리한다. 외부 코드를 추가로 복사하지 않았다.
