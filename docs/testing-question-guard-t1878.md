# 질문 감시의 폐기 설명·선택적 정정 수리(t-1878)

## 보존하는 계약

`t-1877`에서 관측한 배포 후 오탐 다섯 건을 재구성한 회귀 시험을 추가했다. 주소·계정·기기 정보는 넣지 않았다.

- 현재 카드 안내와 함께 쓴 ‘옛 카드는 닫혔으니 답하지 않아도 됩니다’는 옛 카드 답 요청으로 세지 않는다.
- ‘제가 만든 옛 카드는 닫았습니다’ 뒤 현재 카드의 선택지를 설명해도 폐기한 번호로 교체 경고를 만들지 않는다.
- ‘다르게 뜻하셨으면 말씀해 주세요’, ‘다른 종을 말씀하신 거라면 알려 주세요’, ‘다른 종이라면 그때 말씀해 주세요’는 선택적 해석 정정으로 본다. 오탐을 설명한 답에서도 같은 기준을 적용한다.

`answer_ids()`는 교체 이력과 함께 닫음·폐기·답변 불필요를 설명한 참조만 제외한다. 평문과 Markdown 링크를 함께 다루고, 같은 번호를 실제 답할 곳으로 다시 안내한 참조는 남긴다. 폐기 번호로 ‘질문 N개’의 수를 채우지 못한다.

`requests()`는 조건문 전체 대신 위 정정 조건과 바로 이어진 내용 없는 피드백 요청만 지운다. 같은 문장 앞뒤의 배포 승인, 다음 문단의 독립 질문, 카드 없는 선택지, 정정 조건 안의 실제 승인·선택 내용은 계속 경고한다. 조건문의 일반적인 배포 요청을 면제하지 않는다.

## 회귀 owner와 실제 실행

- `question_guard::tests::observed_discarded_cards_and_optional_corrections_are_not_answer_requests`: 다섯 관측 발췌와 정정 구절 앞뒤의 승인·선택 요청을 대조한다.
- `question_guard::tests::discarded_card_references_do_not_cover_or_count_as_answer_targets`: 평문·링크·답변 불필요 설명, 앞뒤 카드 연결, 질문 수, 동일 번호의 실제 옛 카드 안내를 확인한다.
- `post::tests::question_guard_accepts_optional_feedback_and_existing_card_reply_methods`: 실제 Host·DB에 열린 카드를 만든 뒤 완료 사건으로 세 정정 발췌를 처리한다. 기존 별도 배포 질문과 같은 문장의 승인 요청에 각각 누락 경고가 생기는지 확인한다.
- `post::tests::question_guard_keeps_reported_card_links_through_replacement`: 실제 보고 개정으로 카드가 교체된 뒤 두 폐기 설명에는 경고하지 않고 실제 옛 카드 안내에는 현재 카드 번호를 포함한 경고를 만든다. 폐기 설명 뒤의 새 배포 질문도 누락 경고를 만든다.

새 sleep·재시도·시한 변경은 없다. 카드 상태 조회·답 처리·현재 턴의 카드 접수 증거는 바꾸지 않았다. UI·프로토콜·Windows 전용 코드는 변경하지 않았다.

## 수정 전과 검증

수정 전 추가한 순수 판정 owner 두 개가 실패했다. 다섯 발췌에서 정정 세 건은 `request` 누락, 폐기 설명 두 건은 열린 번호와 닫힌 번호를 함께 연결한 `request`·`choices` 교체 경고로 재현됐다. 첫 폐기 설명은 현재 카드 안내와 같은 문단으로 구성해야 재현되며, 별도 문단으로 분리하면 기존 코드도 경고하지 않았다.

첫 수리 후에는 순수 판정과 정정 완료 사건이 통과했다. 확장한 카드 교체 시험은 새 배포 경고를 먼저 만든 다음 전체 경고 이력이 비었다고 단언해 실패했다. 완료 처리한 경고도 전체 이력에 남는 정상 동작이 원인이었다. 기존 ‘옛 카드 안내에는 누락 없음’ 검사를 먼저 유지하고, 그 뒤 새 배포 요청을 검사하도록 사건 순서를 정리했다.

모든 검증은 `aaa verify -- <명령>`으로 실행했다. 최종 제품 코드에서
`nice -n 15 python3 scripts/verify.py develop --base 9e1c6a3a --package aaa-host`가 통과했다.

| 검사 | 결과 |
| --- | --- |
| aaa-host 전체 | 922개 통과·기존 수동 2개 제외 |
| 질문 감시 판정·Host/DB·전달 경로 | 위 전체에 포함된 관련 21개 통과 |
| aaa-host doctest | 0개 |
| aaa-host 모든 target strict Clippy | `-D warnings` 통과 |
| Rust 형식 | `cargo fmt --all -- --check` 통과 |
| 공개 저장소 검사 | 통과 |

기존 수동 제외는 외부 서버 요청과 설치된 Codex 설정 확인이다. 형식 검사도 통과했다.
개발 원장 계획·결과는 `target/verification/develop-1794674631284556000/`에 보존됐다.
`aaa verify`의 workspace 원장은 미커밋 상태라 수집되지 않았다. 개발 owner 통과를 착지 gate 통과로 기록하지 않는다.
최종 통합 SHA의 전체 gate·OS cross·배포 검사는 승인된 [착지 창](verification.md)에서 수행한다.

## 참고한 공개 구현

- [pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark)와 [개발자 문서](https://pulldown-cmark.github.io/pulldown-cmark/dev/index.html): 구조를 먼저 나눈 뒤 본문 이벤트를 처리하는 접근을 대조했다. [MIT 라이선스](https://github.com/pulldown-cmark/pulldown-cmark/blob/main/LICENSE)를 확인했다. 이번 결함은 한국어 참조 의도의 구분이어서 Markdown 파서 교체는 필요하지 않았다.
- [Vale의 본문 범위](https://vale.sh/features/markup): 규칙을 적용할 본문 구절과 제외할 범위를 구분하는 접근을 참고했다. [MIT 라이선스](https://github.com/vale-cli/vale/blob/v3/LICENSE)를 확인했다. 예외를 문장 전체가 아닌 해당 구절에만 적용한다.

외부 코드·새 의존성은 추가하지 않았다. 시험은 관측 표현과 인접 경계의 회귀를 막으며 모든 자연어 의도를 보장하지 않는다.
