# 기획서 ID 시험(t-1957)

설계는 [plan §8.566](plan.md#8566-기획서-idt-1957)이다.

| 시험 | 고정한 것 |
| --- | --- |
| `aaa-proto` `design::tests::id_marker_is_read_the_same_from_head_and_whole_text` | 표지 줄 꼴(BOM 뒤 넣기·8KiB 넘는 YAML 머리는 그대로 포함, 공백·CR·`k-0`·다른 접두어·뒤 글자 거부), 코드 울타리 안 예시 제외, 첫 표지, 앞 8KiB 경계에서 agent의 잘린 앞부분과 전문 읽기가 같은 답, `vendor/` 판정 |
| `aaa-proto` `design::tests::id_marker_goes_under_the_title_without_eating_blocks` | 제목 아래·빈 줄 더하기·끝 줄바꿈 없는 제목·CRLF·제목 없음·빈 글·YAML 머리·울타리 안 제목·있는 표지 그대로·늦은 제목은 맨 앞, 공용 견본에 넣어도 표지 블록 글과 차례 그대로 |
| `aaa-proto` `design::tests::handoff_marks_only_the_picked_plan_blocks`·agent `marks_match_the_shared_web_fixture`·웹 「agent와 같은 공용 견본으로 센다」 | 공용 견본 끝에 표지 줄 바로 아래 붙은 `> [!계획] d-0015`를 더해 Rust·웹이 같은 덩어리(표지 7·미정 2)로 센다 |
| `aaa-host` `db::design_ids::tests::numbers_follow_markers_and_paths` | 경로 번호와 다시 읽기, 표지를 단 뒤 옮김, 복사는 새 번호, 표지가 사라져도 같은 경로 번호, 원장에 없는 표지 번호 받기와 그 뒤 새 번호, 터무니없이 큰 표지 번호 거부(독립 리뷰 지적), 다른 저장소 번호의 표지, 저장 번호(main에서 표지로 적힌 경로 번호는 새 문서에 다시 쓰지 않음, 리뷰 지적), 프로젝트를 지운 뒤에도 번호 재사용 안 함 |
| `aaa-agent` `design_write::tests::mark_ids_adds_markers_in_one_commit` | 트리 `ids`, 한 commit, YAML 머리 뒤 제목 아래, 없는 문서·`vendor/` 건너뜀, 다시 해도 commit 없음, branch 트리의 ID, 잘못된 ID 거부, 뒤 문서가 symlink면 앞 문서도 쓰지 않음(리뷰 지적, Unix), 그림은 건너뜀 |
| `aaa` m13 `design_branch` 넷 | 저장할 때 표지(`k-0001`)·줄 수, 대화 thread의 원격 저장(512KiB 가까운 문서 포함)에 표지, 사람이 표지 없이 저장해도 같은 번호 |
| 웹 `appIds.test.mjs` 「기획서 ID」·`designModel.test.mjs` 「ID 표지 줄」 | 지금 트리 번호만 링크·앞 0 정규화·`#design=k-NNNN`, 표지 줄 판정(Rust와 같은 꼴), 홀로 한 덩어리, 그릴 글에서 빼기(울타리 안 제외) |
| 웹 E2E `139-design-ids` | 실제 origin·agent: 표지 번호와 원장 번호, `aaa plan show k-8`, 문서 머리 ID·표지 줄 숨김·`k-0008`만 링크(`k-0999`는 글)·누르면 그 문서·`#design=k-9`와 새로 고침, `aaa plan ids`가 표지 없는 문서만 `design/aaa-ids` 한 commit에 넣고(`vendor/`는 그대로) 다시 해도 commit 없음·`skipped` 2 |

독립 리뷰 지적 가운데 다른 프로젝트의 같은 이름 저장소 번호가 그 프로젝트 링크 목록에 빠지는 것과 저장소 이름을 바꾸면 표지가 다른 저장소 번호로 보이는 것은 host가 프로젝트 하나만 맡는 지금 고치지 않았다(plan §8.291).
