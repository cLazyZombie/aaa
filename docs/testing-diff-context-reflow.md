# E42의 관측보다 먼저 제거된 단추(t-1236, q-1736 1A)

## 최초 원문과 원인

- main e321baab와 제품·웹 코드가 같은 미push 후보 51c62078의 전체 ci-local에서
  `42-thread-diff`의 iPhone 맥락 단계가 `page.waitForFunction: Timeout 5000ms exceeded`로 실패했다.
  원래 m13은 통과했고 웹 132개 작업 중 E42 하나만 실패했다. 최초 로그·그림·JSON은 t-1236 library에 보존했다.
- `context-failure.json`은 관측 준비 때 이미 옛 단추가 `connected=false`, 0×0이었음을 보인다.
  `__contextReflow.detached`는 false이며 사건은 `old-resolved`, `focus-refresh`뿐이다.
  ElementHandle을 얻은 뒤 관측 callback을 실행하기 전에 단추가 제거될 수 있다.
- 원래 helper는 MutationObserver를 설치한 뒤 새 mutation에서만 연결 여부를 읽었다.
  이미 끝난 제거를 observer가 뒤늦게 받지는 못하므로, 추가 mutation이 없으면 원래 5초 상한에서 실패한다.
  CPU 부하나 실제 터치 단추 크기 결함으로 분류하지 않는다.

## 결정적 재현과 수리

- `checkContextReflow`는 실제 브라우저·Shadow DOM·ElementHandle을 사용한다.
  handle을 얻고 실제로 단추를 제거한 뒤 원래 `reflowContextButton`을 부른다.
  focus가 발생하면 route로 격리한 `thread_diff` POST에 답하되 DOM mutation은 만들지 않는다.
  타이머·고정 sleep·CPU 부하 없이 원문의 사건 순서를 고정한다.
- 수리 전 WebKit은 5,096ms에 원문과 같은 `connected=false`, 0×0, `detached=false`와
  `old-resolved`, `focus-refresh`를 남기고 원래 5000ms 대기에서 실패했다.
- 수리는 observer 설치와 `old-resolved` 기록 직후 기존 `record()`를 한 번 부르는 것이다.
  이미 제거됐으면 현재 상태를 기록하고, 아직 연결돼 있으면 원래 observer가 뒤의 제거를 기록한다.
  focus 재조회 응답·제거 확인·실제 터치의 동일 DOM·44px·원문 한 번 읽기 단언과 5초 시한을 유지한다.
- 같은 회귀는 아직 연결된 단추를 focus 응답 뒤 제거하는 반대 순서도 검사한다.
  무조건 제거 완료로 두거나 focus 재조회를 생략하는 수리는 통과할 수 없다.
  필요한 이유: 기존 E42의 자연스러운 재그림만으로는 관측보다 제거가 먼저 온 순서를 결정적으로 지키지 못한다.
- 수리 후 같은 두 순서는 WebKit 118ms·Chromium 243ms에 통과했다.
  기존 E42 안에서 두 엔진의 이 회귀를 함께 실행하며 새 웹 E2E 번호·재시도·시한 변경은 없다.
- 첫 재현의 짧은 구간에 다른 aaa Cargo 작업이 겹쳤다. 이후 검증 전에는 해당 작업의 종료와 ps의 빈 검증 목록을 확인했다.

## 참고 구현

- [DOM Testing Library wait-for](https://github.com/testing-library/dom-testing-library/blob/main/src/wait-for.js)는
  observer 설치와 현재 조건 확인을 함께 사용한다
  ([MIT](https://github.com/testing-library/dom-testing-library/blob/main/LICENSE)).
- [Playwright frames](https://github.com/microsoft/playwright/blob/main/packages/playwright-core/src/server/frames.ts)는
  element의 현재 attached/detached 상태를 판정하는 기다림을 제공한다
  ([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE)).
  현재 상태도 확인한다는 관측 원칙을 따랐으며 외부 코드는 복사하지 않았다.

## 검증과 재착지

- 수리 전후의 같은 WebKit 회귀와 Chromium 대조를 t-1236 library에 보존한다.
- E42 전체는 수리 후 첫 실행에서 통과했다(19.4초). 새 두 엔진 회귀 단계는 0.902초,
  원래 실패했던 iPhone 맥락 단계는 2.624초였다. 웹 유닛 651개·fmt·공개 검사도 통과했다.
  수리는 기존 m13 수리와 별도 커밋한다.
- 미push 51c62078은 bundle과 최초 로그를 보존하고 detach → branch -d → worktree remove로 강제 옵션 없이 옛 착지 branch·worktree를 정리했다.
  최신 origin/main에서 원래 m13 수리와 이번 별도 수리를 no-ff 통합해 새 착지 SHA를 만든다.
- 사용자 착지 순서 지시에 따라 t-1238의 push로 main이 바뀐 뒤 최신 main에서 시작한다.
  같은 머신의 전체 검증을 겹치지 않게 ps로 확인한다.
- 새 착지는 다른 aaa 검증이 없는 것을 ps로 확인하고 nice 15 전체 ci-local부터 시작한다.
  LLVM PATH·착지 폴더 안 Playwright 1.64.1·공용 브라우저 캐시를 쓰며 ci-local에 salt를 넣지 않는다.
  최초 실패 원문·uptime을 보존하고 재시도·시한 확대·기대값 완화 없이 멈춘다.
- 전체 ci-local → 공개 검사 → Linux/Windows strict cross → salt 없는 Windows production → 개인정보 grep →
  ff push와 원격 SHA 대조 → 실제 착지 SHA salt 설치기 빌드·Windows 격리 폴더 전달·SHA/--version 확인 순서다.
  최종 실행 결과와 착지 SHA는 t-1236 보고에 남긴다.
