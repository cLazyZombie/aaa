# Rust doctest 중 SIGINT 시험의 준비 대기

macOS Intel CI의 Python unittest 105건 중 1건이 오류, 2건이 건너뜀이었다.
`python.log`의 오류는 `test_sigint_during_doctests_keeps_preceding_binary_failures`가
신호 전송 전에 `listener.accept()`에서 맞은 3초 시한이다. Python 전체는 185.590초에 끝났다.
`job.log`와 `test.log`는 Rust 실행까지 끝난 뒤 Python 원문을 출력했다.
같은 단계의 실제 Rust runner는 준비를 시작 약 1초 뒤 끝냈고, 226.49초에 doctest 자식을 시작했다.
실제 doctest는 29.31초에 종료 코드 0, Rust 전체도 종료 코드 0이었다.
[macOS Intel CI 원본](https://github.com/cLazyZombie/aaa/actions/runs/44925000885/job/136909039052)

Python 시험은 실제 Cargo 빌드 대신 가짜 Cargo 준비와 실패하는 binary 두 개를 실행하고,
가짜 Cargo의 `--doc` 자식에서 준비 소켓에 접속한다. 기존 시험은 러너 기동 직후부터
대상 자식의 접속까지 한 번에 3초를 쟀다. 실패한 시험의 임시 러너 출력·이벤트가
보존되지 않아 당시 어느 전환이 늦었는지, 자식이 먼저 끝났는지, 접속이 실패했는지는 확인할 수 없다.
실패 SHA `6ee5dfd9`와 그 직전 변경 `9d3ea7b4`·`d4acd227`은 가짜 러너와 시험의 실행 경로를 바꾸지 않았다.

이제 러너의 시작·완료 출력을 읽으며 대상 자식의 `START`를 기다린다. 해당 PID가
기록된 뒤에만 준비 소켓에 3초 시한을 적용한다. 접속한 자식의 PID와 시작 기록을 대조한 뒤
SIGINT를 보내고, 소켓 EOF와 요약의 `[7, 7, 130]`을 확인한다.
실패하면 러너 출력·접속 시도와 결과·실패 지점·종료 관측을 같은 단조 시계로 정렬한다.
정리 직전의 상태와 러너 사건도 `python.log`에 남긴다.

## 대조 실행

- 격리 사본에서 앞선 binary 두 개와 doctest 자식의 기동 직전에 각각 1.05초를 주입했다.
  기존 시험은 3.049초에 CI와 같은 `accept()` 오류로 끝났고, 수정한 시험은 같은 조건에서 통과했다.
  지연 주입은 시험 코드에 포함하지 않았다.
- 대상 자식 기동 직전 정체는 doctest 예약과 `START` 부재를, 조기 종료는 시작과 종료 코드 19를 남겼다.
  접속 거절은 접속 시도와 `ConnectionRefusedError` 및 자식 종료를 남겼다. 세 대조 시험 모두 실패로 판정됐다.
- macOS arm64의 Python 3.14.7에서 CI가 호출하는 모듈 17개의 시험 105건을 검증했다.
  103건이 통과했고 기존 플랫폼 조건에 따라 2건을 건너뛰었다.

Python 표준 `Queue.get(timeout=…)`으로 러너 출력을 받는다. pytest의 출력 대기·로그 보존과
CPython의 Popen 직접 자식 종료·회수 사례를 비교했다. 구현은 저장소의 기존 도구를 사용한다.
[Python queue](https://docs.python.org/3/library/queue.html#queue.Queue.get) ·
[pytester](https://github.com/pytest-dev/pytest/blob/main/src/_pytest/pytester.py)
([MIT](https://github.com/pytest-dev/pytest/blob/main/LICENSE)) ·
[CPython subprocess 시험](https://github.com/python/cpython/blob/main/Lib/test/test_subprocess.py)
([PSF License 2](https://github.com/python/cpython/blob/main/LICENSE))
