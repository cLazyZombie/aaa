# 자동화 정의와 실행

자동화는 `schema = 2` 정의 하나와 실행 원장 하나를 사용한다. 일정·PR·CI·Git·thread 사건·수동 실행이 공통 내구 큐로 들어오며, 명령 조회도 같은 실행 안에서 처리한다. frontend는 aaa-proto에서 생성한 계약만 사용한다. 별도 workflow 서버는 없다.

```toml
#:schema ./automation.schema.json
schema = 2
title = "야간 검사 안내"
enabled = true

[trigger]
kind = "schedule"
daily = "03:00"
catch_up = "once"

[action]
kind = "notice"
text_file = "automations/nightly-coverage.brief.md"
```

`trigger`는 schedule(every/daily 중 하나, host 지역 시각) 또는 event(source=thread/pr/ci/git)다. CI·Git의 저장소와 조회 머신은 trigger에, 사건 종류·저장소/branch/workflow 필터는 condition에 둔다. 기존 명령 출력 감시는 schedule + observe.command + condition.output_changed=true다. 현재 명령 조회는 일정에만 붙이며 한 규칙에 조회 명령과 직접 명령 동작을 함께 쓰지 않는다.

| action.kind | 동작 영수증 | 완료의 뜻 |
| --- | --- | --- |
| notice | notice_created | coordinator 안내 등록 |
| command | command_exited | 종료값 0이며 timeout 없음 |
| thread | thread_started | 새 thread 시작 |
| message / 열린 thread 재사용 | message_delivered | 입력 전달 |
| proposal | proposal_created | 배포 제안 등록 |

안내 등록·thread 시작은 본래 작업의 완료가 아니다. 실행 원장은 queued/waiting/running/succeeded/failed/skipped/cancelled/unknown과 단계·사유·설정 판·입력·thread·처리 신고·보고를 구분한다. 옛 routine의 `done`은 관측 기록으로만 표시한다. 명령이나 메시지를 보낸 뒤 결과를 잃으면 unknown으로 보존하며 자동으로 재전송하지 않는다. 사람의 `automation unblock --batch`도 결과를 성공으로 바꾸지 않는다.

정책은 policy.batch/min_gap/max_per_day/max_running/usage_session_below/usage_week_below다. `min_gap=0`, `max_per_day=0`은 무제한이며 thread 동시 상한의 기본은 1이다. 기억·재사용·close_idle·기다릴 조건은 thread action에 둔다. 조회 명령의 종료 실패도 출력 변화에 포함해 기존 오류 안내를 보존한다.

```sh
aaa automation list
aaa automation show nightly-coverage
aaa automation runs nightly-coverage
aaa automation runs nightly-coverage --before ar-...
aaa automation run show ar-...
aaa automation enable nightly-coverage
aaa automation disable nightly-coverage
aaa automation run nightly-coverage
```

새 실행 목록은 100건씩 읽으며 이전 페이지 cursor를 반환한다. 상세는 실행 ID로 읽는다. 기존 routine list/runs/toggle/approve와 automation convert API는 호환 입구다. routine_runs 응답에는 새 executions와 옛 runs가 함께 있다. 웹은 host 1.119 이상에서 직접 명령 동작을 편집한다. 구형 host에는 기존 편집 동작만 보낸다.

## 권한과 입력 처리

명령은 기존 safety `routine_commands`와 사람의 명령·머신 hash 승인이 모두 필요하다. 직접 명령과 출력 조회는 승인 해시를 구별하므로 조회 승인이 직접 실행으로 확대되지 않는다. thread 시작 승인·workspace 허용·holds·실행 상한은 기존 owner가 검사한다. profiles와 harness에 별도 허용 목록을 만들지 않는다.

```sh
aaa automation approve --capability command check <명령-hash>
aaa automation approve --capability thread ci-watch <시작-범위-hash>
```

Reader·worker·coordinator는 사람 전용 승인·safety 변경을 할 수 없다. 이관은 grant를 만들지 않고, 배포는 proposal만 지원한다. 일반 merge/push/deploy/resolve 동작을 추가하지 않는다. idle-audit의 웹 읽기 전용 보호와 기존 close_idle 수명 조건을 유지한다.

기억은 기존 `.memory` 경로와 processed.txt를 유지한다. SessionStart(compact/resume)은 t-0941의 harness 공통 재독 안내를 쓴다. processed.txt는 입력 ACK이며 작업 성공 증거가 아니다. 새 입력 JSON의 execution_ids 각각에 대해 현재 thread가 처리 결과를 남길 수 있다.

```sh
aaa automation input complete ar-... --outcome completed --report r-0123
```

outcome은 completed/failed/needs-decision 중 하나다. 기존 peer+token post 전송 경로로 오며, host는 실행에 연결된 thread와 준비된 보고의 소유자를 확인한다. 다른 thread·coordinator·웹·자기 신고 session으로 대신 기록할 수 없다. 완료 신고는 agent의 보고이며 작업 권한·resolve를 만들지 않는다. 같은 결과는 멱등이고 다른 결과로 덮을 수 없다. 새 CLI는 명시적 automation_completed 영수증만 기록 성공으로 인정한다. 구형 host/agent와 섞인 동안에는 기존 processed.txt를 유지하고 입력 결과 명령은 함께 업그레이드한 뒤 쓴다.

## 이관과 복구

```sh
aaa automation migrate --dry-run
aaa automation migrate --plan-sha256 <미리보기-hash>
```

계획은 적용 직전의 파일을 읽는다. on/do/limits와 옛 routine/mission을 v2로 바꾸고 본문·정규화된 범위가 같은지 검사한다. 현재 profiles·머신·시각·enabled·상한을 보존한다. 야간 검사·git-backup의 notice를 thread나 command로 승격하지 않는다. 긴 본문은 brief 파일로 분리하고 원문은 `.converted/`에 보존한다. 기존 a-ID·기억·processed·열린 thread·예산·source cursor·중복 방지 영수증·승인은 그 자리에 유지한다.

프로젝트 dispatch gate는 실행 중 RPC가 끝나기를 기다린다. DB journal에 계획을 내구 저장한 뒤 완성된 파일을 rename하며, 중단되면 알려진 원본/목적 bytes만 써서 재개한다. 다른 편집이나 사본 충돌은 덮지 않고 실행을 막는다. 외부 동작 뒤 과거 DB snapshot을 복원하면 중복 실행할 수 있으므로 영수증을 보존한 전진 복구를 한다. schema 2를 모르는 바이너리로 내리지 않는다.

큐·cursor·budget·receipt의 기존 meta 저장은 호환 정본으로 유지한다. 새 automation_runs 테이블은 같은 SQLite transaction에서 갱신하며, UI의 관측 고리 버퍼로 실행 여부를 판정하지 않는다. 명령 intent를 기록한 뒤 응답을 잃으면 재실행하지 않고, 결과를 저장했다면 후속 동작만 이어간다.

## 참고

[herdr](https://github.com/devswha/herdr)(Apache-2.0), [herdr-projects](https://github.com/devswha/herdr-projects)(MIT), [herdr-web-ui](https://github.com/devswha/herdr-web-ui)(MIT)의 프로젝트·일정·thread 소유 구조를 따른다. [GitHub Actions runner](https://github.com/actions/runner)(MIT)의 명시적 단계/결과, [systemd timer](https://www.freedesktop.org/software/systemd/man/latest/systemd.timer.html)(LGPL-2.1-or-later)의 일정/실행 분리, [Temporal server](https://github.com/temporalio/temporal)(MIT)의 내구 상태와 결과 불명 구분을 비교했다. 외부 코드·새 의존성은 복사하지 않았다.
