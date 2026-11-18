# 기획 main 직접 저장 시험 기록(t-2002)

설계는 [plan §8.567](plan.md#8567-기획-main-직접-저장t-2002)이다. 기획 저장이 곧 origin main commit·push인 것을 층마다 실제 Git으로 본다.

| 무엇 | 어디 | 실행 |
| --- | --- | --- |
| agent main 기획 자리(detached, 사람 checkout 그대로, 추적 ref 옮김), 한 commit 여러 파일, 기준 blob 거절과 남의 다른 파일 위 저장, 검사 `{base}`·`{head}`와 실패 까닭(stderr)·올리지 않음·다음 저장이 자리를 맞춤, non-fast-forward 뒤 새 끝에서 다시 하기와 세 번 뒤 멈춤(검사가 남의 commit을 올리는 고정 경합), 조각 그림·같은 그림 다시 붙임·지울 때 참조 경고·없는 파일 지우기 거절, 죽은 쓰기가 남긴 commit·파일 버림, `--first-parent` 이력(병합 안 branch commit 빠짐·기획 꼬리의 누가), ID 표지 한 commit | `crates/aaa-agent/src/design_main/tests.rs` 8개 | `cargo test -p aaa-agent design_main` |
| host 권한(thread·평가·보기 기기는 저장·끝내기·말하기·되돌리기 거부, coordinator는 열기·말하기만), 대화 thread가 `--branch` 없이 자기 대화로 저장, 「main에 반영」 없어짐 까닭, 되돌리기의 표지 옮겨 넣기(`revert_text`), 그림 이름 규칙, 맡김 brief(표지를 main에 적은 기록·적기 전 미룸·옛 기록) | `crates/aaa-host/src/design_branch.rs`·`design_handoff.rs`·`design_chat.rs` 시험 | `cargo test -p aaa-host design` |
| 실제 host·agent·Git·CLI: 저장이 곧 origin main(한 commit, checkout 그대로), 판 거절, 저장소 검사 거절(`git diff --quiet {base} {head} -- docs/generated.md`, 종료 코드 까닭)과 올리지 않음, `--also` 한 commit과 함께 저장한 문서의 ID 표지, 이력·`--at`, 대화 사건, 반영 없어짐, 끝내기 뒤 main 그대로 / 다른 머신(labB) 대화 thread의 중계 저장·그림(이름 지정·확장자 거절)·지우기 경고·남의 세션 거부, 구현 thread가 origin main에서 그 commit을 받음 / coordinator 열기·말하기 | `crates/aaa/tests/m13/design_branch.rs` 3개 | `cargo test -p aaa --test m13 design_branch` |
| 실제 host·agent·Git·브라우저: 트리·문서에 branch 표시·판 고르기 없음, 대화 목록 닫기(Chromium·WebKit, 1280·390px), 고치기·그림이 main에 commit(그림 주소 `?c=<commit>`), 저장 실패 상자(그 사이 다른 판, main 그대로, 글 남김), 새 대화(branch 칸 없음), 대화 입력창 그림, 이력·차이·되돌리기(옛 판 글이 새 저장으로), 대화 끝내기, 블록 표지, 구현 맡기기(표지 main에 한 commit, 예약한 구현 thread는 main에서, brief), 고침 알림, 휴대폰 칸 | `web/e2e/specs/136-design-main.mjs` | `ci-local --only web,e2e --spec 136` |
| 웹 모델: 사건 줄 글(main에 올림·사람 고침·구현 맡기기·되돌림·옛 반영 사건), 저장 수, 이력 덩어리 차이(LCS) | `web/test/designModel.test.mjs` | `cd web && node --test test/designModel.test.mjs` |

저장소 검사를 E2E에서 따로 걸지 않은 까닭: E2E 실험실은 host.toml을 실행 전체가 함께 쓴다. 검사 거절은 m13이 실제 검사 명령으로 보고, 웹의 「저장하지 않았다」 상자는 같은 오류 길인 판 거절로 본다.
