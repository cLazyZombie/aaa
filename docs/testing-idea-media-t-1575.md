# 아이디어 그림 붙이기·편집·넘기기 검증(t-1575)

계획(사용자 답 1A 글 단위 편집, 2A 고르면 노트 끝에 바로 저장, 3B 사진 원본 보관)과 추가 요구(그림 든 아이디어를 할 일로 넘기기)를
실제 host·agent·브라우저로 확인한 기록이다.

## 새 시험과 까닭

| 시험 | 꼭 필요한 까닭 |
| --- | --- |
| host `ideas::tests::media_lines_go_to_the_end_or_the_section` | 붙이는 자리(노트 끝·절 끝, 코드 안 머리 무시, 없는 절 거부)와 설명 정리. 덧붙이기가 다른 글을 건드리지 않는 계약 |
| host `ideas::tests::attach_adds_one_revision_and_guards_sources` | `idea attach`가 한 판으로 쓰고 출처를 남기며, 제한 폴더 밖·그림 아닌 파일·올리지 않은 이름·잠긴 아이디어를 거부하는 권한 경계와 `uploads/` 링크 옮기기 |
| host `ideas::tests::handoff_locks_and_copy_reopens`(늘림) | 넘길 때 노트 그림이 `uploads/` 사본과 메모 그림 줄로 가고, 실제 메모로 thread를 시작하면 그림이 한 번만 `attachments/`로 가는 것 |
| 웹 `test/idea-media.test.mjs` | 그림 메뉴의 글 변환(설명·바꾸기·지우기·위아래)이 코드 예시·다른 줄을 건드리지 않는 것, 형식 없는 파일 판정 |
| 웹 E2E 130 `130-idea-media.mjs` | 실제 흐름: 「그림」 단추 → 새 판, 못 받는 파일 표시, CLI 절 붙이기, 메뉴 편집, WebKit 390 휴대폰 붙이기·메뉴, 넘긴 할 일 화면의 첨부 썸네일, thread `attachments/`·task·brief·노트 사본의 그림 링크 |

## 결과(11-07, 로컬 Mac 하나)

- `cargo test -p aaa-host -p aaa-proto --lib`: 통과(host 856, proto 54).
- `npm test`(web) 711개·`npm run typecheck` 통과.
- E2E 130 단독: 7단계 통과, 파일 전체 5.3초(host 시작 1.1초 포함, 단계마다 1.5초 이하).
- 전체 `aaa verify`(ci-local) 결과는 보고에 적는다.

## E2E에서 찾아 고친 것

- 위로·아래로는 사이 빈 줄을 제자리에 둔다: 머리 아래로 옮긴 그림은 `## 결정` 다음 빈 줄 뒤에 온다(시험 기대값을 규칙에 맞췄다).
- 1×1 시험 그림으로는 설명·메뉴 자리를 눈으로 볼 수 없어 시험 안에서 320×180 단색 PNG를 만든다.
