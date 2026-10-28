# 기록 검색 본문 겹침 수리(t-1257)

- 사용자 최종 범위: 닫기 단추·닫기 동작은 추가하지 않는다. 기록 검색·보고서·할 일을 전환할 때 선택한 본문 하나만 보이게 한다.
- 원인: `RecordSearch`는 다른 본문으로 이동해도 검색어·결과·굴림 자리를 보존하려고 DOM을 유지한다. `is-hidden` 클래스와 `inert`는 이미 전환되지만 해당 CSS가 없어 비활성 검색도 화면과 flex 배치에 남았다. `inert`는 입력만 막으며 시각적으로 숨기지 않는다.
- 수리: `RecordSearch.css`에 기존 보고서·답할 것·할 일과 같은 `position: absolute; inset: 0; visibility: hidden; pointer-events: none` 규칙을 추가했다. 주소·활동 선택·기존 토글 동작은 바꾸지 않는다.
- 회귀는 기존 E2E118에 한 개를 추가했다. 실제 host와 Chromium에서 1280px·390px로 보고서 → 검색 → 보고서 → 할 일 → 검색을 누르고, 선택한 본문이 보이며 아래 끝까지 채우고 나머지는 실제로 숨겨졌는지 검사한다. 필요한 이유: `data-shown`이나 `inert` 값만 확인하면 CSS 누락으로 두 화면이 함께 보이는 결함을 놓친다.
- 수정 전 같은 회귀는 1280px의 「보고서에서 기록 검색이 보이지 않는다」에서 `true !== false`로 실패했다. 이때 캡처에도 검색 머리·검색칸 아래에 보고서 머리·목록이 함께 보인다. 고정 sleep·재시도·시한 확대·기대값 완화 없이 같은 시험으로 수정 후를 확인한다.
- 웹 타입·번들 빌드, 웹 유닛 651개 통과. 수정 후 E2E118 전체 통과: 새 회귀 3,413ms, CLI·원문 준비 75ms, 기존 Chromium 716ms, WebKit 휴대폰 Reader 1,967ms. 개별 단계는 모두 5초 미만이다. Playwright 1.64.1 npm 패키지만 작업 폴더에 두고 공용 브라우저 캐시를 사용했다. `scripts/check-public.sh`와 `git diff --check`도 통과했다. 전체 ci-local·cross·병합·push는 착지 창에서 수행한다.

참고한 오픈소스는 [VS Code paneCompositePart](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/browser/parts/paneCompositePart.ts)의 활성 패널 숨김([MIT](https://github.com/microsoft/vscode/blob/main/LICENSE.txt))과 [Radix Tabs](https://github.com/radix-ui/primitives/blob/main/packages/react/tabs/src/tabs.tsx)의 비활성 본문 숨김([MIT](https://github.com/radix-ui/primitives/blob/main/LICENSE))이다. 선택 상태와 실제 숨김을 함께 처리하는 원칙을 확인했고, 구현은 저장소의 기존 CSS를 따랐다. 외부 코드나 제품 의존성을 가져오지 않았다.
