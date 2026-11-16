# coordinator의 기획 대화 열기·말하기(t-1928)

## 변경

4단계(t-1894)로 아이디어 쓰기 길이 없어져, 사용자의 「아이디어」·「기획」 요청은 coordinator가 기획 대화로 넘긴다(q-4013 2B).

- host `design_opener`: `DesignBranchOpen`(`aaa plan new`)·`DesignSay`(`aaa plan say`)에서만 그 프로젝트 coordinator
  (`coordinator_project_of_actor`: host 머신에서 프로젝트 폴더에 도는, 끝나지 않은 coordinator pane 안의 CLI)를 받는다.
  나머지는 전처럼 `design_writer`(사람·자기 branch 대화 thread)를 지난다. 만든 이는 `coordinator`다.
- 저장·버리기·반영·구현 맡기기·옛 아이디어 옮기기는 coordinator에게 닫혀 있다(거부 문구가 열기·말하기만 된다고 알린다).
- coordinator 안내: 매 턴 본문 한 줄(`aaa guide plan`을 가리킴)과 새 주제 `plan`. 원격 thread 전달 허용 목록은 그대로다.

## 시험

| 계약 | owner |
| --- | --- |
| 그 프로젝트 coordinator pane의 열기·말하기는 권한을 지나고(저장소 확인에서 멈춤), 저장·버리기·반영·맡기기·옛 아이디어 옮기기는 Forbidden, 프로젝트 폴더 밖·끝난 coordinator pane은 열기·말하기도 Forbidden, pane으로 확인하지 못한 세션은 전처럼 사람(coordinator 아님) | `aaa-host` `design_branch::tests::project_coordinator_opens_and_says_but_does_not_write` |
| 실제 host·agent·Git: `project open`으로 띄운 coordinator pane 안 CLI가 `plan new`(만든 이 coordinator)·`plan say --file`로 대화 thread를 띄워 말이 닿고 `pending` 0, 할 일 없음, 같은 pane의 `plan save`·`discard`·`apply`는 거부, 사람이 버리면 대화 thread가 끝남 | `aaa` m13 `design_branch::coordinator_opens_a_plan_chat_and_says_but_does_not_write` |
| AGENTS.md 본문이 `plan` 주제를 가리키고 주제가 열기·말하기 명령과 하지 않는 쓰기를 적으며, 거둔 `aaa idea new`·`say`를 안내하지 않음. 크기 상한(16 KiB) 안 | `aaa-host` `guide::tests::` |

- RED: `design_opener`의 coordinator 허용을 끄면 단위 시험이 열기에서 Forbidden으로 실패했다(되돌림).
- 첫 안내 문안은 AGENTS.md가 16,423바이트로 `coordinator_guide_stays_small`(16 KiB)을 넘었다. 상한은 두고 새 줄과 주제 설명을
  줄였다(옛 아이디어 읽기 전용 안내는 주제로 옮김).
- 첫 단위 시험은 다른 프로젝트 coordinator를 만들려다 「host는 프로젝트 하나만 맡는다」로 멈췄다: 프로젝트 폴더 밖
  coordinator 역할 pane으로 바꿨다.

## 검증

모두 `aaa verify --`로 실행했다(미커밋 작업이라 원장 미수집).

- `cargo test --profile verify --locked -p aaa-host --lib`: 949 통과(2 ignored).
- `cargo test --profile verify --locked -p aaa --test m13 design_branch::`: 4 통과.
- `cargo test --profile verify --locked -p aaa --bins`: 103 통과. `cargo test -p aaa --test deps`: 2 통과.
- `cargo clippy --profile verify --locked -p aaa-host -p aaa --all-targets -- -D warnings`: 통과. `cargo fmt --all -- --check`: 통과.
