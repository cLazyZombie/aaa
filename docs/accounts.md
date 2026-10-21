# 작업별 계정 인증

profile은 실행할 harness·model·effort를 고르고, account는 인증 신원을 고른다.
host의 **aaa 세션 밖 터미널**이나 **웹 설정 「계정」(조작 기기)**에서 공식 harness 로그인을 시작하고 기본 계정을 정한다.
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
로그인 상태는 `waiting`, `succeeded`, `failed`, `cancelled`, 그리고 다시 로그인의 `mismatch`·`kept`(아래 「메일·인증일·상태」)이며
host를 다시 시작하면 진행 상태 ID는 유지하지 않는다.
`aaa account login-cancel <로그인-ID>`는 자식 종료·임시 자료 정리가 끝난 최종 상태를 반환한다. 같은 ID를 여러 번 취소해도 모두 완료 상태를 받으며 이미 성공/실패한 결과는 유지한다. 5분 상한·출력 128KiB 상한을 넘으면 실패한다.
`waiting`은 로그인 미완료 상태이며 브라우저가 열렸다는 확인은 아니다. 원래 실행한 host의 로그인 사용자 브라우저에서 인증한다. 로그인별 자식 PID·종료 코드·최종 상태·임시 자료 정리 여부는 host 로그로 확인할 수 있지만 원본 출력·인증 URL·토큰은 기록하지 않는다.
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

## 계정별 남은 사용량(A2)

웹 설정 「계정」에서 등록한 계정마다 창별 **남은 %**, 리셋·관측 시각, 머신과 수집 출처를 본다.
없는 창은 「사용량 모름」이다. 기간이 정확히 300분·10080분일 때만 5시간·주간이라고 부른다.

- Codex는 host가 격리된 임시 홈에 해당 등록 계정의 인증만 전달하고 공식 app-server의
  `initialize` → `initialized` → `account/rateLimits/read`를 호출한다. `thread/start`·`turn/start`는
  보내지 않는다. 기존 중앙 갱신과 경합하지 않도록 helper의 갱신은 자기 loopback 거부 서버로만
  보낸다. 인증을 새로 만들거나 갱신하지 않으며 실패는 카드에 남긴다. 인증 전달 전에 기존 계정
  실행과 같은 `codex-cli 0.163.0`을 확인한다. 다른 판은 「조회 지원 안 됨」이다.
  [공식 RPC 문서](https://learn.chatgpt.com/docs/app-server#6-rate-limits-chatgpt)를 따른다.
- Claude는 새 계정 pane에 주입한 상태줄 명령이 stdin의 `rate_limits.five_hour`·`seven_day`를
  그 pane의 전용 파일에 기록한다. 원격 agent도 비밀 없는 snapshot만 host에 보낸다. 이전 pane·
  옛 agent·한 번도 관측하지 않은 계정은 모름이다. 조회를 위해 Claude 작업을 만들지 않는다.
  실험 `get_usage`는 쓰지 않는다. 기존 로컬 사용자 설정 파일은 고치지 않는다.
- 수집 대상은 host 등록 목록과 계정 pane의 결속이다. 이 경로는 기본 인증 홈·형제 폴더·키체인을
  탐색하지 않으며 인증 원문·메일·대화 본문을 사용량 응답에 싣지 않는다. 기존 「구독 사용량」의
  로컬 로그인 호환 조회는 별도 기능으로 유지한다.

host는 약 1분마다 갱신하고 수동 「사용량 다시 읽기」는 30초 하한을 지킨다. 429의 재조회 가능
시각이 있으면 그 전에는 묻지 않는다(없으면 60초). 실패에는 마지막 숫자를 흐리게 보인다.
2분까지 최신, 2~10분 오래됨, 10분 초과·리셋 경계 뒤·큰 머신 시각 차이는 모름이다.
리셋 시각을 지났다는 이유로 100% 남음이라고 만들지 않는다. 상태줄 입력에서 빠진 창은 이전 값과
그 창의 관측 시각을 유지한다. 파일 수정·host 수신 시각으로 관측 시각을 바꾸지 않는다.
같은 계정·창의 최신 관측을 택하고 같은 시각의 다른 숫자는 작은 잔량과 충돌 표시를 쓴다.

등록·다시 로그인마다 토큰과 무관한 새 세대를 만든다. 삭제·같은 별칭 재등록도 새 세대다.
Codex 중앙 갱신은 세대를 유지한다. 이전 인증으로 실행 중인 pane과 늦게 끝난 조회는 새 세대의
숫자가 될 수 없다. 옛 금고 파일은 계속 읽으며 옛 pane의 귀속을 추정하지 않는다.
웹은 `GET /api/v1/account-usage[?refresh=1]`(FRONT_API 1.83)을 사용한다.
원격 snapshot은 `account_quota.v1`을 알린 agent에만 요청한다. 자동 배정·계정 전환(A3)은
하지 않는다. 실계정 로그인·운영 quota 조회는 가짜 시험의 보증 범위 밖이다.

## host 공통 허용 풀 자동 배정(A3)

사람이 host 공통 풀에 넣은 계정만 자동 배정한다. 새 로그인은 풀에 자동으로 넣지 않는다.
프로젝트 참여도 따로 켠다. 현재 aaa host는 프로젝트 하나를 맡으며, 풀은 그 host 상태 DB가 관리한다.
웹 설정 「계정」의 「자동 배정」과 host의 aaa 세션 밖 CLI에서 같은 설정을 읽고 고친다.
보기 기기·세션 안 agent는 설정을 읽을 수 있고, 변경은 세션 밖 사람·웹 조작 기기만 가능하다.
설정판을 대조하므로 다른 기기가 먼저 저장했으면 다시 읽어야 한다.
프로젝트 이름 변경은 참여·대기를 함께 옮기며, 삭제 뒤 같은 이름으로 새로 만든 프로젝트는 참여를 물려받지 않는다.

```sh
aaa account pool add work-claude --group work
aaa account pool add spare-claude --group spare
# 실제 한도를 공유하는 별칭은 같은 그룹을 지정한다. 다른 harness는 묶지 않는다.
aaa account pool list
aaa account pool project demo --on
aaa thread start demo --profile claude --account-pool --account-size medium \
  --start-id request-001 --title '독립 작업' --task-file task.md
aaa account starts demo
aaa account cancel-start request-001 demo
aaa account pool project demo --off
aaa account pool remove spare-claude
```

참여를 켜면 계정을 생략한 새 thread에 공통 풀을 적용한다. CLI·웹 할 일·예약·자동화도 host의
공통 시작 경로에서 선택한다. 명시 `--account <별칭>`은 계속 우선하고, 참여를 끈 프로젝트의 기본
계정 동작과 새 coordinator의 기본 인증도 유지한다. 같은 대화의 restart·resume은 처음 계정을
유지하며, 자동으로 다른 계정·모델·머신·API 과금 방식으로 바꾸지 않는다.
`--account-pool`은 별칭 `auto`와 충돌하지 않는 별도 요청이다. 참여를 끈 프로젝트에서는 거부한다.

자동 선택은 profile의 harness·모델, 등록 인증 상태·세대와 대상 agent의 인증 전달 지원을 검사한다.
모델 목록이 비면 해당 harness의 모든 모델을 허용한다. 관측한 필수 5시간·주간 창과 해당 모델의
창이 모두 2분 이내이고 리셋·기간을 알 때만 잔량 후보로 쓴다. 창별 계산은
`100 - 사용률 - 미소진 예약 - 안전 여유 10 - 이번 예상량`이며 모두 0 이상이어야 한다.
예상량은 small/medium/large 5/15/30%p, 기본 medium이다. 실제 미래 소비량을 보장하는 숫자는 아니다.
최소 여유가 가장 큰 후보에서 5%p 이내인 후보끼리는 빠른 리셋·적은 예약·오래된 마지막 배정·별칭
차례로 고른다. 공유 그룹은 별칭들의 관측 제약과 예약을 함께 계산한다.

사용자 선택에 따라 최신 잔량 후보가 없으면 **미관측 계정의 자동 첫 실행**을 허용한다.
아직 상태줄 관측이 없는 새 Claude 계정도 이 경로로 시작할 수 있다. 잔량 100%를 합성하지 않고
선택 이유에 「사용량 미관측 · 자동 첫 실행」을 표시한다. 같은 로그인 세대에서 이미 첫 실행을
보냈으면 실제 관측 전에는 두 번째 미관측 실행을 만들지 않는다. 오래된 관측·조회 실패·잘못된
숫자·리셋 뒤 미확인과 공유 그룹의 알려진 제한은 이 경로로 우회하지 않는다.

선택·예상량 예약·thread ID·시작 영수증은 하나의 SQLite transaction에서 확정한다. 같은 계정의
동시 작업은 허용하며, 시작 두 건이 같은 잔량을 중복으로 예약하지 않게 한다. 실행 중 예약은
새 사용량이 왔다는 이유만으로 지우지 않아 보수적으로 이중 계산할 수 있다. 종료를 확인한 뒤
그 이후의 신선한 관측으로 교체한다. spawn 전의 확정 실패는 해제하고, 응답 유실·연결 단절·
실행 결과 불명은 해제하지 않는다. 기존 계정 작업도 풀 도입 뒤 예상량 예약에 포함한다.
인증 세대가 기동 전에 바뀌면 새 신원으로 조용히 실행하지 않는다.

후보가 없으면 직접 시작 요청은 thread를 만들지 않고 내구 배정 대기 하나를 반환한다. 웹 설정
「계정」에서 이유·취소를 보고, CLI는 `account starts`로 본다. `--start-id`의 재전송은 같은 작업을
가리키며 내용을 바꾸면 conflict다. 웹 할 일 시작도 같은 ID를 유지한다. host 재시작 뒤에도 대기와
영수증·기존 관측/실패/backoff가 남는다. 새 관측·상태 변화 뒤 ticker가 다시 판정한다. 실제 시작
실패는 실패로 남기며 자동으로 반복하지 않는다. 대기 중 task 내용·모델·effort가 바뀌면 새 요청이
필요하다. task 예약과 자동화의 대기는 기존 내구 큐가 같은 `start_wait` 원인을 유지한다.
한도에 도달한 실행 중 작업은 같은 계정에서 기다리고 사용자가 명시적으로 재개한다.

API 연결·계정 간 대화 인계·인증 회전 crash 복구 보강은 이 단계에 포함하지 않는다. 검증은
가짜 계정·상태줄·공식 RPC만 사용하며 실계정 동시 실행이나 운영 한도를 확인한 결과는 아니다.

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
Codex는 cwd 기록을 저장소 루트보다 먼저 조회하므로 worktree에서도 본 저장소 신뢰를 넓힐 필요가 없다.
session은 허용 경로와 실제 cwd가 다르거나 thread가 아닌 실행의 사전 신뢰 요청을 거부한다.

저장 형식은 [Codex 공식 설정 문서](https://developers.openai.com/codex/config-reference/),
cwd 우선 조회는 [Codex 0.163.0 소스](https://github.com/openai/codex/blob/rust-v0.163.0/codex-rs/config/src/config_toml.rs),
Claude의 설정 홈·신뢰 기록은 [Claude 공식 설정 문서](https://code.claude.com/docs/en/settings)를 근거로 한다.
Claude의 cwd·조상 조회와 격리 홈 `.claude.json` 경로는 설치된 공식 CLI 코드에서도 확인했다.
시험은 실제 CLI 로그인 대신 임시 홈의 가짜 harness로 시작과 조회 계약을 검사한다.


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
