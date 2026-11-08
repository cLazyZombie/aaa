# 시험 host가 남긴 개발 모드 로컬 agent(t-1646)

Rust 시험이 끝난 뒤 `aaa agent --host local --dev-mode`가 부모 없이(PPID 1) 남아 재접속을 되풀이했다.
머신은 공개용 별칭으로 적는다: `mac-a`, `mac-b`, `mac-c`는 개발 Mac 세 대다.

## 출처

세 Mac에 남은 11개(`mac-a` 3, `mac-b` 7, `mac-c` 1)의 환경과 열린 파일을 읽기만 했다. 모두 실험실 node 이름이 `host`이고
`PATH` 맨 앞이 `<실험실>/tailscale-fixture`였다. 이 `PATH`는 m13
`web_publication::public_port_change_migrates_links_and_share_tokens_and_survives_host_restart`만 넣는다.
실험실 폴더는 시험 성공 뒤 지워졌고 agent는 지워진 `agent.log`를 연 채 남아 있었다.

## 원인

- host 설정 `[local_agent] mode`의 기본값은 `child`(개발 모드)다. 이 host는 준비 뒤 1.5초부터 로컬 머신이 연결돼 있지 않으면
  로컬 agent를 `setsid`로 분리해 띄운다. agent는 host가 없어도 재접속을 반복한다(host 재시작 동안 세션을 지키는 설계).
- 실험실은 자기가 띄운 `Child`(host)만 끝낸다. host가 띄운 분리된 agent는 실험실의 자식이 아니다.
- 실험실 host 설정 가운데 이 시험만 `mode = "none"`이 없었다(`[host]`가 있는 시험 설정 전수, 웹 E2E harness 포함).
- 첫 host가 준비 뒤 1.5초 넘게 살아 있을 때만 agent가 떠서 부하가 클수록 잘 샜다. 무부하 한 번(2.4초)은 새지 않았다.

## 재현과 수리

- 재현(커밋하지 않음): host 준비 뒤 host 로그의 「로컬 agent를 띄웠다」를 기다리는 줄을 시험에 넣자 시험은 통과하고
  그 worktree 실행 파일의 agent가 PPID 1로 남았다. 그 PID 하나만 끝냈다.
- 수리: `Lab::node`가 `[local_agent]`가 없는 host.toml에 `mode = "none"`을 넣는다(`[projects] root` 기본값과 같은 방식).
  적은 모드는 그대로 둔다. 제품 동작(분리된 agent·재접속)은 바꾸지 않았다.
- 확인(커밋하지 않음): host 준비 뒤 `status`의 `dev_mode`가 false라는 단언을 넣어 수리 뒤 통과, 수리를 되돌리면 실패했다.
  수리 뒤 시험이 끝나도 agent가 남지 않았다.
- 회귀 시험: m13 `lab_host_without_local_agent_mode_starts_no_detached_agent`가 실제 설정 loader로 실험실 host.toml을 읽어
  적지 않은 모드는 `none`, 적은 `child`는 그대로인지 본다. 프로세스를 띄우지 않는다. 수리를 되돌리면 실패한다.

## 검증 중 만난 간헐 실패

- m13 `thread_park::park_moves_between_machines_through_the_pushed_branch`가 owner 검증에서 한 번 실패했다:
  resolve가 「마지막 사본이 실패해 resolve하지 않는다: sub/d.txt: 받는 동안 바뀌었다」로 거부됐다.
- 원인: 앞 머신으로 돌아온 brief가 배달되자마자 시험이 resolve했다. 가짜 agent는 그 brief를 처리하며 library 파일을
  다시 쓰는 중이었고(로그에서 brief 입력 수락 직후 resolve), host의 마지막 사본이 그 변화를 잡았다. 제품의 거부는 맞는 동작이다.
- 수리: 새 대화 pane에서 가짜 agent의 처리 끝 표지 `done`을 기다린 뒤 resolve한다. 시한·기대값은 그대로다.

## 남은 것

- host.toml 없이 만든 node(`lab.node(name, None)`)에서 `host`를 띄우면 여전히 개발 모드다. 지금 그런 시험은 없다.
  개발 모드를 일부러 쓰는 시험은 그 agent를 직접 끝낸다(`crates/aaa/tests/lab/mod.rs` 머리 주석).
- 이미 남은 고아는 각 머신에서 PID와 환경을 확인한 뒤 하나씩 끝낸다.
