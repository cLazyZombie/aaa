# Codex 선제 갱신 시험의 저장 경계 경합 수리

## 원인과 변경

CI의 선제 갱신 시험은 금고 버전 2를 보고 갱신 결과 기록까지 끝났다고 판단했다.
금고의 새 인증 저장과 host의 `refreshed_ms` 저장은 순서가 있는 별도 단계다.
그 사이 목록에는 버전 2와 이전 metadata가 함께 보일 수 있다.
기존 E2E의 대기 조건에 `refreshed_ms`의 숫자형 확인을 추가한다.
공급자 요청 수 1회, 버전 2, 실패 기록 없음, 새 인증의 만료 미상 단언은 그대로 유지한다.
제품 코드·시한·재시도·인증 정책은 바꾸지 않는다.

## CI 근거

- 실패 SHA: `cc4a6af54e83fa6ed0c194bf3f463961524bd935`.
- [CI 실행과 macOS job](https://github.com/cLazyZombie/aaa/actions/runs/42820567533/job/129844760298).
- `rust-tests-macos-15-42820567533-1` artifact의 `rust/239-m13-17.log`를 확보했다.
- 실패는 `crates/aaa/tests/m13/accounts.rs:1294`의
  `refreshed_ms.is_u64() && refresh_failure 없음` 단언이다. 대기 시간 초과가 아니다.
- 실패 snapshot은 버전 2, `refreshed_ms` 없음, 실패 기록 없음, 이전 access token 만료 정보가 있는 상태다.
- 최초·최근 인증 시각은 같았다. 자동 갱신은 두 시각을 바꾸지 않으므로 그 사실만으로 갱신 미발생을 판정할 수 없다.
- 계정 식별자·인증값은 이 문서에 싣지 않았다.

## 시각과 사건 대조

| 단계 | 증거와 의미 |
| --- | --- |
| 초기 JWT | 만료 `1794319900000`ms, 시험 시작 때 현재 시각의 정수 초 + 3600초로 생성 |
| import 기록 | `1794316302076`ms. 만료까지 3,597,924ms로 하루 선제 갱신 창 안 |
| 판정 | `account_refresh_pass`는 `now_ms()`와 `expires - 86,400,000`을 비교한다. CI의 정확한 판정 시각은 로그에 없어 알 수 없다 |
| 예약 | tick은 `tokio::spawn`으로 갱신을 예약하고 반환한다. tick 응답은 갱신 결과 저장 완료가 아니다 |
| 요청 | 실패 단언 직전의 가짜 공급자 호출 수 1회 단언은 통과했다 |
| 응답·금고 | 가짜 공급자는 첫 요청에 200·새 비 JWT 인증을 반환한다. `renew`는 이를 금고 버전 2로 쓰고 `Renewed`를 반환한다 |
| 결과 기록 | 다음 `account_refresh_seen`이 `refreshed_ms`를 저장하고 새 인증의 만료 정보를 반영한다 |
| 재조회 | `account_views`는 금고 목록과 metadata를 따로 읽는다. 기존 대기는 금고 버전만 확인해 결과 기록 전 snapshot을 반환했다 |

CI job 원문과 실패 요약 annotation도 확보했다. 해당 실험실의 `host.log` 내용은 제공된 artifact·annotation에 없어
과거 요청·저장 시각을 host 로그로 복원할 수 없다. 요청 수·버전·실패 단언과 실제 코드 순서로 위 범위를 확인하고,
아래 사건 장벽 실험으로 같은 저장 경계의 실패를 재현한다. 과거 판정 시각·응답 시각을 추측해 채우지 않는다.

## 변경 귀속 확인

`t-1728`의 `9a0f786e`와 병합 SHA를 앞선 `05a150f0`에 대조했다.
`account_meta`에는 coordinator 예약분 조회 함수만 추가됐고, 갱신·조회 함수와 이 시험은 바뀌지 않았다.
`account_pool`·`account_plan`은 자동 배정의 예약분·점수 계산 변경이다.
이 시험은 계정을 import하고 갱신하며 자동 배정 요청을 하지 않는다.
기존 시험과 저장 순서는 `76292180`에서 도입됐다. 이번 실패를 예약분 변경의 제품 회귀로 귀속하지 않는다.

## 검증

모든 명령은 `aaa verify -- <명령>`으로 실행했다. 임시 실험 코드·가짜 인증·원시 로그는 커밋하지 않는다.

### 사건 장벽으로 원인 확인

1. 수정 전 원문을 로컬에서 한 번 실행: 통과, 1.48초. 자연 실행 한 번의 통과로 CI 실패를 회복으로 분류하지 않았다.
2. 임시 진단에서 `account_refresh_seen(Renewed)` 진입을 저장소의 TCP `Gate`로 막았다.
   이 시점에는 공급자 응답과 금고 버전 2 저장이 끝나 있다.
   목록 조회가 버전 2·`refreshed_ms` 없음 snapshot을 읽은 뒤에만 gate를 풀었다.
3. 기존 완료 조건은 그 snapshot을 반환하고 원래 `refreshed_ms` 단언에서 실패했다(1.35초).
   실험 host 로그에서도 새 계정 인증 버전 2 저장을 확인했다. 이로써 공급자 갱신 부재와 구분했다.
4. 같은 장벽에서 수정 완료 조건은 중간 snapshot을 넘겨 결과 기록을 기다린 뒤 통과했다(1.58초).
5. host와 시험의 임시 장벽 코드를 모두 제거했다. 최종 제품 코드 차이는 없다.

첫 진단은 중간 snapshot에 옛 만료 값도 반드시 남는다고 보조 단언해 그 단언에서 멈췄다(1.26초).
그 사이 다음 `account_refresh_pass`가 새 금고 인증을 읽어 만료 정보를 먼저 지울 수도 있음을 코드로 확인했다.
진단 보조 단언을 제거하고 핵심 경계인 `refreshed_ms` 미저장을 확인했다.
제품 시험의 시한·기대값·재시도는 변경하지 않았다.

### 최종 관련 owner 검사

| 명령 (`aaa verify --` 뒤) | 결과 |
| --- | --- |
| `nice -n 15 cargo test --profile verify --locked -p aaa --test m13 accounts:: -- --test-threads=1` | 20개 통과, 실패 0, 실행 48.31초 |
| `nice -n 15 cargo clippy --profile verify --locked -p aaa --test m13 -- -D warnings` | 통과 |
| `nice -n 15 cargo fmt --all --check` | 통과 |

원인에 맞춰 기존 E2E의 완료 사건을 고쳤다. 별도 제품 동작·상수 모양 시험은 추가하지 않았다.
최종 Git 변경은 m13 계정 시험과 검증 Markdown뿐이다. 착지 전체 gate는 승인된 착지 창의 최종 통합 SHA에서 수행한다.
공개 저장소 검사 결과와 로컬 커밋은 작업 보고의 상세 기록에 남긴다.

## 참고 구현

- [golang/oauth2 시험](https://github.com/golang/oauth2/blob/master/oauth2_test.go),
  [BSD 3-Clause](https://github.com/golang/oauth2/blob/master/LICENSE): 외부 계정 대신 가짜 HTTP 공급자로 요청·응답을 확인하는 경계를 참고했다.
- [Tokio oneshot](https://docs.rs/tokio/latest/tokio/sync/oneshot/),
  [MIT](https://github.com/tokio-rs/tokio/blob/master/LICENSE): 정한 시간 대신 송신·수신 사건으로 실행 순서를 제어하는 방식을 참고했다.
  이번 별도 프로세스 실험은 저장소에 있는 TCP `Gate`를 재사용한다.

외부 소스 코드·의존성은 추가하지 않았다.
