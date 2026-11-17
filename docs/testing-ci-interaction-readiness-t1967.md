# CI 웹 조작 준비와 질문 guard 회귀 수리

## 실패 자료와 판정 범위

GitHub의 실패 job 네 건 전체 로그와 웹 artifact 세 묶음, Windows Rust artifact를 확보했다.
rerun·revert·skip·고정 sleep·기존 timeout 확대는 하지 않았다.

| 원래 job | 원문에서 확인한 실패 | 자료의 한계 |
| --- | --- | --- |
| [133888667526](https://github.com/cLazyZombie/aaa/actions/runs/44038980693/job/133888667526) | E2E02: 새 pane dialog가 detached 되기를 30초 기다림. 터미널 전송 기록에 Escape 입력이 남음 | DOM·화면·trace 없음 |
| [133901395032](https://github.com/cLazyZombie/aaa/actions/runs/44042860898/job/133901395032) | E2E138: 390px에서 `page.reload: net::ERR_ABORTED` | 실패 context가 먼저 닫혀 그 화면·navigation 기록 없음 |
| [133924023055](https://github.com/cLazyZombie/aaa/actions/runs/44049673196/job/133924023055) | E2E133: `#workspace-drawer.is-open` visible 30초 대기 | 이름 편집 전 실패. 남은 화면은 정상 pane 머리와 닫힌 drawer |
| [133924023280](https://github.com/cLazyZombie/aaa/actions/runs/44049673196/job/133924023280) | 질문 guard의 마지막 `began.elapsed() < 5s` | 기능 단언은 이미 통과. 개별 실제 경과·잠금·자원 상태는 없음 |

runner 부하 하나가 세 웹 실패를 모두 일으켰다고 확정하지 않는다. 아래는 실제 브라우저에서
사건 순서를 고정해 입증한 대기 경계다. 과거 CI138의 정확한 navigation 중단 원인은 여전히 미확정이다.

## 바꾼 계약과 회귀

- E2E02: `useFocusTrap`의 초점 이동은 rAF다. 창 DOM만 기다리면 xterm에 초점이 남아
  Escape를 셸로 보낼 수 있다. 실제 창 안 `activeElement`를 기다린 뒤 Escape를 보낸다.
- E2E133: machine 행 부착 뒤에도 첫 threads 응답과 자동 첫 선택은 남는다. 실제 응답을
  mouse down/up 사이에 전달하자 빈 화면 머리가 pane 머리로 바뀌며 click이 사라졌다.
  계정 thread의 pane 주소를 명시하고 첫 목록과 해당 pane 머리의 반영 뒤 설정을 연다.
  기능을 시작하기 위한 준비를 바꿨으며 계정 이름 변경·오류 표시의 기존 단언은 유지한다.
- E2E138: RowMenu DOM 제거 뒤 `leaveSheet`의 timer와 native `history.back`이 남을 수 있다.
  메뉴 DOM과 `aaa:sheet` 기록이 모두 없어지고 popstate 처리가 끝난 뒤 reload한다.
  실제 sheet의 native 복귀만 보류하는 회귀를 390px 기존 시나리오에 넣었다.
  waiter를 즉시 성공하는 변이로 바꾸면 「기록 복귀 전에는 메뉴 닫힘이 완료되지 않는다」에서 실패했다.
  이를 `ERR_ABORTED` 자체의 재현으로 쓰지는 않는다.
- 질문 guard 두 시험: Host·파일·SQLite 준비부터 재던 wall clock 대신 호출 직후 DB의 마지막
  완료 시각과 모든 기존 inbox·카드·역순·중복·push 단언을 확인한다. 로컬 카드 답 future는
  `now_or_never`로 한 번의 poll에서 끝남을 요구한다. timer나 다음 task 대기는 실패다.
  실제 nudge/Agent link 전달 시험의 기존 5초 실패 상한은 유지한다.

`ci-readiness.browser.mjs`는 E2E02에서 항상 실행한다. Chromium·WebKit 각각에서 첫 threads
응답을 보류하고 준비가 먼저 끝나지 않는지 검사한다. 별도 경우에는 초점 rAF를 보류하고
dialog 부착만으로 키보드 준비가 끝나지 않는지, 해제 뒤 Escape가 셸로 가지 않는지 확인한다.
각 새 단계는 정상 실행에서 5초 이내다.

## 다음 자연 실패의 진단

세 spec만 context trace를 켠다. locator·call log·원인 chain을 포함한 stack은 단계 실패 때
파일로 남긴다. 화면·DOM·focus·drawer·history·popstate·pointer 사건과 Playwright trace는
안쪽 finally의 `context.close` 전에 보존한다. 열린 context도 viewport를 바꾸는 기존 termDiag
전에 저장한다. 성공한 실험실에서는 함께 지우고 실패하면 기존 `e2e-failures` artifact로 회수한다.

진단 저장의 실패 상한은 5초다. 화면 저장 하나가 실패해도 DOM·사건·trace·최초 오류를 유지한다.
별도 진단 회귀는 없는 locator를 의도적으로 실패시켜 context를 먼저 닫은 뒤 call log·원인 stack·
DOM·화면·trace 내용을 확인한다. WebKit에서는 화면 저장 거부도 주입한다. 이 두 경우도 E2E02에
포함되며 단독 owner는 `web/e2e/failure-diagnostics.test.mjs`다.

## 개발 검증

- 웹 타입·번들·웹 유닛 전체와 E2E02·133·138, 공개 검사.
- `cargo test --profile verify --locked -p aaa-host --lib question_guard`: Mac 23개 통과(1.27초),
  별도 격리한 Windows 실기 23개 통과(0.71초). 두 환경의 수정한 `post.rs` SHA-256을 대조했다.
- `cargo clippy --profile verify --locked -p aaa-host --all-targets -- -D warnings`와 Rust fmt 확인.
- sheet 대기 제거 변이는 의도한 단언에서 실패했다. 최초 진단 fixture는 HTML 인코딩 선언이 없어
  실패했고 HTTP charset·HTML meta를 명시해 고쳤다. 기대 문자열을 완화하지 않았다.
- 최종 웹 검사에서 새 harness 상태 두 이름의 export 누락을 발견해 기존 규칙대로 내보냈다.
  앞 단계 실패로 실행되지 않은 E2E까지 수리 후 확인하며 검사 기대값은 바꾸지 않았다.
- navigation 중단을 강제하려던 별도 실험 두 건은 browser route/평가의 대기 경계에서 단계 상한에
  도달했다. `ERR_ABORTED` 재현 증거로 쓰지 않았고 상한을 늘리거나 성공 재시도를 하지 않았다.

이는 개발 owner 검증이다. 착지 창의 최종 통합 SHA에 대한 전체 gate·cross·병합·push는 별도다.
제품 Rust·웹 코드와 protocol은 바꾸지 않았고 Windows 설치 PTY는 범위에 넣지 않았다.

## 참고한 공개 구현

- [Playwright tracing](https://playwright.dev/docs/api/class-tracing),
  [Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE): 기존 Playwright의
  snapshots·screenshots·API call 기록을 사용한다. 별도 trace 엔진을 만들지 않았다.
- [Tokio 시험 시계](https://tokio.rs/tokio/topics/testing),
  [MIT](https://github.com/tokio-rs/tokio/blob/master/LICENSE): 검토했으나 filesystem·SQLite의
  실제 준비 경과를 가상 시계로 바꾸는 것은 원래 성능 단언을 검증하지 못하므로 채택하지 않았다.
- [FutureExt::now_or_never](https://docs.rs/futures-util/latest/futures_util/future/trait.FutureExt.html#method.now_or_never),
  [MIT](https://github.com/rust-lang/futures-rs/blob/main/LICENSE-MIT): 기존 futures-util로
  로컬 답 처리의 즉시 완료를 검사한다. 외부 코드 복사와 새 의존성 추가는 없다.
