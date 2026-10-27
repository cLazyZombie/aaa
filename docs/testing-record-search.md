# 작업 기록 검색과 E42 검증(t-1205)

## 작업 기록 통합 검색(t-1205, §8.527, E2E118)

- host `record_search::tests` 네 계약은 실제 SQLite·프로젝트 파일·보고 판 보존 API를 사용한다. 필요한 이유는 최신 보고 덮어쓰기 뒤 과거 r 판 열기, 답과 직접 의견 보존, 할 일 current/before/after 중복 제거, 정리·재시작·프로젝트 경계, 거른 뒤 커서 페이지와 한글·문자 그대로 검색을 함께 지켜야 하기 때문이다. 추가로 같은 보고 본문의 여러 r 별칭, 마지막 출처 삭제, 1MiB를 넘는 원문의 UTF-8 조각/응답 상한/완전 복원을 검사한다.
- E2E118은 실제 host 보관 DB·파일에 보고 34판·답한 질문·할 일 변경 본문을 준비하고 실제 CLI 검색/원문을 대조한다. Chromium desktop과 WebKit 390px Reader가 모든 종류 30+7건, 종류 필터, 과거 보고·선택과 직접 답, 현재와 다른 할 일 메모 원문을 연다. 필요한 이유는 Rust 저장 계약만으로 웹 종류 선택·다음 페이지·원문 전환과 Reader의 실제 HTTP 접근을 확인할 수 없기 때문이다. 준비와 각 브라우저 계약을 나눠 개별 5초 미만을 확인한다.
- 실행: `cargo test --profile verify --locked -p aaa-host --lib record_search::`, `cargo test --profile verify --locked -p aaa-proto`, `cargo test --profile verify --locked -p aaa --test deps`, 웹 유닛·타입/번들, 고정 Playwright 1.64.1과 공용 브라우저 캐시에서 `node web/e2e/web.e2e.mjs --spec 118-record-search`다. 새 도구 설치는 작업 폴더의 Playwright npm 패키지뿐이며 브라우저·시스템 설정은 바꾸지 않는다.
- 기존 시험 삭제·단언 완화·재시도·고정 sleep·시한 확대는 없다. 전체 ci-local·cross와 main 병합/push는 착지 창에서 수행한다.

## E42 휴대폰 맥락 단추의 측정·터치 경계(t-1205 착지 수리)

- t-1208의 같은 DOM callback 안 `getBoundingClientRect` 수리는 callback 전에 locator가 잡은 단추의 수명까지 보장하지 않는다.
  t-1205 첫 전체 회차의 원문은 크기0, 뒤이은 iPhone 진단은44×44다. 원문에는 DOM 교체 trace가 없으므로 당시 정확한 교체 시점은 소급 단정하지 않는다.
- 고정 Playwright의 locator 해석→handle callback 경계에 실제 focus 재조회 gate를 두었다. 잡은 단추44×44→실제 HTTP 재조회→
  MutationObserver가 옛 단추 제거 확인→같은 handle의 connected=false·크기0 순서로 동일 오류를 재현했다. DOM·응답을 대역으로 만들지 않았다.
- 기존 iPhone 맥락 owner가 같은 실제 재조회를 한 번 수행한다. Playwright의 기본 안정된 배치·히트 대상 확인과 실제 tap은 유지하고,
  shadow DOM의 composedPath로 실제 touchstart·touchend·click이 받은 단추를 측정한다. 세 사건의 연결된 동일 DOM과 각각44px를 단언한다.
  따라서 자동화가 미리 잡은 옛 handle의 크기를 실제 터치 대상 크기로 해석하지 않는다. 재배치 뒤 실제 입력 사건과 측정이 같은 DOM에서 일어난다.
- 기존 초기 원문0회·20줄·전체·끝 줄·원문1회·실제 tap 단언과 시한은 유지한다. 새 E2E 번호·단계·제품 변경·재시도·sleep·완화·삭제가 없다.
  gate는 실제 DOM 제거 사건으로 끝나며 준비 실패 상한5초다. 필요성: 일반 단독 통과만으로 locator 해석 사이 DOM 교체와 실제 터치 크기를 검증할 수 없다.
  `AAA_E2E_ARTIFACTS`에는 이 owner의 재조회·옛/새 DOM 크기와 실제 터치 사건을 보존한다.

