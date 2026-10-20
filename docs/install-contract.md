# aaa install 공통 계약 v1

I0~I2와 I5(t-0485)의 정본이다. `aaa.install.v1`은 CLI 설치 계약이며 FRONT_API/AGENT_LINK와 독립이다.
I1은 **공유 검증·읽기 탐지·계획 hash·dry-run CLI**, I2는 private 파일 적용·재개다.
순서는 Windows(I5) → 대화형/승인 executor(I6) → Windows 최소 bootstrap/사용자 인수 → macOS(I3) → Linux(I4)다.
아래 전체 계약 중 현재 실행 가능한 범위와 미지원 결과는 끝의 단계별 절에 구분한다.

## 두 기획의 차이와 채택 기준

t-0472 확정판 §4~5·9~11과 사용자 D1~D8을 따른다. t-0471 §5.2의 인자 이름은 초안의 예시였으므로 다음처럼 맞춘다.
원본 기획서는 수정하지 않았다. 웹 머신 추가 구현은 이 변경에 포함하지 않는다.

| t-0471 초안 | 확정 공통 계약 |
|---|---|
| `--check` 사전 검사 | I1의 `--dry-run`: 읽기 탐지와 전체 계획. 별도 check 명령은 예약하지 않음 |
| `--json` JSON Lines, `v:1` | `--json` 단일 결과, `--json-stream` NDJSON, schema 문자열로 판 구분 |
| `needs_human`, 초안 exit | `needs_user`, 아래 exit 표 |
| enroll 코드·지문 stdout 사건 | 일반 JSON/로그에 출력하지 않음. private 승인 입력과 보호 표시 사용 |
| `--host-fingerprint` argv | 기존 pin 또는 private 승인 입력의 host identity. 첫 응답을 신뢰 root로 쓰지 않음 |
| `--hooks claude,codex` | 반복 `--harness claude\|codex`와 `--hooks install\|preserve\|skip` |
| Windows 로그온 예약 작업 | 설치 요청 사용자 계정 SCM. LocalSystem/가상 계정 자동 대체 없음 |
| Linux user unit 기본 | system unit + 요청자 `User=` 기본. 명시 선택으로 user unit + linger |
| `--yes`로 원격 적용 | 계획 확인 생략만. W0 유지, 유효한 범위 제한 승인 executor가 별도로 필요 |

참고 넷의 사본을 읽었다: herdr `distribution/install.sh`, herdr-projects `src/setup.rs`,
herdr-web-ui `server/tailscale.ts`, t3code `scripts/install.sh`·`docs/user/background-service.md`.
OS/arch·checksum·사용자 bin·소유 journal·동시 편집 보존·기존 공개 보존·데이터 보존 제거를 따른다.
참고와 다른 부분은 확정 결정을 따른 Windows SCM/Linux system unit, 자동 재시도·외부 설치기·PATH 자동 덮어쓰기 제외다.

## 입력과 검증 소유자

- `aaa-proto::install`: `InstallChoices`, `InstallInvocation`, 결과/사건/승인서/등록 허가의 Rust 정본.
- `aaa-config::install::plan`: 질문과 CLI가 공통으로 쓰는 순수 선택 검증·기본값 정규화.
- I1 adapter: 현지 탐지·실효 설정/경로·OS 계정·mount·충돌·profile 파일 검증.
- 웹은 proto 타입만 사용한다. host가 agent/session 구현을 직접 호출하지 않는다.

입력 우선순위는 명시 인자 → 재개 초안 → 기존 실효값 → 탐지 후보 → 공개한 기본값이다.
새 자동 설치의 필수 선택은 숨은 값으로 보충하지 않는다. I0 planner에는 이미 선택한 값과 읽기 탐지 context를 준다.
repair/resume의 기존 계획 로딩과 실물 비교는 I2에서 한다. 문법/누락 오류는 변경 전에 끝낸다.
I0의 함수는 파일·env·OS·프로세스·네트워크를 읽거나 쓰지 않는다.

전역 `--instance`, `--config-dir`, `--state-dir`도 choices에 포함한다. CLI 인자 이름은 필드의 `_`를 `-`로 바꾼다.
`InstallInvocation.choices` 중첩은 내부 DTO이며 최상위 CLI에 `--choices` 인자를 만들지 않는다.
비대화/질문 입력에 다른 검증기를 만들지 않는다.

## 인자

아래 선택 인자의 전체 타입/enum은 [생성 schema](install.schema.json)의 `InstallChoices`와
[생성 TS](../web/src/gen/proto.ts)에 있다. 현재 설치 명령을 문서화한 README와 혼동하지 않는다.

| 영역 | 인자 | 규칙/기본값 |
|---|---|---|
| 역할 | `--role host\|agent`, `--name` | 새 자동 설치에서 둘 다 필수. hostname 자동 보충 없음 |
| 인스턴스 | `--instance`, `--config-dir`, `--state-dir` | 기존 instance 이름 규칙. default 예약, config/state 충돌 금지. 실물 격리 검사는 I1 |
| 설치 | `--mode fresh\|repair\|reinstall` | fresh. 같은 실효 계획은 unchanged. 실물 대조는 I1/I2 |
| binary | `--bin-path`, `--version` 또는 `--from`+`--sha256` | 현재 binary 기본. exact 판만, latest/stable/nightly 금지. from의 파일 SHA 대조는 I1 |
| 예상 대상 | `--expected-os macos\|linux\|windows`, `--expected-arch aarch64\|x86_64` | 현지 context와 일치해야 함. Windows aarch64 release 지원 주장 없음 |
| 로컬 agent | `--local-agent enabled\|disabled` | host 기본 enabled, agent에는 금지 |
| 프로젝트 | `--project-mode create\|use\|skip`, `--project`, `--projects-root`, `--goal`, 반복 `--repo`, `--no-repo` | host project-mode 필수. create/use는 이름과 repo/no-repo 선택 필수. agent는 로컬 repo만, host 프로젝트 변경 금지 |
| 작업 디스크 | `--work-root`, `--work-mount` 또는 `--work-storage internal` | agent/로컬 agent host 필수. mount와 internal 상충. 실물 mount/volume identity는 I1 이후 |
| 작업 기본 | `--min-free-gb`, `--default-cwd`, `--shell` | 신규 20 GiB, work-root, 탐지한 OS shell. 기존값 보존은 I1 |
| harness | 반복 `--harness claude\|codex` 또는 `--harness none` | agent/로컬 agent host 명시 필수. none과 다른 harness 상충 |
| 경로 | `--claude-path`, `--codex-path`, `--claude-home`, `--codex-home` | 선택한 harness만. 절대 경로. 미존재/login 상태는 I1 이후 |
| 환경 | 반복 `--harness-env harness:key=value` | typed harness/key/value로 변환. v1 허용 key는 LANG/LC_ALL/LC_CTYPE/TZ/TERM. 같은 key 중복 거부, 비밀 전달에 사용 금지 |
| 로그인/hook | `--login require\|defer`, `--hooks install\|preserve\|skip` | require. Codex면 hooks 명시 필수, 그 밖 preserve. Claude 전역 hook 설치 없음 |
| profile | 반복 `--profile-file`, `--thread-profile`, `--coordinator-profile`, 반복 `--allow-thread-profile`, `--allow-coordinator-profile` | host만. profile 파일 정본·비밀 env·자유 shell 검사는 I1, safety 확대는 별도 승인 |
| 감독자 | `--service auto\|launchd\|systemd-system\|systemd-user\|windows-scm\|none` | 자동화 명시 필수. auto는 Mac LaunchAgent/Linux system unit/Windows SCM. 다른 OS 감독자 거부 |
| 계정 | `--service-account current-user` | 요청자 UID/SID 고정, 승격 helper 계정으로 변경 금지. none에는 지정 금지 |
| Windows 암호 | `--service-password-source prompt\|stdin\|broker`, `--secrets-stdin`, `--service-password-ref` | 비TTY 신규 apply는 stdin/broker private 일회 입력 필수. dry-run에는 비밀 불필요. 암호 값 argv/env/journal 금지 |
| 시작 | `--startup auto\|manual`, `--start now\|deferred` | auto/now. Mac auto는 로그인 시 RunAtLoad. deferred 또는 service none은 configured 범위 |
| linger | `--linger require\|preserve` | 명시 systemd-user에만 사용, require 기본 |
| PATH/launcher | `--path preserve\|user`, `--launcher`, `--launcher-policy install\|preserve` | PATH preserve. launcher는 Mac만, 기존 asset preserve. 새 asset 검증/설치는 I3 |
| agent 네트워크 | `--host name:port`, `--network tailscale\|direct`, `--enroll wait\|grant\|skip`, `--wait-timeout` | host/network/enroll 필수. wait 상한 기본 600초, 양수. skip는 configured |
| host listen | `--agent-listen`, `--web-listen` | agent-listen 필수, port 0 금지. web loopback 7431 기본, 둘째 instance는 명시 port. 두 port 분리 |
| host 웹 | `--web local\|off`, `--tailscale-serve on\|off`, `--serve-port`, `--pairing controller\|skip` | web/Serve/pairing 명시 필수. on이면 1..65535 외부 port 필수, off면 port 금지. web off와 Serve on/controller 상충 |
| coordinator | `--open-coordinator yes\|no` | host만, no. 자동 유료 harness 대화 없음 |
| 모드 | `--non-interactive`, `--yes`, `--dry-run`, `--json`, `--json-stream`, `--prompt auto\|plain` | JSON 두 모드 상충, JSON은 질문 금지. 비TTY 무인자 exit 2. yes는 필수값/권한/승인 대체 아님 |
| 기록/승인 | `--events-file`, `--authorization-stdin` 또는 `--authorization-file` | 승인 입력 둘 중 하나, EOF 뒤 적용. 비밀 stdin의 질문 응답 혼합 금지 |
| 재개/조회 | `--resume ID`, `--status ID` | status 읽기만. ID와 소유자·instance·판·hash·실물 대조. 변경은 새 계획/승인 |

역할에 맞지 않는 선택을 버리지 않는다. 공백·한글·괄호·`&`·`%`·`!`·따옴표·역슬래시·drive/UNC는 argv 데이터다.
줄바꿈/NUL/제어 문자·`$(`·backtick은 인자에서 거부한다. 경로는 대상 OS 절대 경로를 요구한다.
SSH의 POSIX/PowerShell/cmd 인용은 t-0471 전송 adapter의 소유다. 공백 join으로 명령을 만들지 않는다.

## 계획 hash

`InstallPlan`은 private 데이터다. context에는 현지 OS/arch·정확한 binary 판/SHA·실효 설정 hash·요청 계정과
탐지한 경로/shell이 있다. choices에는 정규화된 모든 실효 선택을 둔다. path/symlink/case/`..`를 실물 없이 합치지 않는다.
동일 실효값 표현은 기본값 보충, SHA 소문자화, 집합인 harness/repo/허용 profile 정렬·중복 제거,
expected OS/arch 검증 후 제거, 현재 판과 같은 version 생략으로 정규화한다. profile-file 배열은 병합 순서를 보존한다.

canonical bytes는 객체 key를 재귀 사전순으로 정렬한 compact UTF-8 JSON이다. 배열 순서는 보존하고 BOM/마지막 LF는 없다.
SHA-256 소문자 hex가 `plan_sha256`이다. ID·시각·진행·출력 방식·yes·dry-run은 계획 밖이다.
[고정 입력](install/agent-input.json)과 [기대 hash](install/agent-plan.sha256)는 독립 Python JSON/SHA 계산으로 대조한다.
경로·role·공개 범위·계정·binary·기존 설정 내용 변경은 hash를 바꾸고 재승인을 요구한다.
계획 hash는 **승인이나 비밀 제거 기능이 아니다**.

## 결과와 스트림

`InstallResult`는 t-0472 §4.4 envelope를 고정한다. ID/hash/role/platform/version은 계획 전 오류에서 null일 수 있다.
changes.kind는 확장 가능한 항목 이름(agent_config/service 등), checks.name도 항목 이름이다.
공유 결과는 경로·SSH 계정/주소·host 지문·token·SAS·pairing·암호·private 설정을 포함하지 않는다.
하위 오류 원문을 넣지 않고 고정 code와 한국어 안내를 반환한다. 로컬 private 위치 안내는 이후 별도 화면으로 제공한다.

| status | exit |
|---|---|
| planned/completed/unchanged/configured | 0 (checks로 실제 완료 범위 구분) |
| needs_user | 3, needs_user 항목 필요 |
| failed | 1 적용/검증, 2 인자/계약, 4 lock/충돌, 5 지원 범위, 6 명시 대기 상한. error 필요 |
| cancelled | 130 |

`--json`은 stdout에 envelope 하나와 LF만 쓴다. `--json-stream`은 schema `aaa.install.event.v1`의
`type:event`와 마지막 `type:result`를 NDJSON으로 매 줄 flush한다. 최종 result는 envelope 전체를 담는다.
seq는 1부터 단조 증가, 최종 결과는 정확히 한 번, OS exit와 result.exit_code 일치가 필수다.
stdout은 UTF-8/BOM·ANSI 없음, stderr/사건 파일도 정제한다. 소비자는 CRLF/청크 분할/분할 UTF-8를 처리한다.
미지원 schema·잘린 줄·result 없는 EOF는 실패/결과 불명이며 같은 ID status로 확인한다.
I0는 typed envelope만 제공하며 스트림 I/O/수명/상태 조회는 I1/I2/I6에서 구현한다.

## 승인서와 등록 허가

schema는 **데이터 형식만** 고정한다. deserialize 성공·`approved:true`·`--yes`로 신뢰를 만들지 않는다.
I0에는 서명 발급/검증·pin 저장·grant 소비·worker가 없으며 기존 W0/사람 전용 명령을 열지 않는다.

- `aaa.install.authorization.v1`: 설치 ID, 계획 hash, 대상 OS/arch/계정/instance, binary SHA,
  허용 operation/정확한 target와 전후 hash, 발급·만료, nonce, issuer를 claims에 묶는다.
- `InstallAuthorization`: claims + signature + 선택적 enrollment grant. private 입력에만 사용한다.
- `aaa.install.enroll-grant.v1`: 설치 ID/계획 hash/agent identity/host identity/issuer/만료/nonce/signature.
- signature는 I6의 고정 Ed25519 검증으로 처리한다. 승인서 서명 대상은 canonical claims,
  grant 서명 대상은 signature를 뺀 나머지 필드의 canonical bytes다. signature는 base64url(no padding).
  issuer는 검증 root를 참조하는 식별자이며 envelope가 신뢰 root를 공급하지 않는다.
- 신뢰는 검증된 SSH bootstrap의 pin 또는 외부 TTY가 만든 private 로컬 발급 기록과 대조한다.
  대상·소유권/ACL·만료·허용 범위·nonce 일회 소비·재개 정책을 모두 검증해야 한다.
- 허용 operation은 파일·binary/launcher staging·hook/root·서비스·공개·등록·프로젝트/PATH의 typed 목록뿐이다.
  임의 shell·safety 확대·기기 추가·다른 머신 승인 권한은 없다. application 승인과 OS UAC/sudo는 별개다.
- grant는 host가 TLS에서 증명한 agent 신원과 대조해 한 번만 소비한다. 응답 유실은 ID+신원으로 조회한다.
  `install_enroll.v1` 기능 협상·AGENT_LINK 증가는 I6의 당시 main에서 한다.
- Windows 암호는 보호 stdin/broker에서 메모리로 일회 전달한다. SCM/LSA 밖에 보관하지 않는다.
  승인서·grant·암호 내용은 argv/env/공개 JSON/로그/일반 journal에 넣지 않는다.

[fixture](install/fixtures.json)의 signature/신원/nonce는 의도적으로 유효하지 않은 예시다.
`InstallContract`의 kind/value wrapper는 schema/fixture 도구용이며 실제 CLI stdout에 붙이지 않는다.

## 생성·검증

```sh
AAA_UPDATE_INSTALL=1 cargo test -p aaa-proto install_schema
AAA_UPDATE_TS=1 cargo test -p aaa-proto typescript
cargo test -p aaa-config install
cargo test -p aaa-proto install
cargo test -p aaa --test deps
bash scripts/ci-local.sh
bash scripts/check-public.sh
```

Rust 타입 → JSON schema/TS를 생성하고 drift를 시험한다. role·조건부 필수/상충·hash의 의미 판정은 공통 planner가 소유한다.
JSON Schema는 shape, enum, 알려진 key를 검사하는 도구이며 실제 권한/서명/OS 지원을 증명하지 않는다.
새 계약/보안 시험은 해당 owner에 결함을 심어 RED, 원본 복원 뒤 GREEN을 남긴다.
I3 진입 전 실제 LaunchAgent 시험에 필요한 깨끗한 계정/GUI/권한을 보고하고 운영 설정에는 손대지 않는다.

## I1에서 실행 가능한 범위

자동화/세션 안에서는 `--dry-run --json` 또는 `--dry-run --json-stream`으로 읽기 계획을 얻는다.
`--json`은 결과 하나, stream은 현재 계획의 최종 result 한 줄(seq=1)을 flush한다. 적용 사건·설치 ID를 만들지 않는다.

```sh
aaa install --role agent --name mac-a --host example.ts.net:7430 \
  --network direct --enroll skip --work-root /work/aaa --work-storage internal \
  --harness none --service none --start deferred --dry-run --json
```

- 누락/상충/문법/잘못된 설정은 exit 2, 현지 OS/arch·binary 형식 불일치/미지원 기능은 5,
  기존 설치 fresh·다른 내용의 target binary·Serve port 충돌은 4다. 없는 harness는 needs_user/3이다.
  하위 clap·TOML·OS·Git 원문은 stdout/stderr/JSON에 복사하지 않는다. 전역 instance 오류도 같은 envelope다.
- 현재 실행 binary를 SHA-256하고 요청 UID/Windows SID와 정규화 경로를 묶는다. from은 SHA/형식만 읽고 실행하지 않는다.
  from의 판은 unknown이며 result.version=null이다. 현재 판 외 release 조회/받기는 지원하지 않아 exit 5다.
- 기본/local TOML은 기존 loader로 읽고 실효값과 관련 파일 내용 hash를 private context에 넣는다.
  기존 min-free-gb는 명시 override가 없으면 보존한다. 내용 변경도 계획을 무효화하므로 주석 변경에 hash가 달라질 수 있다.
  binary 공유 위치의 같은 내용은 보존하고 다른 내용은 소유 증명/갱신 절차 전까지 충돌로 둔다.
- 존재하는 조상부터 실제 경로·mount를 확인한다. 빠진 mount 자리의 일반 폴더와 볼륨 밖 symlink를 통과시키지 않는다.
  profile-file/bin-path는 파일로 해석하고 원래 경로와 symlink 해석 뒤 부모의 mount를 확인한다.
  config/state/harness home/work-root는 디렉터리이며 기존 일반 파일을 work-root로 지정하면 work_root_not_directory/2다.
  Mac /Volumes 선택에는 명시 mount가 필요하다. 없는 작업 root/config/state를 만들지 않으며 lock/probe/기록도 만들지 않는다.
  여유 공간·Unix access만 사전 관찰한다. Windows ACL 쓰기 가능성, 안정 volume UUID, root 소유권은 unknown이다.
  device/volume serial 관찰은 계획 hash에 묶지만 적용 직전 안정 신원 재검사를 대신하지 않는다.
- repo는 Git rev-parse를 고정 argv·fsmonitor 비활성·optional lock 없음·상한으로 읽는다. clone·checkout·trust 설정을 하지 않는다.
  Windows는 SCM CONNECT/QUERY_STATUS만으로 같은 instance의 서비스 유무를 읽는다. Linux는 실제 unit 경로와
  systemctl show LoadState만 읽으며 pager/암호 질문/서비스 변경은 하지 않는다. Mac은 기존 LaunchAgent plist를 읽는다.
  systemd unit/SCM 이름은 기본 aaa-host/aaa-agent, instance는 aaa-<instance>-host/agent다. Mac label은 기존 dev.aaa.<instance>.<role>이다.
  remote repo, 실제 포트 bind, 서비스 ready/연결/등록/pairing/승인, harness 판·로그인·hook 신뢰는 관찰 범위에 따라 unknown/not_run이다.
  harness는 실행 권한/경로/내용만 읽는다. --version/로그인 명령도 설정을 만들 수 있어 dry-run에서 실행하지 않는다.
- profile-file은 `schema = 1`과 `[profiles.<이름>]`만 있는 host TOML이다. 기존 profile 검증을 공유하고
  새 import는 claude/codex·model/effort/args와 LANG/LC_ALL/LC_CTYPE/TZ/TERM만 허용한다.
  secret env·shell/custom·command override는 거부한다. 실행 경로는 typed claude-path/codex-path로 고른다.
  기존 사용자의 다른 profile은 읽어 hash에 묶고 보존한다. 허용 범위 확대는 적용 승인 전까지 실행하지 않는다.
- I1의 planned는 읽기 계획 성공이다. configured/completed/unchanged, 실제 연결/서비스 정상 또는 세 OS 실행 검증 완료를 뜻하지 않는다.
  기존 완료 receipt/소유 증명과 unchanged는 I2가 실물을 대조한 뒤 제공한다. repair/reinstall도 I1에서는 계획뿐이다.
- apply·resume·status는 각각 install_apply_not_available/install_resume_not_available(exit 5)로 거부한다.
  마법사는 아직 없어 기본 비TTY 무인자는 non_interactive_required/2다. dry-run+events-file은 2로 거부한다.
  승인 파일/암호 stdin/handle을 소비하거나 OS 권한을 올리지 않는다. `--yes`로 이 경계를 열 수 없다.

## I2의 파일 엔진과 CLI 경계

`install --status <ID>`는 전역 `--state-dir`/`--instance`로 소유 job을 찾는 읽기 조회다.
ID·schema·계정/instance·private 파일 권한·manifest hash·실제 파일/진행 상태를 대조한다.
설정/사건 파일·lock/probe/journal을 만들거나 고치지 않는다. 모르는 ID는 installation_not_found/2,
잘못된 ID는 invalid_installation_id/2, 신뢰할 수 없는 권한/내용·실제 상태 충돌은 4다.

apply/resume은 자동화·SSH·외부 TTY 모두 `needs_user: installation_authorization_required`/3으로 닫혀 있다.
`--yes`·임의 승인 JSON/파일은 권한이 아니며 승인 입력을 실행하거나 신뢰하지 않는다.
내구 job을 만들기 전에는 설치 ID/resume을 꾸며내지 않는다. 서명·issuer pin·worker/executor·발급은 I6다.

private 파일 엔진의 configured/unchanged는 이 단계의 파일 결과다. 서비스 ready·연결·pairing 완료를 뜻하지 않는다.
repair는 빠진 소유 파일을 복원하고, reinstall은 소유 binary를 다시 배치하며 현재 사용자 TOML/데이터를 보존한다.
기본 제거는 마지막 참조의 소유 binary/hook만 정리하고 TOML·identity/pin·DB·repo/worktree·root·backup을 남긴다.
외부에서 편집한 hook은 conflict로 보존한다. 버전 갱신·purge·서비스/PATH/등록/신뢰 변경은 후속 경계다.
I3~I5 실험실 owner는 I6 전까지 엔진/고정 adapter를 직접 호출하며 CLI 승인 우회 기능을 넣지 않는다.

재개는 현재 active job의 같은 ID만 허용한다. rollback은 소유를 입증한 stage만 정리하며,
보존한 root나 입증하지 못한 stage가 있으면 `rollback.state=partial`과 고정 remaining code로 남긴다.


## I5 Windows 감독자와 읽기 상태

Windows 감독자 기반은 I2 파일 receipt·같은 계획/계정/ID를 쓰며, I6가 검증한 executor에서 연결할 경계다.
공개 CLI의 apply/resume은 위 needs_user/3 경계를 유지한다. service 등록·암호 입력·UAC를 실제 수행할 사용자 명령은 I6/Windows bootstrap에서 제공한다.

`install --status <ID>`는 소유 service record가 있으면 등록·RUNNING·실제 backend ready를 나눠 읽는다.
SCM 접근 승인이 없거나 실행/연결 proof를 확인하지 못하면 고정 needs_user code를 반환한다.
실제 SID·프로필·실행 SHA·Session0·PID/생성 시각·연결과 receipt를 대조하며 조회는 기록/lock을 만들지 않는다.
비밀번호를 SCM에 갱신해도 이전 process가 계속 실행 중이면 새 logon 검증으로 표시하지 않는다.

Windows·Linux 실제 실행은 main push 뒤 CI다. 실제 SCM logon·Session0·로그인 전 부팅·UAC/LSA·암호 변경·
ConPTY/인증은 사용자 Windows PC 인수로 확인한다. CI/cross만으로 실제 설치를 완료했다고 부르지 않는다.
