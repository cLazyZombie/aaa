# 인증 창 완료 뒤 시트 이동 관측(t-1889)

## 실패와 출처

[macOS CI job](https://github.com/cLazyZombie/aaa/actions/runs/43654811095/job/132630898223)은
`f04267bb`에서 E2E 118 「휴대폰 Chromium 인증 창 회전·설정 기록 경합」의
「닫기가 viewport 안에 있다」 단언에 실패했다.
인계 기록의 화면은 390×900·scale 1이고 닫기 top=870.416687, bottom=910.416687,
`hit=true`, `overflow=false`였다. 시간 초과가 아닌 좌표 단언 실패다.

공개 API로 job·SHA·실패 상태와 웹 artifact 존재를 확인했다.
원본 artifact 다운로드는 HTTP 401, job 로그는 HTTP 403으로 거부되어 원래 실패 순간의
DOM·애니메이션 상태·회전 사건 기록은 확보하지 못했다. 아래는 로컬에서 확인한 원인이며,
원본 CI 순간에도 동일한 애니메이션 진행률이었다고 단정하지 않는다.

작업 기준 `1cad4151`은 실패 SHA의 후손이다. 두 SHA 사이에서 이 spec과 인증 창의 TSX·CSS,
공통 modal CSS에는 변경이 없다. 기존 회전 기록 수리 `f333d4fe`와 닫기 뒤 기록 복귀 수리
`be067a6e`도 이미 포함돼 있다. 이번 수리는 그 기록 관리 로직을 바꾸지 않는다.

## 확인한 원인

휴대폰으로 돌아오면 실제 `.modal`에 360ms `sheet-in`이 시작된다.
인증의 「로그인 완료」는 데이터 반영 사건이므로 그 애니메이션보다 먼저 표시될 수 있다.
기존 시험은 문구 표시 직후 `getBoundingClientRect()`를 읽었다.
이 API는 이동 중인 transform까지 포함하므로 최종 배치가 정상이어도 단추가 화면 아래로 보인다.

실제 CSS를 불러온 Chromium의 390×900 회전 표본에서는 120ms 프레임의 닫기 bottom이
913.034px, transform Y가 25.034px였다. 같은 애니메이션의 `finished` 뒤에는 bottom=888px,
transform=none이었다. 제품의 정착 배치와 이동 중 좌표를 구분했다.

실제 앱의 기존 회전·인증 시나리오에도 같은 순서를 주입했다.
회전이 시작한 실제 CSSAnimation을 120ms 프레임에서 일시 정지하고,
인증 완료 문구가 나타나면 실제 단추가 화면 아래에 있는지 확인한 뒤 재생한다.
가짜 좌표·애니메이션 대체·제품 시간 변경 없이 관측 순서만 제어한다.

완료 대기를 제거한 음성 대조는 원래 viewport 단언에서 실패했다.
그때 390×900·scale 1, top=876.224854, bottom=916.224854, `hit=true`, `overflow=false`,
transform Y=28.2249px, `sheet-in/running/currentTime=120`이었다.
원본과 같은 전체 노출 실패이며 중심점 hit만으로는 잡지 못하는 조건도 보존한다.

실패 뒤 붙는 1280px 진단은 실패한 휴대폰 context와 구분해야 한다.
`checkRotation`의 finally가 해당 context를 닫으면 `openPages`에서 빠지고,
그 뒤 공통 step 진단에는 기본 1280px controller가 남는다. 로컬 음성 대조에서도 이를 확인했다.
이번에는 단언 메시지의 같은 휴대폰 좌표 표본에 transform·진행 중 애니메이션을 함께 남긴다.

## 수리와 회귀

휴대폰 `closeCompleted`에서 해당 dialog의 실제 `Animation.finished`를 기다린 뒤 좌표를 잰다.
화면 배율, 단추 전체 노출, hit-test, 가로 넘침, 사용자 확대 허용, 실제 tap,
닫기 뒤 실제 popstate와 계정 설정 복귀 단언은 그대로다. sleep·재시도·시한 연장은 없다.

기존 10개 시나리오는 유지하고 위 순서를 제어하는 Chromium·WebKit 시나리오 2개를 추가했다.
새 단계도 닫기 뒤 계정 본문과 다음 조회 인증 요청이 한 번 발생하는지까지 확인한다.
제품 파일·화면 모양·움직임은 변경하지 않았다.

## 개발 확인

모든 검증은 `aaa verify -- <명령>`으로 실행했다.
작업 폴더의 Playwright 1.64.1 패키지와 기존 공용 Chromium·WebKit 캐시를 사용했다.

- 수리 전 기존 E2E 118: 기존 10개 시나리오 통과. 간헐 경합의 수리 근거로 삼지 않았다.
- 실제 CSS 회전 표본: 이동 중 화면 이탈과 `finished` 뒤 정상 배치를 확인했다.
- 회귀의 완료 대기 제거 대조: 위 viewport 단언으로 실패했다.
- `python3 scripts/verify.py develop --base 1cad4151 --spec 118 --logs target/t-1889-development`: 종료 코드 0.
- 웹 타입·번들·유닛 720개, E2E 118 기존 10개와 새 2개, 공개 검사 통과.
- 새 두 단계: Chromium 2.408초, WebKit 3.021초. E2E 118 전체 33.8초.
- 기존 휴대폰 WebKit 메뉴·설정 단계는 기준 5.765초, 수리 뒤 5.627초였다. 이번 수리로 늘어난 단계는 아니다.

개발 실행은 미커밋 상태였으므로 workspace의 완료 원장에는 수집되지 않았다.
실행 계획·성공 종료 코드·검사 로그는 `target/t-1889-development/`에 남겼다.
음성 대조 출력은 `target/red-account-sheet.log`, 초기 대조는 `target/green-account-sheet.log`에 있다.
착지 gate는 별도 승인과 착지 창을 받은 뒤 최종 통합 SHA에서 웹 전체로 수행한다.
부분 검증 통과를 전체 gate나 원본 CI의 재실행 성공으로 기록하지 않는다.

## 참고한 공개 구현

- [Playwright 화면 캡처](https://playwright.dev/docs/api/class-page#page-screenshot)
  ([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE)):
  `animations: disabled` 캡처는 유한 애니메이션을 끝으로 옮기므로 원인 관측에 사용하지 않았다.
- [Headless UI use-transition](https://github.com/tailwindlabs/headlessui/blob/main/packages/%40headlessui-react/src/hooks/use-transition.ts)
  ([MIT](https://github.com/tailwindlabs/headlessui/blob/main/LICENSE)):
  실제 요소의 애니메이션과 `finished`로 이동 종료를 판단하는 접근을 참고했다.

외부 코드를 복사하거나 새 런타임 의존성을 추가하지 않았다.
