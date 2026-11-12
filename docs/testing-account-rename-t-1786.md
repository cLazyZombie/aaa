# 실행 중 계정 이름 변경과 저장 오류 표시(t-1786)

## 원인과 동작

기존에는 열린 thread가 사용하는 계정의 이름 변경을 거부했다. 웹은 거부 이유를 계정 목록 맨 위에만 표시해,
목록 아래쪽에서 저장하면 요청과 응답이 있어도 아무 반응이 없는 것처럼 보였다.

사용자 검수로 실행 중 이름 변경을 허용하고 기존 thread·pane의 표시 이름은 유지하는 동작을 확정했다.
인증·자동 배정의 `account`는 현재 금고 이름을 따라가고, 선택적인 `account_label`은 최초 이름 변경 전 값을 보존한다.
새 thread는 새 이름을 사용한다. 오류는 새 이름 입력 아래, 저장 버튼 위에 표시하고 입력값을 유지한다.

## 회귀 owner

- `web/e2e/specs/133-account-rename.mjs`: 실제 host·agent·시험 Claude 계정으로 실행 중 이름 저장, 기존/새 thread·pane의 이름,
  기본 계정·풀 이전을 검사한다. 목록 조회 후 저장 요청 직전에 다른 계정이 이름을 차지하게 해 실제 `name_taken` 응답을 받는다.
  목록 응답은 오류가 표시될 때까지 보류하고 새 행의 DOM 반영을 기다려, 오류 뒤 목록 삽입 순서를 고정한다.
  이 전송 경계를 직접 제어하는 화면만 service worker를 끈다(해당 worker의 요청은 Playwright route를 거치지 않는다).
  오류가 폼 안·입력 아래·저장 버튼 위·현재 viewport 안에 있고 다른 요소에 가려지지 않았는지 검사한다.
  실패 후 입력 유지·오류 설명 연결·취소 후 새 편집의 오류 제거도 보존한다. Chromium PC·WebKit PC/휴대폰에서 돈다.
- `aaa-host account_rename::tests`: 기존 금고·세대·인증·설정 이전, 권한·이름 충돌·준비 중 실행 거부와 실패 복구를 유지한다.
  열린 thread의 이름을 여러 번 바꾼 뒤 옛 이름을 재등록·재변경하고 host를 다시 열어도 최초 표시 이름과 현재 인증 참조를 보존한다.
- `aaa-host account_policy::tests`: 같은 실행의 재개 선점과 새 pane 기록이 이전 표시 이름을 유지한다.
- `aaa-host account_pool::tests::allocation_reserves_each_running_coordinator_account_across_projects_once`:
  실행 표시 이름이 현재 인증 이름과 달라도 coordinator 예약분은 현재 계정별로 한 번만 집계한다.
- `m13 accounts::renamed_running_account_refreshes_and_resumes_without_rebinding_to_reused_name`:
  실제 CLI→host→agent→session의 Codex 인증 전달을 검사한다. 실행 중 rename, 다른 token을 가진 옛 이름 재등록,
  host 재시작 후 실제 중계 갱신을 거쳐 원래 계정의 새 token을 받는지 본다. 재개도 같은 기록 홈·인증과 이전 표시 이름을 사용한다.

기존 `renamed_account_starts_with_the_same_auth_without_login_and_rejects_while_running`을 새 동작의 m13 회귀로 바꿨다.
실행 중 거부 단언은 사용자 결정으로 대체했고, 다시 로그인 없는 전달 계약은 그대로 남겼다.

## 검증 중 발견한 경합

첫 웹 회귀에서 실패 이유가 표시된 뒤 새 계정 행이 위에 삽입되어 오류가 화면 아래로 밀렸다.
PC viewport 높이 844px에서 오류는 1032.5~1053.5px에 있었다. 오류 표시뿐 아니라 계정 목록 갱신 뒤에도
오류가 보이도록 위치를 맞춘다. 목록 반영 뒤 풀·사용량 카드의 높이도 달라지므로, 오류가 표시된 동안
`ResizeObserver`로 목록과 각 계정 행의 크기 변화를 관찰한다. 오류를 없애면 관찰도 끝낸다.
같은 단언과 시한을 유지하며 기대값을 완화하지 않았다.
실패 때 화면과 input/alert/submit 좌표·가림 요소를 실험실 diagnostics에 보존한다.

## 후보의 관련 검사 결과

- 계정 관련 Rust owner 64건과 Codex 중계·재개 통합 회귀 1건 통과.
- 웹 타입·번들·유닛 및 E2E 133의 Chromium PC·WebKit PC/휴대폰 조합 통과.
- `aaa-host` 전체 target의 네이티브 strict Clippy와 공개 저장소 검사 통과.
- 독립 리뷰에서 인증 연결·표시 이름·재개·coordinator 집계 경계를 확인했다. 목록 반영을 기다리라는 회귀 보강과 v1/v2 그룹 문서 정리를 반영했다.

## 실행

검증은 `aaa verify -- <명령>`을 통해 실행한다. 관련 검사 뒤 전체 `ci-local`과 Linux/Windows strict cross를 실행한다.
착지 창에서는 [verification.md](verification.md)에 따라 최종 통합 SHA의 gate를 별도로 한 번 수행한다.
Windows 전용 경로나 프로토콜 변경은 없으며 cross 성공을 Windows 실행 확인으로 기록하지 않는다.

```sh
cargo test --profile verify --locked -p aaa-host --lib account_
cargo test --profile verify --locked -p aaa --test m13 renamed_running_account_refreshes_and_resumes_without_rebinding_to_reused_name
bash scripts/ci-local.sh --only web,e2e,public --spec 133
bash scripts/ci-local.sh
bash scripts/ci-linux-clippy.sh
cargo xwin clippy --profile verify --workspace --all-targets --locked --keep-going --target x86_64-pc-windows-msvc -- -D warnings
```

시험 계정과 가짜 제공자·harness만 사용하며 운영 계정 설정은 변경하지 않는다.
대기는 host 응답·출력·DOM 사건·갱신 gate로 끝내며 고정 sleep·재시도·시한 확대는 넣지 않는다.
