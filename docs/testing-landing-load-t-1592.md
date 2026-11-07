# 착지 gate를 어느 Mac에서도: 겹친 검증의 시험 결함(t-1592)

착지 전체 gate(`ci-local` 인자 없음)가 10 CPU Mac과 같은 머신의 겹친 검증에서 깨지던 원인을 찾아 고쳤다.
머신은 공개용 별칭으로 적는다: `mac-a`는 16 CPU, `mac-b`·`mac-c`는 10 CPU macOS다. 부하는 1분 평균이다.

## 재현 방법

같은 기준 SHA의 detached worktree 두 개를 한 머신에 두고 `nice -n 15 bash scripts/ci-local.sh`를 동시에 실행했다.
각 worktree는 자기 `target/`만 쓰고 Playwright 1.64.1 npm 패키지를 `NODE_PATH`로 가리켰다(브라우저는 공용 캐시).
다른 작업의 프로세스는 건드리지 않았다.

## 찾은 결함과 수리

| 시험 | 실패 | 원인 | 수리 |
| --- | --- | --- | --- |
| m13 `claude_thread_states_questions_and_messages` | 30초 안에 Idle 사건 없음 | 가짜 Claude가 큐 턴 사이 하위 프로세스(기록·hook)를 도는 동안 작업 표지도 입력 상자도 없는 화면을 남겼다. 부하에서 이 틈이 평범한 idle로 발표되고, 다음 턴의 짧은 Working은 300ms 감지 간격에 잡히지 않아 시험이 기다린 Idle 전이가 생기지 않았다 | 큐에 다음 말이 있으면 실제 Claude처럼 spinner를 이어 보이고, 다음 턴을 그리기 바로 앞에서 내린다 |
| aaa-host `safe_mode::…ephemeral_port_changed_address_and_disabled_web` | 닫은 옛 주소에 연결됨 | 「닫혔다」를 그 port로 새로 연결해 판정했다. 병렬 시험이 port를 다시 잡으면 성공한다. 「바뀐 주소」도 비워 둔 port 번호를 넘겨 같은 경합이 있었다 | resume 전에 옛 listener 대기열에 넣은 연결이 응답을 받는지·끊기는지로 판정. 바뀐 주소는 `[::1]:0` |
| `file_backup`, `deploy_start` | 5초 안에 host 준비 안 됨 | 다른 시험(30초)과 달리 host 시작을 5초 상한으로 기다렸다. 겹친 gate(load 128~154)에서 시작만으로 넘었다 | lab `wait_host_ready`: status 응답 또는 그 프로세스 끝남으로 끝난다. 상한은 공통 시작 상한 30초. deploy_start의 agent 연결·터미널 응답 상한도 같은 30초로 둔다(사건 대기, 프로세스 끝남 감지는 없다) |
| 웹 E2E 12 WebKit 목록 규모 | 20초 안에 reload 목록의 마지막 응답 없음 | E2E harness가 모든 host에 `AAA_TEST_EVENTS_PING_MS=1000`(3초 무응답이면 host가 끊음)을 걸었다. 이 값을 쓰던 「두 기기·죽은 연결」 단계는 t-1310에서 지워졌다. 부하 아래 늦게 답한 브라우저의 events 연결이 두 번 끊겨 목록을 처음부터 다시 받았다 | E2E는 제품 기본값(15초·45초). 끊기 계약은 `web::events_tests`가 가상 시각으로 본다 |
| aaa-host `machine_setup::…private_remote_temp_dir_and_a_mismatch_removes_it` | 어긋난 전송이 임시 폴더를 남김 | 원격(같은 Mac) `aaa-setup.*` 폴더를 내용으로 골랐는데, 같은 Mac의 다른 검증의 같은 시험도 같은 내용을 보냈다 | 실험실 경로를 가짜 실행 파일 내용에 넣는다 |
| m13 `instance::second_instance_has_its_own_folders_host_and_cookie` | 30초 안에 front.sock 없음 | `init`이 고른 포트(그 HOME 기준 첫 빈 쌍)로 host를 띄워, 겹친 다른 검증의 같은 시험과 같은 포트에 bind했다 | 고른 포트 단언은 두고 띄우는 host는 OS가 고른 포트를 쓴다 |

m13 Claude 시험은 fixture `record`에 dequeue 때만 1초 지연을 임시로 넣어 16 CPU 무부하에서도 결정적으로 실패시켰고,
수리 뒤 같은 지연에서 통과했다. 임시 지연은 커밋하지 않았다.

## 같은 머신의 실행 단계는 차례로

두 gate가 겹치면 각자 머신 전체 CPU 예산(10 CPU: Rust 6·브라우저 4)을 잡아 load가 130~150까지 올랐다. 시험을 고쳐도
이 부하에서는 다음 시험이 상한에 닿는 꼬리가 이어진다. `ci-local`의 test·e2e 단계는 같은 사용자의 머신 잠금을 얻은 뒤
실행한다([verification.md](verification.md#같은-머신에서-겹친-검증)). `scripts/test_ci_resources.py`의 `MachineLock`은
두 셸이 차례로 얻는지, 첫 셸이 끝나면 그 자식이 살아 있어도 풀리는지 실제 `bash`·`flock`으로 본다.

## 원인을 확정하지 못한 것

- 웹 E2E 09 Chromium 「한글 조합 위치 유지」: 겹친 gate(load 150)에서 한 번, 조합 사건 없이 줄 글자는 그대로인 채
  미리보기가 `visibility:hidden`으로 바뀌었다. 조합 상태는 살아 있어 `reset`은 아니다. 같은 머신에서 부하를 걸고 6회
  반복했지만 재현되지 않았다. 다음 실패에서 snapshot·state 도착과 대조하도록 산출물에 그 화면의 pane 연결 기록을 더했다.

## 측정

| 회차 | 머신 | 판 | 결과 |
| --- | --- | --- | --- |
| 1 | mac-b 겹침 둘 | 기준 main | 둘 다 실패: file_backup·deploy_start(둘 다), E2E 12(하나). load 128~154 |
| 2 | mac-b 겹침 둘 | 수리 1·2차 | 둘 다 실패: machine_setup(둘 다), E2E 09(하나). 1회차 실패는 없음 |
| 2 | mac-c 겹침 둘 | 수리 1·2차 | 하나 실패: instance. 다른 하나 통과 |
| 3 | mac-b 겹침 둘 | 수리 전부 + 머신 잠금 | 둘 다 통과. 뒤 gate는 잠금을 298초 기다렸다(그동안 다른 작업으로 load 50~65). 전체 493·726초 |
| 3 | mac-c 겹침 둘 | 수리 전부 + 머신 잠금 | 둘 다 통과. 뒤 gate는 잠금을 222초 기다렸다. 전체 311·579초 |
| 3 | mac-a 단독 | 수리 전부 + 머신 잠금 | 통과, 143초 |

3회차의 수리판과 최종 판의 차이는 lab 도우미의 변수 대입 꼴(clippy `unused_assignments`)뿐이며 동작은 같다.
