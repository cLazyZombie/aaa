# 자동화 보기와 실행 기록

활동 막대의 「자동화」는 프로젝트의 routine과 임무 파일의 사건·일정 조건, 그리고 임무 목록을 함께 보여 준다. 줄마다 조건 → 동작, 켜짐, 지난 결과, 다음 일정(사건 조건은 「사건 오면」), 오늘 처리 횟수, 마지막 건너뜀 까닭이 있고 「자세히」가 상세를 연다(아래 「상세·변환·막힌 큐」). 목록은 열 때·포커스를 되찾을 때·보이는 동안 30초마다 host에서 읽는다. 「다시 읽기」로 바로 갱신할 수 있다. Reader도 기록을 보지만 토글할 수 없다. 상태 점과 승인 필요를 표시하며 임무 조건 줄은 임무 승인 필요도 알린다.

```sh
aaa routine list demo
aaa routine runs demo ci-main
aaa routine runs demo mission:repo-watch
aaa routine toggle demo ci-main --off
```

`routine runs`는 최근 순서로 최대 50건을 읽는다. 정의를 지운 뒤에도 이름을 알면 그 기록을 읽을 수 있다. `--json` 결과는 `runs` 배열이며 항목은 id·at_ms·status·detail·선택 thread다. status는 done(처리됨)·skipped(건너뜀)·waiting(대기)·failed(실패)다. L3 이전 실행을 역으로 채우지는 않는다.

일정의 coordinator 없음·처리하지 않은 안내·명령 승인 필요·routine_commands 꺼짐·지난 명령 실행 중을 기록한다. 명령의 종료/시간 초과·출력 바뀜/같음, PR 글 전달, CI/Git 사건 전달·같은 실패 집합·앞선 결과 따라잡기·없는 임무 대기, 조회 outage 시작/회복도 기록한다. 변화 없는 감시 poll은 기록하지 않는다. 임무 줄은 실제 깨움과 사용량/예산 대기를 기록한다. 사건 자료나 명령 출력 본문을 기록에 복제하지 않으며 detail은 2,000자로 제한한다.

기록은 host DB meta의 `routine_runs:<프로젝트>:<이름>` 한 값에 저장한다. 최근 50건, 마지막 결과/건너뜀, host 지역 날짜의 오늘 처리 횟수를 하나의 transaction으로 갱신한다. 오늘 횟수는 최근 50건 밖의 처리도 포함하며 같은 내구 전달 id를 다시 받으면 기록을 갱신하고 중복 집계하지 않는다. 일반 routine의 오늘 횟수는 done 결과 수다(조회 회복 등도 포함). 임무 줄은 기존 임무의 깨움 횟수/상한과 그 임무의 reset 기준을 그대로 보여 준다. 규칙의 오늘/상한은 관측 고리와 독립인 내구 실행 예산이다. 관측 기록 저장 오류는 host 로그에 남기며 원래 실행/전달을 다시 시도시키지 않는다.

토글은 정의 파일의 enabled만 toml_edit로 바꾼다. routine의 명령·머신·승인 hash는 바꾸지 않는다. `mission:<이름>`은 옛 임무 파일의 조건에서 만든 숨은 공통 규칙이다. 임무 토글도 그 파일의 enabled만 바꾸고 기존 적용 사본/다시 읽기 원칙을 따른다. 끄기는 다음 임무 판정에서 축소 적용된다. 다시 켜기는 적용 사본과 다를 수 있으므로 특수 패널에서 다시 읽기를 확인한다. 토글로 실행 중인 명령이나 조회를 강제 종료하지 않는다.

## 운영 확인

1. 배포 뒤 「자동화」에서 프로젝트·조건·목적지와 출처 파일을 확인한다. 운영 정의 전환은 coordinator가 맡는다.
2. 실행이 없으면 마지막 건너뜀과 기록을 먼저 본다. 승인 필요는 사람이 기존 `routine approve` 절차로 처리한다. CI/Git 인증 문제는 각 [CI](ci-watch.md)·[Git](git-watch.md) runbook을 따른다.
3. 껐다 켤 때는 파일 diff가 enabled뿐인지 확인한다. 임무는 적용 사본 차이도 확인한다. 조회/임무 머신이나 인증을 이 화면이 바꾸지는 않는다.
4. DB를 보존해 재시작하면 기록을 이어 읽는다. 고리 밖 상세 이력은 기존 flow·inbox에서 찾는다. 기록 삭제/초기화용 명령은 제공하지 않는다.

FRONT_API 1.55는 사람 터미널 rollout과 배포 제안을 더한다(1.54의 자동 시작 범위 승인·holds·시작 actor, 1.52의 변환·분리 본문 포함). agent wire1.8은 바뀌지 않는다. 폼 편집·지금 실행은 아직 제공하지 않는다.

## 상세·변환·막힌 큐(관리 GUI W1)

FRONT_API 1.58. 줄의 이름이나 「자세히」가 상세를 연다(데스크톱 가운데 창, 휴대폰 아래 sheet). 상세는 읽기만 한다.

- **규칙:** 요약(상태·형식·파일·지난·다음·오늘), 설정(조건·동작·일정·명령·하루 상한), 대상(동작 대상·명령 머신·자동 시작 후보와 profile·보내던 thread), 승인 범위(명령·자동 시작), 적용 판, 마지막 건너뜀, 기록(`routine_runs` 최근 50건), 파일 원문과 `text_file`·`brief_file` 본문(각 64KiB에서 자름). 규칙은 따로 적용 판을 고정하지 않는다. 사건마다 그때의 동작·범위 사본을 큐에 넣으므로 적용 판 자리에는 큐(대기 입력·처리 중 묶음)를 보인다.
- **임무:** `mission_show`의 상태·thread·승인 hash·적용/파일/예약 판·차이·적용 사본·후보 파일·brief_file과 `mission:<이름>` 기록 50건. 막힘의 「다시 시작」은 아직 특수 패널에 있다.
- **이전 형식:** `routines/*.md` 규칙과 front matter로 시작하는 `missions/*.md` 임무에 「이전 형식」 배지를 단다. 웹은 TOML만 고친다(W2).

**새 형식으로 바꾸기.** 프로젝트 머리의 단추가 미리보기를 연다. 미리보기는 `automation_convert`의 `dry_run`이다: host가 실제 변환과 같은 검증으로 계획만 짜고 아무것도 쓰지 않으며(임무 cursor seed·audit 없음), 실제 변환이 답할 목록(새 파일과 `.converted/` 보존 사본)과 `plan_sha256`을 준다. 화면은 새 규칙·임무 조건에서 생기는 규칙(`<임무>-events`·`<임무>-schedule`)·새 임무 파일·따로 둔 긴 본문·보존 사본·스키마로 묶어 보인다. 「변환」은 본 계획의 hash를 실어 보낸다. 그 사이 옛 파일이 바뀌어 계획이 달라지면 host가 거부하고 아무것도 바꾸지 않는다(다시 미리보기). Reader는 미리보기만 본다. 옛 host(1.57 이하)는 `dry_run`을 버리고 실제로 변환하므로, 웹은 `/api/v1/me`의 `front_api`가 1.58 이상일 때만 단추를 둔다. CLI `aaa automation convert`는 그대로 바로 변환한다.

**막힌 큐 풀기.** `do.message` 묶음을 보내는 중 host가 끊기거나 thread 전송이 확인창·쓰던 글·신뢰 창 밖의 이유로 실패하면, host는 그 묶음을 처리 중으로 남기고 다시 보내지 않는다. 다음 판정이 그 묶음을 다시 만나면 「입력 전달 모름 · 확인 필요」로 적는다(지금 보내는 중인 묶음은 모름이 아니며 풀 수 없다). 1.57 host가 남긴 처리 중 묶음은 이 표시가 없어 배포 뒤 첫 판정이 다시 만날 때 모름으로 바뀌고, 그때부터 풀 수 있다. 뒤 입력도 그 묶음 뒤에서 기다린다. 목록 줄은 빨간 「막힘」, 상세는 대상 thread·묶음 크기·시각·뒤에 쌓인 입력 수를 보인다. 사람이 대상 thread에서 글이 왔는지 본 뒤 고른다.

- 「남은 입력 계속 보내기」: 그 묶음만 닫고(다시 보내지 않음) 뒤 입력은 다음 차례에 보낸다.
- 「남은 입력 버리기」: 그 묶음과 뒤 입력을 모두 닫는다.

어느 쪽이든 닫은 입력은 receipt를 남겨 같은 입력이 다시 와도 받지 않고, 기록에 건너뜀(「확인함 · 다시 보내지 않음」·「확인함 · 남은 입력 버림」)으로 남는다. 요청은 `automation_unblock { name, batch, discard }`이고 `batch`는 화면에서 본 묶음(`queue.inflight.id`)이다. 이미 풀렸거나 다른 묶음이면 바꾸지 않는다. Controller 웹 기기와 세션 밖 Local만 할 수 있다(세션 안 CLI·worker·평가자·Reader 거부). 터미널에서는 `aaa routine list`가 막힌 묶음 id를 보이고 `aaa automation unblock <이름> --batch <id> [--discard]`로 푼다. 임무의 「입력 전달 모름」(아래 막힘 이유)은 이것과 다르며 임무 다시 시작이 푼다.

임무의 「막힘」은 [막힘 이유와 복귀](mission-blocks.md)를 따른다. 확인창 해제는 기록하고 자동 복귀하지만 전달 모름은 자동으로 풀지 않는다.


## 순수 TOML 규칙

`automations/<이름>.toml` 한 파일이 조건 하나와 동작 하나다. 옛 `routines/*.md`도 계속 읽는다. 같은 이름이 두 폴더에 있으면 둘 다 config-error가 되어 실행하지 않는다. 앞머리 없는 본문 md는 규칙으로 읽지 않는다. 오류는 `automations/check.toml:4:12 on.git.branches: …`처럼 파일·줄·열·칸을 포함한다.

```toml
#:schema ./automation.schema.json
title = "저장소 push 확인"
enabled = true

[on.git]
repo = "app"
branches = ["main"]
events = ["pushed", "forced", "deleted"]

[do.wake]
mission = "repo-watch"
reports = true
text = "바뀐 범위를 검토한다."

[limits]
batch = "1m"
```

조건은 `on.schedule`(every 또는 daily·catch_up), `on.command`(every/daily·command·machine), `on.thread`(events·repos), `on.pr`(events), `on.ci`(repo·branches·workflows·events·machine), `on.git`(repo·branches·events·machine) 중 하나다. `on.thread`는 처음 읽을 때 현재 flow를 기준으로 잡고 이후 사건만 받는다. thread 없는 사건의 repos 필터는 사건 repo를 쓴다. 평가자/임무·post·메시지 등 자기 동작을 다시 깨우는 사건은 받지 않는다. 자동화(`do.start`)가 띄운 thread의 사건도 기본으로 받지 않는다: 그 thread의 보고가 규칙을 깨워 다시 thread를 띄우는 고리를 막는다. 다른 규칙이 띄운 thread의 사건까지 받으려면 `include_automation = true`를 적는다. 그래도 그 규칙 자신이 띄운 thread의 사건은 늘 뺀다. 임무와 임무를 바꾼 규칙은 지금처럼 자동화 thread의 사건도 받는다(plan §8.318).

동작은 `do.inbox`(coordinator 안내), `do.message`(thread="owner" 또는 t-NNNN), `do.wake`(mission), `do.start`(일반 Worker 시작) 중 하나다. 짧은 글은 `text = """…"""`, 긴 글은 `text_file = "automations/check.md"`로 쓴다. 두 칸을 함께 쓰면 오류다. 글 파일은 UTF-8·1MiB 이하·프로젝트 내부 상대 경로의 일반 파일이며 중간 폴더를 포함한 symlink·절대 경로·상위 경로 이동을 받지 않는다. inbox/message/wake의 글은 입력과 함께 고정한다. start는 고정한 승인 범위가 현재와 같을 때 현재 작업 본문을 쓰며 아래 L5 규칙을 따른다. command 자료는 자료임을 밝힌 fenced 본문으로 전달한다.

`aaa automation schema`는 Rust 타입에서 만든 JSON Schema를 출력하고 `--mission`은 임무 스키마를 출력한다. 변환기는 각 폴더에 스키마도 둔다. 저장소 기준 산출물은 [automation.schema.json](automation.schema.json)·[mission.schema.json](mission.schema.json)이다. 호스트는 스키마의 구조 검사에 더해 일정·이름·파일 경계를 검사한다. 웹은 목록·기록·enabled만 편집하며 현재 칸별 폼 편집은 없다.

## 공통 실행 안전장치

source가 자료를 확정하면 `automation:<프로젝트>:<이름>` DB meta의 pending에 동작 사본과 함께 넣는다. source 기준(hash·flow cursor)을 바꾸는 곳은 입력 저장과 같은 transaction이다. CI/Git source outbox도 공통 큐에 저장된 뒤 비운다. 정의가 없어 읽지 못하면 source outbox를 보존한다.

`limits.batch`는 첫 입력부터 묶는 시간, `min_gap`은 실행 시작 사이 간격, `max_per_day`는 명시했을 때만 host 지역 날짜별 실행 시작 상한이다. 생략·0은 무제한이며 min_gap도 생략·0(또는 "0m")이면 간격 제한이 없다. 한 번에 같은 동작/목적지의 최대32건을 묶고, 큰 묶음은 나누며 나머지는 pending에 둔다. 대기 입력 건수나 당일 실행 횟수에 기본 상한을 두지 않는다. DB 저장 실패 때는 source 기준을 전진시키지 않아 입력을 버리지 않는다. 기록 고리의 정리나 inbox done은 실행 예산을 지우지 않는다.

`usage_session_below`·`usage_week_below`는 해당 창의 남은 비율 문턱이다. 설정한 창을 확인할 수 없을 때도 대기한다. host ticker가 웹 접속 없이도 별도 task에서 사용량 캐시를 갱신한다. 정상 조회는 5분 캐시, 실패·로그인 만료는 기존 재조회 간격, 429는 서버가 말한 backoff를 지킨다. 느린 조회가 ticker의 다른 판정을 막지 않도록 마지막 완료된 보고를 읽으며 AI 요청을 만들지 않는다. 구독 창을 확인할 수 없으면 규칙의 마지막 대기 이유에 「사용량 확인 불가」를 보이고 coordinator inbox에 한 번 알린다. 입력은 보존하며 새 보고 뒤 다시 판정한다. 상한·간격·사용량 대기 중 입력은 내구 보존한다. 일정의 `catch_up="once"`(기본)는 놓친 차례를 하나로 모으며 `"skip"`은 놓친 차례를 기록하고 넘긴다. 이 옵션은 schedule/command 조건의 칸이다.

전달 전에 inflight와 횟수를 확정한다. inbox·임무 입력은 내구 id로 중복을 막고 재시작 뒤 이어 전달한다. thread 메시지는 전달 여부가 모호하면 재전송하지 않고 「입력 전달 모름 · 확인 필요」로 남긴다. brief·확인창 대기와 전송 전 명확한 거절은 예산을 소모하지 않는다. 꺼진 규칙은 새 동작을 실행하지 않는다. 실행 중 명령/전달을 토글로 종료하지 않는다.

CI/Git 사건 자체의 이력·inbox와 규칙 동작은 다른 경계다. 이미 보여 준 감시 사건에 `do.inbox`가 다시 안내를 만들지는 않는다. 새 TOML의 on.ci가 명시한 passed/fixed도 동작 조건이며 옛 형식의 회복 무깨움은 유지한다. CI 회복이 확인되면 아직 실행하지 않은 그 branch의 실패 입력은 취소해 오래된 실패로 새 동작을 깨우지 않는다. 이미 전달한 임무 입력은 기존 `already_recovered` 표시를 따른다. 외부 push는 기존 coordinator가 검증한 user post 경로만 쓴다.

## 임무 파일과 변환

```toml
#:schema ./mission.schema.json
title = "저장소 확인"
profile = "reviewer"
machine = "mac-a"
repo = "app"
brief_file = "missions/repo-watch.md"

[limits]
batch = "5m"

[reset]
daily = "05:00"
after_wakes = 6
```

`missions/<이름>.toml`은 상주 agent와 예산을 정하고 `brief_file`은 앞머리 없는 md다. 사건·일정은 automations로 분리한다. 옛 임무의 events/repos/schedule도 공통 규칙으로 처리한다. 첫 시작·repo 추가/변경은 사람 승인을 요구한다. 예산은 승인 hash 밖이며 올리거나 없애도 재승인을 요구하지 않는다. 실제 repo/머신 범위가 같은 기존 승인 기록은 이어받는다. 적용 사본은 읽은 정의와 brief를 함께 고정한다. 형식 전환은 파일 변경으로 한 번 표시되고 다음 reset에 적용하며, brief만 바뀌어도 본문 diff를 보여 준다.

운영 파일 전환은 배포 뒤 coordinator가 한다. 아래 명령은 실제 프로젝트를 바꾸므로 운영 전에는 별도 임시 프로젝트에서 결과를 검토한다.

```sh
aaa automation convert
```

변환기는 전체 결과의 조건·동작·명령 승인 값과 임무 승인 범위를 먼저 검증한다. 같은 이름 충돌·기존 결과 파일·잘못된 정의가 있으면 덮어쓰지 않는다. 짧은 글은 inline, 긴 글은 별도 md, 옛 원본은 각 폴더 `.converted/<이름>.md`로 보존한다. 임무 조건은 `<임무>-events`·`<임무>-schedule` 규칙으로 만든다. 옛 flow cursor·일정 기준·당일 횟수도 새 규칙으로 넘겨 전환 경계의 중복을 막는다. 새 형식 전환 뒤 옛 적용 사본의 조건은 다시 수신하지 않는다.

일반 쓰기 실패 때는 이번에 만든 파일만 지우고 원본을 되돌린다. 변환 도중 프로세스나 머신이 중단되는 전체 파일 변경의 원자성까지 보장하지는 않는다. 이때는 재실행 전에 `.converted/`와 현재 파일을 diff해 복구하고, 출처와 값이 같음을 검토한다. 변환 완료 뒤 coordinator가 파일 변경을 프로젝트 저장소의 한 커밋으로 남긴다. host 프로세스는 git을 실행하지 않는다. 재변환은 바꿀 옛 원본이 없으면 파일을 쓰지 않는다.

## 하루 깨움 상한 안내

횟수·시간·최소 간격·철수 일수는 기본 제한 없음이다. max_wakes/max_minutes/retire_cap_days와 host max_running/max_pending은 생략·0이면 무제한이며 명시한 양수만 지킨다. min_gap은 0 또는 "0m"으로 없앨 수 있다. batch·대화 reset·usage·dedupe는 그대로다. 파일에서 예산을 없애거나 바꾸면 repo/본문을 바꾸지 않고 다음 tick에 반영한다.

명시 상한이 있는 임무의 상한 안내는 예산 날짜마다 한 번이다. inbox done·완료 항목 정리·host 재시작은 당일 통지 여부를 초기화하지 않는다. 구판의 당일 완료 안내도 이미 보낸 것으로 인정한다. 다음 예산 날짜에 상한이 다시 차면 한 번 새로 알린다. 통지 표식과 inbox 항목은 한 DB transaction이므로 저장 실패를 보낸 것으로 처리하지 않는다. 임무 예산 자체는 기존 host 지역 날짜 기준이며 대화 reset과 별개다.


## reload 예약과 적용 시점

reload의 성공 응답은 후보 적용 예약을 수락했다는 뜻이다. 현재 작업 중이면 작업이 끝난 뒤, 입력이 없고 아무도 보고 있지 않은 안전한 Idle에서 새 대화로 적용한다. 확인창·초안·pause/disabled·사용량 대기는 강제로 버리지 않는다. CLI와 show·웹의 특수 줄은 예약 판(pending_sha)과 적용 대기 이유(apply_wait)를 표시한다. 예약 뒤 파일이 다시 바뀌면 현재 파일과 예약 판이 다르다고 알리고, 새 파일은 reload로 다시 예약한다. restart만 요청하면 현재 적용 판을 다시 시작하며, 이미 reload한 후보가 있으면 그 예약을 따른다.

명시한 하루 깨움 상한은 새 입력을 깨우는 한도다. 승인된 후보의 명시 reload/reset 자체를 그 상한으로 막지 않는다. 이전 세션의 종료 확인 뒤에만 새 대화를 띄우며, 완료하면 예약 판을 applied_sha로 확정하고 대기 이유를 지운다. approval_sha256은 승인 범위 값의 hash라 file/applied 사본 hash와 서로 다른 값이 정상이다.

## 임무 harness 권한

임무 시작·restart는 일반 thread와 같은 safety yolo·thread_agent_args 규칙을 쓴다. 역할이 evaluator라는 이유로 manual 모드를 강제하지 않는다. host가 푼 인자를 agent의 실행별 임무 설정이 다시 덮어쓰지도 않는다. 프로젝트 yolo가 꺼져 있으면 일반 thread의 해당 설정을 그대로 따른다. 자기 inbox/보고/조회 문맥과 hook은 유지한다.

임무 지침의 push·kill·시스템 변경 금지와 첫 시작·repo 범위 승인은 그대로다. 사용자 설정·전역 harness 설정을 이 구현이 바꾸지 않는다. 이 변경은 host와 agent 양쪽에 있으므로 배포 때 둘 다 새 버전으로 갱신하고, 운영 임무의 새 대화 적용/restart는 coordinator가 확인한다.


### 무제한 표시와 깨움 작업 시간

생략·0은 상한 없음이다. 옛 파일의 `max_wakes=4294967295`·`max_minutes=4294967295`도 상한 없음으로 읽는다. 새 파일에서는 그 큰 수를 쓰지 않고 칸을 생략한다. 웹 특수 패널과 CLI `mission list/show`는 무제한이면 `깨움 1`, 명시 상한3이면 `깨움 1/3`으로 표시한다. 후보 원문은 실제 파일을 그대로 보여 준다.

`일한 분`은 임무의 누적 작업 분이다. 시간 상한을 적었을 때만 별도로 `깨움 작업 19/60분`을 보인다. `max_minutes`는 마지막 깨움 이후 벽시계 시간이 아니라 그 깨움에서 관찰한 Working 시간만 센다. Idle·확인창·멈춤·host 중단을 작업으로 소급하지 않는다. 옛 기록에는 깨움별 작업 셈이 없으므로 배포 후 관찰분부터 센다. 새 깨움은 이 셈을 초기화하며, 같은 깨움의 시간 상한 안내는 완료 처리 뒤 반복하지 않는다.

멈춘 임무의 다시 읽기·다시 시작은 후보를 예약해도 사람의 멈춤을 해제하지 않는다. `적용 대기: 멈춤·꺼짐을 해제한 뒤 적용한다`를 확인하고 재개한다. 예산 값 자체는 다음 판정에서 멈춤 중에도 반영되며, 본문·repo 후보는 안전한 대기와 새 대화 적용을 기다린다.


L4는 plan §8.290, FRONT_API1.52다. S1/S2의 한 프로젝트 계약을 따른다. `automation convert`의 프로젝트 인자는 생략하며 옛 명시 인자는 같은 프로젝트일 때만 호환한다. 원시 Front 요청도 project를 생략하면 host의 한 프로젝트를 쓴다. 프로젝트 설정은 기존 `aaa.toml` 우선·`project.toml` 호환 owner를 그대로 사용한다. agent 계약은 기준1.8에서 바꾸지 않는다.
## 자동 thread 시작(L5)

자동 시작은 일반 Worker thread를 만든다. 보고·착지·정리는 coordinator가 맡으며 운영 자동화의 전환은 배포 뒤 coordinator가 한다.

```toml
title = "새벽 점검"
[on.schedule]
daily = "03:00"
catch_up = "once"
[do.start]
title = "점검 {date}"
brief_file = "scratch/check-brief.md"
repo = "app"
profiles = ["reviewer", "builder"]
profile_pick = "week-left"
machines = ["mac-a", "mac-b"]
machine_pick = "idlest"
wait = ["timing"]
```

`repo`는 각 후보 머신에 등록한 저장소의 이름 또는 정확한 경로다. 머신마다 하나로 해석돼야 한다. 생략하면 일반 작업 폴더(Dir)를 만든다. `brief_file`·`text_file`·`text` 중 하나로 비지 않은 할 일을 준다. 파일은 프로젝트 폴더 안의 일반 파일이어야 하며 바깥 경로·symlink는 거부한다. 본문/제목의 `{machine}`·`{repo_path}`·`{date}`만 채운다. 조건 자료는 별도 인용 블록으로 붙인다.

처음과 범위를 넓힐 때만 사람이 승인한다. `aaa routine list --json`의 start_scope와 start_approval_sha256를 검토한 뒤 aaa 세션 밖 터미널에서 `aaa automation approve check <검토한-hash>`를 실행하거나 Controller 웹 「자동화 → 승인 검토」에서 확인한다. 본 뒤 범위가 바뀌었으면 거부하고 다시 검토하라고 알린다. Reader는 읽기만 한다. hash는 정규화한 조건·일정/필터·명령 조회 머신·입력 저장소·해석된 시작 머신/저장소·profile에 묶인다. 주석·순서·본문·제목·선택 예산은 포함하지 않는다. 좁힌 대상/필터와 더 긴 every 주기는 기존 승인 ceiling 안에서 적용한다. 이미 쌓인 차례와 현재 범위가 다르면 그 차례는 취소한다. 본문만 바뀌면 승인 유지·inbox changed/diff 한 번으로 알린다.

`safety.start_threads=propose`면 thread를 만들지 않고 「띄울 차례」 inbox 한 건을 남긴다. auto면 일반 thread의 profile·권한 설정을 그대로 쓴다. 승인 대기는 실행 횟수를 소비하지 않는다. 명령 조건의 조회 명령 승인은 기존 routine 승인과 별도다.

`idlest`는 연결된 후보 가운데 Working thread 수, CPU 사용률, 목록 순서로 고른다. 모두 일하고 있어도 가장 덜 바쁜 곳을 고른다. `week-left`는 고른 머신이 host의 로컬 agent이고 모든 후보 profile의 host 로그인 주간 사용량을 확실히 비교할 수 있을 때 남은 양이 많은 것을 고른다. 원격 머신·여러 계정·읽기 실패·숫자 없음이면 사용 가능한 첫 profile이다. AI 요청을 만들지 않는다.

측정 작업은 `aaa thread start --holds timing ...`으로 표지를 단다. 그 thread가 Starting/Open인 동안 같은 머신을 후보에서 제외한다. 모든 후보가 표지/연결/profile 때문에 막히면 입력과 까닭을 보존하고 기다린다. 표지 thread를 끝내면 다시 고른다. 동적으로 표지를 바꾸는 명령은 이 단계에 포함하지 않는다.

횟수·간격·동시성·기다림 시간은 기본 제한이 없다. `[limits] max_running`·`max_per_day`·`min_gap`, host `[projects] max_auto_threads`, `do.start.wait_max`는 적었을 때만 사용하며 생략·0은 없음이다. 앞 자동 thread가 열려 있어도 다른 사건은 실행한다. 명시 wait_max를 넘으면 그 차례만 건너뛰고 inbox 한 번을 남긴다. usage 문턱·batch·dedupe는 유지한다.

자동으로 띄운 thread는 기록에 규칙 이름을 남긴다(`aaa thread show`의 `automation`·`automation_title`). 웹 「작업」 목록은 그 줄을 사람이 띄운 thread와 같은 폴더·차례에 두고 왼쪽 호박색 막대와 둘째 줄 「자동 · <규칙 제목>」 표지로 구별한다(누르면 그 규칙 상세). 자동화 패널의 규칙 줄은 그 규칙의 열린 thread를 「열림 t-NNNN」 칩으로 보이고, 「모든 thread」는 「자동화만」으로 거른다. 할 일 화면의 「task 없는 열린 thread」에는 넣지 않는다(출처가 규칙이다).

일정형은 coordinator pane 없이 실행하고, 같은 예정 차례는 한 번만 시작한다. 기다리는 동안 다음 일정은 catch_up once로 묶는다. thread 기록과 시작 key를 같은 DB transaction에 저장하고, 그 직전에도 holds/명시 동시 상한을 확인한다. 최종 전달 영수증 저장이 실패해도 내구 시작 key로 재생성을 막는다. 시작 후 실패한 thread는 일반 thread의 restart/정리 절차를 따른다. flow thread_started.actor는 routine:<이름>, 실행 기록에는 thread id가 남는다.

확인: `aaa routine runs check`의 승인/holds/연결/profile/명시 상한 대기 까닭과 thread id를 본다. 기다림 중 파일 범위를 고쳤으면 이전 차례 취소 기록을 확인한다. DB/시작 key를 지워 재실행시키지 않는다.

## 배포는 제안까지만(L5)

```toml
do = "deploy"
[on.git]
repo = "aaa"
branches = ["main"]
[limits]
min_commits = 10
batch = "30m"
max_per_day = 2
```

`automations/deploy.toml` 같은 현재 순수 TOML 파일에 둔다. main의 pushed만 받으며 다른 branch·forced/deleted·다른 조건은 거부한다. min_commits(생략1, 1~100)는 최근 성공 배포에 기록한 commit 뒤 origin/main에 포함된 새 커밋 수다(merge와 그 안의 커밋을 포함). 성공 commit 기록이 없으면 첫 등록 head를 기준으로 잡고 재시작에도 보존한다. commit을 지정하지 않은 성공 배포는 등록 기준을 바꾸지 않는다. origin을 교체하면 새 head에 등록 기준을 잡고 교체 전 성공 기록을 제외한다. 이 기준은 재시작에도 남으며 교체 뒤 새 성공 배포부터 다시 성공 commit을 쓴다.

임계값 뒤 기존 queue/limits로 묶어 coordinator inbox에 proposed 제안을 만든다. commit과 `scripts/deploy-build.sh` 안내를 보며 사람이 빌드한 후보의 provenance/SHA를 확인한다. 그 뒤 사람이 aaa 밖 host 터미널에서 deploy start의 yes를 입력하거나 사용자 지시를 받은 coordinator가 기존 safety로 시작한다. 자동 빌드/thread 시작·배포는 없다. 제안은 앱 안에만 알리고, 읽고 정리한 항목을 ACK 실패·host 재시작으로 되살리지 않는다.

임계값은 source가 새 head를 받을 때 판정한다. 이미 내구 큐에 받은 제안 자료는 이후 임계값 편집으로 ACK 재개가 막히지 않는다. batch/하루/사용량 실행 예산은 기존 공통 queue 규칙을 따른다.

`automations/pr-followup.toml`로 변환한 기본 PR 규칙은 `project create`와 `doctor files --fix`가 보존한다. `routines/pr-followup.md`를 다시 만들지 않으며, 이미 두 파일이 겹친 경우에는 기존 충돌 진단을 유지한다.
