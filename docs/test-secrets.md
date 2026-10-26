# 시험 인증 fixture

인증·비밀 fixture는 공통 생성기로 만든다. 토큰 본문에 계정명, 업무 이름, 필드 이름을 넣지 않는다. 가림 규칙은 프로세스 전체에 등록된 모든 8바이트 조각을 숨기므로, `automation` 같은 이름을 토큰에 붙이면 다른 시험의 정상 JSON 필드까지 가릴 수 있다.

| 언어 | 같은 seed의 고정값 | 독립적인 새 값 |
| --- | --- | --- |
| Rust | `aaa_test_support::token("회전 전")` | `aaa_test_support::fresh_token()` |
| Python | `scripts.fixture_secret.token("회전 전")` | `scripts.fixture_secret.fresh_token()` |
| Node E2E | `scripts/fixture-secret.mjs`의 `token("회전 전")` | 같은 모듈의 `freshToken()` |

Rust에서는 `aaa-test-support`를 dev-dependency로만 둔다. const 문맥의 설치 암호와 PowerShell 시험은 같은 생성기의 `token("installation")` 산출물인 `scripts/fixtures/secret-canary.txt`를 읽는다. 이 파일에는 개행을 붙이지 않는다. 공통 벡터와 함께 Rust·Python 시험이 생성값 일치를 확인한다.

규약 v1은 UTF-8 seed의 SHA-256 각 byte를 상위 nibble의 `g`~`v` 문자와 하위 nibble의 두 자리 십진수 `00`~`15`로 무손실 표현한다. 본문은 96바이트이고 모든 8바이트 조각에 비-hex 문자와 숫자가 있다. 따라서 사전 단어·필드 이름·정상 hex digest와 겹치지 않는다. 시험 이름은 seed에만 남고 본문에는 남지 않는다. 운영 인증 생성에는 사용하지 않는다.

로그인 출력 파서에 필요한 `sk-ant-oat01-` 접두사, JWT metadata의 header/claims, escape·ANSI 재현에 필요한 구두점은 그 계약에서만 유지한다. 가짜 harness가 계정을 알아야 하면 토큰을 나누지 말고 실험실의 별도 토큰→이름 대응표를 읽는다. 토큰을 바꾸면 원문·부분 누출 검사도 실제 생성값을 검사하게 함께 고친다.

`python3 scripts/check_fixture_secrets.py`는 인증 필드·SecretText·금고 등록·가림 등록·공식 인증 env 호출과 secret/token/canary 선언의 직접 문자열을 검사한다. Rust raw JSON, 내장 Python·Node harness, PowerShell도 대상이다. 위반 시 원문 대신 경로·줄 번호만 출력하고 즉시 실패한다. ci-local의 test 진입 전에 실행하며, 문법 오류/문자열 파서의 고정 원문만 `scripts/fixtures/secret-exceptions.json`에 정확한 경로·값·사유로 둔다. 쓰이지 않는 예외도 실패한다. 이 검사는 임의의 변수 간 데이터 흐름을 해석하지 않으므로 동적 fixture는 반드시 공통 생성기로 만들고 전달·가림 owner를 유지한다.

새 회귀는 필요한 계약만 검사한다.

- 공통 벡터: Rust·Python·Node 구현 차이로 한 언어의 fixture가 달라지는 것을 막는다.
- 업무 단어 거부와 소스 검사: 평문 토큰 재유입을 제품 시험 실행 전에 알린다. CI의 가짜 실험실에 업무 단어 토큰을 넣으면 경로·줄 번호만 알리고 Python·Rust 실행 전에 멈춰야 한다.
- 같은 프로세스의 가림 회귀: 생성 토큰은 숨기면서 `automation`·`owner`·인증 JSON 필드가 보존되는지 확인한다.
- 기존 계정 canary owner: 정상 digest와 구별되고 모든 8바이트 부분 누출을 계속 검출한다.

참고한 OSS는 [Sentry Python 공통 fixture](https://github.com/getsentry/sentry-python/blob/master/tests/conftest.py)의 시험 사이 자료 유출 방지([MIT](https://github.com/getsentry/sentry-python/blob/master/LICENSE))와 [stripe-mock](https://github.com/stripe/stripe-mock/blob/master/README.md)의 API 자료형·fixture 공통화([MIT](https://github.com/stripe/stripe-mock/blob/master/LICENSE))다. fixture 격리와 공통 규약 원칙만 따랐으며 외부 코드·의존성을 추가하지 않는다. 이름을 포함한 가짜 인증 문자열은 이번 가림 계약에 맞지 않아 사용하지 않는다.
