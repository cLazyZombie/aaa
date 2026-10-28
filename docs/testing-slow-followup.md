# 느린 시험 후속(t-1261)

기준은 t-1206의 168개 역사 항목과 새 5초 상승 네 건이다. 새 상승은
`report_copy::tests::real_size_budget_preserves_report_links_and_small_evidence_before_images`,
`automation::start_tests::memory_return_preserves_edits_made_on_host_while_thread_is_open`,
`image::host_and_thread_restart_preserve_generated_images_and_coordinator_scope`,
`preparation::interrupted_placement_and_resolve_resume_from_agent_records`다.
단독 통과나 다른 머신의 시간을 이전 평소 병렬 원장에서 빼지 않는다.

## 변경과 유지하는 계약

- E37/E102는 실제 파일을 바꾸기 전에 브라우저의 단조 시각에 2초를 더한다.
  실제 파일 감지·push·HTTP·DOM, 모든 조합·이전 쪽·중복 전송·초안·ACK·좌표 단언은 그대로다.
  `ConversationRefresh`의 기존 시각 주입 시험이 2초 이전 중복 읽기 금지를 소유한다.
  native `requestAnimationFrame`·timer·Date는 바꾸지 않는다. E37의 두 frame와 25ms CPU 점유도 유지한다.
- E94는 같은 pane으로 직접 들어간다. drawer 전환은 기존 E39가 소유한다.
  앞 두 알림은 새 제목에 실제 키 세 개가 들어온 뒤 교체한다. 마지막 등장은 끝까지 보고,
  제목을 기다리는 접힘에도 원래 6 frame 입력 간격을 유지한다. CSS 속도는 바꾸지 않는다.
  세 제목 각각 실제 애니메이션 중 키를 받았는지 새로 단언하며, 세 알림·보고서 배지·질문/알림함
  갱신·포커스·정확한 전송/PTY 표시/초안·닫기의 기존 계약은 유지한다.
  같은 전체 등장 대기를 세 번 반복하는 대신 등장 중 교체 경계를 실행한다. 시험 이름/개수는 유지한다.
- rollout은 같은 서명 판을 한 번 만들고 각 머신에 독립 사본을 복사한다. 실제 실행 파일·전송 바이트·SHA·
  부분 전송 복구·동일 job·실제 재시작·실패 복구는 유지한다. 조각별 디스크 동기화를 생략하지 않는다.
- 이미지의 로컬/원격 준비만 배경으로 함께 접수하고 각 실제 brief 전달을 모두 기다린다.
- GitGate는 100ms accept 조회 대신 Tokio의 socket 준비 사건을 기다린다. 중단 준비의
  agent 종료/재연결도 구독한 실제 상태 사건을 기다린다. 기존 Git·영수증·tick 울타리와 시한은 유지한다.
- Unix pin 충돌 owner는 실제 대화형 설치 한 번에서 계획 검토·명시 동의·TLS/SAS·host 승인을 지난다.
  그 사이 pin을 편집하고 원래 종료 코드 4·편집 원문 보존을 검사한다. 중복하던 별도 승인 파일→공개 표시 거부→resume은
  기존 `private_tty_enrollment_waits_on_the_real_tls_connection_and_codes_never_enter_public_events`가 전부 유지한다.
  Windows는 기존 승인/공개 거부/resume 경로를 유지한다. 제품 설치·권한 경로는 바꾸지 않는다.
- 새 5초 경계의 보고 선택과 기억 회수는 준비/계약/정리를 원래 owner 안에서 계측한다.
  pin은 실제 승인 PTY·공개 표시 거부·TLS/SAS/편집 충돌을 구분하고, interrupted 준비는
  실제 자리/정리 영수증과 새 root 준비를 구분한다. 비용을 다른 단계로 옮기거나 빼지 않는다.

## 빠른 갱신에서 드러난 끝 보정

원래 제품은 E37의 빠른 append 뒤 새 줄은 받지만 끝과 82px 떨어진 채 따라가기를 잃었다.
마지막 본문의 mount와 두 frame를 기다려도 다른 조합에서 같은 실패가 나왔다.
가상 행의 추정 높이가 실측으로 바뀌면 브라우저가 scrollTop부터 줄이는데, 기존 보정은
이 값이 이전 scrollTop과 다르면 손의 이동으로 간주해 건너뛰었다.

ChatRow의 측정은 실제 높이가 바뀌었는지도 반환한다. 끝을 따라가는 중이고 실제 실측 변화가
있을 때만 그 clamp를 보정한다. 위로 읽는 wheel·키·touch 입력은 먼저 따라가기를 해제한다.
같은 크기의 ResizeObserver 통지가 손의 scroll보다 먼저 오는 기존 E37 경계는 그대로 보호한다.
새 안내 문구·API·상한·좌표 허용 오차는 없다. 기존 E37이 두 경계를 함께 검사하며 별도 E2E 번호는 만들지 않는다.
실패 당시 내부 ref의 모든 전이를 기록한 것은 아니다. 원문 DOM·기존 guard와, 준비 보강만 한 실패·보호를
깨뜨린 첫 수리·실제 크기 변화만 허용한 통과를 대조해 이 경계를 확인했다.

## 독립 성능 판정

빌드와 성능 실행을 분리하며, 실행 전 같은 머신의 다른 전체 검증·cross가 없는지 PID와 명령으로 확인한다.
실행 중 다른 검증이 시작되면 그 회차는 기능 결과와 별도로 표시하며 독립 성능 개선의 근거로 채택하지 않는다.
정규 gate의 결과를 성능 통과로 대신하지 않는다.

사전 기준은 E37 전체 wall 25% 이상 감소, E102 전체 wall 30% 이상 감소,
E94의 원래 여덟 owner 각각 5초 미만이다. 전체 wall은 준비·계약·정리를 모두 포함하며
기존 owner의 누락·실패·미완료는 통과로 세지 않는다. rollout과 나머지 Rust는 각 owner의
전체 wall과 배타 구간을 함께 기록하고, 5초 잔여는 그대로 남긴다.

## 선택 실행 결과

동일 작업 폴더의 verify 실행 파일·Playwright 1.64.1·기존 공용 브라우저, nice 15,
웹 worker 1·Rust test thread 1로 실행했다. 빌드는 계측과 분리했다. 시작 전 다른 무거운 실행이
없음을 확인했으며 다른 작업의 프로세스나 설정을 바꾸지 않았다. 웹의 같은 37개 owner 회수와 성공을 대조했다.

| 대상 | 원본 → 수리판 | 판정 |
| --- | ---: | --- |
| E37 기존 12 owner 합계 | 103.664 → 62.844초 | 39.4% 감소, 기준 25% 통과 |
| E102 기존 8 owner 합계 | 50.814 → 24.339초 | 52.1% 감소, 기준 30% 통과 |
| E94 기존 8 owner | 모두 5초 초과 → 모두 5초 미만 | 기준 통과 |
| rollout transfer | 62.741 → 62.645초 | 전체 비용 개선 미입증 |
| rollout rejected | 35.803 → 34.996초 | 중복 서명 제거, 5초 잔여 |
| rollout shared recovery | 42.111 → 43.491초 | 전체 비용 개선 미입증 |
| 이미지 복원 | 4.715 → 4.379초 | 양쪽 실제 brief·복원·권한 계약 통과 |
| interrupted 준비 | 5.674 → 5.413초 | 사건 대기로 줄었으나 5초 잔여 |
| pin 충돌 | 5.548 → 4.759초 | 실제 동의·SAS·편집 보존, 5초 미만 |

새 경계의 report-copy와 automation-memory는 이번 단독 선택에서 각각 0.307/1.300초였다.
이 값으로 t-1206의 평소 병렬 5초 상승을 해소했다고 하지 않는다. rollout도 실제 대용량 전송·사전검사·
host/agent 재시작·복구가 대부분이며 전체 개선을 주장하지 않는다. E68은 실제 3초 Refresh 요청·같은 host·
agent·기존 PTY 복귀 계약을 유지한다. 과거 15초 지연의 원인은 이번 실행으로 확정하지 않는다.
원래 168개 역사 항목은 미측정/미해결도 포함해 그대로 남기며, 이번 선택만으로 전수 느린 개수를 계산하지 않는다.

Rust 원본 6개, 수리판 12개와 pin의 기존 공개 표시/응답 유실/복구 owner가 통과했다.
관련 E30의 읽던 자리·보내기와 E44의 가상 줄 찾기·앞 기록·iPad/iPhone 회귀도 통과했다.
웹 전체 유닛 651개·타입/번들·crate 의존성 2개·fmt·공개 검사가 통과했다.
정규 전체 ci-local·cross는 선택 성능 판정과 별도로 착지 차례에서 수행한다.

## 실패 원문과 수리 경위

- 다른 cross가 도중에 시작된 첫 원본 웹 실행은 기능 통과만 남기고 독립 성능 근거에서 제외했다.
- 첫 수리판은 E37 끝 따라가기·E94의 같은 대상/종류 400ms 중복 방지에서 실패했다.
  E94는 실제 automation/post_muted 종류를 번갈아 보내 중복 방지를 우회하는 시계 변경 없이 세 교체를 검사한다.
  이후 실제 닫힘을 DOM MutationObserver로 기다려 반복 조회의 꼬리 비용을 없앴다.
- E37의 두 frame 준비 보강만으로는 실패했고, 모든 높이 관측에서 끝을 강제하는 첫 제품 수리도
  기존 「같은 크기 관측이 손의 scroll보다 먼저 옴」을 깨뜨렸다. 실제 측정 변화만 보정하도록 좁힌 뒤 12조합이 통과했다.
- 계측 표지 한 줄의 잘못된 scope 빌드 실패와 복사 사본의 0644/EACCES 준비 실패도 보존했다.
  표지 위치와 자식 `cp -p`의 실행 모드 보존을 고쳤다. 실제 시험이 시작하지 못한 준비 실패를 통과 표본으로 세지 않았다.
- pin 직접 설치의 첫 시도는 wizard의 선택 질문을 빠뜨려 원래 PTY 상한에서 실패했다.
  선택 값을 명시하고 실제 review/yes 동의 두 사건에 답하도록 수리했다. 시한과 pin 원문 단언은 유지한다.
- 판독기의 공통 host 준비는 spec마다 같은 이름이다. 이름별 회수까지 비교하도록 판독만 고쳤으며,
  최초 실패 판정·실패/누락 owner 원문을 덮지 않았다. 기준 비율·5초 상한은 바꾸지 않았다.

## 참고

[Playwright Clock](https://playwright.dev/docs/clock)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))과
[Rust 파일 복사](https://doc.rust-lang.org/stable/std/fs/fn.copy.html)([MIT](https://github.com/rust-lang/rust/blob/main/LICENSE-MIT))를 대조했다.
Playwright 전체 시계는 performance와 rAF를 함께 제어하므로 E37의 실제 CPU 점유에 맞지 않는다.
갱신용 단조 시각만 진행하고 native 사건은 유지한다. 복사는 시험 소유 자식 프로세스에서 수행해
기존 실행 파일 쓰기 FD 상속 방지 경계를 유지한다. 외부 코드나 의존성은 추가하지 않았다.

## 착지 통합

원격 main `c478e5c1`을 확인하고 후보 `1c4614bd`를 no-ff 통합한다.
main의 plan §8.543·FRONT_API 1.147·AGENT_LINK 1.11·웹 E2E122까지 보존하며,
새 API·E2E 번호·계획 절은 추가하지 않는다. testing.md의 충돌은 양쪽 링크를 모두 남긴다.
독립 성능 판정과 별도로 salt 없는 nice15 전체 ci-local·공개·Linux/Windows strict cross·
Windows production·개인정보 확인 뒤 ff push/원격 SHA를 대조한다. 실제 착지 SHA salt의
설치기를 격리 전달 폴더에 복사해 SHA·판을 확인하며 배포하지 않는다. 첫 검증 실패는 원문을 남기고 멈춘다.
