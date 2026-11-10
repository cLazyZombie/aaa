# 질문 감시의 의미 확인·기존 카드 안내 경계

## 원인과 수정 범위

의미를 바로잡는 “다르게 생각하신 거라면 말씀해 주세요.”와
“다른 작업을 뜻하셨다면 말씀해 주세요.”는 선택적 피드백 목록에 없어 일반 답 요청으로 잡혔다.
두 조건 표현이 일반 피드백 요청과 직접 이어진 구절만 제외한다. 조건문 전체를 면제하지 않으므로
같은 문장의 별개 승인·선택 요청과 다음 문장의 질문은 계속 검사한다.

기존 카드 설명의 “별도로 정할 일로 넘겼습니다.”는 완료 보고인데도 `fresh_tail`이
“별도로”를 무조건 새 요청 표지로 삼았다. 같은 블록의 선택지에 `fresh=true`가 붙어
`inspect`가 뒤의 카드 링크를 찾는 경로를 건너뛰었다. 이 완료 구절의 표지만 제외하고
실제 별도 요청과 다른 새 요청 표지는 유지한다. 표지의 모든 출현을 확인해, 뒤의 완료 설명이
앞의 실제 새 요청 표지를 덮어쓰지 않게 한다.

q-ID가 있다는 이유로 답 전체를 면제하지 않는다. 카드 없는 선택지는 누락으로,
닫힌 카드에 연결된 선택지는 오래된 안내로 계속 감지한다. 주제가 바뀌면 링크를 공유하지 않는다.
카드 접수·host/DB·경고 전달·승인 정책은 바꾸지 않는다.

## 회귀 owner와 필요한 이유

`crates/aaa-host/src/question_guard/tests.rs`에 다음 두 시험을 추가했다.

- `meaning_corrections_are_optional_without_hiding_decisions`: 실제 두 안내는 경고하지 않고,
  같은 문장 앞뒤의 승인 요청·다음 문장의 질문·선택지는 계속 잡는지 확인한다.
  조건 뒤에서 승인 여부나 선택을 직접 묻는 문장도 검사해 조건문 전체 면제를 방지한다.
- `completed_handoff_keeps_existing_card_choices_linked`: 실제 완료 구절과 q-ID에
  재구성한 A/B/C 선택지를 붙여 기존 카드 연결 경로를 확인한다. 카드 없음·알 수 없는 카드·다른 주제·
  새 별도 요청·닫힌 카드도 검사해 연결을 넓게 허용하는 회귀를 방지한다.

세 번째 발생 기록의 선택지 전문은 제공되지 않았다. 실제 짧은 완료 구절과 q-ID를 보존하고
선택지 내용은 공개 가능한 예시로 재구성했다. 앞선 두 문장은 제공된 그대로 썼다.
기존 시험·시한·단언은 유지한다. 고정 sleep·재시도·기대값 완화는 없다.
순수 문자열 판정의 변경으로 웹·Windows 전용 owner는 추가하지 않는다.

## 검증

수정 전 회귀 시험은 기존 9개가 통과하고 새 2개가 실패해 오탐을 재현했다.
수정 후 질문 감시·host/DB 회귀 11개가 통과했다.

첫 형식 검사는 새 배열 뒤 여는 중괄호의 줄바꿈을 지적해 수정했다. 첫 strict Clippy는
시험 문자열 3곳의 불필요한 `format!` 호출을 지적해 `.to_owned()`로 바꿨다.
동작이나 기대값은 바꾸지 않고 수정된 시험 코드로 정규 검사를 다시 실행해 통과했다.

모든 검사는 `aaa verify -- <명령>`으로 실행한다.
관련 owner 정규 검사는 시작 SHA `15eac219feceb3f1c98677ec0f72518d789e039a`를 기준으로 실행했다.

| 검사 | 명령 | 결과 |
| --- | --- | --- |
| 질문 감시·host/DB 회귀 | `nice -n 15 cargo test --profile verify --locked -p aaa-host --lib question_guard` | 11개 통과 |
| 형식 | `nice -n 15 cargo fmt --all -- --check` | 통과 |
| 관련 owner 정규 검사 | `nice -n 15 python3 scripts/verify.py develop --base 15eac219feceb3f1c98677ec0f72518d789e039a --package aaa-host` | 통과 |

정규 검사는 aaa-host 시험 891개 통과·기존 수동 시험 2개 제외·doctest 0개,
`cargo clippy --profile verify --locked -p aaa-host --all-targets -- -D warnings`,
`scripts/check-public.sh`까지 모두 성공했다. 제외된 시험은 설치된 Codex 설정 확인과 실제 외부 서버 요청이며,
기존 `ignored` 설정을 바꾸지 않았다.

미커밋 상태에서 실행해 `aaa verify`의 자동 원장에는 수집되지 않았다.
정규 계획·결과는 작업 폴더의 `target/verification/`에 남겼다.
최종 통합 SHA의 전체 gate와 OS cross는 승인받은 착지 창에서 수행한다.
개발 owner 통과를 착지 gate 통과로 기록하지 않는다.

## 참고 구현과 라이선스

- [Vale existence 판정](https://github.com/vale-cli/vale/blob/v3/internal/check/existence.go)은
  탐지한 구절과 그 구절의 예외를 대조한다.
  [MIT 라이선스](https://github.com/vale-cli/vale/blob/v3/LICENSE)를 확인했다.
- [textlint filter rule](https://github.com/textlint/textlint/blob/master/docs/filter-rule.md)은
  범위와 규칙 ID를 지정해 경고를 제외한다.
  [MIT 라이선스](https://github.com/textlint/textlint/blob/master/LICENSE)를 확인했다.

두 구현의 제한된 범위에서 예외를 적용하는 접근을 참고했다. aaa의 기존 구절 단위 제외와
주제별 카드 연결 구조를 유지하며, 외부 코드·런타임·새 의존성은 추가하지 않는다.
이번 수정은 알려진 세 표현의 회귀를 막으며 모든 자연어의 의도를 보장하지 않는다.
