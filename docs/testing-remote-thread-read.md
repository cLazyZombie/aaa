# 원격 thread의 host 자료 읽기 검증(t-1267)

## 계약과 구현

로컬 Front socket이 없거나 연결을 거부한 aaa 세션의 CLI만 hook → agent → 기존 host 연결로
읽기 요청을 전달한다. `thread_read.v1`을 양쪽이 광고할 때만 새 메시지를 보낸다.
세션 밖 CLI와 로컬 Front의 다른 오류는 이 경로로 우회하지 않는다.

허용 요청은 Projects·Threads·ThreadShow·ThreadSummary·ThreadLibrary·LibraryReports·LibraryOpen·
LibraryRead·RecordSearch·RecordOpen이다. agent는 hook token과 peer PID의 세션을 검사하고,
host는 세션이 요청한 머신 소속이며 실행 중인지 검사한다. host는 허용 목록을 다시 적용하고
Reader로 기존 Front dispatcher를 호출한다. 쓰기·승인·병합 요청은 전달 대상이 아니다.

`library cat`은 웹 파일 보기의 manifest·경로·symlink·불변 판 경계를 재사용한다.
64 KiB 조각으로 파일 bytes를 반환하며 이미 보관한 파일을 전달 중 다시 가리지 않는다.
보고·오류 JSON은 기존 가림을 적용한다. relay 응답은 Front 상한보다 1 KiB 작게 제한해
agent 메시지 외피를 더해도 frame 상한을 넘지 않는다. host 연결이 끊기면 응답 대기자를 정리한다.

## 참고한 구현

- [Tailscale LocalAPI](https://github.com/tailscale/tailscale/blob/main/ipn/localapi/localapi.go),
  [BSD-3-Clause](https://github.com/tailscale/tailscale/blob/main/LICENSE): 요청마다 읽기·쓰기 권한을
  나누는 방식을 참고했다. aaa는 별도의 읽기 허용 목록과 Reader 검사를 함께 적용한다.
- [Moby client](https://github.com/moby/moby/blob/master/client/client.go),
  [Apache-2.0](https://github.com/moby/moby/blob/master/LICENSE): transport와 API 요청을 분리하고
  상대 판을 협상하는 방식을 참고했다. aaa의 기존 hook·agent link·Front 계약을 재사용했다.

외부 코드는 복사하지 않았다.

## 자동 검증

| 명령 | 결과 | 필요한 이유 |
| --- | --- | --- |
| `cargo test --profile verify -p aaa-proto --lib --locked` | 50개 통과 | wire 호환·TS 정본 확인 |
| `cargo test --profile verify -p aaa-host --lib thread_read --locked` | 2개 통과, 0.43초 | 다른 머신·없는/끝난 세션·목록 밖 요청 거부, bytes 보존·큰 응답 오류 |
| `cargo test --profile verify -p aaa-agent --lib thread_read --locked` | 1개 통과 | hook 파일 bytes 보존과 오류 비밀 가림 |
| `cargo test --profile verify -p aaa --test m13 remote_thread_reads_host --locked` | 1개 통과 | 실제 CLI·hook·TLS·host 전체 전달과 권한 경계 |
| `cargo test --profile verify -p aaa --test deps --locked` | 2개 통과, 0.05초 | 역할별 crate 의존 규칙 |
| `cargo clippy --profile verify --workspace --all-targets --locked --keep-going -- -D warnings` | 통과 | 전체 대상 정적 검사 |
| `cargo fmt --all --check`·`git diff --check` | 통과 | 형식 검사 |
| `npm run typecheck` | 통과 | 생성한 Front 타입의 웹 호환 |
| `bash scripts/check-public.sh` | 통과 | 공개 저장소 검사 |

새 m13 시험은 격리된 원격 TLS agent의 thread 안에서 호출한다. host Front socket이 없음을
확인하고 thread 목록·상세·요약·library, r-ID 보고 본문·목록, 검색·원문과 150,001바이트 파일을
검사한다. 토큰 오류·다른 세션의 올바른 토큰으로 peer 위장·ticker·읽음 표시·thread stop 거부도
확인한다. 파일 조각 전체를 원본 bytes와 비교한다. 고정 sleep 없이 보고·출력 사건으로 끝낸다.
새 개별 시험은 모두 5초 안에 끝났다.

초기 검증에서 시험 fixture의 필수 title 누락, ThreadShow에 본문이 있다는 잘못된 단언,
agent 상태 폴더 한 단계 누락을 발견했다. 각각 CLI 계약에 맞는 title, LibraryOpen의 실제
보고 본문 검사, hook socket 기준 세션 경로로 수정했다. 실기 fixture의 검색 결과가 비었던
원인은 완료 표지 누락이었다. `[완료]` 보고로 고쳤다. 시한·재시도·합격 기준 완화로 넘기지 않았다.
정적 검사의 큰 enum 경고는 응답을 Box로 담아 해결했다. 최초 웹 타입 검사는 의존성 미설치로
실패해 이 worktree에서 `npm ci`한 뒤 검사했다.

## 실제 원격 Mac

별도 Mac의 aaa worker thread와 로컬 격리 host를 실제 agent TLS 연결로 연결했다.
같은 빌드 사본을 임시 폴더에 복사했고 기존 aaa 실행 파일·설정·서비스는 바꾸지 않았다.
SSH reverse forwarding은 격리 host의 loopback listener 연결에만 썼다.

- 원격 머신에 host Front socket과 원본 library 파일이 없음을 먼저 확인했다.
- 원격 thread의 `thread show`, `library show/list`, `search`, `record`가 host 자료를 반환했다.
- `library cat`으로 64 KiB를 넘는 한국어 문서를 받아 원본 SHA-256과 일치함을 확인했다.
- `thread stop`은 실패했고, 검증 뒤 자기 pane과 agent·host·tunnel을 종료했다.
- 양쪽 시험 폴더를 지웠고 운영 설치는 그대로다.

전체 ci-local·Linux cross·운영 배포는 이 후보에서 실행하지 않았다. 착지 창에서 통합 검증하며,
이 후보의 배포는 별도로 진행한다. 이름·t-ID로 여러 조각을 읽으면 갱신이 끼어들 수 있으므로
특정 판의 파일은 r-ID로 읽는다. host·agent 모두 `thread_read.v1`, 파일 읽기는 Front API 1.146이 필요하다.
