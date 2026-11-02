# 사용량 관측의 표시·배정 유효기간 검증

## 원인과 기준

상태줄은 다시 그려질 때 관측하므로 쉬는 pane에서 관측 간격이 길어진다. 기존 공통 규칙과 웹이
10분 뒤 모름을 만들고, 배정의 소진·자동화 문턱은 2분 뒤 사라졌다.

| 소비자 | 적용 기준 |
| --- | --- |
| host 계정 보고·웹 계정 카드 | 2분 뒤 오래됨, 리셋 전까지 마지막 숫자·관측 시각 유지 |
| 리셋이 없는 관측 | 종전 10분 상한 유지 |
| 자동 배정의 현재 잔량·소진·자동화 문턱 | 리셋 전이면서 관측 후 한 창 길이 이내, 리셋 미상은 10분 |
| 배정용 마지막 정상 기록 | 종전 주간 잔량 비교·리셋 뒤 추정 유지, 새 관측으로 가장하지 않음 |
| 하단 구독 사용량 | 원래 checked_at 유지, 마지막 숫자와 경과 시간 표시 |
| Claude pane 상태줄 | 빠진 창의 원래 시각 유지, 창별 경과 시간·리셋 뒤 마지막 관측 표시 |
| 이미지 시작의 엄격한 사용량 사전 검사·종료 예약 정산 | 기존 2분 최신 조건 유지 |
| OAuth 보충 조회 | 기존 2분 최신 조건 유지, 오래된 표시를 이유로 수집을 멈추지 않음 |

배정의 긴 상한은 임의의 몇 시간이 아니라 제공자가 알린 리셋과 창 길이에 따른다. 마지막 관측은
실제 다른 작업의 추가 소비를 예측하지 않으며, 한 창 길이 상한은 잘못된 먼 리셋 때문에 오래
차단되는 것을 제한한다. 표시·배정·수집 주기는 서로 다른 의미다. 미래 시각·충돌·실패·인증 세대
분리는 유지하고 수신/파일 시각으로 관측을 새것으로 바꾸지 않는다. protocol 변경은 없다.

## 참고한 공개 구현

- [SWR 오류 처리](https://swr.vercel.app/docs/error-handling)
  ([MIT](https://github.com/vercel/swr/blob/main/LICENSE)): 마지막 정상 데이터와 새 오류를 함께
  표시하는 접근을 참고했다. 별도 라이브러리·재시도는 추가하지 않았다.
- [Prometheus staleness](https://prometheus.io/docs/prometheus/latest/querying/basics/#staleness)
  ([Apache-2.0](https://github.com/prometheus/prometheus/blob/main/LICENSE)): 수집이 주기적인 시계열의
  짧은 lookback과 비교했다. 상태줄의 사건 기반 관측에는 고정된 짧은 만료가 맞지 않아 도입하지 않았다.
- [RFC 9111 freshness](https://www.rfc-editor.org/rfc/rfc9111.html#section-4.2): 명시 만료와 age를
  구분하는 개념을 참고했다. HTTP 캐시 규칙을 사용량에 그대로 적용한 것은 아니다.

외부 코드를 복사하거나 새 제품 의존성을 추가하지 않았다.

## 필요한 회귀 시험

- 공통 규칙: 2분·10분·5시간·7일·리셋 직전/경계, 리셋 미상, 먼 리셋의 배정 상한.
- host 병합: 10분 뒤에도 원래 수치·시각, 리셋 뒤 모름. 기존 세대·시계·충돌 시험 유지.
- 배정: 한 시간 전 소진·자동화 문턱이 실제 선택을 막고 리셋 때 해제됨. 미관측·실패 fallback 유지.
- 자동화: 오래된 유효 관측에 문턱 적용, 만료·시각·충돌·scope 방어.
- agent: 부분 상태줄 응답에서 주간 시각 유지, 경과 분·리셋 뒤 마지막 관측.
- 웹 유닛: 리셋까지 오래됨 유지, 리셋 미상의 10분 경계, 경과 시간.
- 웹 E2E 124: Chromium·WebKit, PC·휴대폰에서 45분 전 숫자와 경과 시간, 리셋 뒤 마지막 숫자,
  조회 실패 뒤 보존. 응답·DOM 사건으로 끝나며 고정 sleep은 없다.

## 실행 결과

모든 검증 명령은 `aaa verify -- <명령>`으로 실행했다. 개발 관련 owner 결과이며 착지 전체 gate는
별도 승인된 착지 창에서 최종 통합 SHA에 수행한다.

- `verify.py develop --base HEAD --package aaa-common --package aaa-host`: common 73개·host 810개,
  doctest 및 두 crate strict Clippy·공개 검사 통과. 기존 수동 시험은 각각 2개 제외다.
- `verify.py develop --base HEAD --package aaa-agent`: agent 199개·doctest·strict Clippy·공개 검사 통과.
- `ci-local.sh --only web`: 타입 검사·번들 성공 뒤 새 E2E의 단계 표기 규칙에서 실패했다.
  동적 단계 이름을 저장소의 명시적 단계 형식으로 고쳤다. 그 뒤 웹 전체 685개 통과.
- `ci-local.sh --only web,e2e,public --spec 124`: 웹은 통과, PC Chromium 통과 뒤 휴대폰이 숨겨진
  사용량 단추를 눌러 실패했다. 제품 문제가 아닌 새 시험의 목록 서랍 탐색 누락이었다.
  공통 `inList`로 서랍을 열고 누르게 고쳤다. 시한·단언·재시도 설정은 바꾸지 않았다.
- 수리 뒤 `ci-local.sh --only e2e,public --spec 124`: Chromium·WebKit × PC·휴대폰 네 단계와
  공개 검사 통과. 단계별 실행은 모두 5초 이내였다.
- `cargo test --profile verify --locked -p aaa --test deps`: crate 의존 경계 2개 통과.
- `cargo fmt --all --check`와 최종 E2E 구성·관측 모델 유닛 8개 통과.

기존 자동화 시험 `quota_gate_rejects_stale_reset_invalid_conflicting_and_scoped_windows`는
새 계약에 맞춰 `quota_gate_ignores_expired_reset_invalid_conflicting_and_scoped_windows`로
바꿨다. 유효한 오래된 관측을 버리는 단언을 리셋 미상의 10분 초과 검사로 대체하고, 잔량을 실제로
문턱 아래로 내려 잘못된 시각·충돌·scope 방어가 차단을 해제하는지 확인하도록 강화했다.
시험 삭제는 없다. 실제 계정·운영 API·배포·Windows 실행은 이번 개발 검사 범위 밖이다.
