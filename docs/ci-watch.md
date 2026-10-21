# 착지 뒤 GitHub CI 확인

착지 thread는 로컬 검증과 main push를 끝내면 바로 보고한다. GitHub CI를 기다리지 않는다. host가 조건부 요청을 예약하고 **host 머신의 agent**가 그 머신의 기존 `gh` 로그인으로 실행한다. host 프로세스는 `gh`를 직접 실행하지 않는다. 실패가 오면 자동화 규칙이 보통 thread를 띄우고 열린 동안 입력을 모은다.

## 준비 확인

- host·조회 agent를 `ci.v1` 지원 버전으로 배포한다. 작업 머신에는 파일 전송을 지원하는 agent와 프로젝트 thread safety를 따르는 저렴한 profile이 있어야 한다.
- 사람이 host 머신 터미널에서 `gh auth status --hostname github.com`으로 기존 로그인을 확인한다. 토큰 문자열이나 설정 파일은 보고·post에 복사하지 않는다. 기존 로그인은 `repo` 등 넓은 범위일 수 있다. 이 기능은 조회만 수행하며 토큰·키체인을 바꾸지 않는다.
- `project.toml`의 `[[repos]]`에 조회 머신의 checkout을 등록한다. 예시:

```toml
[[repos]]
path = "/work/aaa"
# machine을 생략하면 host 머신

[[repos]]
path = "/work/aaa"
machine = "mac-b" # 실패 분류 작업 머신
```

등록은 coordinator가 한다. 이 문서의 파일을 저장소 안에 둔 것만으로 운영 감시가 켜지지는 않는다.

## 자동화 규칙 등록

`automations/ci-main.toml`에 조건 `[on.ci]`와 동작 `[do.start]`를 둔다. [규칙 예시](examples/ci/ci-main.toml)와 [brief 예시](examples/ci/ci-watch.md)를 프로젝트 자동화 폴더에 복사한다.
범위를 검토·승인하고 규칙을 켜면 실패가 올 때 보통 Worker thread를 띄운다. `reuse="open"`은 열린 thread에 이어 보내고, `memory=true`는 앞 판단을 다음 thread에 넘긴다.
`close_idle="30m"`은 새 완료·Idle·빈 입력을 함께 확인한 뒤 끝낸다. 기록만 필요하면 동작을 `do.inbox`로 둔다.
옛 파일은 자동 실행하지 않으며 `aaa automation convert`로 먼저 옮긴다.

조회 머신은 작업 머신과 독립이다. 기본은 host 머신이고, 다른 머신이 필요하면 host 설정의 아래 키 또는 routine의 `machine = "mac-a"`를 쓴다. 우선순위는 routine machine → host 설정 → host 머신이다.

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
- 같은 실패 job·단계 집합이고 job마다 실패한 시험도 같으면 앞 CI 항목에 SHA를 덧붙이고 깨우지 않는다. 실패 시험은 조회 agent가 잘라 내기 전 로그 전체에서 `test 이름 ... FAILED` 줄로 모으고, 없으면 panic 위치(`경로:줄`)·끝나지 않은 시험을 쓴다. job의 `failures`에 담겨 입력 자료로도 간다. 같은 job·단계의 다른 시험 실패, 집합이 바뀐 것은 새로운 진단 입력을 보낸다. 표지를 못 뽑은 job(옛 agent)은 job·단계만 비교한다. 규칙의 batch·사용량 대기와 명시한 선택 상한을 따른다. 횟수·시간·최소 간격은 기본 무제한이다.
- 다음 착지가 고치면 앞 항목을 닫고 `ci_fixed`와 `ci_passed`를 남긴다. 아직 전달하지 않은 규칙 입력을 취소하며 회복 자체는 새 깨움을 만들지 않는다.
- 끝난 thread의 flow 이력에도 CI 결과를 남긴다. coordinator는 실패 원인을 확인한 뒤 **새 수리 thread**를 만든다. 자동화 thread는 원 thread를 되살리거나 다른 thread에 말하지 않으며 rerun·수정·push·revert하지 않는다. revert·착지 중단은 사용자/coordinator 결정이다.
- 오래 안 끝난 run은 `ci_stuck`. 기본 `events = ["failed"]`에는 stuck 깨움이 없고 기록만 한다. 필요하면 routine events에 `stuck`을 추가한다. `fixed`·`passed`를 적어도 초록은 깨우지 않는다.
- ETag·workflow 마지막 번호·판정 SHA·알림 표시·전달 대기는 DB meta에 저장한다. host 재시작·조회 단절 뒤에는 마지막 판정 전 10분부터 쪽을 넘겨 따라잡는다. 꺼진 동안 실패 후 회복했으면 이력만 기록하고 깨우지 않는다. 마지막 결과가 빨강이면 그것만 입력으로 보낸다.
- `gh` 실패가 `outage_secs`를 넘으면 CI 조회 outage 한 건, 회복하면 닫고 추적을 잇는다. 머신이 잠들거나 끊겨도 운영 설정·키체인을 자동으로 바꾸지 않는다.

## 운영 확인과 중단

`aaa routine list <project>`, `aaa inbox list <project> --peek`, `aaa events <project>`에서 감시 설정·깨움·결과를 본다. 첫날에는 하루 깨움 상한과 GitHub 사용량을 확인한다. 입력이 없는 규칙은 AI 토큰을 쓰지 않는다. 304는 inbox·flow를 만들지 않는다.

규칙을 끄려면 `aaa routine toggle <project> ci-main --off`를 쓴다. 배포·운영 등록·사람 승인은 구현 검증과 별개다.

## L1 공용 watch와 저장 상태 호환

공용 `watch.rs`가 조회 실행 상태·첫 조회/단절 따라잡기 판단·outage·DB meta·outbox 전달을 맡고, `ci.rs`는 GitHub 조회와 CI 판정을 맡는다. 운영 파일·설정·승인 방법은 그대로다. 기존 `ci:` DB 문서는 같은 key와 최상위 필드로 읽고 쓴다. 업그레이드 전에 전달하지 못한 입력도 그대로 이어서 전달하며, 같은 `ci_input:` ID는 다시 넣지 않는다. 별도 변환·재승인·등록 작업은 없다.
