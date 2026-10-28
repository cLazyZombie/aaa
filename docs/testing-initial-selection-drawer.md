# 첫 자동 선택과 열린 목록 경합 검증(t-1273)

## 원인과 수리

[원본 CI 웹 job](https://github.com/cLazyZombie/aaa/actions/runs/40713234115/job/122903861901)의
E2E23 마지막 단계는 Enter 입력이 아니라 `inList`의
`locator('#workspace-drawer.is-open').waitFor()`에서 30초를 넘겼다.
원본 전체 로그에는 call log가 남아 있으나 요약은 첫 줄만 옮겨 selector를 빠뜨렸다.
iPad(820px)와 데스크톱(1280px)은 목록 열기 분기로 가지 않으므로 해당 경로는 iPhone WebKit이다.
실패 뒤 WebKit을 먼저 닫아 원본의 휴대폰 DOM·응답 순서·stack은 남아 있지 않다.

새 탭은 machine 목록을 받은 뒤에도 첫 threads 조회가 진행 중일 수 있다.
사용자가 목록을 열고 그 응답이 도착하면 `firstPick`이 pane 또는 thread를 골라
`goHash`의 `beforeNavigation`이 쓸기 상태와 drawer를 닫고,
`choose`/`chooseThread`도 `setDrawerOpen(false)`를 실행한다.
그 결과 사용자가 방금 연 목록이 닫힌다. 느린 연결에서 생기는 제품 결함이다.
초기 첫 선택·마지막 화면 복원만 `keepDrawer`로 열린 목록을 유지하고,
사용자가 항목을 고른 때는 계속 닫는다.

기존 E2E23의 Enter 단계를 iPhone·iPad·Chrome·Safari 네 단계로 나눴다.
줄바꿈·버튼 보내기·데스크톱 Enter/Shift+Enter·한국어 IME 무전송·PTY의 정확한 수신 내용·초안 비움
단언은 보존했다. 실패하면 기기·세부 단계와 원래 selector/call log를 한 줄에 남기고,
브라우저를 닫기 전에 stack·목록/선택/입력/전송 상태를 `diagnostics`에 쓴다.

## 결정적 재현

- 수리 전 원래 Enter 검사 단독 실행은 통과했다. 단독 통과로 간헐 실패가 없다고 판단하지 않았다.
- 실제 첫 threads HTTP 응답의 JSON 완료를 보류하고, 실제 iPhone 목록 단추 클릭 뒤에 전달했다.
  Playwright click 반환 경계에 초기 선택·닫힘을 마치게 한 실험에서 원래
  `#workspace-drawer.is-open`의 30초 대기 초과가 재현됐다. Enter·submit·PTY에는 도달하지 않았다.
- 처음 보류 장치를 iPad까지 적용한 실험은 iPad gate에서 잘못 멈췄다.
  iPhone으로 한정하고 전달 경계를 바로잡은 실험만 위 재현 근거로 쓴다.
- 첫 수리에서 직접 `setDrawerOpen(false)`만 막았을 때도 keeper가 실패했다.
  `goHash` → `beforeNavigation` → `cancelNavigation` → `setOpen(false)`가 앞서 목록을 닫았기 때문이다.
  초기 선택에 한정해 이 사용자 이동 정리도 생략하도록 수리를 완성했다.
- 영구 keeper는 iPhone에서 threads 응답 전 목록을 실제로 연 뒤 JSON을 전달한다.
  첫 선택의 `aria-current` DOM을 기다리고 목록이 계속 열려 있음을 단언한다.
  이어 같은 목록에서 composer를 골라 원래 Enter 계약을 검사한다.
  이 회귀는 느린 연결에서 사용자의 목록이 닫히는 제품 계약을 지키기 위해 필요하다.
- 이 실험은 원본 CI와 같은 실패 경로를 만드는 충분한 사건 순서를 증명한다.
  원본 CI의 실제 응답 도착 순서 자체를 관측한 것은 아니다.

고정 sleep·재시도·시한 연장·기대값 완화는 추가하지 않았다.

## 참고한 구현

- [Playwright locator](https://playwright.dev/docs/api/class-locator#locator-wait-for),
  [actionability](https://playwright.dev/docs/actionability)와 설치된 1.64.1 구현(Apache-2.0):
  click 완료와 나중에 조회로 바뀐 DOM을 구별하고, 실제 목록 선택 DOM을 완료 사건으로 쓴다.
- 저장소가 이미 참고한 herdr-web-ui(MIT)의 목록 선택·paneLens 구조도 읽었다.
  사용자 선택의 닫힘을 유지하고 초기 자동 선택에만 예외를 두는 구조를 택했다.
- [React controlled input](https://react.dev/reference/react-dom/components/input),
  설치된 React 19.3.1(MIT): 초안 상태 반영과 DOM을 실제 제품 흐름에서 검사한다.
  외부 코드를 복사하지 않았다.

## 실행 결과

macOS arm64, Playwright 1.64.1과 공용 캐시 Chromium 1248·WebKit 2370을 사용했다.
도구 패키지는 작업 폴더에만 설치했고 브라우저·전역 설정은 바꾸지 않았다.

| 검증 | 결과 |
| --- | --- |
| `npm run build --prefix web` | TypeScript와 웹 번들 통과 |
| `cargo build --profile e2e --locked -p aaa --bin aaa` | 최종 웹 번들을 포함한 실행 파일 빌드 통과 |
| `npm test --prefix web` | 655개 통과 |
| `node --test web/test/e2e-layout.test.mjs web/test/edgeSwipe.test.mjs` | 수집·쓸기 계약 38개 통과 |
| E2E23 전체 `--workers 1` | 21단계 통과(65.6초), 전체 안의 Enter 네 단계도 각각 1.244~2.028초 |
| E2E23 `--grep '대화 Enter' --workers 1` | 네 기기 통과, 각각 1.194~1.853초 |
| E2E39 `--grep '메뉴 A→B|메뉴 열고 뒤로|설정 준비' --workers 1` | Chromium·WebKit 목록 선택/쓸기·Safari 390/430 탭/PWA 뒤로·앞으로·첫 응답 보류 통과 |

`git diff --check`와 `bash scripts/check-public.sh`도 통과했다.

전체 Rust 시험과 웹 E2E 전수는 이번 후보 검증에 포함하지 않았다.
변경한 웹과 해당 목록·Enter·뒤로 이동 경계를 확인했으며, 병합은 착지 창 뒤에 진행한다.
