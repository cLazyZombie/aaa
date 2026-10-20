# 자동화 보기와 실행 기록

활동 막대의 「자동화」는 프로젝트의 자동화 규칙과 이전 routine을 함께 보여 준다. 줄마다 조건 → 동작, 켜짐, 지난 결과, 다음 일정(사건 조건은 「사건 오면」), 오늘 처리 횟수, 마지막 건너뜀 까닭이 있고 「자세히」가 상세를 연다(아래 「상세·변환·막힌 큐」). 목록은 열 때·포커스를 되찾을 때·보이는 동안 30초마다 host에서 읽는다. 「다시 읽기」로 바로 갱신할 수 있다. Reader도 기록을 보지만 토글할 수 없다. 상태 점과 승인 필요를 표시한다.

```sh
aaa routine list demo
aaa routine runs demo ci-main
aaa routine toggle demo ci-main --off
```

`routine runs`는 최근 순서로 최대 50건을 읽는다. 정의를 지운 뒤에도 이름을 알면 그 기록을 읽을 수 있다. `--json` 결과는 `runs` 배열이며 항목은 id·at_ms·status·detail·선택 thread다. status는 done(처리됨)·skipped(건너뜀)·waiting(대기)·failed(실패)다. L3 이전 실행을 역으로 채우지는 않는다.

일정의 coordinator 없음·처리하지 않은 안내·명령 승인 필요·routine_commands 꺼짐·지난 명령 실행 중을 기록한다. 명령의 종료/시간 초과·출력 바뀜/같음, PR 글 전달, CI/Git 사건 전달·같은 실패 집합·앞선 결과 따라잡기, 조회 outage 시작/회복도 기록한다. 변화 없는 감시 poll은 기록하지 않는다. 사건 자료나 명령 출력 본문을 기록에 복제하지 않으며 detail은 2,000자로 제한한다.

기록은 host DB meta의 `routine_runs:<프로젝트>:<이름>` 한 값에 저장한다. 최근 50건, 마지막 결과/건너뜀, host 지역 날짜의 오늘 처리 횟수를 하나의 transaction으로 갱신한다. 오늘 횟수는 최근 50건 밖의 처리도 포함하며 같은 내구 전달 id를 다시 받으면 기록을 갱신하고 중복 집계하지 않는다. 일반 routine의 오늘 횟수는 done 결과 수다(조회 회복 등도 포함). 규칙의 오늘/상한은 관측 고리와 독립인 내구 실행 예산이다. 관측 기록 저장 오류는 host 로그에 남기며 원래 실행/전달을 다시 시도시키지 않는다.

토글은 정의 파일의 enabled만 toml_edit로 바꾼다. routine의 명령·머신·승인 hash는 바꾸지 않는다. 토글로 실행 중인 명령이나 조회를 강제 종료하지 않는다.

## 운영 확인

1. 배포 뒤 「자동화」에서 프로젝트·조건·목적지와 출처 파일을 확인한다. 운영 정의 전환은 coordinator가 맡는다.
2. 실행이 없으면 마지막 건너뜀과 기록을 먼저 본다. 승인 필요는 사람이 상세의 「명령 승인 검토」나 aaa 밖 터미널의 `routine approve`로 처리한다. CI/Git 인증 문제는 각 [CI](ci-watch.md)·[Git](git-watch.md) runbook을 따른다.
3. 껐다 켤 때는 파일 diff가 enabled뿐인지 확인한다. 임무는 적용 사본 차이도 확인한다. 조회/임무 머신이나 인증을 이 화면이 바꾸지는 않는다.
4. DB를 보존해 재시작하면 기록을 이어 읽는다. 고리 밖 상세 이력은 기존 flow·inbox에서 찾는다. 기록 삭제/초기화용 명령은 제공하지 않는다.

## 자동화 기억의 동시 편집

`memory=true`인 실행은 시작할 때 `automations/<이름>.memory/`를 thread에 넘기며 파일별 공통 판을 host DB에 남긴다. 보고와 마지막 회수 때 이 판으로 비교한다. 한쪽만 고친 파일·추가·삭제는 반영하고, 양쪽에서 같은 파일을 다르게 고쳤으면 host 원문을 유지한다. thread가 `memory.md`를 줄이고 `history/`에 원문을 옮기는 중에도 host에서 덧붙인 줄과 별도 history 파일이 사라지지 않는다.

충돌은 inbox의 `automation-memory`로 알린다. `automations/<이름>.memory-conflicts/<thread>/<판>/` 아래 `host/`·`thread/`에 양쪽 사본, `conflicts.json`에 충돌 경로를 둔다. 삭제는 해당 사본에 파일이 없는 것으로 표시한다. 같은 충돌의 보고 회수·마지막 회수는 보존 사본을 재사용한다. 이 폴더는 다음 thread에 넘기지 않는다. 두 사본을 확인해 실제 기억과 history에 필요한 내용을 합친 뒤 inbox를 닫는다. 보존 사본은 자동 삭제하지 않는다.

줄을 추측해서 합치거나 충돌 표지를 요약에 넣지 않는다. 4KiB 요약과 history 정리를 지키기 위해 같은 파일의 양쪽 수정은 사람이 확인한다. 기존 8MiB·1000개 전송 상한과 64KiB 초과 안내는 유지하며, 합친 기억도 전송 상한을 넘으면 원문을 보존하고 회수를 실패로 알린다. 업그레이드 전에 열린 실행처럼 공통 판이 없는 경우는 서로 같은 파일만 합의로 보고, 다른 파일은 보존·안내한다.

FRONT_API 1.55는 사람 터미널 rollout과 배포 제안을 더한다(1.54의 자동 시작 범위 승인·holds·시작 actor, 1.52의 변환·분리 본문 포함). agent wire1.8은 바뀌지 않는다. 폼 편집·지금 실행은 아래 「만들기·고치기·지우기·지금 실행(W2)」이다.

## 상세·변환·막힌 큐(관리 GUI W1)

FRONT_API 1.58. 줄의 이름이나 「자세히」가 상세를 연다(데스크톱 가운데 창, 휴대폰 아래 sheet). 상세는 읽기만 한다.

- **규칙:** 요약(상태·형식·파일·지난·다음·오늘), 설정(조건·동작·일정·명령·하루 상한), 대상(동작 대상·명령 머신·자동 시작 후보와 profile·보내던 thread), 승인 범위(명령·자동 시작), 적용 판, 마지막 건너뜀, 기록(`routine_runs` 최근 50건), 파일 원문과 `text_file`·`brief_file` 본문(각 64KiB에서 자름). 규칙은 따로 적용 판을 고정하지 않는다. 사건마다 그때의 동작·범위 사본을 큐에 넣으므로 적용 판 자리에는 큐(대기 입력·처리 중 묶음)를 보인다.
- **이전 형식:** `routines/*.md` 규칙과 front matter로 시작하는 `missions/*.md` 임무에 「이전 형식」 배지를 단다. 웹은 TOML만 고친다(아래 W2 절).

**새 형식으로 바꾸기.** 프로젝트 머리의 단추가 미리보기를 연다. 미리보기는 `automation_convert`의 `dry_run`이다: host가 실제 변환과 같은 검증으로 계획만 짜고 아무것도 쓰지 않으며(임무 cursor seed·audit 없음), 실제 변환이 답할 목록(새 파일과 `.converted/` 보존 사본)과 `plan_sha256`을 준다. 화면은 새 규칙·임무 조건에서 생기는 규칙(`<임무>-events`·`<임무>-schedule`)·새 임무 파일·따로 둔 긴 본문·보존 사본·스키마로 묶어 보인다. 「변환」은 본 계획의 hash를 실어 보낸다. 그 사이 옛 파일이 바뀌어 계획이 달라지면 host가 거부하고 아무것도 바꾸지 않는다(다시 미리보기). Reader는 미리보기만 본다. 옛 host(1.57 이하)는 `dry_run`을 버리고 실제로 변환하므로, 웹은 `/api/v1/me`의 `front_api`가 1.58 이상일 때만 단추를 둔다. CLI `aaa automation convert`는 그대로 바로 변환한다.

**막힌 큐 풀기.** `do.message` 묶음을 보내는 중 host가 끊기거나 thread 전송이 확인창·쓰던 글·신뢰 창 밖의 이유로 실패하면, host는 그 묶음을 처리 중으로 남기고 다시 보내지 않는다. 다음 판정이 그 묶음을 다시 만나면 「입력 전달 모름 · 확인 필요」로 적는다(지금 보내는 중인 묶음은 모름이 아니며 풀 수 없다). 1.57 host가 남긴 처리 중 묶음은 이 표시가 없어 배포 뒤 첫 판정이 다시 만날 때 모름으로 바뀌고, 그때부터 풀 수 있다. 뒤 입력도 그 묶음 뒤에서 기다린다. 목록 줄은 빨간 「막힘」, 상세는 대상 thread·묶음 크기·시각·뒤에 쌓인 입력 수를 보인다. 사람이 대상 thread에서 글이 왔는지 본 뒤 고른다.

- 「남은 입력 계속 보내기」: 그 묶음만 닫고(다시 보내지 않음) 뒤 입력은 다음 차례에 보낸다.
- 「남은 입력 버리기」: 그 묶음과 뒤 입력을 모두 닫는다.




## 만들기·고치기·지우기·지금 실행(관리 GUI W2)

FRONT_API 1.76. 자동화 패널의 프로젝트 머리 「새 자동화」, 상세의 「고치기」·「지금 실행」·「지우기」, 목록 아래 「지운 자동화」. 조작 기기(Controller)만 쓰고 Reader는 보기만 한다. 웹은 TOML 규칙만 고치며 옛 `.md`는 먼저 「새 형식으로 바꾸기」를 한다.

- **편집 창.** 폼(조건 6종: 일정·명령 출력 변화·thread 사건·PR·CI·Git, 동작 4종: coordinator 안내·thread에 글·새 thread 시작·배포 제안)과 TOML 탭이 한 초안을 쓴다. 폼을 고치면 host(`automation_validate`)가 정본 타입으로 읽은 값과 원문을 비교해 **바뀐 칸만** 원문에 고친다. 주석·칸 순서·안 바꾼 칸은 그대로이고, 새로 생긴 칸 가운데 기본값과 같은 것은 쓰지 않는다. TOML을 고치면 host가 읽은 값으로 폼을 다시 그린다. 본문은 Markdown으로 고치고 미리 본다. 본문이 `text_file`·`brief_file`이면 그 파일을 고친다(새 본문 파일은 `automations/` 안에만 만든다). 새 규칙은 사용량 문턱 기본값(5시간 창 5%·주간 10% 아래면 쉼)을 채우고 횟수·간격 제한은 두지 않는다.
- **검토 탭.** 저장하면 켜진 채인지, 조건 → 동작, 시작 동작이면 지금 safety `start_threads`로 「바로 시작」인지 「제안만」인지, 자동 시작 범위 승인 여부, 명령 조건이면 명령·머신·승인·`routine_commands`, 사용량 문턱. 저장만으로는 돌지 않는다(일정은 다음 차례, 사건은 저장 뒤 사건부터).
- **저장.** 본 판(sha256)일 때만 쓴다. 새로 만들기는 그 이름 파일이 없을 때만, 고치기는 그 파일이 본 판 그대로일 때만이다. 다른 화면이 먼저 저장했거나 파일을 직접 고쳤으면 아무것도 쓰지 않고 `conflict`로 거부한다: 창은 내 초안과 지금 파일의 줄 차이를 보이고 「지금 파일로 다시 열기」나 「차이를 봤다 · 내 초안으로 저장」을 고르게 한다(자동 병합 없음). 그 사이 지운 규칙은 저장으로 되살아나지 않는다(NotFound). 본문 파일을 먼저, 규칙 파일을 마지막에 쓴다. 쓰기는 공용 저장 helper(파일마다 lock, rename 직전 다시 읽기)로 한다.
- **지우기·되살리기.** 지우기는 먼저 끈 뒤(`enabled = false`) `automations/.trash/<이름>-<시각>.toml`로 옮긴다. 기다리던 입력은 「보내지 않음」으로 닫고, 처리 중 묶음·기록·본문 파일·기억 폴더는 둔다. 「지운 자동화」의 「꺼진 채로 되살리기」는 같은 이름이 없을 때만 되돌리며 늘 꺼진 채다.
- **지금 실행.** 조건을 기다리지 않고 동작을 한 번 돌린다. 창을 열 때 정한 key로 보내며 같은 key는 한 번만 받는다. 수동 입력은 묶는 시간(`limits.batch`)을 기다리지 않지만 사용량 문턱·하루 상한·최소 간격은 지킨다. 일정 차례가 쌓여 있어도 따로 받는다. 명령 조건은 명령을 지금 돌려 출력이 바뀌었을 때만 동작이 돈다(명령 승인·`routine_commands` 필요). 꺼진 규칙·파일 오류·주인 thread(`thread = "owner"`)·배포 제안·승인되지 않은 시작 범위는 거부한다.
- **명령 승인.** 상세 「명령 승인 검토」가 명령·머신·hash·`routine_commands`를 보이고 승인하면 그 hash를 승인한다. 승인은 웹 조작 기기와 aaa 밖 터미널에서만 된다(aaa 세션 안·worker·evaluator·detached 연결은 거부).
- **CLI.** `aaa automation run <이름>`·`delete <이름>`(지금 판을 읽고 그 판을 지운다)·`restore <id>`. `aaa routine list`는 지운 자동화 줄과 되살리기 명령을 보인다.

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

[do.start]
title = "변화 검토"
profiles = ["reviewer"]
machines = ["mac-a"]
reuse = "open"
memory = true
reports = true
text = "바뀐 범위를 검토한다."

[limits]
batch = "1m"
```

조건은 `on.schedule`(every 또는 daily·catch_up), `on.command`(every/daily·command·machine), `on.thread`(events·repos), `on.pr`(events), `on.ci`(repo·branches·workflows·events·machine), `on.git`(repo·branches·events·machine) 중 하나다. `on.thread`는 처음 읽을 때 현재 flow를 기준으로 잡고 이후 사건만 받는다. thread 없는 사건의 repos 필터는 사건 repo를 쓴다. post·메시지 등 자기 동작을 다시 깨우는 사건은 받지 않는다. 자동화(`do.start`)가 띄운 thread의 사건도 기본으로 받지 않는다: 그 thread의 보고가 규칙을 깨워 다시 thread를 띄우는 고리를 막는다. 다른 규칙이 띄운 thread의 사건까지 받으려면 `include_automation = true`를 적는다. 그래도 그 규칙 자신이 띄운 thread의 사건은 늘 뺀다. 변환된 규칙은 include_automation을 명시해 기존 사건 범위를 유지한다.

동작은 `do.inbox`(coordinator 안내), `do.message`(thread="owner" 또는 t-NNNN), `do.start`(일반 Worker 시작) 중 하나다. 짧은 글은 `text = """…"""`, 긴 글은 `text_file = "automations/check.md"`로 쓴다. 두 칸을 함께 쓰면 오류다. 글 파일은 UTF-8·1MiB 이하·프로젝트 내부 상대 경로의 일반 파일이며 중간 폴더를 포함한 symlink·절대 경로·상위 경로 이동을 받지 않는다. inbox/message의 글은 입력과 함께 고정한다. start는 고정한 승인 범위가 현재와 같을 때 현재 작업 본문을 쓰며 아래 L5 규칙을 따른다. command 자료는 자료임을 밝힌 fenced 본문으로 전달한다.

`aaa automation schema`는 Rust 타입에서 만든 JSON Schema를 출력하고 `--mission`은 제거 안내를 출력한다. 변환기는 각 폴더에 스키마도 둔다. 저장소 기준 산출물은 [automation.schema.json](automation.schema.json)이다. 호스트는 스키마의 구조 검사에 더해 일정·이름·파일 경계를 검사한다. 웹 편집은 아래 W2 절을 따른다.

## 공통 실행 안전장치

source가 자료를 확정하면 `automation:<프로젝트>:<이름>` DB meta의 pending에 동작 사본과 함께 넣는다. source 기준(hash·flow cursor)을 바꾸는 곳은 입력 저장과 같은 transaction이다. CI/Git source outbox도 공통 큐에 저장된 뒤 비운다. 정의가 없어 읽지 못하면 source outbox를 보존한다.

`limits.batch`는 첫 입력부터 묶는 시간, `min_gap`은 실행 시작 사이 간격, `max_per_day`는 명시했을 때만 host 지역 날짜별 실행 시작 상한이다. 생략·0은 무제한이며 min_gap도 생략·0(또는 "0m")이면 간격 제한이 없다. 한 번에 같은 동작/목적지의 최대32건을 묶고, 큰 묶음은 나누며 나머지는 pending에 둔다. 대기 입력 건수나 당일 실행 횟수에 기본 상한을 두지 않는다. DB 저장 실패 때는 source 기준을 전진시키지 않아 입력을 버리지 않는다. 기록 고리의 정리나 inbox done은 실행 예산을 지우지 않는다.

`usage_session_below`·`usage_week_below`는 해당 창의 남은 비율 문턱이다. 설정한 창을 확인할 수 없을 때도 대기한다. host ticker가 웹 접속 없이도 별도 task에서 사용량 캐시를 갱신한다. 정상 조회는 5분 캐시, 실패·로그인 만료는 기존 재조회 간격, 429는 서버가 말한 backoff를 지킨다. 느린 조회가 ticker의 다른 판정을 막지 않도록 마지막 완료된 보고를 읽으며 AI 요청을 만들지 않는다. 구독 창을 확인할 수 없으면 규칙의 마지막 대기 이유에 「사용량 확인 불가」를 보이고 coordinator inbox에 한 번 알린다. 입력은 보존하며 새 보고 뒤 다시 판정한다. 상한·간격·사용량 대기 중 입력은 내구 보존한다. 일정의 `catch_up="once"`(기본)는 놓친 차례를 하나로 모으며 `"skip"`은 놓친 차례를 기록하고 넘긴다. 이 옵션은 schedule/command 조건의 칸이다.

전달 전에 inflight와 횟수를 확정한다. inbox·임무 입력은 내구 id로 중복을 막고 재시작 뒤 이어 전달한다. thread 메시지는 전달 여부가 모호하면 재전송하지 않고 「입력 전달 모름 · 확인 필요」로 남긴다. brief·확인창 대기와 전송 전 명확한 거절은 예산을 소모하지 않는다. 꺼진 규칙은 새 동작을 실행하지 않는다. 실행 중 명령/전달을 토글로 종료하지 않는다.

CI/Git 사건 자체의 이력·inbox와 규칙 동작은 다른 경계다. 이미 보여 준 감시 사건에 `do.inbox`가 다시 안내를 만들지는 않는다. 새 TOML의 on.ci가 명시한 passed/fixed도 동작 조건이며 옛 형식의 회복 무깨움은 유지한다. CI 회복이 확인되면 아직 실행하지 않은 그 branch의 실패 입력은 취소해 오래된 실패로 새 동작을 깨우지 않는다. 이미 전달한 임무 입력은 기존 `already_recovered` 표시를 따른다. 외부 push는 기존 coordinator가 검증한 user post 경로만 쓴다.

## 옛 형식 변환과 호환(A4)

상주 임무·평가자와 특수 패널은 제거했다. 활동 막대는 작업·머신·자동화 세 패널이다. 저장된 `special`은 작업 패널로 읽는다.
자동화 버튼의 배지는 막힌 큐 또는 승인 필요 규칙 수이며 한 규칙에 둘 다 있으면 한 번 센다. 자동화가 만든 thread는 작업 목록의 호박색 막대와 `자동` 표지로 구별한다.

`missions/*.md|toml`, `routine mission=`, `do.wake`는 실행하지 않는다. 옛 임무 파일은 inbox에 한 번, `aaa doctor files`에 같은 제거 안내를 보인다.
웹의 「새 형식으로 바꾸기」 또는 `aaa automation convert`가 원본을 `.converted/`에 보존하고 `do.start(reuse="open", memory=true)` 규칙·brief·기억 폴더로 옮긴다.
변환된 감시 규칙은 꺼진 상태로 검토하고 시작 범위를 승인한 뒤 켠다. 사건과 일정이 둘 다 있으면 별도 규칙이 된다. 옛 profile·머신·저장소·사용량 문턱은 유지한다.
기억·이력은 agent에서 hash를 대조해 가져오며, DB에 남은 미전달 입력은 내구 큐로 옮긴다. 전달 여부가 불명확한 입력은 막힌 묶음으로 남긴다.
미리보기는 파일·DB를 쓰지 않으며 원본·기억·큐가 바뀌면 본 계획 hash로 실행할 수 없다. 변환 도중 중단되면 journal과 원본 DB로 큐 이전을 마친다.

`mission`·`evaluator`·`evaluation` CLI는 한 판 동안 제거 안내만 출력한다. 옛 Front 요청도 제거 오류를 반환하고 `mission_list`만 빈 목록을 반환한다.
`Role::Evaluator` 직렬화 값과 평가 기록 표는 호환을 위해 남기며 새 실행에 쓰지 않는다. DB의 열린 옛 행은 일반 작업 목록에 표시하고 시작 때 inbox에 알린다.
Agent link 1.10은 `work_files.v1`을 사용한다. 1.9 agent에는 기존 파일 전송 태그로 이어 보내기·기억 회수를 계속한다.

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

## 열린 thread에 이어 보내기·기억·짧은 끝내기(A2)

`do.start.reuse = "open"`이면 같은 규칙이 만든 가장 최근 Starting/Open thread로 입력을 모은다. 기본 `new`는 사건마다 새 thread다.
`memory = true`, `close_idle = "30m"`, `reports = true`를 선택할 수 있다. memory·close_idle는 open에서만 받으며 reports는 승인 범위 hash를 바꾸지 않는다.
Codex profile의 open은 거부한다. 기존 임무의 제한은 대화 길이가 아니라 compaction 뒤 지시를 재독시킬 hook이 없는 데서 왔다.

한 사건 자체가 512KiB를 넘으면 접수를 거부해 source 원본·커서를 보존한다. 입력은 최대 32건·512KiB 묶음으로 작업 폴더 inbox에 먼저 기록하고 Queue 메시지로 알린다. 확인창 동안 파일을 보존하며,
전달 여부를 모르면 막힌 큐에 남긴다. 자동으로 다시 보내지 않는다. 「확인하고 풀기」는 기존 상세 화면에서 한다.
propose는 새 thread 만들기만 제안으로 바꾸며, 열린 thread에 이어 보내기는 그대로 한다. reports는 사건 thread 보고 앞 64KiB를 붙인다.

기억은 host 프로젝트 `automations/<규칙>.memory/`에 보존한다. 새 thread의 `memory/memory.md`와 history에 먼저 넣고,
보고 사본과 resolve 전 마지막 사본 때 되가져온다. processed.txt도 보존한다. 이동은 기존 상태 전송의 8MiB·1000개 한도와 hash·symlink 검사를 쓴다.
memory.md가 64KiB를 넘거나 끝내기까지 바뀌지 않으면 inbox에 한 번 알린다. 웹 상세의 「기억」은 읽기만 한다.

close_idle는 마지막 입력 뒤 새로 쓴 `[완료]`, agent의 Idle, 빈 내구 큐, 빈 미처리 inbox를 모두 확인한다.
보고 묶음이 검토/쉼인지는 판정에 쓰지 않는다. 입력 전 보고 hash도 대조하여 늦게 회수한 옛 완료로 끝내지 않는다.
자동화 `[완료]`는 저절로 확인하고 inbox 한 줄만 남긴다. 웹/푸시 완료 알림을 내지 않으며, 확인 필요 보고는 기존 알림을 유지한다.
「열림」 칩 옆 끝남 예정은 보고·활동·처리 상태 사건에 따라 바뀐다.
