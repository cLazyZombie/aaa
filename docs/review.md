# 다른 모델 검수

사용자가 다른 검수 profile을 명시했을 때 `aaa review`를 부른다. 프로젝트의 등록 저장소에서 작업 중인 thread가 자기 후보 커밋을 맡길 수 있다. profile 목록의 Codex·Claude 모델을 쓰며 계정은 host가 고른다.

## 실행과 결과

thread의 작업 사본에서 후보를 커밋한 뒤 실행한다. `--base`에 기준 커밋을 준다. `--commit`을 생략하면 현재 HEAD를 쓴다.

```sh
aaa --json review start --profile <profile> --base <sha>
aaa review status <ID> --wait
aaa review fetch <ID>
aaa review cancel <ID>
```

start는 작업 ID와 상태를 돌려준다. status는 `--wait`를 주면 계정 배정 또는 실행 결과가 나올 때 돌아온다. fetch의 JSON에는 기준·후보·tree와 함께 실제 모델, 파일·줄, 심각도(P0–P3), 근거, 실패 경로, 확신, 확인 방법이 들어간다. 현재 HEAD가 달라지거나 미커밋 변경이 생기면 `status`에 「이전 판」, JSON에는 `earlier: true`가 표시된다. 현재 후보와 일치하는 결과만 그 후보의 검수 근거로 판단한다.

| 상태 | 뜻 |
| --- | --- |
| `waiting` | host 계정 배정 또는 agent 연결 대기. 필요할 때 같은 인자와 `--id <ID>`를 주고 다시 판정한다. |
| `queued` · `running` | host 계정 배정 중 또는 thread 머신에서 실행 중. |
| `done` | 결과 JSON을 받을 수 있다. |
| `failed` · `unknown` | 실행 실패 또는 실행 뒤 응답 유실. 상태에서 원인을 확인한다. |
| `cancelled` | 시작 전 취소했거나 실행기 종료를 확인했다. |

작업 ID는 같은 요청의 중복 실행을 막는다. 실패하거나 결과가 불명인 실행은 자동으로 반복하지 않는다. 실행 중 취소는 agent에 종료를 요청하고, 종료를 확인한 뒤 상태를 바꾼다. 상태·결과·취소는 요청한 thread에서만 접근할 수 있고 흐름 사건에는 ID와 진행 상태가 남는다.

host는 기존 계정 풀·기본 계정을 쓰고 이미지 작업과 같은 small(5%p) 예약을 기록한다. thread 머신의 임시 detached 작업 사본에서 커밋된 diff를 검수한다. Codex 실행기는 읽기 전용 설정으로 diff와 주변 문맥을 받는다. Claude 실행기는 plan 모드로 같은 입력을 받고 사본에서 파일을 읽을 수 있다. 두 실행기 모두 전용 홈과 host 계정으로 한 번 실행한다.

## finish 연동

다른 검수 profile을 명시적으로 받은 경우에만 첫 독립 리뷰 라운드에서 `aaa review start --profile <p> --base <sha>`를 한 번 호출한다. `status --wait`로 결과를 기다리고 `fetch`의 finding을 추가 리뷰어의 지적으로 판정한다. 커밋된 후보를 고정할 수 없거나 배정·인증·실행·결과 확인에 실패하면 「다른 모델 검수 누락: 원인」을 보고하고 자기 모델 리뷰와 필수 gate를 진행한다.
