# 작업별 계정 인증

profile은 실행할 harness·model·effort를 고르고, account는 인증 신원을 고른다.
사람이 aaa 밖 터미널에서 로그인한 결과를 host에 등록한다. aaa는 로그인하지 않는다.

```sh
aaa account import work-claude --harness claude --file /private/path/setup-token.txt
aaa account import work-codex --harness codex --file /private/path/account-home/auth.json
aaa account list
aaa thread start demo --profile codex --account work-codex --task-file task.md
aaa account remove work-codex
```

Claude 입력은 `claude setup-token`으로 발급한 OAuth 토큰 한 줄이다. Codex 입력은 사람이 계정별
`CODEX_HOME`으로 로그인하고 `cli_auth_credentials_store = "file"`로 저장한 auth.json이다.
등록·삭제는 host 로컬 CLI에서만 가능하며 pane 안, 웹, 원격 클라이언트에서는 거부한다.
목록과 thread에는 별칭·harness·버전만 표시한다. 계정 지정이 없으면 기존 인증 환경을 사용한다.

host는 K 저장 층의 최소 부분을 먼저 구현한다. `state/secrets.key`는 32바이트 KEK(0600),
`state/accounts/`는 0700, 그 안의 계정 파일은 0600이다. 각 파일의 무작위 DEK를 KEK로 감싸고
ChaCha20-Poly1305 AAD로 별칭·harness·버전을 묶는다. 저장은 fsync·원자 rename을 사용한다.
키가 없거나 손상되면 해당 인증을 사용하는 시작은 실패하며 계정 미지정 기능은 계속 제공한다.
전체 K 관리 화면·범용 비밀 참조·키 회전은 후속 범위다. 키와 암호문을 함께 백업해야 한다.

비밀은 전용 TLS/UDS 요청과 agent→session host stdin pipe로만 전달한다. Claude는 자식 프로세스의
`CLAUDE_CODE_OAUTH_TOKEN`에 주입한다. Codex는 agent의 `account-runs/<project>/<thread>/codex-home`을
사용한다(폴더 0700, 파일 0600). 인증은 종료 때 삭제하고 대화 기록은 조회·재개를 위해 보존한다.
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
