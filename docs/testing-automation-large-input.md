# 자동화 큰 입력과 접수 실패 기록(t-1238)

## 원인

`rc-verify`의 입력 커서는 152982에서 멈췄다. 바로 다음 `flow:152983`은 t-1120의
`report_ready`이며, 첨부 복사 누락 목록을 붙인 `detail`이 551,245바이트였다.
사건 시각은 2026-10-26 16:47:47 UTC다. 보고 본문 앞 64KiB 제한은 사건 설명에는 적용되지 않았다.
접수 전에 512KiB 검사가 오류를 반환하여 source 커서와 뒤의 사건이 계속 대기했다.
`flow:error`는 관측 기록에만 남고 실행 원장에는 들어가지 않았다.
t-1233의 최근 보고 사건도 그 커서 뒤에 있으며 직접적인 크기 초과 원인은 아니다.
운영 DB는 읽기 전용으로 조회했고, 설정·커서·thread·원본 파일은 바꾸지 않았다.

## 계약과 꼭 필요한 시험

- `oversized_flow_replays_after_preservation_failure_and_delivers_originals_once`: 실제 host·DB·agent frame owner에서
  한글·개행·따옴표·역슬래시가 섞인 큰 설명을 받는다. 원본 저장 자리를 막으면 실패 원장이 생기고 커서는 유지된다.
  저장 자리를 복구하고 host를 다시 열면 같은 입력 ID의 실패 이력을 남긴 채 접수·전달한다. 원본 SHA256과
  host/worker 파일 전체 바이트를 대조하며 새 thread와 열린 thread 모두 확인한다. 200건을 넘는 과거 사건 뒤
  놓친 사건도 다음 페이지에서 처리하고 완료 receipt가 있는 입력을 다시 보내지 않는다. `mission.v1`과
  `work_files.v1` 양쪽을 검사한다. 필요성: 이번 영구 접수 거부·뒤 사건 누락과 원격 원본 접근을 함께 지킨다.
- `oversized_input_above_transfer_limit_keeps_host_original_and_bounded_preview`: 8MiB 넘는 원본도 host에
  보존하면서 제한된 입력을 접수하고, 작업 폴더에 없는 원본을 `file=null`로 알린다. 필요성: 원본 이동 한도가
  새 영구 차단으로 바뀌지 않아야 한다.
- `admission_failures_recover_without_overwriting_accepted_runs`: 같은 접수 실패의 중복 이력을 만들지 않으며
  복구 접수에서 revision·source와 실패 이력을 보존한다. 이미 접수한 입력을 늦은 관측 오류로 덮지 않는다.
  필요성: 원장 보완이 실행 상태를 손상하지 않게 한다.
- `copy_notes_in_flow_keep_a_bounded_preview_and_the_total_count`: 많은 누락 경로를 새 흐름 사건에 통째로
  복사하지 않고 UTF-8 경계·전체 개수·상세 위치를 보존한다. 필요성: 이번 크기 초과 생산 지점을 제한한다.

입력 파일과 원본 전송을 검사하기 위해 기존 frame fixture의 파일 쓰기를 실제 임시 작업 폴더에 반영하고,
조각 offset·최종 hash를 검사한다. 기존 reuse 시험의 가상 `/lab` 경로도 자기 임시 폴더로 바꾼다.
고정 sleep·재시도·시한 연장·기대값 완화는 없다. thread 시작은 사건으로 기다린다.
이 frame fixture는 실제 harness의 brief echo를 흉내 내지 않으므로 이어 보내기는 brief 전달 완료 상태를 명시해 검사한다.

## 참고

[GitHub webhook payload cap](https://docs.github.com/en/webhooks/webhook-events-and-payloads)은 크기 초과로
사건 자체가 전달되지 않는 위험을 대조하는 자료다. [Buildkite artifacts](https://buildkite.com/docs/pipelines/configure/artifacts)의
큰 원본 별도 보존과 경로·무결성 hash 참조 방식을 따른다.
[Buildkite agent 라이선스](https://github.com/buildkite/agent/blob/main/LICENSE.txt)는 MIT다.
[GitHub Docs 라이선스](https://github.com/github/docs/blob/main/LICENSE)는 CC-BY-4.0이다.
외부 코드·의존성을 가져오지 않았다.

## 검증 결과

- `cargo test --profile verify -p aaa-host --lib`: 755 통과, 기존 제외 2건.
- 원본 보존을 빼고 옛 `check_input_size` 거부만 실행하도록 되심은 경로는 큰 흐름 사건 회귀에서 실패했다
  (0.69초, 저장 실패 원인 대신 크기 거부가 돌아옴). 수리 파일은 즉시 복원했다.
- 시험 준비의 첫 실패는 파일 저장을 하지 않던 frame fixture와 가상 `/lab` 경로였다. 실제 자기 임시 폴더에
  쓰고 조각 offset·hash를 확인하게 고쳤다. 이어 보내기에서 `unknown`이 보인 것은 fixture가 초기 idle을
  보내지 않아 host가 hook 없는 harness로 판정한 까닭이다. 실제 초기 idle 사건을 제공해 고쳤다.
- 전체 로컬 CI의 웹 유닛 651건, fmt, build, macOS clippy, Rust·Python 전체 시험과 웹 E2E가 통과했다.
  웹 E2E는 120개 파일을 분할 실행한 132묶음에서 실패 0건이다. Linux strict cross와 공개 검사도 통과했다.

전체 명령은 `NODE_PATH=<작업 폴더의 Playwright node_modules> nice -n 15 bash scripts/ci-local.sh
--only web,fmt,build,clippy,test,e2e,public,linux-clippy --logs target/ci-local/t-1238`이다.
기존 공용 Chromium·WebKit을 썼고 브라우저 설치나 시스템 설정 변경은 하지 않았다.
검증 로그는 `target/ci-local/t-1238/`에 있다.

| 단계 | 결과 | 시간 |
| --- | --- | ---: |
| 웹 빌드·유닛 651건 | 통과 | 9초 |
| fmt | 통과 | 2초 |
| 실행 파일·workspace 시험 빌드 | 통과 | 105초 |
| macOS clippy | 통과 | 86초 |
| 전체 Rust·Python 시험 | 통과 | 560초 |
| 웹 E2E | 통과 | 730초 |
| Linux strict cross clippy | 통과 | 29초 |
| 공개 검사 | 통과 | 2초 |

새 시험은 전체 Rust 시험이 끝난 뒤 같은 빌드의 실행 파일로 개별 시간을 쟀다.
웹 E2E가 함께 도는 조건에서 1분 load는 약 118.9였다. 모두 5초 미만이다.

| 시험 | 프로세스 전체 시간 | 결과 |
| --- | ---: | --- |
| 큰 흐름 사건·원본 저장 실패·재시작·뒤 페이지·중복 방지 | 3.291초 | 통과 |
| 8MiB 초과 원본 보존 | 2.062초 | 통과 |
| 접수 실패 원장 복구·기존 접수 보호 | 0.039초 | 통과 |
| 복사 누락 설명 제한 | 0.011초 | 통과 |

## 운영 복구

착지 창을 받기 전에는 운영 binary를 바꾸거나 규칙을 수동 실행하지 않는다.
반영 후 `automation runs rc-verify`의 접수 실패와 원래 `flow:*` 실행 ID,
`rc-verify` source 커서의 152983 통과와 t-1233 보고 사건의 처리 결과를 확인한다.
기존 5분 batch와 사용량 정책은 유지하므로 즉시 thread가 뜨는 것만으로 복구를 판정하지 않는다.
