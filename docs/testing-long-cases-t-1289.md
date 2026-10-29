# 60초 경고 시험 7건 정리(t-1289)

## 범위와 기준

[t-1266 기록](testing-flaky-sweep-t-1266.md)의 rollout 3건, 개별 배포 1건,
계정 2건, 이미지 1건을 현재 main 기준으로 조사했다. 인접 이미지 복원도 함께 확인한다.
과거 경고의 횟수는 실행 시간이 아니다. 현재 단독 실행에서 60초 초과는 재현되지 않았다.
이미지·누출 검색에는 이미 t-1117·t-1206·t-1261의 가벼운 검색/준비/입력 대기가 들어 있다.

같은 작업 폴더의 verify 실행본을 빌드와 분리해 nice 15·시험당 한 프로세스로 실행했다.
계정 정책의 마지막 tick/정상 종료 울타리를 확인한 최종판도 부하가 내려간 뒤 별도로 측정했다.
표의 wall은 준비·계약·관측·자식 종료·실험실 삭제까지 포함한다. 각 전후 한 번의 관측이며,
전체 CI 부하나 모든 머신의 5초 충족을 뜻하지 않는다. 재시도·시한 연장·기대값 완화·ignore는 없다.

## 전후 시간

| 시험(m13의 전체 이름) | 전 → 후(초) | 처리 |
| --- | ---: | --- |
| `rollout::rollout_transfer_preflight_and_resume_keep_the_same_jobs` | 58.014 → 33.699 | 사본 심볼 표 제거·셸 입력 간격 제거 |
| `rollout::rollout_rejected_update_and_remote_recovery_keep_the_same_jobs` | 31.204 → 18.943 | 같은 사본과 셸 준비 축소 |
| `rollout::rollout_shared_host_agent_recovery_and_inflight_cancel_preserve_baselines` | 41.748 → 38.390 | 같은 준비 축소, 후반 비용은 남음 |
| `coordinator_deployment_is_gated_and_survives_backend_restarts` | 14.584 → 10.033 | 사본 심볼·안 쓰는 Git·셸 입력 간격 제거 |
| `accounts::account_credentials_parallel_refresh_and_all_records_are_redacted` | 5.144 → 4.917 | 판 조회에서 Python 대형 import 생략 |
| `accounts::host_login_defaults_pin_threads_and_coordinators_without_exposing_credentials` | 10.967 → 9.510 | 중복 로그인 종료 시험 통합·판 조회 import 축소 |
| `image::host_account_images_transfer_to_local_and_remote_threads_without_credentials_or_duplicates` | 3.074 → 3.040 | 현재 이미 5초 미만, 유지 |
| 인접 `image::host_and_thread_restart_preserve_generated_images_and_coordinator_scope` | 4.375 → 4.410 | 현재 이미 5초 미만, 유지 |

계정 전달의 0.227초 차이를 큰 개선으로 해석하지 않는다. 7건 합계는 164.735→118.532초로
28.0% 감소했다. rollout 3건과 개별 배포·계정 정책은 여전히 5초 이상이다.
60초 경고의 역사 항목을 지웠다거나 5초 정리를 마쳤다고 하지 않는다.

## 유지한 계약과 단순화

### 실제 배포 사본

`lab::deployment_binary`는 시험 소유 자식의 cp로 사본을 만들고 macOS에서만 strip·재서명한다.
실제 aaa 코드가 들어 있는 실행 파일이며 원본 빌드의 심볼·줄 정보는 보존한다. 다른 OS는 기존 복사다.
이 사본에서 모든 후보 서명 판을 만든다. 쓰기 FD를 부모가 열지 않아 ETXTBSY 방지 경계를 유지한다.
대략 210→114MiB로 줄었고 원본/사본의 `__TEXT,__text` 66,756,348바이트와 SHA-256이 같음을 확인했다.

다음 단언을 지우거나 느슨하게 하지 않았다.

- rollout 전송: 실제 TLS·여러 조각·조각별 디스크 동기화·부분 파일, 전송 중 같은 link의 입력/출력,
  host 재시작 뒤 같은 rollout/job ID·정확한 전송 바이트·원본 후보 제거 뒤 설치,
  사전 SHA 거부·변조한 원본의 재실행 금지, host 웹 준비·새 host/agent 신원·재시작 순서/횟수.
- 원격 복구: 설치 전 거부의 unchanged/감시 취소, 실패한 머신만 baseline 복구,
  agent 밖 guard 생존·위장 peer/직접 guard 실행 거부, host 재시작 뒤 같은 복구 ID,
  옛 프로세스를 성공으로 세지 않음·다음 단계 미실행.
- 공유 복구: 잘못 뜬 host, host와 로컬 agent의 공유 파일 복구, 진행 중 설치 철회와 늦은 rename,
  별도 host 재시작의 웹 준비, 원격 baseline과 서비스 호출 횟수.
- 개별 배포: 기본 거부·사람 승인·coordinator/worker peer PID 권한·자기 신고 거부,
  실제 update/rollback·설치 SHA/판과 실행 중 SHA 구분, 감독자 재시작 성공/실패·감시 밖 개별 재시작,
  승인 철회와 backend 재시작 뒤 같은 coordinator PID/기존 job 조회.

rollout·개별 배포의 PTY는 줄 단위 custom 셸이다. 기존 시험용 설정으로 brief settle와 submit gap을 0으로
주고 실제 prompt·완료 출력을 기다린다. TUI paste 간격의 검증 위치나 제품 기본값은 바꾸지 않는다.
개별 배포의 안 쓰는 Git 저장소만 생략한다.

### 계정 시험 통합

`host_login_defaults_pin_threads_and_coordinators_without_exposing_credentials`의 마지막
로그인 실패·취소·host 종료 구간을 삭제했다. 같은 파일의 기존
`parallel_host_logins_finish_independently_and_cancel_returns_after_cleanup`가 두 harness의
동시 진행·실패·취소·자손 종료·임시 인증 정리·정상 host 종료를 더 강하게 검사한다.
옮긴 응답 비노출(원문·8바이트 조각·인코딩), 실패/취소의 계정 미등록, 최초 waiting 단언도 그곳에 둔다.
마지막 누출 검색은 tick 완료·정상 host 종료를 지난 뒤 수행해 원래의 쓰기 완료 울타리도 보존한다.
새 시험은 추가하지 않았다. 기본 계정 고정·명시 계정 우선·깨진 기본 거부·로컬/원격 profile 변경·
worker 직접 재개·coordinator 갱신/복원/동시 재개·기록 홈 유지·파일/DB/로그 누출 검색은 원래 owner에 남긴다.

두 인증 fixture의 `--version` 결과·종료 코드는 같고 필요한 import를 그 분기 뒤로 옮겼다.
실제 시작·hook·소켓 gate·4개 병렬 refresh·회전 횟수·재개 기록·durable outbox·PTY 가림은 유지한다.
계정 owner 두 곳과 개별 배포에 선택 단계 계측을 추가해 준비/계약/누출 검색/정리를 나눠 남긴다.

### 이미지

생성·권한·기본 계정·같은 ID 재실행 금지·544KiB 조각 전달·바이트 보존·인증 정리와
host/thread 복원·coordinator 범위는 기존 owner 그대로다. 현행 두 시험이 모두 5초 미만이므로
역사 경고만을 근거로 더 쪼개거나 실제 전송을 모형으로 바꾸지 않았다.

## 남은 비용

전송 keeper의 실제 전송/사전 검사는 38.818→20.972초, 이후 host/agent 재시작은
11.100→6.522초다. 공유 복구의 마지막 묶음은 24.273→26.815초로 줄지 않았다.
계정 정책은 준비 1.319초·worker 계약 5.324초·coordinator 복원/재개와 마지막 tick/종료 2.784초·누출 검색 0.032초다.
이 고유한 실제 프로세스/설치 계약을 없애고 5초 미만이라고 세지 않는다.

## 검증

- 옮긴 미등록 계약의 결함 심기: host가 실패 로그인 뒤 계정 파일을 남기게 하면
  `parallel_host_logins_finish_independently_and_cancel_returns_after_cleanup`가 그 파일의 부재 단언으로
  exit 101을 냈다. 제품 소스는 원복했다. 임시 자료 정리·종료 응답을 받은 뒤 검사하는 기존 울타리를 유지한다.
- 먼저 실패 응답에 원시 출력을 넣은 대조는 통과했다. 공통 Front 응답 가림도 작동하므로 외부 누출 결함이
  되지 않았으며, 이 대조를 회귀 검출의 근거로 세지 않는다.
- 선택 측정 이후 같은 머신에서 다른 전체 검증이 시작돼 load 74.05를 관측했다.
  결함 심기 재빌드는 38.47초에서 126초로 늘었다. 이 부하 구간을 단독 성능 표에 섞지 않는다.
- 최종 m13 전체: 152 통과·기존 제외 1, 174.18초. 같은 실행본, `RUST_TEST_THREADS=4`, nice 15로
  다른 웹 검증과 겹쳐 실행했다. 대상/인접 owner의 전체 시간은 아래 표이며 선택 성능 표와 합치지 않는다.
- `deps` 2 통과, `cargo clippy --profile verify --locked -p aaa --tests -- -D warnings`,
  `cargo fmt --all --check`, 고정 sleep ratchet·diff 검사는 통과했다.
- `cargo clippy --profile verify --locked -p aaa --test m13 --test deps --target x86_64-unknown-linux-gnu -- -D warnings`
  통과. Linux 시험을 실행한 결과는 아니다. Windows에서는 원래부터 m13 전체가 제외되며 Windows 전용 코드는 바꾸지 않았다.
- 최종 공개 검사도 통과했다.

| 전체 m13 실행의 대상 | 초 |
| --- | ---: |
| rollout 전송 | 35.075 |
| rollout 원격 복구 | 19.296 |
| rollout 공유 복구 | 37.450 |
| 개별 배포 | 10.623 |
| 계정 전달/회전 | 10.528 |
| 계정 정책/복원 | 14.931 |
| 이미지 생성/전달 | 3.466 |
| 인접 이미지 복원 | 5.976 |

이 실행에서도 대상 7건에 60초 경고가 없었다. 계정과 인접 이미지의 5초 초과는 부하가 겹친 결과로
별도 보존하며 단독 표의 값을 바꾸거나 상한을 늘리지 않았다. 새 ignore·시험 삭제·E2E 번호는 없다.
전체 ci-local·배포/착지는 착지 창에서 수행한다.

## 참고

- [Tokio 시험 가상 시계](https://tokio.rs/tokio/topics/testing)
  ([MIT](https://github.com/tokio-rs/tokio/blob/master/LICENSE)): 같은 runtime의 시간 규칙에는 적합하지만
  이번 실제 자식·TLS·파일 전송을 빠르게 하는 대체 수단은 아니다. 해당 규칙의 기존 유닛 owner를 유지했다.
- [nextest 개별 slow 판정](https://nexte.st/docs/features/slow-tests/)
  ([MIT/Apache-2.0](https://github.com/nextest-rs/nextest/blob/main/README.md)):
  경고와 실제 완료 시간을 구별해 원래 case의 전체 시간을 기록하는 방식을 참고했다. runner·재시도는 도입하지 않았다.
- [Rust 파일 복사](https://doc.rust-lang.org/stable/std/fs/fn.copy.html)
  ([MIT](https://github.com/rust-lang/rust/blob/main/LICENSE-MIT)): 사본의 권한/플랫폼별 복사를 확인했다.
  외부 코드나 의존성은 가져오지 않았다.
