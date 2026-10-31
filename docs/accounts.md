# 작업별 계정 인증

thread에서 host Codex 계정으로 이미지 생성·편집을 요청하는 방법은 [이미지 CLI](images.md)에 있다.

profile은 실행할 harness·model·effort를 고르고, account는 인증 신원을 고른다.
host의 **aaa 세션 밖 터미널**이나 **웹 설정 「계정」(조작 기기)**에서 공식 harness 로그인을 시작하고 기본 계정을 정한다.
웹에서는 접속한 폰·PC에서 인증한다. CLI 명령은 기존 host 브라우저 인증을 유지한다.
기본 계정을 지정하기 전에는 기존 머신 로컬 로그인을 사용한다.

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

CLI 로그인은 host의 격리된 임시 홈과 별도 PTY에서 공식 `claude setup-token` 또는
`codex -c 'cli_auth_credentials_store="file"' login`을 실행한다. host 브라우저가 열리면 해당 계정으로
인증한다. 다른 머신 브라우저로 localhost callback을 대신하지 않는다. 원본 출력·오류·토큰·인증 URL은
aaa 화면이나 로그에 중계하지 않는다. 원본 출력이 필요한 수동 코드 입력 방식은 이 경로에서 지원하지 않는다.
로그인 상태는 `waiting`, `succeeded`, `failed`, `cancelled`, 그리고 다시 로그인의 `mismatch`·`kept`(아래 「메일·인증일·상태」)이며
host를 다시 시작하면 진행 상태 ID는 유지하지 않는다.
`aaa account login-cancel <로그인-ID>`는 자식 종료·임시 자료 정리가 끝난 최종 상태를 반환한다. 같은 ID를 여러 번 취소해도 모두 완료 상태를 받으며 이미 성공/실패한 결과는 유지한다. 5분 상한·출력 128KiB 상한을 넘으면 실패한다.
`waiting`은 로그인 미완료 상태이며 브라우저가 열렸다는 확인은 아니다. 원래 실행한 host의 로그인 사용자 브라우저에서 인증한다. 로그인별 자식 PID·종료 코드·최종 상태·임시 자료 정리 여부는 host 로그로 확인할 수 있지만 원본 출력·인증 URL·토큰은 기록하지 않는다.
성공 결과를 암호화 저장하고 성공·실패·취소의 임시 홈 정리를 끝낸 뒤 종료 상태를 표시한다.
정상 host 종료도 진행 중인 로그인을 취소하고 정리가 끝나기를 기다린다.

## 접속한 기기에서 다시 로그인

FRONT_API 1.151부터 왼쪽 메뉴 아래 「인증 갱신」에 만료·갱신 거부·곧 만료인 계정을 보인다.
항목을 누르면 해당 계정의 인증 창이 열린다. 기본 설정 → 계정 → 「다시 로그인」도 같은 창을 연다.
정상 계정·일시적인 조회 실패·사용량 소진은 갱신 목록에 추가하지 않는다.

- **Codex**: 「인증 열기」로 공식 기기 인증 페이지를 열고 화면의 코드를 입력한다. host의 공식
  `codex app-server`가 `account/login/start(type=chatgptDeviceCode)`로 시작하고 해당 loginId의 완료를 기다린다.
  계정에서 기기 코드 로그인을 허용해야 한다. 인증이 성공하면 host 임시 홈의 auth.json을 기존 금고에 저장한다.
- **Claude**: 「인증 열기」에서 로그인한 뒤 제공자가 보여 주는 코드를 aaa 인증 창에 붙여넣고 「확인」한다.
  공식 `claude setup-token`의 수동 코드 경로를 사용하며, host 브라우저는 열지 않는다.
  발급된 장기 OAuth 토큰은 기존 Claude 계정 형식으로 저장한다. 웹·메일·Google 로그인 여부는 제공자 화면에서 선택한다.
  현재 Claude 원격 경로는 POSIX host(macOS·Linux)에서 제공하며 Windows host에서는 시작을 거부한다.

인증 링크와 기기 코드는 **시작한 웹 조작 기기만**, 5분 동안 볼 수 있다. 같은 기기의 새로고침·화면 복귀는
host의 진행 중 인증을 다시 읽는다. 다른 기기·보기 전용 기기·pane 안 agent는 조회·코드 제출·취소·교체를 할 수 없다.
기기별 진행은 하나이며 코드는 한 번만 제출한다. 잘못 입력해 실패하면 다시 시작한다.
브라우저 저장소·URL hash·보고·감사 기록·일반 이벤트·로그에는 인증 링크와 코드를 넣지 않는다.
API 응답은 기존 `no-store` 정책을 따른다. 실제 OAuth 토큰·refresh token·CLI 원본 출력은 웹에 보내지 않는다.

「취소」·5분 만료·기기 권한 해제·정상 host 종료는 로그인 자식을 종료하고 임시 자료를 지운다.
기존 인증은 새 인증이 성공적으로 저장될 때까지 유지한다. 다른 메일이거나 기존 계정의 새 메일을 확인할 수 없으면
「유지 / 교체」를 고르게 한다. 이 확인에도 5분 시한이 있으며 권한 해제·취소 시 새 인증을 버린다.
강제 종료·전원 손실 뒤 진행 복원은 제공하지 않는다.

원격 경로는 [Codex 공식 인증 문서](https://learn.chatgpt.com/docs/auth#login-on-headless-devices)와
[app-server 기기 코드 계약](https://learn.chatgpt.com/docs/app-server#3b-log-in-with-chatgpt-device-code-flow),
[Claude 공식 인증 문서](https://code.claude.com/docs/en/authentication)를 따른다.
[Orca](https://github.com/stablyai/orca)·[T3 Code](https://github.com/pingdotgg/t3code)·
[CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI)(모두 MIT)의 공식 CLI 중계·요청자 결속·상태 검증 방식을 참고했다.
외부 코드를 복사하거나 새 런타임 의존성을 추가하지 않았다.

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

`thread restart --profile`도 처음 고른 계정을 유지한다. 같은 harness 안에서는 다른 profile로 바꿀 수 있지만,
고정 계정과 harness가 다른 profile은 brief·실행 기록을 바꾸기 전에 거부한다. 다른 harness를 쓰려면 새 thread를
시작한다. 새 thread는 그 harness의 기본 계정을 고르며 `--account`로 직접 고를 수도 있다.

실행 중인 thread를 다시 띄우려면 `aaa pane kill <pane>`으로 먼저 세션을 끝내고 종료 상태를 확인한다.
restart가 새 pane을 띄우면 옛 pane의 이름·화면 자리를 잇고 옛 기록을 정리한다. 새 pane 기동이 실패하면
옛 기록은 남는다. `pane close`로 먼저 기록까지 지웠어도 thread의 작업 폴더·저장된 대화로 재시작할 수 있다.

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
로그인·로그인 상태/취소/확인·기본 변경은 사람이 한다: host의 aaa 세션 밖 로컬 CLI나 웹 조작 기기(FRONT_API 1.79).
pane 안 agent·worker·evaluator·detached와 보기 전용 기기는 거부한다. 등록(`import`)·삭제는 host의 aaa 세션 밖 로컬 CLI에서만 가능하다.
thread에는 별칭만 표시한다. 기본 설정 조회에는 인증값이 없다.

## 다시 로그인 없이 이름 바꾸기

웹 설정 「계정」의 계정 행에서 「이름 바꾸기」를 누르고 새 이름을 저장한다. 390px 휴대폰에서도 같은 입력과 결과를 제공한다.
조작 기기 또는 host의 aaa 세션 밖 CLI만 바꿀 수 있다. 보기 전용 기기·pane 안 agent·worker·coordinator는 거부한다.

```sh
aaa account rename work-codex team-codex
```

새 별칭은 영문·숫자·`-`·`_` 1~64자이며, 이미 쓰는 별칭(다른 harness 포함)은 거부한다. harness는 바꾸지 않는다.
인증 원문·판·로그인 세대·메일·plan·최초/최근 인증일·남은 사용량 관측은 유지한다. 금고를 옛 별칭 AAD로 복호화하고,
새 별칭 AAD와 새 DEK·nonce로 재암호화해 fsync·원자 rename으로 저장한다. 다시 로그인하거나 제공자에게 인증을 요청하지 않는다.

harness별 기본 계정·자동 배정 풀의 계정·대기 시작·보류/종료 thread·pane 실행 인증 참조·이미지 요청의 계정 칸을 함께 옮긴다.
같은 한도를 공유하는 **풀의 한도 그룹 이름은 유지**한다. profile·모델 이름이 계정 별칭과 같아도 바꾸지 않는다.
현재 할 일/예약/자동화의 profile 칸은 계정 별칭이 아니다. 구조화된 `account` 칸이 있는 저장 기록·설정만 이전하고,
제목·메모·작업 본문·명령·env·과거 감사 원문은 치환하지 않는다. 감사에는 옛 이름과 새 이름만 담은 `account_rename` 사건을 남긴다.

이 계정을 쓰는 열린 thread·실행/준비 중 pane·로그인/메일 확인이 있으면 이유를 보이고 거부한다.
thread를 보류하거나 완료하고 실행 중인 pane을 종료한 뒤 바꾼다. 계정 시작·복원·중앙 갱신과 이전은 겹치지 않는다.
느린 사용량 조회는 이름 변경을 막지 않는다. 늦게 끝난 옛 별칭 조회는 버리고 새 별칭에 이전한 관측을 다시 읽는다.

새 금고·설정을 준비한 뒤 DB 참조와 확정 표지를 한 트랜잭션으로 옮기고 옛 금고를 치운다. `account-rename:pending` journal에는
인증 원문이나 금고 암호문을 넣지 않는다. 저장 실패는 원래 파일·참조로 복구한다. host가 중단되면 다음 시작에서
DB 확정 전에는 원래 상태로, 확정 뒤에는 새 상태로 복구한 뒤 인증을 읽는다. 복구 중 직접 편집을 발견하면 덮어쓰지 않고 경로와 원인을 알린다.

재암호화는 [SOPS rotate](https://github.com/getsops/sops/blob/main/cmd/sops/rotate.go)([MPL-2.0](https://github.com/getsops/sops/blob/main/LICENSE))의
인증된 복호화→새 데이터 키→재암호화 순서와 [age](https://github.com/FiloSottile/age/blob/main/age.go)([BSD-3-Clause](https://github.com/FiloSottile/age/blob/main/LICENSE))의
인증된 암호문 처리 원칙을 참고했다. 외부 코드·의존성은 추가하지 않는다. 파일/DB의 중단 복구는 저장소의 머신 이름 이전 journal 방식을 따른다.

## 메일·인증일·상태(관리 GUI W5a)

`aaa account list`와 웹 설정 「계정」은 계정마다 별칭·harness·판에 더해 메일·plan·최초 인증일·최근 인증된 날·상태를 보인다:

```text
work	codex	판 2	work@example.com	plus	최초 2026-10-21	최근 2026-10-21	정상
ci	claude	판 1	ci@example.com	-	최초 2026-10-21	최근 2026-10-21	곧 만료(2026-10-24)
```

- **메일·plan**은 공식 CLI가 로그인 때 남긴 것만 적는다. Codex는 `auth.json`의 id_token 계정 정보(메일, `chatgpt_plan_type`),
  Claude는 로그인 폴더의 `.claude.json`에 `oauthAccount.emailAddress`가 있을 때만이다(`claude setup-token`은 메일을 출력하지
  않는다: 없으면 「메일 모름」). 이 기록 전에 등록한 Codex 계정은 host가 시작할 때 저장된 인증에서 메일·plan을 채운다.
- **최초 인증일**은 aaa가 처음 기록한 로그인, **최근 인증된 날**은 마지막 로그인 성공이다(날짜만, CLI는 UTC·웹은 그 기기 시간대).
  이 기록 전에 등록한 계정은 「모름」이며 「다시 로그인」하면 채운다. Codex 갱신은 날짜를 바꾸지 않는다.
- **상태**: `정상`, `곧 만료`(알려진 만료 7일 전부터), `끊김(만료)`, `끊김(갱신 거부)`. 판정은 실제 쓰임과 알려진 만료 시각뿐이다:
  Codex 갱신을 제공자가 400·401로 거부하면 끊김이고(망·시한·5xx는 끊김이 아니다), 갱신이 다시 성공하거나 로그인하면 지운다.
  만료 시각은 `claude setup-token`이 알린 유효 기간뿐이다(Codex는 갱신으로 이어 가 만료를 모른다). 제공자에게 따로 묻지 않으므로
  Claude 토큰이 만료 전에 취소되면 쓰기 전까지 모른다.
- 상태가 `곧 만료`·`끊김`으로 바뀌면 알림함에 「확인 필요」 항목(`account`)을 한 번 낸다. 다시 정상이 되면 끝낸다. 항목에는 별칭·
  harness만 싣는다.
- 메일·plan은 웹 조작 기기와 host의 aaa 세션 밖 CLI에만 싣는다. 보기 전용 기기와 pane 안 agent에는 별칭·상태만 간다.
  감사 기록·알림함·로그·외부 푸시에는 메일을 쓰지 않는다. 기록은 host DB(`account-meta:<별칭>`)에 두며 금고 파일은 그대로다.

**다시 로그인**은 같은 별칭으로 `aaa account login`(웹 「다시 로그인」)이다. 그 별칭에 메일이 기록돼 있으면 「이 메일로 로그인하세요」를
보인다. 다른 메일로 로그인하면 금고를 바꾸지 않고 `mismatch`로 기다린다(5분, 지나면 새 인증을 버린다):

```sh
aaa account login-status <로그인-ID>          # 기록된 메일·로그인한 메일
aaa account login-confirm <로그인-ID> --switch # 이 별칭을 새 메일 인증으로(최초 인증일도 새로)
aaa account login-confirm <로그인-ID> --keep   # 새 인증을 버리고 옛 인증 그대로(kept)
```

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
중계 경로를 참고했다. 실행한 `--version` 결과는 진단 로그와 pane 시작 기록에 남긴다.
판·출력 형식·조회 실패로 인증 전달이나 시작을 막거나 경고하지 않는다. [공식 CI 인증 안내](https://learn.chatgpt.com/docs/auth/ci-cd-auth)의
사본 동시 갱신 경고를 피하려고 갱신을 중앙에 모았다. [Claude 환경 변수](https://code.claude.com/docs/en/env-vars)와
[Codex file 인증](https://learn.chatgpt.com/docs/auth)을 함께 참고했다.

host가 끊기거나 제공자가 거부하면 갱신은 실패한다. 직접 제공자로 우회하거나 자동 재시도하지
않는다. 제공자의 회전 성공과 host 저장 사이에 host가 죽으면 재로그인·재등록이 필요할 수 있다.
영속 갱신 의도 기록, 회전 결과 회수, 비정상 종료 뒤 임시 auth 청소는 다음 단계다.
작업 실행권·계정 lease로 동시 작업을 막지 않는다.

등록한 토큰의 원문·8바이트 조각·JSON/URL/base64 변형을 출력·기록 경계에서 가린다. PTY는 ANSI·
개행·chunk 경계를 넘어 검사하며, post는 미전송 outbox에 쓰기 전에 가린다. 재접속 agent는 인증한
session IPC로 메모리 가림 규칙을 복원한다. 인증 전달과 실행 입력에는 가림을 적용하지 않는다.
실시간 PTY는 q-1285 1A 승인에 따라 정상 입력을 다음 출력까지 보류하지 않는다. 인증 조각의 앞
최대 7바이트가 화면·ring·snapshot과 그 출력 기록에 먼저 남을 수 있고, 8바이트 조각이 완성되는
바이트부터 가린다. 일반 로그·오류·post 등 완성된 자료는 앞부분까지 가리는 기존 방식을 유지한다.
같은 OS 사용자의 다른 프로세스·harness 자체의 원본 기록 접근 차단은 K의 L1과 같이 보장하지 않는다.

검증은 빈 포트·임시 폴더·가짜 OAuth 제공자와 harness만 사용한다. 실계정 로그인이나 실제 인증
파일을 읽지 않는다. `m13::accounts`가 두 머신 네 작업의 동시 회전·부분 응답·거부·기록 조회·재개·
정상 청소·미전송 post·로그/DB/화면 비노출을 소유한다.

## 계정별 남은 사용량(A2)

웹 설정 「계정」에서 등록한 계정마다 창별 **남은 %**, 리셋·관측 시각, 머신과 수집 출처를 본다.
없는 창은 「사용량 모름」이다. 기간이 정확히 300분·10080분일 때만 5시간·주간이라고 부른다.

- Codex는 host가 격리된 임시 홈에 해당 등록 계정의 인증만 전달하고 공식 app-server의
  `initialize` → `initialized` → `account/rateLimits/read`를 호출한다. `thread/start`·`turn/start`는
  보내지 않는다. 기존 중앙 갱신과 경합하지 않도록 helper의 갱신은 자기 loopback 거부 서버로만
  보낸다. 인증을 새로 만들거나 갱신하지 않으며 실패는 카드에 남긴다. `--version`은 진단 로그에만 남기며 판이나 조회 실패로 RPC 실행을 막지 않는다.
  실패 로그에는 계정 별칭·RPC 단계·구조화된 HTTP 상태·사용량 오류 종류·로컬 갱신 차단 여부를
  남긴다. HTTP 상태를 message에서만 추론했다면 구조화된 상태는 없음으로 남긴다. 제공자 원문·
  인증값·코드·URL은 기록하지 않는다. 이 조회 오류로 계정의 인증 건강을 바꾸거나 배정을 막지 않는다.
  [공식 RPC 문서](https://learn.chatgpt.com/docs/app-server#6-rate-limits-chatgpt)를 따른다.
- Claude는 새 계정 pane에 주입한 상태줄 명령이 stdin의 `rate_limits.five_hour`·`seven_day`를
  그 pane의 전용 파일에 기록한다. 원격 agent도 비밀 없는 snapshot만 host에 보낸다. 이전 pane·
  옛 agent·한 번도 관측하지 않은 계정은 상태줄 수치가 없다. 최신 공용 5시간·주간 창이 모두
  모이지 않으면 host가 그 등록 계정 인증으로 OAuth 사용량 API도 조회한다. 조회 권한이 부족하면
  미지원으로 남기며 작업 인증을 고장으로 바꾸지 않는다. API 429는 재조회만 미룬다.
  조회를 위해 Claude 작업을 만들거나 인증을 갱신하지 않는다. 실험 `get_usage`는 쓰지 않는다.
  기존 로컬 사용자 설정 파일은 고치지 않는다.
- 수집 대상은 host 등록 목록과 계정 pane의 결속이다. 이 경로는 기본 인증 홈·형제 폴더·키체인을
  탐색하지 않으며 인증 원문·메일·대화 본문을 사용량 응답에 싣지 않는다. 기존 「구독 사용량」의
  로컬 로그인 호환 조회는 별도 기능으로 유지한다.

하단 「구독 사용량」의 Claude는 등록 계정이 있으면 이 계정별 상태줄·OAuth 관측을 쓴다. 기본 홈의
OAuth가 만료돼도 정상 동작하는 등록 계정에 재로그인을 요구하지 않는다. 등록 계정이 없으면
기존 로컬 로그인 조회를 쓴다. Codex 하단 자료원은 그대로다. 수치가 없으면 관측 대기·단절 등
까닭을 칩에 표시하고, 오래된 숫자는 팝업에 마지막 관측이라고 알린다(FRONT_API 1.124).

host는 약 1분마다 갱신하고 수동 「사용량 다시 읽기」는 30초 하한을 지킨다. 429의 재조회 가능
시각이 있으면 그 전에는 묻지 않는다(없으면 60초). 실패에는 마지막 숫자를 흐리게 보인다.
2분까지 최신, 2~10분 오래됨, 10분 초과·리셋 경계 뒤·큰 머신 시각 차이는 모름이다.
화면은 리셋 시각을 지났다는 이유로 100% 남음이라고 만들지 않는다. 자동 배정 계산은 아래 마지막
기록 규칙을 따른다. 상태줄 입력에서 빠진 창은 이전 값과
그 창의 관측 시각을 유지한다. 파일 수정·host 수신 시각으로 관측 시각을 바꾸지 않는다.
같은 계정·창의 최신 관측을 택하고 같은 시각의 다른 숫자는 작은 잔량과 충돌 표시를 쓴다.

등록·다시 로그인마다 토큰과 무관한 새 세대를 만든다. 삭제·같은 별칭 재등록도 새 세대다.
Codex 중앙 갱신은 세대를 유지한다. 이전 인증으로 실행 중인 pane과 늦게 끝난 조회는 새 세대의
숫자가 될 수 없다. 옛 금고 파일은 계속 읽으며 옛 pane의 귀속을 추정하지 않는다.
웹은 `GET /api/v1/account-usage[?refresh=1]`(FRONT_API 1.83)을 사용한다.
원격 snapshot은 `account_quota.v1`을 알린 agent에만 요청한다. 자동 배정·계정 전환(A3)은
하지 않는다. 실계정 로그인·운영 quota 조회는 가짜 시험의 보증 범위 밖이다.

## harness별 계정 자동 배정

계정은 Claude·Codex 등 harness별로 등록한다. 새 로그인·가져오기를 마치면 자동 배정에 바로 사용한다.
설정 → 계정의 각 계정에 있는 「자동 배정에 사용」으로 켜고 끈다. 그룹·허용 모델·프로젝트 참여는 없다.
진행 중 작업이 적은 계정부터, 같으면 가장 오래전에 배정한 계정부터 고른다. 잔량 숫자가 큰 계정에
작업을 몰지 않으며, 같은 계정의 여러 작업도 허용한다. 새 thread·할 일·예약·자동화가 같은 host 정책을 쓴다.

```sh
aaa account login work-claude --harness claude
aaa account pool list
aaa account pool remove work-claude  # 자동 배정에서 사용 해제
aaa account pool add work-claude     # 다시 사용
aaa thread start demo --profile claude --title '독립 작업' --task-file task.md
# 같은 시작을 재전송해야 하면 고유 ID를 준다.
aaa thread start demo --profile claude --account-pool --start-id request-001 \
  --title '독립 작업' --task-file task.md
aaa account starts demo
aaa account cancel-start request-001 demo
```

그 harness에 사용할 계정이 없으면 기존 harness 기본 계정을 쓴다. 기본 계정도 없으면 대상 머신의
로컬 로그인으로 시작한다. 다른 harness의 풀 때문에 배정 대기로 멈추지 않는다. 명시한 `--account`는
우선하며, 기본 계정 설정은 자동 배정에서 다른 계정을 고르더라도 바꾸지 않는다. 기존 대화의
restart·resume은 처음 계정을 유지하고 다른 계정·모델·머신으로 바꾸지 않는다.

사용량은 같은 인증 세대의 마지막 완료된 캐시만 읽는다. 기동이 별도 조회 RPC를 기다리지 않는다.
**5시간 창이 없고 주간이 10% 사용이면 주간 창만 보고 시작한다.** 최신 주간 창이 100%면 그 계정을
건너뛴다. 배정은 주간 잔량이 많은 계정을 우선한다. 지금 읽지 못하면 같은 인증 세대의 마지막 정상
관측(사용률·관측 시각·주간 리셋 시각)을 사용한다. 저장된 주간 리셋 시각이 지났으면 배정 계산에서
100% 남은 것으로 본다. 리셋을 모르면 원래 잔량을 유지한다. 주간 기록이 없으면 다른 해당 창의
최소 잔량을 비교한다. 같은 잔량은 진행 중 작업 수·최근 배정 시각·별칭 순으로 고른다.
잘못된 숫자·창 충돌·미래 관측은 비교에서 제외한다. 기록도 없으면 미관측 계정끼리 기존 작업 수와
차례로 배정하며, 미관측 실행을 첫 한 번으로 제한하지 않는다. 정상 기록이 있는 계정이 먼저다.
마지막 정상 관측은 빈 응답·부분 응답·조회 실패 뒤에도 DB에 보존하고 재로그인 때 초기화한다.
실제 금고 인증의 잠김·없음, 계정 메타에 기록한 갱신 거부·만료와 확실한 현재 한도(최신 창 100%,
또는 유효한 제공자 한도 재시도 시각)만 제외한다. 사용량 조회의 401·잠김 오류는 작업 인증의
끊김으로 보지 않는다. helper가 자체 갱신을 차단했거나 조회에 쓸 인증이 오래됐어도 마지막 정상
관측이나 미관측 규칙으로 배정한다. 계정 목록과 자동 배정은 같은 인증 건강 판정을 쓴다.
예상 작업 크기와 고정 안전 여유는 미래 소비를 정확히 알 수 없어 배정 차단에서 뺐다.

자동화에 지정한 잔량 문턱은 들어온 최신 공용 창에만 적용한다. 없는 창은 통과하고, 알려진 창이
문턱 아래이면 그 계정을 제외한다. 사용할 계정 중 하나라도 통과하거나 판단 불가면 실행한다.
선택 시에도 같은 문턱을 검사한다. 이미 열린 thread에 보내는 자동화는 그 thread의 계정만 판정한다.
같은 자동화·입력·이유의 사용량 대기는 inbox를 처리하거나 host를 재시작해도 한 번만 알린다.
입력이나 이유가 바뀌면 새로 알리고, 차단이 풀릴 때도 한 번 안내한다.

선택·thread ID·시작 영수증은 한 SQLite transaction으로 확정한다. 같은 ID의 재전송은 같은 thread를
반환하며 내용을 바꾸면 conflict다. 모든 사용 계정이 한도·인증 문제로 제외되면 입력과 대기를 보존한다.
웹은 할 일 화면의 「계정 배정 대기」와 할 일 시작 창에서 대기 이유·취소를 보인다(대기 중일 때만).
설정 「계정」에는 대기·끝난 시작 기록을 두지 않는다. 대기 뒤 host pass에서 시작하지 못하면 inbox
`task-start`·`account start failed`로 한 번 알린다(thread가 생긴 실패는 thread 사건이 알린다).
host 재시작 뒤에도 대기와 영수증은 남는다.
실제 실행 실패를 자동으로 반복하지 않으며, 인증 세대가 바뀌면 기존 자료로 기동하지 않는다.

이전 설정은 `account-pool-v1` 원문을 남기고 별도 v2에 한 번만 이전한다. 옛 풀 사용 계정은 유지하고,
옛 풀에 넣지 않은 계정은 그대로 꺼 둔다. 기존 설정 자체가 없으면 등록 계정을 사용으로 시작한다.
그룹은 계정별로 풀고 모델·참여 프로젝트 조건은 없앤다. 진행 중 thread의 계정·원장·관측은 고치지 않는다.
다시 로그인은 사람이 꺼 둔 설정을 되살리지 않는다. 계정 이름 변경은 v2 사용 설정도 함께 옮긴다.
구판 `--group`·`--model`은 받아도 무시하고, 프로젝트 참여 변경은 없어진 기능이라는 오류를 돌려준다.
`--account-size`·선택의 그룹/예상량 필드는 옛 영수증 호환용으로만 남긴다.

변경은 세션 밖 사람의 CLI·웹 조작 기기만 한다. Reader와 세션 안 agent는 읽기만 된다.
설정 revision 충돌은 다시 읽어야 하며 인증 원문은 풀 API에 포함하지 않는다. FRONT_API 1.129부터
새 계정 화면을 표시한다. 운영 계정·실제 구독·외부 호출 대신 격리 금고·가짜 harness/상태줄/RPC로 검사한다.

## 계정 thread의 폴더 신뢰

계정 Codex는 새 `codex-home/config.toml`을 만들므로 로컬 설정의 신뢰 기록을 물려받지 않는다.
Claude 계정은 원래 토큰만 주입했고, 인증 환경 정리에서 기존 `CLAUDE_CONFIG_DIR`도 제거해
기본 로컬 홈으로 돌아갔다. 이제 새 Claude 계정 thread는
`account-runs/<project>/<thread>/claude-home`을 `CLAUDE_CONFIG_DIR`로 사용한다.
coordinator와 로컬 로그인 thread의 설정 홈은 그대로다. 도입 전에 로컬 홈에서 시작한 Claude 대화의
재개도 원래 홈을 유지한다. 새 격리 홈의 대화·계획 조회와 재개는 같은 홈을 사용한다.
로컬 로그인·인증 파일·키체인을 읽거나 설정 홈 전체를 복사하지 않는다.

host의 유효한 safety `trust_screens`가 `coordinator`이면 thread의 실제 실행 폴더 한 곳만 미리 기록한다.
`user`이면 미리 신뢰하지 않는다. 재시작 때도 현재 정책으로 신뢰 목록을 새로 만들므로
`coordinator`에서 `user`로 바꾸면 이전 자동 기록이 남지 않는다. 격리 Claude 홈의 나머지 환경설정은 보존한다.
Codex는 `config.toml`의 `projects.<절대 경로>.trust_level = "trusted"`, Claude는
격리 홈 `.claude.json`의 `projects[절대 경로].hasTrustDialogAccepted = true`를 사용한다.
경로는 정규화한 cwd 하나이며 부모 폴더·본 저장소·다른 worktree를 추가하지 않는다.
Windows Codex에는 공식 CLI와 같은 `dunce::simplified`로 DOS 표기를 쓴다. Rust의
`canonicalize`가 붙이는 `\\?\` 접두를 그대로 신뢰 키로 쓰면 TUI가 cwd를 찾지 못해 본 저장소의
신뢰 창을 띄운다. 긴 경로·예약 이름처럼 DOS로 줄일 수 없는 경로는 dunce가 그대로 보존한다.
Codex는 cwd 기록을 저장소 루트보다 먼저 조회하므로 worktree에서도 본 저장소 신뢰를 넓힐 필요가 없다.
session은 허용 경로와 실제 cwd가 다르거나 thread가 아닌 실행의 사전 신뢰 요청을 거부한다.

저장 형식은 [Codex 공식 설정 문서](https://developers.openai.com/codex/config-reference/),
cwd 우선 조회는 [Codex 0.163.0 소스](https://github.com/openai/codex/blob/rust-v0.163.0/codex-rs/config/src/config_toml.rs),
Claude의 설정 홈·신뢰 기록은 [Claude 공식 설정 문서](https://code.claude.com/docs/en/settings)를 근거로 한다.
Claude의 cwd·조상 조회와 격리 홈 `.claude.json` 경로는 설치된 공식 CLI 코드에서도 확인했다.
시험은 실제 CLI 로그인 대신 임시 홈의 가짜 harness로 시작과 조회 계약을 검사한다.

### 격리 Claude thread의 advisor

계정 Claude 실행은 머신 사용자 설정 `settings.json`의 문자열 `advisorModel`만
실행별 `quota-settings.json`에 담는다. 설정 홈은 실행 profile·머신 환경의
`CLAUDE_CONFIG_DIR`, agent 환경의 같은 변수, 기본 `~/.claude` 순으로 정한다.
명시적으로 빈 변수를 넘기면 기본 홈을 쓴다. 계정 격리가 홈을 바꾸기 전에 읽으며,
새 실행과 재개 때마다 다시 읽어 변경·삭제가 다음 실행에 반영된다.

권한·hooks·env·상태줄·다른 모델 설정은 가져오지 않는다. 기존 aaa hook과 사용량
상태줄을 유지하며 사용자 파일도 고치지 않는다. 파일이나 키가 없으면 기존처럼 실행한다.
읽기·JSON 오류나 문자열이 아닌 값은 원인과 함께 agent 경고로 남기고 기존 실행을 계속한다.
이미 실행 중인 Claude에는 소급 적용하지 않는다.

[Claude advisor 문서](https://code.claude.com/docs/en/advisor)는 `advisorModel`과
모델·계정별 적용 조건을 설명한다. 설정 전달이 성공해도 advisor의 실제 활성 여부는
Claude의 계정·모델 정책에 따른다. 검증 범위는 [별도 기록](testing-claude-advisor.md)에 남긴다.


## 계정 Claude thread의 첫 실행 안내

계정별 격리 홈은 사용자 홈의 첫 실행 기록도 물려받지 않는다. 인증을 전달받은 새 Claude 홈의
`.claude.json`에는 기존대로 `hasCompletedOnboarding=true`를 초기화한다. 이 키는 첫 실행 안내를
닫지만 우회 권한 경고를 수락하지는 않는다.

session은 safety와 profile을 합친 실제 실행 argv에 `--dangerously-skip-permissions`,
`--allow-dangerously-skip-permissions`, `--permission-mode bypassPermissions` 또는
`--permission-mode=bypassPermissions`가 있을 때만 격리 홈 `settings.json`의
`skipDangerousModePermissionPrompt=true`를 기록한다. `--allow-`도 Claude가 같은 경고를 여는
명시적 우회 허용이다. 권한 모드 자체와 다른 안내 수락은 바꾸지 않는다. 일반 실행은 이 파일을
새로 만들지 않으며, 우회 인자 없이 다시 시작하면 이전 자동 수락 키를 제거한다.
기존 격리 설정의 다른 키는 보존하고 symlink·깨진 JSON은 조용히 덮어쓰지 않는다.

[Claude 공식 설정 문서](https://code.claude.com/docs/en/settings)는 설정 홈과 파일별 범위를,
[권한 모드 문서](https://code.claude.com/docs/en/permission-modes)는 위 실행 옵션을 설명한다.
정확한 경고 판정과 수락 키는 설치된 공식 Claude 2.1.304의 시작·설정 코드에서 확인했다.
현재 키를 쓰며 이전 `.claude.json`의 `bypassPermissionsModeAccepted`는 새로 만들지 않는다.
원래 사용자 홈의 로그인·인증 파일·키체인·다른 설정은 읽거나 복사하지 않는다.

Codex 0.163.0의 [공식 시작 코드](https://github.com/openai/codex/blob/rust-v0.163.0/codex-rs/tui/src/lib.rs)와
[첫 실행 화면 코드](https://github.com/openai/codex/blob/rust-v0.163.0/codex-rs/tui/src/onboarding/onboarding_screen.rs)에서는
Claude의 우회 경고에 대응하는 별도 첫 실행 수락을 찾지 못했다. 로그인·폴더 신뢰와 Windows
샌드박스 안내는 별개이며 이번 변경에서 Codex 수락 키를 추가하지 않는다.


## Windows Codex의 신뢰·샌드박스 첫 실행(t-0953)

YOLO 인자는 이미 최초 실행과 재개에 모두 전달된다. Windows의 반복 안내는 인자 누락이 아니라
격리 설정의 확장 경로 키와 Codex TUI의 DOS 경로 조회가 어긋난 데서 시작한다.
Codex는 신뢰 결정을 그 자리에서 새로 받으면 Windows sandbox 안내를 이어 열며,
비관리자 sandbox를 고르면 `auto` 권한 preset을 적용한다. 그래서 처음 표시한 YOLO와 달리
도구 명령의 승인을 요구할 수 있다. 다음 시작에 aaa가 격리 설정을 다시 만들면 이 선택도 사라진다.

cwd 신뢰 키를 공식 구현과 맞추어 첫 안내에 진입하지 않게 한다. `windows.sandbox` 선택이나
`windows_wsl_setup_acknowledged`를 YOLO의 필수 설정으로 기록하지 않는다. 관리자 설치·UAC·
비관리자 sandbox 초기화 없이 기존 `--dangerously-bypass-approvals-and-sandbox`가 적용된다.
사용자의 개인 Codex 홈은 읽거나 바꾸지 않으며 `trust_screens=user`일 때 사전 신뢰도 넣지 않는다.

근거는 [OpenAI 권한 문서](https://learn.chatgpt.com/docs/agent-approvals-security),
[Windows 설정](https://learn.chatgpt.com/docs/config-file/config-reference),
Codex 0.164.0의 [신뢰 키 정규화](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/config/src/loader/mod.rs),
[첫 실행 sandbox preset](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/tui/src/chatwidget/windows_sandbox_prompts.rs),
[선택 뒤 권한 적용](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/tui/src/app/event_dispatch.rs)이다.
Codex와 [herdr의 CODEX_HOME 연동](https://github.com/herdrdev/herdr/blob/v0.9.5/src/integration/env.rs)은
Apache-2.0이며, herdr의 hook 설치는 격리 홈 선택만 참고하고 코드는 복사하지 않는다.
[dunce 1.0.5](https://docs.rs/dunce/1.0.5/dunce/)의 라이선스는
CC0-1.0 OR MIT-0 OR Apache-2.0이며 Windows 의존성으로만 사용한다.
