# 머신 배정: 독점 예약·착지·비중

host가 각 머신의 사용을 통제한다. coordinator가 말이나 memory 규칙으로 머신을 비우거나 미루지 않는다(t-1265).

## 자동 배정 차례

머신을 정하지 않은 시작(할 일 예약 큐, `thread start --from-task`에서 머신 칸이 빈 할 일, `machines`를 비운 자동화)은 아래 차례로 머신을 고른다.

1. 연결돼 있고 점검 이상이 없는 머신
2. 그 일의 workspace(저장소 이름 또는 작업 폴더) 자동 배정을 사람이 허용한 머신([workspace 허용](workspace-access.md))
3. 다른 일의 독점 예약이 걸리지 않은 머신
4. 착지 중이 아닌 머신: 보고 첫 줄이 `[착지]`인 thread가 지금 쓰는 머신이다. 착지는 host 머신 하나에 묶이지 않고 그 thread의 작업 머신에서 할 수 있으므로 thread 머신을 본다.
5. 열린 thread÷비중이 작은 머신, 같으면 1분 부하가 낮은 머신, 그다음 이름

후보가 없으면 시작하지 않고 기다린다. 할 일 예약 큐의 대기 이유와 자동화 `routine runs`에는 「뺀 머신: mac-a 독점 예약(d-0001 · 빌드 측정), mac-b 착지 중(t-0002)」처럼 뺀 까닭이 남는다.

머신을 지정한 시작(할 일 owner `모델@머신`, 자동화 `machines` 목록, `--machine`)에는 착지 중 제외와 비중을 쓰지 않는다. 독점 예약은 지정한 시작도 막는다(아래).

## 독점 예약

머신 하나에 주인 하나를 건다. 주인은 `t-NNNN` thread나 할 일(`d-NNNN`)이다.

- 주인이 아닌 시작은 그 머신에 자동 배정하지 않는다. 그 머신을 지정한 시작은 예약이 풀릴 때까지 기다린다: 할 일 예약 큐는 `자리 대기`로, 자동화는 입력을 보존한 채 대기하고, 직접 `thread start`는 `start_wait: mac-a 독점 예약(…) 풀림 대기`로 거부한다.
- 주인이 할 일이면 그 할 일에서 띄운 thread는 그 머신에 들어간다. 주인이 thread면 새 시작은 모두 막는다(그 thread가 머신을 혼자 쓴다).
- 저절로 풀림: 끝 시각이 지났을 때, 주인 thread가 끝났을 때(Resolved·Failed·기록 없음, 보류는 유지), 주인 할 일이 활성 목록에서 빠졌을 때. 읽는 쪽이 풀고 감사 기록(`machine_release`, actor `host`)에 까닭을 남긴다.
- 이미 시작한 thread는 그대로 둔다. 예약은 새 시작만 막는다.
- 판정은 시작 직전과 DB 선점 트랜잭션 안에서 두 번 본다. 판정 뒤에 걸린 예약도 지킨다.

| 누가 | 걸기·풀기 | 방법 |
| --- | --- | --- |
| 사용자 | 아무 머신·주인 | 웹 기기 정보 「자동 배정」, 세션 밖 `aaa machine reserve|release` |
| coordinator | 사용자가 말했을 때만 | `--user-request '<사용자 지시>'`를 함께 준다 |
| thread | 자기 머신을 자기 이름으로만 | thread 세션 안 `aaa machine reserve <자기 머신>`·`release`(post 길이라 원격 머신 thread도 된다) |

```sh
aaa machine reserve mac-b --holder d-0486 --note "빌드 성능 실측" --for 1d   # 사용자
aaa machine reserve mac-b --holder d-0486 --user-request "mac-b는 d-0486 전용"   # coordinator
aaa machine reserve mac-b --note "타이밍 측정"   # 그 머신의 thread 자신
aaa machine release mac-b
aaa machine list   # 비중과 예약 주인·까닭·끝 시각
```

웹 왼쪽 머신 목록은 예약된 머신의 모니터 아이콘 오른쪽 아래에 자물쇠를 달고, 이름 아래에 「독점 d-0486 빌드 성능 실측 · 18:00까지」처럼 주인·까닭·끝 시각을 쓴다(끝 시각이 오늘이 아니면 날짜도, 없으면 「풀 때까지」). 예약된 머신이 하나라도 있으면 왼쪽 막대의 머신 단추 왼쪽 아래에도 자물쇠가 붙는다. 건 쪽·프로젝트는 기기 정보 「자동 배정」에서 본다.

다른 주인의 예약이 있으면 거는 요청은 `conflict`다. 먼저 푼다. 같은 주인이면 까닭·끝 시각을 고친다.
예약을 무시하고 시작하는 것은 사용자, 또는 사용자 지시를 인용한 coordinator의 `thread start --ignore-reservation [--user-request …]`뿐이다.

## 비중

자동 배정 몫이다. 기본은 agent 건강 보고의 CPU 수(논리 코어)이고 사람이 머신마다 숫자로 바꾼다(웹 기기 정보 또는 세션 밖 `aaa machine weight <머신> <N|auto>`). 0이면 자동 배정에서 뺀다(지정한 시작은 받는다). 24코어 머신에 열린 thread 3개(0.125)는 8코어 머신에 2개(0.25)보다 먼저 고른다. 비중은 동시 thread 상한이 아니다. 상한은 프로젝트 `max_parallel_threads`다.

## 자동화

`do.start`의 `machines`를 비우거나 생략하면 위 자동 배정 차례로 고른다. workspace는 `repo`의 이름(없으면 작업 폴더)이다. 고정 목록을 쓴 규칙도 독점 예약·착지 중 머신은 후보에서 뺀다. 자동 시작 범위 승인은 「자동: <workspace> 자동 배정을 허용한 머신」으로 묶인다. 후보 머신 집합은 사람만 바꾸는 workspace 허용이 정하므로, 머신을 더하는 것은 웹 기기 정보의 허용 켜기다. 고정 목록 규칙을 자동으로 바꾸면 범위가 바뀌어 다시 승인한다. 고정 목록 규칙의 승인 hash는 그대로다.

ci-watch 같은 운영 자동화는 `machines`를 지우고 쓴다([예시](examples/ci/ci-main.toml)). 조회 머신(`on.ci.machine`·`ci_machine`)은 작업 머신과 별개다.

## 참고

개념만 따르고 코드는 가져오지 않았다.

- Kubernetes [cordon·taint](https://kubernetes.io/docs/concepts/scheduling-eviction/taint-and-toleration/)(Apache-2.0)와 Nomad [node eligibility·drain](https://developer.hashicorp.com/nomad/docs/commands/node/eligibility)(BUSL-1.1): 머신마다 새 배정을 받지 않는 표지와 그 주인. 이미 도는 일은 그대로 둔다.
- Slurm [reservation](https://slurm.schedmd.com/reservations.html)(GPL-2.0): 주인·기간이 있는 예약, 주인 일만 들어감.
- Jenkins executors·GitHub Actions [runner labels](https://docs.github.com/en/actions/hosting-your-own-runners/managing-self-hosted-runners/using-labels-with-self-hosted-runners)(MIT runner): 머신 능력만큼의 몫, 라벨로 후보 거르기(aaa는 workspace 허용이 라벨 역할).
