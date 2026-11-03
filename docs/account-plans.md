# 계정 요금제

설정 → 계정에서 계정마다 요금제를 선택한다. 저장하는 값은 aaa의 사용량 표시·배정 정책이며,
제공자의 실제 구독을 변경하거나 결제하지 않는다. 기본값은 자동 감지다.
직접 고른 값은 로그인·인증 갱신 뒤에도 우선하며, 자동 감지로 돌리면 보존한 제공자 값을 다시 쓴다.
기존 `plan`은 표시용 유효값, `detected_plan`은 제공자 원래 값, `plan_override`는 선택한 ID다.
메일과 같은 사람 전용 조회 경계를 따른다. 선택 변경은 웹 조작 기기·세션 밖 로컬 CLI 요청만 허용한다.

## 목록과 공식 근거

2026-11-03 확인. 금액은 미국 달러의 웹 월간 개인 요금이며 세금·지역·모바일 결제 차이는 제외한다.
목록 정본은 `aaa-host/src/account_plan.rs`, 웹에는 `account_plans` 응답으로 전달한다.
기존 계정의 감지값을 새 금액으로 추측해 표시하지 않는다.

| 제공자 | 선택 목록 | 5시간 창 |
| --- | --- | --- |
| Claude | Free, Pro($20), Max($100) | 있음 |
| Claude | Ultra($200) · 인증됨 | 없음 |
| Claude | Ultra($200) · 임시 5시간 제한 | 있음 |
| Claude | Team Standard, Team Premium | 있음 |
| Claude | Enterprise | 계약·관측에 따름 |
| Codex | Free, Go($8) | 관측에 따름 |
| Codex | Plus($20), Business | 있음 |
| Codex | Pro($100), Pro($200), Pro($500) | 없음 |
| Codex | Enterprise, Edu | 계약·관측에 따름 |

Claude의 이름·금액·개인 창 정책은 [공식 요금표](https://claude.com/pricing)를 따랐다.
옛 Max 20x는 Ultra로 이전됐으며, 5시간 창 제외는 인증된 계정에만 적용된다.
공식 사용량 화면에 Current session이 없으면 인증된 상태다.
미인증 계정은 임시 제한 항목을 고른다. [Ultra 공식 안내](https://support.claude.com/en/articles/17728952-what-is-the-ultra-plan)
Team과 계약별 차이는 [사용량 제한 안내](https://support.claude.com/en/articles/11647753-how-do-usage-and-length-limits-work)를 참고했다.

Codex의 목록·금액과 Pro의 5시간 창 제외, Plus·Standard Business의 5시간 창은
[OpenAI 공식 요금 문서](https://learn.chatgpt.com/docs/pricing)를 따랐다.
Free·Go의 CLI 접근은 배포 범위에 따라 달라지므로 목록 선택이 접근 권한을 부여하지 않는다.
Enterprise·Edu의 계약별 창 유무와 Free·Go의 창 정책은 자료만으로 확정하지 않는다.

자동 감지는 Codex `pro`를 Pro 공통 창 정책으로 판정하되 금액은 붙이지 않는다.
Claude의 모호한 `max`, 옛 `max_20x`, 인증 여부를 모르는 `ultra`는 5시간 제외로 추정하지 않는다.
새 이름·계약이 생기면 공식 근거를 확인한 뒤 목록과 정책을 함께 갱신한다.

## 제공되지 않은 창과 과거 관측

5시간 제한 없는 선택은 계정 화면·하단 상세에서 「5시간 · 해당 없음」으로 보인다.
제한이 있는 요금제에서 정상 응답의 창이 null·누락이면 「5시간 · 제공 안 됨」으로 보인다.
조회 거부·인증 오류는 이 상태와 별도로 남긴다.

과거 5시간 숫자는 흐리게 표시하고 기본으로 접는다. 관측 시각은 바꾸지 않으며 현재 잔량을 합성하지 않는다.
하단 칩은 해당 없음·제공 안 됨·오래된 5시간 값을 대표 숫자로 고르지 않는다.
배정의 고갈·문턱·잔량 비교와 자동화는 해당 없음·현재 입력에서 빠진 창을 제외한다.
마지막 정상 관측의 보조 자료도 현재 창 정책을 따르며, 지난 5시간 창을 되살리지 않는다.
주간 창과 별도 모델 창의 실제 제한은 계속 적용한다.

## 참고한 공개 구현

- [CodexBar](https://github.com/steipete/CodexBar), [MIT](https://github.com/steipete/CodexBar/blob/main/LICENSE):
  공급자별 구독 창과 신선도 표시를 참고했다.
- [CC Switch](https://github.com/farion1231/cc-switch), [MIT](https://github.com/farion1231/cc-switch/blob/main/LICENSE):
  계정 선택과 사용량 조회 설정을 분리하는 구성을 참고했다.

두 후보를 비교한 뒤 기존 aaa의 host 저장·프로토콜·기본 select를 확장했다. 외부 코드·의존성은 추가하지 않았다.
