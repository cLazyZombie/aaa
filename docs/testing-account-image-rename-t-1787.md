# Codex 계정 이름 변경의 이미지 차단 확인·정리(t-1787)

## 원인과 수리

계정 이름 변경은 이미지 원장의 `running | unknown`을 함께 차단했다. 이미지 RPC는 요청 전송 후 최종 응답을 얻지 못하면
로컬 자식을 회수한 뒤에도 결과를 `unknown`으로 보존한다. host 재시작도 이미 전송한 요청을 다시 생성하지 않고
결과 불명으로 남긴다. 기존 CLI의 `forget`은 결과 불명 정리를 거부했고 웹에는 차단 작업 확인 경로가 없었다.
이 때문에 로컬 실행이 끝난 과거 결과 불명 기록만으로도 이름 변경이 계속 막혔다.

FRONT_API 1.169의 `AccountImageJobs`는 계정별 실행 중·결과 불명 작업의 ID·프로젝트·소속·계정 배정 시각·짧은 이유를 보낸다.
프롬프트·인증·머신 경로는 보내지 않는다. `AccountImageDismiss`는 사람이 확인한 결과 불명만 `forgotten`으로 표시한다.
파일·중복 방지 영수증·사용량 예약·인증 판·로그인 세대는 그대로다. 같은 ID는 다시 생성하지 않는다.
실제 실행 중인 이미지의 인증 갱신 프록시는 실행 당시 계정 이름을 사용하므로 계속 보호하고, 오류에 해당 ID와 완료 뒤
이름 변경 가능 상태를 표시한다. 결과 불명 오류는 확인 후 정리 경로를 알린다.

웹 조회는 마지막 요청만 반영한다. 독립 리뷰에서 찾은 옛 응답 역전 문제를 수리하여 정리 후 늦게 온 결과 불명 행이 되살아나지 않는다.
사람이 승인한 화면의 배치·문구는 유지했다. 계정 목록 사건·응답·입력·실행 상태로 대기하며 고정 sleep은 추가하지 않았다.

## 회귀 owner

- `aaa-host image::tests::account_image_*` 3건: Codex 금고 판 3, 차단 ID·이유, 조회의 비노출, 사람 외 신원의 조회·정리 거부와
  원장 보존, 실행 중·계정 불일치·잘못된 ID 거부, 확인 정리 후 파일·예약·인증 보존, 기본 계정과 이미지 참조 이전,
  같은 ID의 Begin/Start 재생성 방지, host 복구 뒤 사람 확인·동시 정리 직렬화.
- `m13 accounts::renamed_running_account_refreshes_and_resumes_without_rebinding_to_reused_name`: Codex 판 3으로 시작하여 미사용·사용 중
  이름 변경, 기존 표시 이름·CODEX_HOME·실제 갱신·재개, 옛 별칭 재등록과 host 재시작 후 인증 결속을 검사한다.
- `m13 accounts::renamed_codex_default_auto_assignment_and_relogin_use_the_new_name`: 이름 변경 뒤 새 기본 계정 실행과
  새 이름에서 같은 메일의 공식 로그인 fixture, 새 인증 세대로 자동 배정한 실제 자식의 인증을 검사한다.
- `web/e2e/specs/134-account-image-jobs.mjs`: Chromium PC·WebKit PC/휴대폰에서 Codex 판 3과 열린 thread를 두고
  결과 불명·생성 중 항목, 정리 전 확인/취소, 정리 후 예약 원문 보존, 생성 중 거부·완료 후 이름 변경,
  기본·자동 배정 풀·기존/새 thread 표시 이름을 실제 host로 확인한다.
  옛 `account_image_jobs` 응답을 실제 host에서 받은 뒤 보류하고, 정리 후 최신 목록을 먼저 반영한 다음 옛 응답을 풀어
  행이 되살아나지 않는지 확인한다. 실패 시한·단언을 완화하지 않는다.

## 결함 주입 대조

마지막 조회만 반영하는 guard를 제거한 변이는 E2E 134의 첫 Chromium 단계에서
‘늦은 조회가 정리한 행을 되살리지 않는다’ 단언(`1 !== 0`)으로 실패했다. 변이를 되돌린 뒤 원래 guard를 유지한다.
이는 재시도로 제품 실패를 덮은 실행이 아니라 독립 리뷰에서 찾은 결함을 회귀가 잡는지 확인한 대조다.

## 검사 명령

모든 검증은 `aaa verify -- <명령>`으로 실행한다. 후보 관련 owner를 먼저 검사하며, 맡긴 전체 검증은 정규 ci-local과
Linux/Windows strict cross로 확인한다. 착지 창을 받으면 최종 통합 SHA의 gate는 [verification.md](verification.md)에 따라 별도로 실행한다.
Windows 전용 코드는 바꾸지 않았으며 cross 성공을 Windows 실행 검사로 기록하지 않는다. 배포·병합은 이 작업에 포함하지 않는다.

```sh
cargo test --profile verify --locked -p aaa-host --lib account_image
cargo test --profile verify --locked -p aaa --test m13 renamed_
bash scripts/ci-local.sh --only web,e2e,public --spec 134
cargo clippy --profile verify --locked -p aaa-host -p aaa-proto -p aaa --all-targets -- -D warnings
bash scripts/ci-local.sh
bash scripts/ci-linux-clippy.sh
cargo xwin clippy --profile verify --workspace --all-targets --locked --keep-going --target x86_64-pc-windows-msvc -- -D warnings
```

## 공개 구현 비교

[BullMQ stalled jobs](https://docs.bullmq.io/guide/jobs/stalled)([MIT](https://github.com/taskforcesh/bullmq/blob/master/LICENSE))의
실제 실행 상태와 작업 원장 상태를 구별하는 경계를 참고했다. BullMQ의 자동 대기열 복귀는 이미지 중복 소비를 막는 이 작업에는 적용하지 않는다.
[Temporal Activity Execution](https://docs.temporal.io/activity-execution)([MIT](https://github.com/temporalio/temporal/blob/main/LICENSE))의
영속 실행 이력과 완료 응답 경계를 비교했다. 이 저장소의 기존 ID 영수증·명시 확인을 재사용하며 새 큐나 외부 코드는 들이지 않았다.
