# Windows bulk-fixture 단위 시험의 준비 사건

Windows Server 2025 CI에서 Node 24.22.2로 실행한 bulk-fixture의 첫 시험이 5초 시한을 넘겨 5,704.9ms에 취소됐다. 같은 파일에서 SQLite를 열고 writer를 닫는 나머지 두 시험은 각각 55.5ms와 64.5ms에 끝났다. 원문에는 단계별 시각이 없어 자연 실패가 어느 호출에서 지연됐는지는 확인할 수 없다. [Windows CI 원문](https://github.com/cLazyZombie/aaa/actions/runs/44629108200/job/135930722256)

## 재현과 수정

- Windows 11의 Node 24.22.2로 기존 시험을 단독 실행하면 첫 시험은 약 78ms에 통과했다. 별도 프로세스가 실제 SQLite WAL 쓰기 잠금을 잡은 실험에서도 잠금 오류는 8.5ms, 자식 종료 사건은 3.9ms, 종료 뒤 다시 열어 쓰고 닫기까지는 8.9ms였다. 실패한 단위 시험의 종료 동작은 한 프로세스 안에서 실제 연결을 커밋하고 닫는다.
- 같은 Windows에서 실제 `fs.watch` 알림을 받아 두 준비 문구가 기록된 것을 확인한 뒤, 마지막 알림만 시험에 전달하지 않았다. 기존 시험은 5,017.3ms에 같은 5초 시간 초과로 취소됐다. 같은 계측을 켠 수정판에서는 세 시험이 통과했다. 이 대조로 로그가 이미 기록되어도 OS 알림 없이는 기존 시험이 끝나지 않는 경로를 확인했다.
- 가짜 host가 실제 로그 파일에 `웹 listen`과 `agent 연결`을 차례로 쓰고 시험용 watcher에 쓰기 사건을 바로 보낸다. 잠금 거부, 비동기 종료와 실제 SQLite 커밋, 두 로그가 모두 있어야 하는 조건, 부분 쓰기 롤백, 조기 종료 오류, 관측 해제를 검사한다. 재현 사본에서 웹 준비만으로 완료하게 바꾸면 agent 연결 전에 감시가 닫혀 순서 단언이 실패한다.
- 기존 시험의 5초 시한과 실제 E2E의 파일 감시 경로를 유지한다. Node의 시험별 mock 수명과 libuv의 Windows 파일 알림 구현을 참고했다. Win32는 캐시된 파일 크기와 마지막 쓰기 변경의 통지가 디스크 갱신에 따라 늦어질 수 있다고 명시한다. Node와 libuv는 MIT이며 외부 코드와 의존성을 추가하지 않았다. [Node 시험 mock](https://nodejs.org/docs/latest-v24.x/api/test.html#mockmethodobject-methodname-implementation-options), [Node 라이선스](https://github.com/nodejs/node/blob/v24.22.2/LICENSE), [libuv Windows 감시](https://github.com/libuv/libuv/blob/v1.x/src/win/fs-event.c), [libuv 라이선스](https://github.com/libuv/libuv/blob/v1.x/LICENSE), [Win32 변경 알림](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-readdirectorychangesw)

## 확인

- Windows 11의 Node 24.22.2에서 수정판의 세 관련 시험을 실행했다. 전체 웹 유닛 실행에서도 세 시험이 통과했고 취소는 0건이었다. 기본 로캘이 `ko-KR`인 실기에서는 별도 `automationChoices`의 모델명 정렬 단언 1건이 기준판과 수정판 모두에서 실패했다.
- macOS의 Node 24.22.3으로 웹 타입·번들·유닛 전체 746건을 통과했다. E2E 113은 실제 host와 agent를 띄운 Chromium·WebKit 작업 3건이 모두 통과했다. 공개 저장소 검사도 통과했다.
