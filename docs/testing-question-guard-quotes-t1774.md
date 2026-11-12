# 질문 감시의 인용 설명·카드 번호 연결

## 원인과 수정

간접 인용 설명이 있는 줄 전체를 `blocks()`가 버려 같은 줄의 실제 카드 링크도 없어졌다.
뒤의 A/B 선택지만 남아 `rule=choices refs=`인 카드 누락으로 분류됐다.
발생 형태는 t-1768의 점검 자료를 읽고, 비공개 원문 대신 재구성한 문장으로 시험했다.

`prose()`에서 따옴표·코드와 간접 인용 구절만 제외한다. 보고 동사 뒤의 카드 안내와
같은 줄의 별개 요청은 남긴다. 예시·인용 표시도 해당 구절 범위에서만 제외한다.
제목·주제·새 요청 표지는 정리한 본문으로 판정하므로 인용 속 표지를 새 요청으로 취급하지 않는다.

`requests()`는 본문에 있는 요청들의 위치를 바이트 오프셋으로 돌려준다.
같은 줄의 첫 카드 안내만 검사하고 뒤의 별도 요청을 놓치지 않도록 각 요청을 검사한다. `inspect()`는 실제 요청 앞의 문장 경계를
찾아 앞 문장의 카드 번호가 같은 줄의 새 질문·승인 요청을 가리지 않게 한다.
선택적 피드백은 같은 바이트 길이의 공백으로 가려 요청 위치·원래 행 번호를 유지한다.
같은 문단에서 카드를 명시해 답하도록 안내하는 경우에는 앞 문장의 번호를 계속 연결한다.
열린 카드·답한 카드·닫히거나 교체된 카드는 기존 host/DB 상태 확인과 알림 경로를 그대로 쓴다.

## 관련 owner

- `reported_explanations_keep_card_links_and_states`: 설명 앞뒤·마침표·쉼표·선택지 사이 빈 줄을
  바꾸어 카드 연결을 확인한다. 열린 카드·답한 카드는 경고하지 않고 닫힌 카드는 교체 안내로 분류한다.
- `reported_explanations_do_not_hide_unlinked_requests`: 설명 앞뒤의 실제 질문·승인 요청,
  같은 줄의 기존 번호 뒤 새 요청, 카드 답 안내 뒤 별도 승인 요청, 한 줄의 여러 질문,
  없는 번호·다른 주제·인용 안의 번호를 검사한다.
  간접 질문 자체는 제외하고 진단의 원래 행 번호를 유지한다.
- `post::tests::question_guard_keeps_reported_card_links_through_replacement`: 실제 Host와 DB에
  thread 질문을 등록하고 `on_harness`에 완료 사건을 보낸다. 첫 열린 카드 안내에는 경고가 없고,
  같은 thread의 질문을 교체한 뒤 옛 번호를 안내하면 누락 경고 없이 새 번호를 알려야 한다.

기존 질문 감시의 host/DB·중복 알림·다른 프로젝트·빠른 답 뒤 접수 증거 시험도 함께 실행한다.
고정 sleep·재시도·시한 확대·기대값 완화는 없다. 순수 본문 판정만 바꾸며 웹·Windows 전용 경로는 바꾸지 않는다.

## 검증

모든 검증은 `aaa verify -- <명령>`으로 실행한다.
수정 전 기존 11개는 통과하고 새 두 회귀 시험은 각각 카드 번호 유실과 같은 줄 질문 누락으로 실패했다.
첫 형식 검사에서 새 코드의 줄바꿈 지적을 받아 형식을 맞췄다. 동작·기대값은 바꾸지 않았다.

추가한 host 통합 시험의 첫 실행은 thread를 DB에 넣기 전에 갱신만 하여 열린 카드가 없는
fixture 오류로 실패했다. thread를 먼저 등록하고 열린 목록에 실제로 있는지 단언하도록 고쳤다.
판정 코드나 기대 결과를 완화하지 않았다.

기준 SHA는 `91664a181851be255173b6f6369fed1740e4a916`이다.

| 범위 | 명령 (`aaa verify --` 뒤) | 결과 |
| --- | --- | --- |
| 질문 판정·host/DB 완료 사건 | `nice -n 15 cargo test --profile verify --locked -p aaa-host --lib question_guard` | 14개 통과 |
| 형식 | `nice -n 15 cargo fmt --all -- --check` | 통과 |
| 관련 owner 정규 검사 | `nice -n 15 python3 scripts/verify.py develop --base 91664a181851be255173b6f6369fed1740e4a916 --package aaa-host` | 통과 |

정규 검사는 aaa-host 시험 896개 통과·기존 수동 시험 2개 제외·doctest 0개,
`cargo clippy --profile verify --locked -p aaa-host --all-targets -- -D warnings`,
`scripts/check-public.sh`까지 성공했다. 기존 수동 시험은 설치된 Codex 설정 확인과
실제 외부 서버 요청이며 `ignored` 설정을 바꾸지 않았다.

코드 최종 보강 뒤 정규 검사를 다시 실행했다. 실행 계획·결과는 작업 폴더의
`target/verification/`에 보존했다. 미커밋 상태의 검증이므로 `aaa verify`의
자동 원장에는 수집되지 않았다.

착지 창은 받지 않았으며 최종 통합 SHA의 전체 gate·OS cross·Windows production은 실행하지 않았다.
개발 owner 결과를 착지 gate 통과로 기록하지 않는다.

## 참고한 공개 구현

- [strip-markdown의 노드별 처리](https://github.com/remarkjs/strip-markdown/blob/main/lib/index.js)
  ([MIT](https://github.com/remarkjs/strip-markdown/blob/main/license)): 지울 내용과 남길 내용을
  범위별로 나누는 구조를 참고했다.
- [markdown-rs의 구문 트리](https://github.com/wooorm/markdown-rs/blob/main/src/mdast.rs)
  ([MIT](https://github.com/wooorm/markdown-rs/blob/main/license)): 인용·링크·일반 본문을
  서로 다른 구문 요소로 다루는 구조를 대조했다.

둘 다 한국어의 간접 인용 의도를 판정하지 않는다. aaa의 기존 제한된 본문 판정을 유지하고
문장·구절 경계만 좁게 고쳤으며 외부 코드·의존성은 추가하지 않았다.
이 시험은 관측한 문장 형태와 인접 경계의 회귀를 막으며 모든 자연어 의도를 보장하지 않는다.
