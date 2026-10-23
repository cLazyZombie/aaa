# 하위 오류의 진단

실패를 사용자 안내로 바꿀 때 하위 오류를 버리지 않는다. `aaa_common::diagnostic::context`는 기존 오류 코드와 안내를 유지하면서 안전한 원인 요약을 덧붙이고, 변환하는 경계에서 info 사건 하나를 남긴다. 다른 층은 이 요약을 전송하며 같은 원인을 다시 기록하지 않는다. 오류 사슬에 OS 오류가 있으면 OS 번호·종류, JSON 오류가 있으면 분류·행·열을 추출한다. 판별할 수 없는 오류는 타입과 발생 위치를 기록한다.

내부 ErrorBody 자체도 원문 신뢰의 근거가 아니다. 설정 파서의 값을 ErrorBody에 감싼 경로가 있으므로, 고정 안내로 바꾸는 경계에서는 원문 대신 코드를 추출한다. 전송 자체는 기존 ErrorBody를 보존한다.

인증 JSON·TOML 파서, 사용자 입력, 외부 실행기의 stdout/stderr, `SendError<T>`와 panic의 Display/Debug는 비밀을 포함할 수 있다. 이 원문을 로그·API에 넣지 않는다. 알려진 오류의 구조화 필드와 컴파일 시점의 타입·상대 소스 위치만 추출한다. `anyhow`의 맥락 문자열도 이 경계에서는 원문으로 포맷하지 않는다. 이미 안전한 작업 오류는 기존 `{error:#}` 전달을 유지한다.

정리·없는 선택 파일·후보 탐색은 기존 fallback을 유지한다. `ignored`의 stage에는 계속하는 까닭을, `observe`에는 복구/기본값을 쓰는 까닭을 적는다. 이 경로는 debug 수준이며 정상 흐름을 warn으로 올리지 않는다. 메모리 String에 쓰는 `fmt::Write`는 실패하지 않으므로 로그 대상이 아니다. panic hook과 tracing writer의 stderr 쓰기 실패에는 추가 tracing을 하지 않는다. 진단 자체가 같은 writer로 재귀하기 때문이다.

설치 엔진의 `Fault`와 Tailscale의 `ExactError`는 안전한 원인도 함께 전달한다. 설치 NDJSON/JSON의 error details 또는 needs_user 안내까지 보존한다. SCM·레지스트리 adapter는 원래 Win32 오류 번호도 보존한다. 웹은 서버 ErrorBody의 message를 그대로 보여 준다. 손상된 응답은 HTTP 상태와 JSON/읽기 단계만 표시하고 본문은 표시하지 않는다. 사용량의 typed 문제 상태는 기존 wire 계약을 유지하며 helper RPC의 상세 실패는 host 로그에 기록한다.

## 참고한 오픈소스

- [anyhow README](https://github.com/dtolnay/anyhow/blob/master/README.md): 작업 단계에서 맥락을 붙이고 상위 경계에서 원인을 전달하는 방식. [MIT/Apache-2.0](https://github.com/dtolnay/anyhow#license)을 확인했다. 인증 원문이 섞일 수 있는 경계는 구조화된 안전 필드만 추출한다.
- [tracing README](https://github.com/tokio-rs/tracing/blob/master/tracing/README.md): 오류를 이벤트의 error 필드와 단계로 남기고 라이브러리는 subscriber를 설치하지 않는 관례. [MIT](https://github.com/tokio-rs/tracing/blob/master/LICENSE)를 확인했다.
- [rustup self_update](https://github.com/rust-lang/rustup/blob/main/src/cli/self_update.rs): 설치의 단계 맥락, 실패 보고와 최선 노력 정리를 구별하는 방식. [MIT/Apache-2.0](https://github.com/rust-lang/rustup#license)을 확인했다.

외부 코드나 새 의존성을 가져오지 않았다. 기존 tracing·anyhow와 오류 타입을 쓴다. API 판과 데이터 스키마는 바꾸지 않는다. t-0910의 계정 pane 시작 오류 변환은 담당 변경을 따른다.
