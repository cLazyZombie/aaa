# 착지 뒤 GitHub CI 확인

착지 thread는 로컬 검증과 main push를 끝내면 바로 보고한다. GitHub CI를 기다리지 않는다. host가 조건부 요청을 예약하고 **host 머신의 agent**가 그 머신의 기존 `gh` 로그인으로 실행한다. host 프로세스는 `gh`를 직접 실행하지 않는다. 실패를 분류하는 임무는 별도 상주 머신에서 대기하며, CI 입력이 있을 때만 깨어난다.

## 준비 확인

- host·조회 agent를 `ci.v1` 지원 버전으로 배포한다. 임무 머신에는 `mission.v1` 지원 agent와 권한 확인을 켠 저렴한 profile이 있어야 한다.
- 사람이 host 머신 터미널에서 `gh auth status --hostname github.com`으로 기존 로그인을 확인한다. 토큰 문자열이나 설정 파일은 보고·post에 복사하지 않는다. 기존 로그인은 `repo` 등 넓은 범위일 수 있다. 이 기능은 조회만 수행하며 토큰·키체인을 바꾸지 않는다.
- `project.toml`의 `[[repos]]`에 조회 머신의 checkout을 등록한다. 예시:

```toml
[[repos]]
path = "/work/aaa"
# machine을 생략하면 host 머신

[[repos]]
path = "/work/aaa"
machine = "mac-b" # 실패 분류 임무의 상주 머신
```

등록은 coordinator가 한다. 이 문서의 파일을 저장소 안에 둔 것만으로 운영 감시가 켜지지는 않는다.

## routine과 임무 등록

1. [routine 예시](examples/ci/ci-main.md)를 프로젝트 `routines/ci-main.md`에 복사한다. `repo`는 등록 경로나 그 마지막 이름이며, 조회 머신에서 하나로 찾아져야 한다. `branches`는 정확히 일치하는 branch 목록이다. thread branch·tag는 기본 대상에 넣지 않는다. `workflows`를 비우면 모든 push workflow, 채우면 이름 또는 workflow ID로 거른다.
2. [임무 예시](examples/ci/ci-watch.md)를 프로젝트 `missions/ci-watch.md`에 복사한다. 상주 머신·profile·repo를 운영 값으로 고친다. 예시의 `cheap`은 사람이 정의한 저렴한 Claude profile 이름이다. Codex profile을 택하면 기존 임무 계약대로 `reset_after_wakes = 1`로 바꾼다.
3. 사람이 aaa 세션 밖 터미널에서 `aaa mission show <project> ci-watch`로 범위·예산을 읽고 `aaa mission approve <project> ci-watch`를 실행한다. 물음에는 임무 이름 `ci-watch`를 그대로 친다. 그 뒤 `aaa mission start <project> ci-watch`로 시작한다. 임무 파일은 coordinator가 작성하고 사람이 승인하며, 임무 자신은 고치지 않는다.
4. 임무 없이 기록만 하려면 routine의 `mission`을 생략한다. AI 토큰을 쓰지 않는다. 적힌 임무가 아직 시작되지 않았다면 config-error와 전달 대기를 보존하고 조회는 계속한다.

조회 머신은 임무 머신과 독립이다. 기본은 host 머신이고, 다른 머신이 필요하면 host 설정의 아래 키 또는 routine의 `machine = "mac-a"`를 쓴다. 우선순위는 routine machine → host 설정 → host 머신이다.

```toml
[projects]
ci_poll_secs = 60     # 운영 하한 30초
ci_stuck_mins = 60
# ci_machine = "mac-a"
```

`gh api -i`의 HTTP 상태 줄을 읽으므로 304의 종료 코드 1은 실패가 아니다. 인증된 조건부 요청은 바뀐 내용이 없을 때 API 사용량을 아낀다. GitHub 응답 필드·쪽 넘김은 [workflow runs API](https://docs.github.com/en/rest/actions/workflow-runs#list-workflow-runs-for-a-repository)와 [workflow jobs API](https://docs.github.com/en/rest/actions/workflow-jobs#list-jobs-for-a-workflow-run-attempt)를 따른다.

## 판정과 처리

- main 등 지정 branch의 push만 본다. SHA의 모든 알려진 workflow가 끝나야 판정하며 취소는 제외한다. 재실행 성공은 `rerun_passed`로 표시한다. CI를 다시 실행해서 통과시켜 주는 기능은 없다.
- 초록은 흐름 사건 `ci_passed`로만 기록한다. 빨강은 inbox 종류 `ci`, 실패 job·단계, job당 32KiB·합계 128KiB 로그 꼬리와 커밋 범위의 주인 thread·보고 사본을 남기고 `ci_failed`로 기록한다. 본문·로그·보고는 전부 JSON 자료다.
- 착지 병합 첫 줄은 반드시 `병합: <요약> (t-NNNN)`으로 끝낸다. 한 push 안 여러 착지는 compare 결과에서 모은다. 번호를 못 찾으면 주인 불명과 커밋 SHA 목록을 남긴다.
- 같은 실패 job·단계 집합이면 앞 CI 항목에 SHA를 덧붙이고 깨우지 않는다. 집합이 바뀌면 새로운 진단 입력을 보낸다. 임무의 batch·min_gap·하루 상한 20은 기존 임무 규칙을 따른다.
- 다음 착지가 고치면 앞 항목을 닫고 `ci_fixed`와 `ci_passed`를 남긴다. 아직 전달하지 않은 임무 입력에는 `already_recovered`를 표시하며 회복 자체는 새 깨움을 만들지 않는다.
- 끝난 thread의 flow 이력에도 CI 결과를 남긴다. coordinator는 실패 원인을 확인한 뒤 **새 수리 thread**를 만든다. 임무는 원 thread를 되살리거나 다른 thread에 말하지 않으며 rerun·수정·push·revert하지 않는다. revert·착지 중단은 사용자/coordinator 결정이다.
- 오래 안 끝난 run은 `ci_stuck`. 기본 `events = ["failed"]`에는 stuck 깨움이 없고 기록만 한다. 필요하면 routine events에 `stuck`을 추가한다. `fixed`·`passed`를 적어도 초록은 깨우지 않는다.
- ETag·workflow 마지막 번호·판정 SHA·알림 표시·전달 대기는 DB meta에 저장한다. host 재시작·조회 단절 뒤에는 마지막 판정 전 10분부터 쪽을 넘겨 따라잡는다. 꺼진 동안 실패 후 회복했으면 이력만 기록하고 깨우지 않는다. 마지막 결과가 빨강이면 그것만 입력으로 보낸다.
- `gh` 실패가 `outage_secs`를 넘으면 CI 조회 outage 한 건, 회복하면 닫고 추적을 잇는다. 머신이 잠들거나 끊겨도 운영 설정·키체인을 자동으로 바꾸지 않는다.

## 운영 확인과 중단

`aaa routine list <project>`, `aaa mission show <project> ci-watch`, `aaa inbox list <project> --peek`, `aaa events <project>`에서 감시 설정·깨움·결과를 본다. 첫날에는 하루 깨움 상한과 GitHub 사용량을 확인한다. 임무 대기는 AI 토큰을 쓰지 않는다. 304는 inbox·flow를 만들지 않는다.

폴링을 끄려면 `aaa routine toggle <project> ci-main --off`, 임무만 멈추려면 `aaa mission pause <project> ci-watch`를 쓴다. 배포·운영 등록·사람 승인은 구현 검증과 별개다.

## L1 공용 watch와 저장 상태 호환

공용 `watch.rs`가 조회 실행 상태·첫 조회/단절 따라잡기 판단·outage·DB meta·outbox 전달을 맡고, `ci.rs`는 GitHub 조회와 CI 판정을 맡는다. 운영 파일·설정·승인 방법은 그대로다. 기존 `ci:` DB 문서는 같은 key와 최상위 필드로 읽고 쓴다. 업그레이드 전에 전달하지 못한 입력도 그대로 이어서 전달하며, 같은 `ci_input:` ID는 다시 넣지 않는다. 별도 변환·재승인·등록 작업은 없다.
