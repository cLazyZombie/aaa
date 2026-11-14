# 질문 감시의 제목 경계·교체 이력·선택적 정정 수리(t-1849)

## 보존하는 계약

`question_guard::tests::observed_reply_sections_history_and_corrections_keep_decision_boundaries`의 사례 표 하나로 관측 발췌 여섯 건과 인접 경계를 확인한다. 주변 설명·선택지·주소는 공개 가능한 내용으로 재구성했다.

- 앞 문단의 일반 안내와 뒤 제목의 카드 목록, 앞 카드와 뒤 ‘고칠 방식’·‘선택지’ 목록이 연결된다.
- ‘질문 N개에 답해 주세요’는 답 전체의 서로 다른 정본 카드 수를 요구한다. 같은 링크·0채움 별칭·모르는 번호·교체 이력은 수를 채우지 못한다.
- 표의 ‘옛 번호에서 바뀜’·‘옛 번호:’는 이력으로 구분한다. 평문·Markdown 링크·괄호 표지 뒤 실제 옛 카드 답 요청은 여전히 교체 안내를 경고한다.
- ‘겹쳐야 한다면 말씀해 주세요’는 선택적 해석 정정이다. 같은 문장 앞뒤의 배포 승인·독립 질문·조건문의 실제 선택·카드 없는 A/B/C는 계속 경고한다.

기존 실제 Host·DB 시험 두 개도 확장했다. `post::tests::question_guard_accepts_optional_feedback_and_existing_card_reply_methods`는 이전 턴의 열린 카드와 완료 사건을 통해 일반 안내·목록 제목·선택적 정정 및 별개 배포 경고를 확인한다. `post::tests::question_guard_keeps_reported_card_links_through_replacement`는 보고 교체 뒤 현재 카드+옛 번호 이력에는 경고하지 않고 실제 옛 카드 안내에는 최신 번호를 포함한 경고가 생기는지 확인한다.

카드 상태 조회·답 처리·현재 턴 접수 증거는 그대로 유지한다. 새 sleep·재시도·시한 변경·기대값 완화는 없다.

## 수정 전과 리뷰

수정 전 새 표 시험에서 여섯 사례가 모두 오탐으로 재현됐다. 첫 일반 안내는 request 1행, 두 선택지 사례는 choices 3행, 질문 둘 안내는 request 1행, 해석 정정은 request 3행의 누락이었다. 교체 이력 표는 실제 현재 카드와 닫힌 옛 번호를 함께 연결해 stale이 발생했다.

리뷰에서 일반 요청까지 목록 제목을 건너 연결하면 같은 문장의 별개 승인도 뒤 카드로 가려지는 것을 추가 시험으로 확인했다. 선택지만 목록 제목을 통과하게 하고 일반 승인 요청의 경계는 유지하도록 수리했다. ‘옛 번호’ 표지 뒤 실제 답 요청도 함께 보강했다.

## 검증 결과

최종 코드에서 `aaa verify -- nice -n 15 python3 scripts/verify.py develop --base 6fabde6b --package aaa-host`가 통과했다.

| 검사 | 결과 |
| --- | --- |
| 질문 감시 판정·실제 Host·DB·전달 경로 | 관련 19개 통과 |
| aaa-host 전체 | 914개 통과·기존 수동 2개 제외 |
| aaa-host doctest | 0개 |
| aaa-host 전체 target strict Clippy | `-D warnings` 통과 |
| 형식 검사 | `cargo fmt --all -- --check` 통과 |
| CLI 실행 | `cargo run --profile verify --locked -p aaa --bin aaa -- guide questions` 성공·갱신 안내 출력 |
| 공개 저장소 검사 | 통과 |

수동 제외는 기존 외부 서버 요청과 설치된 Codex 설정 확인이며 설정을 바꾸지 않았다. 새 사례 표는 여섯 관측 발췌와 23개 인접 경계를 확인한다.
최종 통합 SHA의 전체 gate·OS cross·배포 검사는 [착지 창](verification.md)에서 수행한다. 개발 owner 통과를 착지 gate 통과로 기록하지 않는다.

## 참고한 공개 구현

- [cmark의 참조 관리](https://github.com/commonmark/cmark/blob/master/src/references.c)와 [CommonMark 참조 정의](https://spec.commonmark.org/current/#link-reference-definitions): 문서 범위의 참조와 배치 위치를 구분하는 방식을 대조했다. [본체 BSD 2조항·파일별 라이선스](https://github.com/commonmark/cmark/blob/master/COPYING)를 확인했다.
- [Vale의 본문 범위 구분](https://vale.sh/features/markup): 코드·구문을 제외하고 실제 본문 범위에 규칙을 적용하는 접근을 참고했다. [MIT 라이선스](https://github.com/vale-cli/vale/blob/v3/LICENSE)를 확인했다.

한국어 질문 의도·카드 상태는 기존 aaa 판정에 맞춰 직접 구현했다. 외부 코드·새 의존성은 추가하지 않았다. 시험은 관측한 표현과 인접 경계의 회귀를 막으며 모든 자연어 의도를 보장하지 않는다.
