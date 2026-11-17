# 옛 아이디어 지우기와 n-ID 링크 표 검증(t-1959)

옛 아이디어(n-ID) 노트·화면·「기획으로 옮기기」·`aaa idea`·번호 원장을 지우고, 글 속 n-ID를 프로젝트 `idea-links.toml` 표로 옮긴 기획
문서에 잇는다([옛 아이디어 링크](usage.md#옛-아이디어-링크)).

## 바꾼 계약과 시험

| 계약 | 시험 |
| --- | --- |
| 링크 표는 정규 n-ID와 `<저장소>/<경로>.md`만 받고, 틀린 줄이 있으면 표 전체를 버린다 | `aaa-host` `design::tests::idea_links_accept_only_ids_and_repo_docs` |
| `idea_links`는 기획 폴더가 있는 저장소의 그 폴더 안 문서만 싣는다. `aaa idea`는 없다 | E2E `137-idea-links` 「CLI」 단계(`aaa plan links`) |
| 기획 트리·휴대폰 drawer에 「옛 아이디어」 묶음이 없다 | E2E `137-idea-links` 1280px·390px |
| 문서 글의 n-ID와 옛 메모 링크 `ideas/n-NNNN`은 표에 있으면 그 문서 링크, 없거나 열 수 없으면 글자다. 누르면 그 문서가 열린다 | E2E `137-idea-links` 1280px·390px |
| 옛 주소 `#idea=n-NNNN`은 표의 문서로, 표에 없으면 처음 화면으로 바뀐다 | E2E `137-idea-links` Chromium·WebKit |
| 기획 대화 branch가 main에 병합되면 「main에 들어감」(t-1950 계약, 옛 `137-idea-move`에서 옮김) | E2E `137-idea-links` 마지막 단계 |
| 기획 대화 thread의 쉼(옛 `idea_rest_pass`)은 그대로다. 설정 `chat_rest_secs`는 옛 이름 `idea_rest_secs`도 읽는다 | `aaa-host` `chat_rest::tests::rests_only_idle_chat_threads_after_the_window` |
| 파일 백업은 `PROJECT.md`·`uploads/`만 묶고, 남은 `ideas/` 폴더는 묶지 않는다. 옛 백업의 번호 원장·아이디어 노트는 복원하지 않는다 | `aaa-host` `file_backup::tests`(첨부 복원·옛 백업 복원·symlink 원본 거부·경로 거부·잠금), `aaa` `tests/file_backup.rs`(CLI·별도 프로세스 잠금) |
| 지운 아이디어 의견(note·media)·방문 기록(`idea:`)·알림 종류는 버린다 | 웹 유닛 `reviewNotes`·`recentScreens`·`notificationSummary` |
| 대화 모델 고르기·옛 메모 링크 읽기는 `designModel`로 옮겼다 | 웹 유닛 `designModel` |

지운 시험: `idea_thread`·`ideas`·`idea_move`의 Rust 시험, 웹 `ideas.test.mjs`, E2E `137-idea-move`와 그 견본 `web/e2e/fixtures/ideas`.
모두 지운 기능(노트 읽기·쓰기 거절·옮기기·열린 아이디어 thread 끝내기)의 계약이다. 남는 계약(대화 쉼·모델 고르기·메모 링크)은 위 시험으로 옮겼다.

## 실행 결과

결과는 이 thread의 보고 library `details.md`에 둔다(검증 명령·기준 SHA).

## 착지 gate의 137 간헐 실패(옛 주소 넘기기 기다림)

- 증상: 착지 gate(6 worker, 부하 13~20)에서 「옛 주소 #idea=…: chromium」이 `hashIs` 20초 한도로 실패했다. 실패 때의 주소는 이미 `#design=app%2Fdocs%2Fcamp.md`였다. `137`만 24번 반복하자 WebKit에서 1번 같은 실패가 났고, 화면 기록상 주소는 연 뒤 약 350ms에 바뀌었으며 문서도 그려져 있었다.
- 원인: 넘기기는 링크 표(`idea_links`)가 온 뒤 `goHash(…, true)` → `history.replaceState`로 주소를 바꾼다. `replaceState`는 `hashchange`를 내지 않는다. 공용 `hashIs`는 처음에 한 번 보고 그 뒤에는 `hashchange`만 기다리므로, 표가 `hashIs` 시작 뒤에 오면 바뀐 주소를 놓친다. 제품 동작은 맞다.
- 증명: 임시 spec으로 `idea_links` 답을 `hashIs`가 기다리기 시작한 뒤에 풀었다. 옛 `hashIs`는 3번 모두 같은 글로 실패했고(주소는 이미 목표), 새 기다림은 3번 모두 통과했다. 임시 spec은 지웠다.
- 수리: 이 단계는 문서가 그려지기를 기다린 뒤 주소를 단언한다. 넘기기는 주소를 먼저 바꾸고 그다음 문서를 그린다. 공용 `hashIs`가 `replaceState`를 놓치는 점은 다른 spec에도 해당할 수 있어 기록만 남긴다(이 일에서 바꾸지 않는다).
- 확인: 고친 `137`을 6 worker로 96번 돌려 96번 통과했다(부하 7~36).
