# E39 빈 주소 정규화의 본문 관측 누락(t-1288)

## 원인

착지 통합본 `ff432d10`의 E39 WebKit 「휴대폰 본문 기록 뒤로·앞으로 · 모든 목적지·메뉴」가
원래 20000ms 본문 확인 대기에서 실패했다. 이는 디스크 부족 진단 변경과 무관한 시험 결함이다.
이번 수리는 제품 결함 수리가 아니다. 제품의 주소 처리·본문 선택 코드는 바꾸지 않았다.

`shown()`은 주소·설정 화면 DOM·선택자를 함께 검사하지만 `domIs`는 DOM mutation·scroll·load/error만
다시 관측했다. 앱은 빈 기록에 돌아오면 현재 pane을 그대로 두고 hashchange 처리 안에서
`replaceState`로 주소만 정규화하고 끝낸다. DOM 관측이 먼저 시작되면 주소 정규화 뒤 DOM 변화가
없어 조건이 참이어도 다시 검사하지 않았다.

## 수리 전 사건 주입

- 원본 main `d9be75a76a95129bc0d8511d85c48372e5c7b346`으로 별도 수리 branch를 시작했다.
  그 소스의 웹과 실행 파일을 빌드했다. 제품 파일은 고치지 않고 E39에 사건 주입만 더했다.
- 실제 브라우저가 빈 주소로 돌아오며 만든 hashchange를 capture 단계에서 보류했다.
  popstate가 닫은 메뉴의 상태·transition 완료를 확인하고 기존 본문 MutationObserver 설치를 기다렸다.
  그 뒤 보존한 같은 hashchange 사건을 앱에 한 번 전달했다. 재전달 때 isTrusted는 false이며,
  앱의 이 주소 처리 경로는 그 값으로 분기하지 않는다. oldURL/newURL·처리 순서를 보존했다.
- trace는 `held → observer → release → normalized`였다. observer 설치 때 hash는 빈 문자열이고,
  정규화 뒤 기대 pane 주소였다. 두 시점 모두 선택자는 true·설정 화면 DOM은 false였다.
  원래 본문 대기 observer의 mutation callback은 한 번도 오지 않았다.
- 원래 20000ms 대기가 같은 오류와 found=true·shown=[]로 실패했다. 원문의 실행 스케줄 자체를
  관측한 것은 아니며, 동일한 실패를 만드는 충분한 사건 순서와 시험의 관측 결함을 확인했다.
- 주입은 시간 지연·반복·CPU 부하로 순서를 만들지 않는다. 실제 popstate·hashchange·메뉴 완료·
  observer 설치 사건으로만 해제한다. 원래 본문 조건·시한을 유지했다.

## 수리와 회귀

- `domIs`에 선택적 window 사건 목록을 추가하고 E39 `shown()`만 popstate·hashchange를 함께 듣는다.
  DOM·주소·설정 화면·선택자의 원래 조건은 그대로이고 전체 기다림 상한도 20000ms 그대로다.
  완료·실패 때 추가 listener도 함께 해제한다. 다른 호출의 기본 동작은 바꾸지 않는다.
- 사건 주입을 기존 E39 WebKit 단계에 남겼다. 같은 주소 정규화가 DOM 변화 없이 완료돼도
  원래 조건을 통과해야 한다. 필요한 이유는 일반적인 화면 이동만으로는 URL만 변하는 순서를
  결정적으로 지키지 못하기 때문이다. 새 E2E 번호·제품용 시험 hook·상시 polling은 추가하지 않는다.

## 참고 구현

- [DOM Testing Library waitFor](https://github.com/testing-library/dom-testing-library/blob/main/src/wait-for.js)
  ([MIT](https://github.com/testing-library/dom-testing-library/blob/main/LICENSE)): 현재 조건과 observer의
  관계를 비교했다. interval polling은 가져오지 않는다.
- [Playwright Frame](https://github.com/microsoft/playwright/blob/main/packages/playwright-core/src/server/frames.ts)
  ([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE)): 같은 문서 이동 신호와
  DOM 판정을 구분하는 구조를 참고했다. 외부 코드는 복사하지 않았다.
- [HTML Standard의 History](https://html.spec.whatwg.org/multipage/nav-history-apis.html#the-history-interface):
  기록 교체와 기록 순회의 popstate/hashchange를 구분한다.

## 실행·부하 기록

- 최초 착지 실패 당시 1분 load는 182.86이었다. 이후 최초 조사 시점에는 다른 Cargo·브라우저가 없었다.
- 원본 재현 직전 다시 확인하자 같은 머신에서 t-1289의 전체 웹 E2E가 실행 중이었다.
  원본 재현은 이 실행과 겹쳤으며 load 약 139.51에서 시작해 49.1초에 예상한 20초 실패를 냈다.
  이 결과를 부하 없는 성능 측정으로 해석하지 않는다. 사건 주입으로 순서는 명시적으로 고정했다.
- 수리 후 검증 전에는 다른 무거운 실행을 다시 확인하고 기록한다. 다른 작업의 프로세스는 종료하지 않는다.
- 다른 무거운 실행이 끝난 것을 ps로 확인한 뒤 같은 원본 main 바이너리 사본으로 같은 주입 순서를 실행했다.
  수리 후 대상 WebKit 단계는 13.77초, 준비·정리를 포함한 실행은 18.0초에 통과했다.
  이 시간은 기존 모든 목적지·메뉴 순회를 포함하며, 시한을 늘리거나 실패를 다시 돌린 결과가 아니다.
- E39 전체 101.8초에 통과했다. 기존 Chromium·WebKit 목적지·메뉴·기록 순회와 설정 검증을 보존했다.
  새 회귀의 held→normalized 구간은 204ms, observer 설치→normalized는 2ms였다. DOM callback 없이 끝났다.
- E39 전체 시작 직전에는 다른 Cargo·브라우저가 없었으나, 종료 시점에 t-1289의 Windows 교차 release
  빌드가 새로 실행 중이었다. PID·작업 폴더·부하를 기록했다. 이후 전체 착지는 그 빌드 종료를 확인한다.
- E2E 배치·고정 sleep 검사 14개와 diff·공개 검사를 통과했다. 수리는 별도 커밋으로 보존하고,
  최신 원격 main에 디스크 진단 변경과 함께 새로 통합해 전체 착지 절차를 다시 수행한다.
