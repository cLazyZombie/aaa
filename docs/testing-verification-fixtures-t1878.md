# 재시작·Rust 실행기 시험의 준비 경계 수리(t-1878)

## 조사 범위와 근거

질문 감시 후보의 첫 착지 전체 gate에서 재시작 owner 한 개와 Python 중단 처리 시험 다섯 개가 실패했다. 실패 원문은 작업 폴더의 `target/verification/landing-t1878/`에 보존했고, 그 창에서는 재시도·수리·push를 하지 않았다. 이후 원인 조사·수리 승인을 받아 아래 검사를 했다.

- 해당 재시작·실행기 소스는 당시 후보와 현재 main에서 같았다. 현재 main의 detached 작업 폴더를 이 작업의 `target/` 아래 만들고, 자기 target으로 같은 재시작 owner를 실행해 18.60초 통과를 확인했다. 미수정 Python 중단 시험 다섯 개도 별도 진단 실행에서 2.719초에 통과했다. 이 통과만으로 원래 실패를 무효 처리하지 않는다.
- 실패 당시 재시작 owner가 기록한 load 1분 값은 시작 27.39·종료 93.13이었다. agent 종료 직후 교체 image에 대한 macOS 무결성 검사 기록이 있고, 약 64.88초 뒤 같은 PID와 `aaa-restart-second`의 최초 실행 정책 판정이 기록됐다. 그 사이 새 image 로드 표지는 없었다. 거부가 확정돼 실행이 금지됐다거나 제품 exec 코드가 교착됐다고 단정하지 않는다.
- Python `process_tree()`는 조회 실패 때 목록 대신 `error` 객체를 반환한다. 기존 시험은 이를 항상 프로세스 목록으로 읽어 `TypeError`를 냈다. 조회 실패를 주입한 새 회귀에서 같은 오류를 0.280초에 재현했다. 기존 타임아웃 네 건의 임시 로그는 원래 시험 정리로 없어져 개별 ps 호출 상태까지 소급 확정할 수 없다. 전체 머신 조회를 신호 처리의 3초 검사와 함께 수행한 외부 부하 의존성은 코드에서 확인했다.

## 변경

`crates/aaa/tests/restart.rs`는 두 시험 worker image를 서명한 뒤 `--version`을 한 번 실행해 OS 최초 실행 판정을 준비 단계에서 마친다. 그 뒤 같은 교체 파일을 이동하고 실제 service restart를 요청한다. 새 준비 기록을 기다리는 30초와 다음 계약은 유지한다.

- 같은 worker PID·launcher 보존과 nonce 변경, 서로 다른 두 image SHA.
- 옛 PID·설치 승인 불일치 거부와 실제 재시작 신호 1회.
- 막힌 FDA 조회 중 재시작, 새 image의 FDA 재진입과 완료 파일 저장.
- 기존·새 pane의 실제 책임자, C·Rust·Cargo/build.rs 실행의 책임자 관측, launcher 바이트 보존과 분리 뒤 경고.

이 owner는 재시작 책임·세션 보존을 검사한다. 서명한 image를 처음 실행할 때 OS가 내리는 정책 판정의 소요를 재시작 준비 기한과 섞지 않는다. 제품 재시작·자가 점검·보안 정책은 변경하지 않았다.

`scripts/test_rust_tests.py`의 다섯 신호 owner는 실제 SIGINT/TERM·소켓 EOF·Popen 회수·외부 프로세스 생존을 그대로 검사한다. 진단용 ps만 fixture로 격리했다. 기존 3초, 130/143 종료 코드, TERM 무시 자식의 SIGKILL 회수, 대기 작업 미시작, 앞선 실패 보존 단언을 유지한다.

새 owner 두 개가 격리한 계약을 보존한다.

- `test_snapshot_lookup_failure_still_reaps_the_interrupted_child`: 진단 조회가 명시적으로 실패해도 오류 원문·직접 자식 PID·중단 상태가 보존되고 실제 자식은 회수된다. 기존 owner의 정상 목록 검사를 없애지 않고 실패 분기를 따로 검사한다.
- `test_process_snapshot_filters_descendants_and_preserves_lookup_errors`: root·자손만 남기고 다른 프로세스를 제외하며, 공백이 있는 실행 파일 이름과 조회 시한 오류를 보존한다. 인자·환경 대신 기존 comm 필드만 조회하는 경계를 확인한다.

기존 owner 삭제·skip 추가·기한 연장·재시도는 없다. 운영 실행기 소스·제품 코드·웹·프로토콜·Windows 전용 코드는 변경하지 않았다.

## 관련 검증

모든 실행은 `aaa verify -- nice -n 15 <명령>`으로 수행했다.

| owner | 결과 |
| --- | --- |
| `python3 -m unittest scripts.test_rust_tests` | 17개 통과, 3.727초 |
| 정규 gate의 Python suite 전체 | 91개 실행·기존 수동 2개 제외, 통과, 38.936초 |
| 실제 macOS 재시작 owner | 1개 통과, 17.51초 |

재시작 실행 명령은 `env TOKIO_WORKER_THREADS=2 RUST_TEST_THREADS=1 cargo test --profile verify --locked -p aaa --test restart -- --exact worker_exec_preserves_launcher_existing_pane_and_new_pane_responsibility`다.

`cargo clippy --profile verify --locked -p aaa --test restart -- -D warnings`도 통과했다. `cargo fmt --all -- --check`와 공개 저장소 검사도 통과했다. 이번 관련 검증을 최종 통합 SHA의 전체 gate 통과로 기록하지 않는다. 새 착지 창에서 현재 main과 통합하고 정규 전체 gate·양 OS strict cross를 수행한다.

## 참고 구현

- [CPython subprocess](https://docs.python.org/3.14/library/subprocess.html): 자식 종료·회수와 timeout 오류가 별개의 사건임을 대조했다. [PSF 등 라이선스](https://github.com/python/cpython/blob/3.14/LICENSE)를 확인했다.
- [Tokio Runtime 종료 규칙](https://docs.rs/tokio/latest/tokio/runtime/struct.Runtime.html#shutdown): blocking 작업과 runtime 종료의 관계를 대조했다. [MIT 라이선스](https://github.com/tokio-rs/tokio/blob/master/LICENSE)를 확인했다. 기존 자가 점검은 이미 전용 thread를 사용하며 이번에는 제품 runtime 정책을 변경하지 않았다.

외부 코드·새 의존성은 추가하지 않았다. OS 최초 실행 판정을 시험 준비에서 분리하는 변경은 실제 제품의 최초 실행 지연을 단축했다는 주장이 아니다.
