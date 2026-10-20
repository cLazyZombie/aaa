# 작업별 계정 인증

profile은 실행할 harness·model·effort를 고르고, account는 인증 신원을 고른다.
host의 **aaa 세션 밖 터미널**에서 공식 harness 로그인을 시작하고 기본 계정을 정한다.
브라우저 인증은 사용자가 host에서 한다. 기본 계정을 지정하기 전에는 기존 머신 로컬 로그인을 사용한다.

```sh
aaa account login work-claude --harness claude
aaa account login work-codex --harness codex
# 각 명령이 반환한 ID로 상태를 확인한다. succeeded 뒤에 기본으로 지정한다.
aaa account login-status <로그인-ID>
aaa account default claude work-claude
aaa account default codex work-codex
aaa account default claude
aaa account list
```

로그인은 host의 격리된 임시 홈과 별도 PTY에서 공식 `claude setup-token` 또는
`codex -c 'cli_auth_credentials_store="file"' login`을 실행한다. host 브라우저가 열리면 해당 계정으로
인증한다. 다른 머신 브라우저로 localhost callback을 대신하지 않는다. 원본 출력·오류·토큰·인증 URL은
aaa 화면이나 로그에 중계하지 않는다. 원본 출력이 필요한 수동 코드 입력 방식은 이 경로에서 지원하지 않는다.
로그인 상태는 `waiting`, `succeeded`, `failed`, `cancelled`이며 host를 다시 시작하면 진행 상태 ID는 유지하지 않는다.
`aaa account login-cancel <로그인-ID>`로 취소한다. 5분 상한·출력 128KiB 상한을 넘으면 실패한다.
성공 결과를 암호화 저장하고 성공·실패·취소의 임시 홈 정리를 끝낸 뒤 종료 상태를 표시한다.
정상 host 종료도 진행 중인 로그인을 취소하고 정리가 끝나기를 기다린다.

새 worker·평가 thread와 새 coordinator pane은 profile의 harness에 지정된 host 기본 계정을 사용한다.
CLI·웹·할 일·자동화·임무에서 시작해도 host가 같은 규칙을 적용한다. `thread start --account`가 있으면
그 계정이 기본보다 우선한다. 기본 미지정은 기존 동작 그대로다. 지정한 인증이 없거나 잠겼거나
harness가 다르면 시작을 거부한다. 다른 계정·머신 로컬 로그인으로 조용히 바꾸지 않는다.

시작할 때 고른 계정 별칭을 기록한다. 기본 변경·해제는 **그 뒤 새로 만드는 작업에만** 적용한다.
기존 작업의 재시작·resume·재부팅 복원과 coordinator의 `pane resume`은 처음 선택한 계정의 최신 인증과
같은 기록 홈을 사용한다. 기존 로컬 인증 작업도 그대로 로컬을 쓴다. `project open`이 실행 중인
coordinator를 다시 보여 주는 동작은 새 시작이 아니다. 기본으로 쓰는 계정은 먼저 기본 설정을 해제해야 삭제할 수 있다.

인증값 없이 적용을 확인한다:

```sh
aaa thread show demo t-0001
aaa --json pane list
aaa account default codex
```

thread의 `인증: <별칭>`/JSON `account`, coordinator pane의 JSON `account`가 host 계정 선택의 표지다.
이 표지는 선택·전달 경로를 알려 주며 실제 제공자 로그인 성공까지 대신 확인하지는 않는다.
host 감사 기록 `account_login`, `account_default`, `pane_run_account`에도 별칭만 남긴다.
금고 내용·원본 인증 파일·자식 env를 열어 확인하지 않는다.

기본 설정을 끄면 이후 새 작업은 기존 로컬 로그인을 쓴다. 기존 host 인증 작업의 선택은 유지한다.

```sh
aaa account default claude --off
aaa account default codex --off
```

기존 결과 파일을 가져오는 경로도 유지한다:

```sh
aaa account import work-claude --harness claude --file /private/path/setup-token.txt
aaa account import work-codex --harness codex --file /private/path/account-home/auth.json
aaa account list
aaa thread start demo --profile codex --account work-codex --task-file task.md
aaa account remove work-codex
```

Claude 입력은 `claude setup-token`으로 발급한 OAuth 토큰 한 줄이다. Codex 입력은 사람이 계정별
`CODEX_HOME`으로 로그인하고 `cli_auth_credentials_store = "file"`로 저장한 auth.json이다.
로그인·로그인 상태/취소·등록·삭제·기본 변경은 host의 aaa 세션 밖 로컬 CLI에서만 가능하며
pane 안, 웹, 원격 클라이언트에서는 거부한다. 첫 배포에는 웹 로그인 시작을 제공하지 않는다.
목록과 thread에는 별칭·harness·버전만 표시한다. 기본 설정 조회에는 인증값이 없다.

host는 K 저장 층의 최소 부분을 먼저 구현한다. `state/secrets.key`는 32바이트 KEK(0600),
`state/accounts/`는 0700, 그 안의 계정 파일은 0600이다. 각 파일의 무작위 DEK를 KEK로 감싸고
ChaCha20-Poly1305 AAD로 별칭·harness·버전을 묶는다. 저장은 fsync·원자 rename을 사용한다.
키가 없거나 손상되면 해당 인증을 사용하는 시작은 실패한다. 기본을 해제하면 로컬 로그인 작업은 계속 제공한다.
전체 K 관리 화면·범용 비밀 참조·키 회전은 후속 범위다. 키와 암호문을 함께 백업해야 한다.

비밀은 전용 TLS/UDS 요청과 agent→session host stdin pipe로만 전달한다. Claude는 자식 프로세스의
`CLAUDE_CODE_OAUTH_TOKEN`에 주입한다. Codex는 agent의 `account-runs/<project>/<thread>/codex-home`을
사용한다(폴더 0700, 파일 0600). 인증은 종료 때 삭제하고 대화 기록은 조회·재개를 위해 보존한다.
coordinator는 thread와 다른 `c-<실행 ID>` 이름 공간을 쓴다. 새 coordinator 인증에는
`account_auth.v2` agent가 필요하며 미지원 agent에서는 시작을 거부한다.
profile의 실행 옵션과 aaa hook을 유지하며 기존 홈·인증·설정 전체를 복사하지 않는다.

같은 계정의 작업은 머신 안팎에서 동시에 실행한다. Codex의 **갱신 요청만** host에서 계정별로
직렬화한다. pane마다 임의 capability를 가진 localhost 중계를 두고 CLI의
`CODEX_REFRESH_TOKEN_URL_OVERRIDE`를 지정한다. host가 요청 버전을 비교해 오래된 요청에는 최신
토큰을 반환하고, 최신 요청 하나만 공식 제공자에 갱신을 요청한다. 새 인증을 암호화·원자 저장한
뒤 응답한다. CLI는 자기 홈에 응답을 저장한다. 응답에 없거나 null인 토큰은 이전 값을 유지한다.
외부에서 바뀐 auth.json 사본을 host에 무조건 덮어쓰지 않는다.

이 중계는 [Codex 0.163.0 공식 소스](https://github.com/openai/codex/blob/rust-v0.163.0/codex-rs/login/src/auth/manager.rs)의
동작에 맞춘다. URL override는 안정된 공개 설정 계약이 아니다. 지원하지 않는 CLI는 인증을
전달하기 전에 거부한다. [공식 CI 인증 안내](https://learn.chatgpt.com/docs/auth/ci-cd-auth)의
사본 동시 갱신 경고를 피하려고 갱신을 중앙에 모았다. [Claude 환경 변수](https://code.claude.com/docs/en/env-vars)와
[Codex file 인증](https://learn.chatgpt.com/docs/auth)을 함께 참고했다.

host가 끊기거나 제공자가 거부하면 갱신은 실패한다. 직접 제공자로 우회하거나 자동 재시도하지
않는다. 제공자의 회전 성공과 host 저장 사이에 host가 죽으면 재로그인·재등록이 필요할 수 있다.
영속 갱신 의도 기록, 회전 결과 회수, 비정상 종료 뒤 임시 auth 청소는 다음 단계다.
작업 실행권·계정 lease로 동시 작업을 막지 않는다.

등록한 토큰의 원문·8바이트 조각·JSON/URL/base64 변형을 출력·기록 경계에서 가린다. PTY는 ANSI·
개행·chunk 경계를 넘어 검사하며, post는 미전송 outbox에 쓰기 전에 가린다. 재접속 agent는 인증한
session IPC로 메모리 가림 규칙을 복원한다. 인증 전달과 실행 입력에는 가림을 적용하지 않는다.
같은 OS 사용자의 다른 프로세스·harness 자체의 원본 기록 접근 차단은 K의 L1과 같이 보장하지 않는다.

검증은 빈 포트·임시 폴더·가짜 OAuth 제공자와 harness만 사용한다. 실계정 로그인이나 실제 인증
파일을 읽지 않는다. `m13::accounts`가 두 머신 네 작업의 동시 회전·부분 응답·거부·기록 조회·재개·
정상 청소·미전송 post·로그/DB/화면 비노출을 소유한다.
