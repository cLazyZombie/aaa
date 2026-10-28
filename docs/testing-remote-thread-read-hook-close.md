# 원격 읽기 Python fixture의 peer 종료 경합 수리(t-1271)

## 원인과 분류

GitHub CI main `c478e5c1`의 [macOS job](https://github.com/cLazyZombie/aaa/actions/runs/40708195052/job/122885936942)은
`thread_read::remote_thread_reads_host_reports_library_and_search_without_write_access`에서 실패했다.
인계된 요약은 Python traceback 첫 줄만 포함했지만 공개 check-run annotation에 전체 traceback이 있었다.
다른 세션의 올바른 토큰으로 peer를 위장하는 요청에서 Python `socket.shutdown(SHUT_WR)`가
`OSError: [Errno 57] Socket is not connected`로 실패했다. 권한 거부 단언이나 원격 읽기 응답의 실패가 아니다.

agent는 완성된 JSON 요청을 읽으면 EOF를 기다리지 않고 응답하고 연결을 닫는다.
macOS에서는 서버가 먼저 닫은 뒤 클라이언트가 쓰기 반쪽을 닫으면 ENOTCONN이 발생하지만,
이미 도착한 응답은 계속 읽을 수 있다. 제품 `aaa-agent::hooks::exchange`는 이미 이 경우를 처리한다.
Python fixture에만 같은 처리가 빠져 있었다. macOS에서 순서에 따라 발생하는 fixture 결함으로 분류한다.

추가로 t-1272가 분류한 main `48c3d067`의 [macOS job](https://github.com/cLazyZombie/aaa/actions/runs/40713234115/job/122903862178)도
전체 annotation을 직접 확인했다(48 통과·1 실패). 두 실패는 다음과 같이 같은 원인으로 연결된다.

| main | 요청 | 실패 연산·errno |
| --- | --- | --- |
| `c478e5c1` | 다른 세션의 올바른 토큰으로 peer 위장 | Python hook 31행 shutdown, ENOTCONN(57) |
| `48c3d067` | 잘못된 토큰 | Python hook 31행 shutdown, ENOTCONN(57) |

서로 다른 거부 요청에서 같은 helper의 같은 shutdown이 먼저 닫힌 peer를 만나 실패했다.
두 사례 모두 원래 기대한 거부 응답 내용을 검사하기 전에 예외가 났다. 단순히 같은 traceback 머리라는
이유로 묶은 것이 아니며, 제품의 토큰·peer 검사 결함이나 새로운 원인으로 분류하지 않는다.

## 수리와 결정적 회귀

Python hook 교환 코드를 공통 문자열로 분리해 실기 probe와 회귀 시험이 같은 함수를 실행하게 했다.
shutdown의 ENOTCONN만 허용한 뒤 원래 응답을 읽는다. 다른 errno는 그대로 올린다.
토큰·peer PID·요청 허용 목록·읽기 전용 권한·파일 바이트와 거부 단언은 그대로다.

새 `python_hook_reads_closed_peer_reply_without_hiding_other_errors`는 실제 Unix socket pair를 쓴다.
peer가 요청 전체를 읽고 응답한 뒤 socket을 닫았다는 Event를 보내야 클라이언트가 shutdown한다.
정상 응답·거부 응답·빈 응답의 원문 보존과 다른 shutdown 오류(EIO)의 전파를 확인한다.
시한은 사건 누락 때 멈추는 상한이며 sleep·재시도·시한 증가·기대값 완화는 없다.
이 시험은 제품 Rust 회귀만으로 지킬 수 없던 Python fixture의 실제 IPC 순서를 보호하므로 필요하다.

## 수리 전 증거

- 원본 Python hook 코드를 추출하고 peer 종료 Event 뒤 shutdown하도록 고정한 단일 실행은
  macOS 오류 57로 실패했다. CI traceback과 같은 연산·errno다.
- 수정하지 않은 원래 통합 시험의 단일 실행은 통과했다(6.18초).
  동시에 다른 작업의 전체 검증이 진행 중이었고 당시 1분 load는 14.24였다.
  이 통과는 간헐 결함의 부재를 뜻하지 않으며, 평상시 5초 기준의 합격 근거로 쓰지 않는다.
- 증거와 전체 CI annotation은 t-1271 보고의 library에 보관했다.

## 수리 후 검증

main `2928129f`에 rebase한 뒤 `verify` profile로 검증했다.

| 검사 | 결과 |
| --- | --- |
| 수리 전 helper를 새 결정적 회귀에 대입 | 실제 ENOTCONN(57)로 실패 |
| m13 `thread_read::` 두 시험, 순차·개별 시간 기록 | 2개 통과 |
| Python peer 종료 회귀 | 0.029초 |
| 실제 원격 읽기·권한 경계 통합 시험 | 2.108초 |
| m13 전체, 2개 병렬·개별 시간 기록 | 152 통과·0 실패·기존 CI 전용 1개 ignored, 전체 327.79초 |
| 전체 m13 안의 Python 회귀·원격 읽기 | 0.029초·2.053초 |
| agent·host `--lib thread_read` | 3개 통과, 각 0.005~0.630초 |
| 제품 hook `closed_peer_reply_survives_write_shutdown` | 1개 통과, 0.003초 |
| `cargo test --profile verify -p aaa --test deps --locked` | 2개 통과, 각 0.000~0.015초 |
| `cargo clippy --profile verify -p aaa --test m13 --locked -- -D warnings` | 통과 |
| `cargo fmt --all --check`·`git diff --check` | 통과 |
| `bash scripts/check-public.sh` | 통과 |

두 시험은 순차 실행과 평상시 2개 병렬 m13 실행에서 모두 개별 5초 이내다.
전체 m13에서 기존 다른 시험 28개는 5초를 넘었다. 이번 fixture 수리의 대상과 구분하며,
이름·개별 시간은 보고 library의 `expanded-m13-summary.json`에 기록했다.
agent·host의 원격 읽기 권한·응답 경계, 제품 hook과 crate 의존 검사 및 변경 대상의 정적 검증도 모두 통과했다.

개별 시간은 저장소 runner와 같은 libtest `--report-time`으로 기록했다. 주요 실행 명령은 다음과 같다
(무거운 Cargo 실행에는 `nice -n 15`를 적용했다).

```sh
RUSTC_BOOTSTRAP=1 cargo test --profile verify -p aaa --test m13 thread_read:: --locked -- -Z unstable-options --report-time --test-threads=1
RUSTC_BOOTSTRAP=1 cargo test --profile verify -p aaa --test m13 --locked -- -Z unstable-options --report-time --test-threads=2
RUSTC_BOOTSTRAP=1 cargo test --profile verify -p aaa-agent -p aaa-host --lib thread_read --locked -- -Z unstable-options --report-time
RUSTC_BOOTSTRAP=1 cargo test --profile verify -p aaa-agent --lib closed_peer_reply_survives_write_shutdown --locked -- -Z unstable-options --report-time
```

시험 fixture와 문서만 바꿨다. 전체 ci-local·웹 E2E·cross·production 빌드·운영 배포와
수정 판의 GitHub CI 실행은 이 후보에서 하지 않았다.

## 참고한 오픈소스

- [CPython script_helper](https://github.com/python/cpython/blob/main/Lib/test/support/script_helper.py),
  [PSF-2.0](https://github.com/python/cpython/blob/main/LICENSE): 자식 종료 코드와 오류 원문을
  함께 보존하는 진단 방식을 참고했다.
- [Rust libtest pretty formatter](https://github.com/rust-lang/rust/blob/main/library/test/src/formatters/pretty.rs),
  [MIT](https://github.com/rust-lang/rust/blob/main/LICENSE-MIT): 실패한 시험의 본문을 보존하는
  출력을 확인했다. CI의 첫 줄 요약만으로 원인을 판단하지 않고 원래 annotation을 확보했다.

외부 코드는 복사하지 않았다. 전송 수리는 기존 제품 hook의 오류 경계를 따른다.
