# 지금 운영에서 aaa로 옮기기: inventory·shadow·pilot·전환 gate·rollback(M4)

설계는 기획서 [§15](plan.md#15-현재-운영에서-이전)에 있다. 이 문서는 도구 쓰는 법, 전환 조건(gate), 전환 순서, rollback 절차와 그 rehearsal을 적는다.

- 머신 이름은 일반 이름이다. herdr-projects root와 aaa host가 있는 Mac을 **host Mac**(`mac-a`)이라 부르고, 다른 Mac은 `mac-b`·`mac-c`다.
- **✋ 표시는 사람이 직접 한다**(launchd·감독자·coordinator 이동). 실제 전환과 coordinator 이동은 사용자 승인 대상이다.
- 옮기는 쪽(herdr 0.9.5·herdr-projects 0.2.x) 자료는 처음부터 끝까지 **읽기만** 한다. 그래서 rollback에 「되돌릴 쓰기」가 없다.
- 도구 결과(inventory·shadow 원자료·변환 결과)에는 홈 경로·머신 이름·세션 ID가 들어간다. 저장소 밖에 둔다.

## 1. 도구

| 도구 | 하는 일 | 쓰는 곳 |
|---|---|---|
| `scripts/m4/inventory.sh` | host Mac에서 돈다. 머신마다 `herdr api snapshot`, 로컬 thread의 `herdr-projects profile resolve`를 모아 `aaa migrate inventory`에 넘긴다 | `--out` 폴더 |
| `aaa migrate inventory` | 0.2 프로젝트 폴더의 모든 파일(분류·크기·sha256), thread별 계획(끝난 일 = 기록으로 가져옴, 열린 일 = herdr가 끝까지 맡음), 열린 thread 재개 정보(세션 ID·profile 인자·재개 명령), inbox·routine·설정, herdr 세션 표 | `inventory.json`·`inventory.md` |
| `aaa migrate import` | inventory를 확인하며 aaa 상태 폴더에 프로젝트를 만든다. 끝난 thread 기록·보고·library·inbox·memory·TASKS·uploads·scratch를 검증한 사본으로 둔다 | aaa projects root, host DB |
| `scripts/m4/shadow-capture.sh` | 같은 시각에 지금 운영과 aaa host가 보는 것을 원자료로 모은다(읽기 명령만, pane에 입력하지 않는다) | tick 폴더 |
| `aaa migrate shadow` | tick들에 herdr-projects 규칙과 aaa 규칙을 같은 입력으로 돌려 차이를 표로 낸다 | `shadow.md`·`shadow.jsonl` |

### inventory가 읽지 않는 곳

내용을 읽지 않고 개수·크기만 「뺀 곳」으로 남긴다. import도 같은 규칙으로 복사하지 않는다.

- `.env`·`.env.*`(token), `.herdr-web-ui/`(기기·설정 기록), `.claude/`, `.git/`(프로젝트 폴더 자체 백업)
- 키·인증서 확장자(`.p12`·`.pfx`·`.key`·`.pem`·`.cer`·`.keychain*`). 시험용 공개 인증서라도 이름만 보고 뺀다.
- symlink는 따라가지 않는다(0.2.35 사본 규칙과 같다).

### import가 하는 일

| 0.2 | aaa | 방법 |
|---|---|---|
| PROJECT.md 본문 | PROJECT.md | 복사. 설정(front matter)은 `imported/PROJECT.herdr.toml`에 보존하고 aaa에 같은 설정이 없는 키는 결과에 까닭을 적는다 |
| AGENTS.md | `imported/AGENTS.herdr.md` | 보존만. coordinator 안내는 aaa 것을 새로 쓴다 |
| MEMORY.md·memory/·TASKS.md·later.md·uploads/·scratch/·tools/ | 같은 이름 | 복사 뒤 사본 sha256을 inventory와 대조 |
| routines/*.md | routines/*.md | `enabled = false`로 바꿔 넣는다(두 ticker가 같은 일을 두 번 하지 않게) |
| 끝난 thread 기록 | host DB `threads`(resolved) | 보고 hash·확인(ack) 상태·보고 태그·Next를 옮긴다. 확인 상태는 0.2 기록의 hash와 보고 사본이 같을 때만 옮긴다 |
| 끝난 thread 보고·task·Follow-ups·library | 같은 이름 | 복사·대조. 같은 APFS volume이면 clone이라 공간을 거의 쓰지 않는다(실측: 31.5 GB 복사에 사용량 변화 없음) |
| 열린 thread | 가져오지 않음 | 소유 도구는 하나다(§15 3). 재개 정보만 inventory에 |
| inbox(끝냄·열림) | host DB `inbox` + 같은 투영 파일 | id·시각 그대로. 열린 항목은 「봤음」으로 넣어 다시 알리지 않는다 |
| `.state/`·`state/` | 가져오지 않음 | 0.2 ticker 내부 상태 |

- 대상 프로젝트 폴더가 이미 있으면 거부한다(덮어쓰지 않는다). 그 상태 폴더의 host가 떠 있으면 거부한다.
- 넣었다고 센 thread 수와 DB에 실제로 있는 수를 다시 읽어 비교한다.
- 결과: `imported/convert.json`(+ `--out`이면 `convert.md`). 「실패」는 복사·대조 실패, 「바뀜」은 inventory 뒤 원본이 바뀐 파일(사본은 지금 원본과 같다)이다.

### shadow 판정

aaa host는 herdr pane을 보지 못한다. 그래서 지금 운영 thread는 **같은 시각·같은 입력**(thread 기록, herdr agent 상태와 그 지속 시간, 진행 보고, 보고 사본 첫 줄)을 두 규칙에 넣어 비교한다.

| 열 | 뜻 |
|---|---|
| 0.2 CLI | `herdr-projects thread list --json`이 그때 낸 묶음(정답) |
| 0.2 재현 | 같은 입력에 herdr-projects `thread::group()`(1f05c00 `src/thread.rs:565`)을 옮긴 규칙. 0.2 CLI와 다르면 입력을 다 모으지 못한 것이라 그 줄은 판정하지 않는다 |
| aaa | 같은 입력을 aaa 기록으로 바꿔 aaa `orch::thread_group`(실제 코드)에 넣은 결과 |
| aaa(지금 상태) | 원격 thread는 0.2가 약 1분마다 ssh로 본 기록 상태를 쓴다. 그 tick의 그 머신 herdr 상태를 넣으면 어떻게 되는지 따로 본다 |

aaa thread(pilot 등)는 host가 낸 묶음·harness 활동과 그 pane 화면 맨 아래 줄을 나란히 둔다.

**의도한 차이**(gate에서 차이로 세지 않는다):

| 경우 | 0.2 | aaa | 근거 |
|---|---|---|---|
| 보고 첫 줄이 `[결정 필요]`·`[사용자 작업 필요]` | 검토 대기 | 사람 차례 | aaa `orch.rs` thread_group 5(D9) |
| PR 승인(Landing) | Landing | 다른 묶음 | aaa에 Landing 없음(차이 목록 4장 I, D11 옮기는 중) |
| herdr `unknown`(셸) | agent 있음 | agent 없음 | aaa는 harness 활동이 있어야 agent다 |

## 2. 전환 gate: 무엇이 같아야 하는가

모두 만족해야 전환한다. 기간(N)은 사용자가 정한다.

| # | 조건 | 확인 |
|---|---|---|
| G1 | **자료가 같다.** 마지막 inventory 뒤 import에서 실패 0, 「바뀜」은 0.2 ticker를 멈춘 뒤 0, 끝난 thread 수 = DB resolved 수, inbox 수 = 투영 파일 수 | `convert.md` 요약, `aaa thread list <p> --json` 개수 |
| G2 | **열린 0.2 thread가 없다.** 남은 것은 사용자가 정한 idle 시점에 재개 정보(세션 ID·profile 인자, 인자 출처 `record`·`home-profile`)로 명시적으로 이어 간다 | `inventory.md` 「열린 thread와 재개 정보」 |
| G3 | **0.2 inbox 끝내지 않은 항목 0** | `inventory.md` 「inbox」 |
| G4 | **shadow: N시간 동안 의도하지 않은 차이 0**, 판정한 줄 비율 90% 이상(「입력 재현 안 됨」이 많으면 수집을 고친다) | `shadow.md` |
| G5 | **pilot: 잃은 보고 0, 잘못된 입력 0, 멈춘 감독자 0.** 시작·brief·진행 보고·inbox·사람 차례·착지·resolve를 aaa에서 끝까지 했다 | pilot 기록 |
| G6 | **aaa 상태 = 화면.** shadow의 aaa thread 줄에서 harness 활동과 화면 단서가 어긋난 줄 0(D8 화면 감지가 들어간 binary 기준) | `shadow.md` 「aaa thread」 |
| G7 | **설치가 맞다.** 세 Mac `aaa doctor lineage`가 `aaa-launcher` 책임, `aaa machine list` 모두 연결·이상 없음, 같은 `aaa --version` | install-runbook §3·§4 |
| G8 | **열린 결함.** pilot·shadow에서 기록한 결함이 고쳐졌거나 사용자가 받아들였다 | 차이 목록 |
| G9 | **ticker 하나.** 0.2 ticker를 멈춘 뒤에만 aaa routine을 켠다 | `herdr-projects ticker status`, `aaa routine list` |

## 3. 전환 순서 ✋

1. 0.2에서 새 thread를 더 시작하지 않는다(coordinator에게 알린다). 열린 thread는 0.2에서 끝내거나(G2) 재개 정보를 확인한다.
2. 0.2 inbox를 비운다(G3).
3. ✋ 0.2 ticker를 멈춘다: `herdr-projects ticker stop`. 이 뒤로 0.2 자료가 바뀌지 않는다(import 중 inbox 파일이 옮겨지는 경합을 막는다).
4. 마지막 inventory: `scripts/m4/inventory.sh --aaa ~/.local/bin/aaa --project <p> --home mac-a --herdr mac-a=local --herdr mac-b=mac-b --herdr mac-c=mac-c --out <밖의 폴더>/final`.
5. ✋ aaa host를 멈춘다(세션은 session host가 가지므로 끊기지 않는다): `launchctl bootout gui/$(id -u)/dev.aaa.host`.
6. import: `~/.local/bin/aaa migrate import --inventory <밖의 폴더>/final/inventory.json --to-project <p> --library --out <밖의 폴더>/final`. 요약에서 실패 0·바뀜 0을 본다(G1).
7. ✋ aaa host를 다시 올린다: `launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/dev.aaa.host.plist`. `aaa thread list <p>`·`aaa context <p>`로 본다.
8. routine을 하나씩 켠다(`enabled = true`). 명령 routine은 `aaa routine approve`로 다시 승인한다.
9. ✋ coordinator를 aaa 프로젝트 폴더로 옮긴다(사용자 승인): 0.2 coordinator를 `/exit`한 뒤 `aaa tab create --cwd ~/.aaa/projects/<p> -- claude ...`.
10. 0.2·herdr 설치는 검증 기간 동안 지우지 않는다(§15 5).

## 4. rollback

**언제:** aaa에서 보고를 잃었다, 잘못된 입력이 갔다, 감독자가 멈췄다, gate 조건이 깨졌다.

0.2 자료는 처음부터 바꾸지 않았으므로 「되돌릴 쓰기」는 없다. 할 일은 (a) aaa에서 새 일을 멈추고 (b) 0.2 coordinator가 aaa에서 생긴 일을 읽게 하고 (c) 0.2 ticker를 다시 켜는 것이다.

1. aaa에서 새 thread를 더 시작하지 않는다. 켠 aaa routine을 끈다(`enabled = false`).
2. aaa에서 생긴 일을 0.2 coordinator가 읽을 수 있게 내보낸다. aaa 프로젝트 폴더는 0.2와 같은 Markdown이다.
   - `aaa thread list <p> --json > <밖의 폴더>/rollback/threads.json`
   - 보고: `~/.aaa/projects/<p>/threads/<id>.md`, 결과물: `library/<id>/`, inbox 투영: `inbox/*.md`(0.2.35와 같은 `+++` 형식)
   - 0.2 coordinator brief에 위 경로를 적어 준다(0.2 폴더에 복사하지 않는다. 쓰기를 늘리지 않는다).
3. aaa에서 열린 thread는 둘 중 하나로 한다.
   - aaa에서 끝까지 맡긴다(aaa agent·session host는 host와 따로 산다).
   - 0.2로 넘긴다: agent가 idle일 때 aaa pane을 끝내고, herdr pane에서 같은 cwd로 `claude --resume <harness_session> <profile 인자>`(또는 `codex resume …`)를 띄운 뒤 `herdr-projects thread adopt <p> --pane <id> --title "…"`(0.2.35 `src/cli.rs:410-420`)로 기록한다. harness 세션 ID는 `aaa thread show <p> <id> --json`의 `agent.harness_session`이다.
4. ✋ 0.2 ticker를 다시 켠다: `herdr-projects ticker start`. coordinator를 0.2 프로젝트 폴더로 되돌린다.
5. 확인: 0.2 원본이 inventory 뒤 바뀌지 않았다(아래 rehearsal 4와 같은 비교), `herdr-projects context <p>`가 정상이다.

## 5. rehearsal(운영을 건드리지 않고 연습)

운영 host·ticker는 건드리지 않는다. 시험 host는 저장소 worktree 안 폴더와 빈 port로 띄우고 끝나면 확인한 PID로만 끝낸다.

1. **사본 만들기.** host Mac의 0.2 프로젝트 폴더를 rsync로 시험 폴더에 받는다(`library/`·`scratch/`·`uploads/`와 「뺀 곳」은 빼도 된다): `rsync -a --exclude '/<p>/library/' --exclude '.env' … mac-a:.herdr-projects/<p> <시험>/hp/`.
2. **시험 host.** `<시험>/config/host.toml`에 `agent_listen`·`web.listen`을 빈 port로, `[local_agent] mode = "none"`, `[projects] root = "<시험>/projects"`. 상태 폴더는 짧은 경로(socket 경로는 104자를 넘지 않는다)로 둔다.
3. **inventory → import → host.** `aaa --config-dir <시험>/config --state-dir <상태> migrate inventory --from <시험>/hp …`, `… migrate import …`, `… host`. `aaa thread list <p>`·`aaa context <p>`·`aaa inbox list <p>`·웹을 본다.
4. **실패를 흉내 내고 rollback.** 시험 host에서 일어난 일로 친다: 위 4장 1~2를 시험 host에 대고 하고, 내보낸 `threads.json`·보고 경로를 연다. 원본이 바뀌지 않았는지 inventory를 다시 내고 끝난 thread 파일의 hash를 비교한다(`inventory.json`의 `files`에서 `threads/`·`library/` 항목이 두 번 모두 같아야 한다).
5. **정리.** 시험 host PID를 확인해 끝내고, 시험 폴더를 절대 경로로 지운다.

결과는 pilot thread의 보고(시간표)에 남긴다.

## M4: profile 정의를 host 한 곳으로 옮기기(10-18 사용자 결정)

프로젝트·thread import와 별개인 설정 전환이다. 운영 파일은 자동으로 고치지 않는다. 전환 담당자가 다음 순서로 한다.

1. host Mac에서 현재 `~/.config/herdr-projects/config.toml`의 `[profiles.<이름>]`를 로컬로 확인한다.
   사용자 모델 목록·설정 원문은 저장소·fixture·공개 보고에 넣지 않는다.
2. 그 표의 `agent`·`model`·`effort`·`description`·`args`를 이름 그대로 aaa host.toml `[profiles.<이름>]`로 복사한다.
   aaa가 지원하는 harness는 claude·codex다. 다른 종류가 있으면 임의로 대체하지 말고 전환을 중단해 결정한다.
   같은 이름의 기본 profile도 사용자 표가 우선한다. 이름 규칙(40자, 영문·숫자·`.`·`_`·`-`, `-`·`.`로 시작 금지)과
   model·effort 검증은 참고 `profiles.rs`대로다. 오류는 `aaa config check`로 확인한다.
3. 각 Mac agent.toml에는 host 주소·work_root와 머신별 `[harnesses.<종류>] command/env`만 둔다(건강 점검 설정은 유지).
   옛 `[profiles]`가 있으면 host 정의와 대조한 뒤 사람이 제거한다. 전환 중에도 읽어서 경고하며 새 host는 사용하지 않는다.
4. `aaa profile list --machine mac-a`·`aaa profile resolve <이름>`으로 host 정본을 확인한다.
   허용 목록은 `aaa profile allow threads …`, 기본값은 `aaa profile default threads …`로 host에서 옮긴다.
   프로젝트별 thread_profile이 전역 기본보다 먼저인 규칙은 유지한다.
5. 각 Mac의 짧은 pilot thread를 같은 profile로 시작하고 실제 argv·model·effort·args와 머신별 실행 경로를 확인한다.
   host는 해석 결과를 기록하며 실행 파일이 없거나 실행 권한이 없으면 agent가 명시적으로 거부한다.
   옛 agent도 AGENT_LINK 1.3/1.4 연결은 유지하지만 `host_profiles.v1`이 없으면 시작을 거부하므로 먼저 agent를 갱신한다.
6. 새 대화 restart `--profile`이 최신 host 정의를 읽는지 확인한다. `--resume`은 저장된 launch로 같은 대화를 잇는다.
   실행 중 model 변경은 harness `/model`이며 aaa가 막거나 덮어쓰지 않는다.

기존 shell/custom profile의 command/env는 전환 호환용으로 읽는다. 새 claude/codex 정의는 위 다섯 키를 쓰고
머신별 실행 경로·환경은 agent의 harness 덮어쓰기로 분리한다. 배포·운영 host 재시작은 binary 배포 담당자가 한다.
