# 시험 쓰는 법

## 메뉴 선택 뒤 휴대폰 쓸기의 진단과 정리(t-0805)

- 사용자 순서는 대화 A → 메뉴에서 B 선택 → 왼쪽 가장자리 뒤로다. 수정 전 E2E39의 진단본은 두 메뉴 선택 모두 `history-write`에서 `open=true`, 메뉴 오른쪽 경계 343px로 실패했다. 뒤로가 끝난 상태만 검사하던 시험과 달리 기록을 쓰기 직전의 DOM을 확인한다. 이것은 열린 DOM 경계의 재현이며 Safari OS 미리보기 이미지 자체의 재현은 아니다.
- `drawer-navigation.mjs`는 Chromium·WebKit 390px에서 위 순서, x25의 빠른 쓸기 후 touchcancel만 도착한 경우, native popstate 뒤 늦은 move/end와 move/cancel, pagehide/pageshow 사건 경계를 검사한다. 메뉴 class·inline transform·실제 경계가 닫혀야 한다. Touch 생성은 WebKit의 `document.createTouch/List`와 Chromium의 생성자를 각각 쓴다. 설정 내부 되감기·모든 목적지·390/430px·홈 화면 앱 흉내·넓은 화면의 기존 E2E39 단언은 유지한다. 실제 bfcache 복원이나 iPhone OS 몸짓을 합성 사건으로 입증했다고 쓰지 않는다.
- 유닛 `node --test web/test/edgeSwipe.test.mjs`의 빠른 취소·popstate·pagehide·pageshow 네 회귀는 원래 구현에서 모두 메뉴 재개방으로 실패했다. 설정 내부 되감기와 정상 안쪽 flick·기존 손가락 추적은 유지한다. 시한 연장·실패 재시도·단언 완화·고정 sleep은 없다.
- 실기는 배포 뒤 새 탭의 앱 주소 query에 `drawer-debug=1`을 넣고 진단 창을 접은 채 한다. A → 메뉴로 B → 가장자리 뒤로를 보통 속도 3회·천천히 3회, 같은 몸짓을 중간에 취소하기 3회 진행한다. 매번 정상/미리보기만/계속 열림 중 하나를 표시하고 마지막에 기록을 저장한다. 시작 위치·속도·사건 순서와 총 빈도는 JSON에 남는다. 처음부터 보이던 오래된 미리보기와 새 이동 뒤의 결과를 구분하려고 배포 뒤 새 탭에서 시작한다.
- `?drawer-debug=1` 없는 탭에는 진단 UI·사건 수집이 없다. 진단은 최근 800개 사건과 전체 개수만 탭 메모리에 두며 URL·history.state·본문·인증 값을 기록하지 않는다. DOM 경계가 닫혔는데 사용자에게 미리보기만 보이면 OS 스냅숏 후보로 분류한다. 화면 합성 결과는 DOM 로그만으로 단정하지 않는다. 정본 검증은 전체 `nice -n 15 bash scripts/ci-local.sh --tee`와 공개 검사다.

## 새 launcher 아래 restart fixture 격리(t-0798)

- `nice -n 15 cargo test --locked -p aaa --test restart -- --nocapture`는 등록되지 않은 새 launcher를 기본으로 쓴다. `AAA_RESTART_TEST_LAUNCHER`로 기존 launcher를 지정하는 읽기 전용 경로도 유지한다. 두 경로 모두 기존 launcher/worker PID·기존 pane 보존·새 pane·책임 PID·승인 거부·detached 재관측 단언을 그대로 검사한다.
- 원본은 보호 볼륨의 Python 스크립트를 open하다가 `probe-before`의 30초 시한으로 실패했다. 같은 새 책임자 아래 Apple·Homebrew Python 모두 일반 임시 폴더의 같은 스크립트는 즉시 성공하고 보호 볼륨에서는 정지했으며, 해당 PID의 TCC `SystemPolicyAllFiles` 거부를 확인했다. 자료만 옮긴 대조는 다음으로 `kache rustc -vV`에서 보호 볼륨의 사용자 store를 열다가 멈췄다. 시험 전용 `KACHE_CACHE_DIR`를 준 대조는 실제 cargo/build.rs까지 통과했다. Python 판이나 CPU 부하로 단정하지 않는다.
- fixture 자료·cwd·생성 실행 파일·Cargo target·kache store는 사용자 캐시 아래 실행마다 새로 만든 전용 폴더에 둔다. kache wrapper는 유지하고 전역 설정·기존 store·TCC·launchd 등록을 변경하지 않는다. Cargo target을 OS 임시 폴더나 다른 worktree에 공유하지 않는다. 완료된 각 단계의 원본 결과는 시험 책임자가 자기 worktree의 `target/restart-probes`로 복사하고, 성공하면 전용 캐시를 지우며 실패하면 경로를 남긴다.
- 새 launcher는 기존 C 소스를 그대로 쓰고 별도 시험 신원의 Info.plist·adhoc 서명으로 정상 번들을 만든다. 준비 전에 launcher가 종료되면 바로 실패한다. 진단 하위 명령은 실제 stdout/stderr와 종료 코드를 남기고 완료 출력 사건으로 끝낸다. 정상 출력·시한·기대값을 완화하거나 실패한 명령을 재실행하지 않는다.
- 조사 중 별도 실행에서는 syspolicyd의 EMFILE·YARA 검사 오류와 launcher 종료를 기록했다. 또 다른 cargo 실패의 같은 시각에도 해당 시스템 오류가 있었지만 당시 숨겨진 cargo stderr가 없어 그 실패의 직접 원인은 확정하지 않는다. 정상 번들·격리는 시스템 검사 오류 전체의 수리라는 뜻이 아니다. 이후 실패는 추가된 종료·compiler 오류와 원본 시스템 로그를 함께 확인한다.
- 첫 전체 검증은 재시작 전 진단 뒤 새 worker 준비 기록의 30초 대기에서 한 번 실패했다. 그 실행에는 정리 전 스택이 없어 원인을 확정하지 않는다. 해당 대기에 실패하면 아직 자기 실행 파일을 가리키는 worker·launcher만 sample하고 원래 실패를 다시 전파하도록 진단을 추가했다. 진단본의 단독·병렬 Rust·Rust/웹 동시 전체 검증은 통과했지만 앞 실패가 수리됐다는 증거로 쓰지 않는다. 다음 재발의 스택을 확보한다.
- 정본 검증은 전체 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다. 제품 코드·API·E2E 번호·시스템 권한은 바꾸지 않는다.

## ci-local --tee 병렬 출력(t-0797)

- `--tee`도 기본 실행과 같은 `test e2e clippy` 병렬 경로를 쓴다. 세 명령은 각각의 로컬 로그에 stdout/stderr를 쓰고,
  별도 reader가 `[단계]`를 붙여 화면에 출력한다. 로그 원문·단계 결과·실패 요약·GitHub annotation은 유지한다.
  web→fmt→build와 E2E 실행 사본 준비를 먼저 끝내고, 병렬 단계와 reader가 끝난 뒤 public을 실행한다.
  고른 병렬 단계가 하나면 기존 tee 경로를 쓴다. reader 실패도 전체 실패지만 검증 명령은 모두 끝까지 실행한다.
- `python3 -m unittest scripts.test_ci_local -v`는 실제 셸과 격리 도구 자식 세 개를 socket으로 만난다.
  세 자식이 동시에 진입하고 종료를 허락받기 전에 각 로그가 화면에 도착해야 한다. 고정 sleep 없이 socket과 출력 사건으로 끝낸다.
  한 단계 실패 뒤 나머지 완료·public 후행·원문 로그·개행 없는 마지막 바이트·이전 완료 표지 제거·단일 단계·tee 없는 경로·
  빌드 실패의 의존 단계 건너뜀과 reader 실패를 검사한다. UTF-8이 읽기 경계에서 나뉘어도 깨지지 않고 완료 뒤 큰 로그를 모두 읽어야 한다.
  직렬 분기를 되심으면 동시 진입 대기에서 실패한다. 이 keeper는 Unix의 정본 test 단계에 포함한다.
- 전후 비교는 같은 worktree와 머신에서 준비 빌드를 끝낸 뒤 같은 선택 단계·환경·nice 값으로 차례로 실행한다.
  stage 시간 합과 실제 병렬 구간의 벽시계를 구분하고, 준비 빌드·캐시·주변 부하 차이를 절감액으로 단정하지 않는다.
  최종 정본 `nice -n 15 bash scripts/ci-local.sh --tee`와 공개 검사를 수행한다. 검증 범위·worker·시한을 바꾸지 않는다.

## FD 고갈 시험의 Front 연결 준비(t-0791)

- 두 번째 묶음 gate에서 기존 hard64 시험은 웹 EMFILE을 기록했지만 단발 Front status 요청이 이미 성공해 Front EMFILE 대기가 실패했다. 원본 host 로그·요청 stdout·OS fd 목록을 보존했다. 이 owner와 host accept 구현은 후보 3개에서 바뀌지 않았다.
- 웹 부하에 이어 실제 Unix Front socket 80개도 EOF 전까지 유지한다. 시작·ticker 등 임시 fd 반환 때 단발 요청 하나가 통과하는 틈을 지속 연결로 채우며, Front accept의 실제 EMFILE을 계속 필수로 요구한다. 두 부하를 닫은 뒤 fd 회수·새 Front 요청 성공·시스템 경고 한 건·웹 HTTP 복구의 기존 단언과 15초 상한을 유지한다. 운영 한도·제품 accept·재시도·기대값은 바꾸지 않는다.

## 작업 목록 요약의 키보드 준비 경합(t-0791)

- 묶음 착지 첫 전체 gate의 E2E28은 메뉴의 「요약」에 focus한 뒤 Enter를 보냈지만 상세가 열리지 않았다. 기존 RowMenu는 첫 항목 「보고」의 focus를 다음 animation frame에 예약한다. 기준 main 36f3e935의 격리 사본에서 그 콜백을 「요약」 focus 직후 전달하면 focus가 「보고」로 돌아가고 실제 click도 「보고」로 발생하며 같은 상세 대기가 실패한다.
- E2E28의 기존 키보드 경로만 메뉴 초기 frame과 첫 항목의 focusin 완료를 기다린 뒤 「요약」으로 옮긴다. 고정 sleep·시한 연장·기대값 변경 없이 기존 Enter·같은 작업·주소 보존·Reader·휴대폰 검사를 유지한다. 후보를 빼지 않으며 기준 main의 시험 준비 경합 수리로 별도 기록한다.

## 설정 닫기와 휴대폰 메뉴의 통합 경합(t-0780, t-0771)

- 기존 E2E39의 `historyDestinations`를 t-0771과 통합한 코드에서 그대로 실행해 `pane-bk2`의 30초 click 실패를 재현했다. 설정의 작업 패널 선택 → `recordSettings([])`의 내부 되감기 → popstate 메뉴 강제 닫기 순서이며, 도우미가 없는 실제 조작에서도 메뉴가 사라지는 제품 결함이다.
- E2E39의 `settings-drawer-history.mjs`는 `history.go`의 전달만 보류한다. 실제 설정 기록 모듈이 되감기를 요청한 사건을 기다리고, 사용자가 메뉴를 연 상태에서 native go를 풀어 실제 popstate·설정 상태·drawer transition으로 끝낸다. 설정에서 작업 패널을 고른 경우와 X로 닫은 직후 다시 메뉴를 연 경우를 Chromium·WebKit 390px에서 확인한다. 도우미로 재개방하지 않고 pane 줄을 직접 눌러 선택할 수 있어야 한다.
- 복귀 분류는 해당 사건에만 적용한다. 이어서 메뉴를 열고 사용자가 `history.back/forward`하면 두 방향 모두 닫혀야 하며, 기존 목적지 순회의 설정 목록·쪽 단언도 유지한다. 수정 전 Chromium의 작업 패널 선택 경로에서 「브라우저 이동 없이 설정 내부 복귀가 사용자가 연 메뉴를 닫았다」로 실패했다. 시한·단언 완화·고정 sleep·실패 재시도는 없다. 실제 OS 가장자리 몸짓은 headless 합성 터치·history 이동과 구분한다.
- 정본 검증은 `nice -n 15 bash scripts/ci-local.sh --tee`와 후보 commit 뒤 공개 검사다. API·E2E 번호는 유지한다.

## m13 자동화 CI 준비 대기와 실패 요약(t-0787)

- main `212d8425`의 macOS 실패 원문(run `39768260021`, job `119789031781`)은 `30s 안에 labA 연결이 되지 않았다`다. `ci.rs`의 FIFO·batch·ack 단계에 들어가기 전 `world_build`에서 실패했다. 같은 실험실의 host/agent 로그는 약 1초 안에 연결 성공을 기록했다. 당시 `machine list` 응답·오류와 CLI 실행 시간은 남아 있지 않아 과거 실패의 직접 원인은 확정하지 않는다. 변경 전 해당 시험의 로컬 단독 실행은 통과했다.
- 공통 준비 단계는 agent를 띄우기 전에 Front 상태 사건을 구독하고 `labA`의 실제 `Connected`를 기다린다. CLI 반복 기동·조회 오류를 `None`으로 버리는 경로에 준비 판정을 맡기지 않는다. 30초 상한과 자동화의 기존 단언·FIFO·ack 순서는 유지한다.
- `cargo test -p aaa --test m13 agent_readiness_uses_initial_state_and_reconnection_events -- --exact`는 실제 host/agent에서 구독 전의 현재 연결, 종료 사건, 구독 뒤 재연결 사건을 확인한다. 연결 대기 실패에는 마지막 머신 목록·host/agent 로그를 붙이고, Front 수신 오류는 원문 이유를 남긴다. 진단용 추가 CLI 조회는 하지 않는다.
- 공통 준비를 쓰는 async N-1 호환 시험은 동기 `world()`를 `spawn_blocking`에서 준비한다. 첫 전체 검증에서 감시용 runtime 중첩이 드러났고 이를 수리한 뒤 같은 호환 시험이 통과했다. 해당 시험의 연결·profile 거부 단언은 유지한다.
- `python3 -m unittest scripts.test_rust_tests`는 실제 자식 runner가 실패 코드·모든 시험·doctest를 보존하면서 긴 진단 뒤에도 `FAIL <묶음> <시험>: <panic 첫 줄>`을 로그 끝과 `summary.json`에 남기는지 검사한다. 원본 runner에서 새 keeper는 `failures` 누락으로 실패했다. 시한·기대값 변경이나 실패 재실행으로 넘기지 않는다.
- 관련 회귀는 `cargo test -p aaa --test m13 ci::`, 정본 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다. 새 macOS CI에서 같은 실패가 나면 추가된 상태·수신 오류·로그로 원인을 좁힌다. 로컬 통과만으로 과거 러너의 원인까지 입증했다고 하지 않는다.

## 질문·보고 영상 재생(t-0767, plan §8.448)

- `nice -n 15 cargo test --locked -p aaa-host --lib web::`: 기존 인증 경로를 cookie 없이 열었을 때 세 종류 모두 401·UTF-8 JSON·한글 원문인지 확인한다. 새 첨부 cap의 재사용·만료·기기 폐기·보고서 주소 변환 거부와 리소스 CSP의 정확한 폴더 제한을 검사한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 88-report-video --workers 1`: 실제 host·agent·publish·coordinator post·thread 보고를 쓴다. 외부 자료 없는 2초 H.264 High/yuv420p/faststart 영상은 `web/e2e/fixtures/report-video.mp4`다. Chromium·WebKit 각각 1280×800·390×844에서 본문과 선택지의 첫 재생 단추·metadata 수신·재생 뒤 실제 decode·시간 전진·seeked·소리 없음·인라인·자동 재생 없음·답 선택 보존·가로 넘침을 검사한다.
- 같은 질문의 HTML 링크는 `/v/…/library/…/index.html#A`다. iframe과 새 탭의 실제 영상 decode, cookie 없는 직접 MP4·206 Range·MIME·Accept-Ranges, HTML의 자기 파일 GET·cookie/API/다른 cap 차단, 보고 Markdown의 재생, 새 보고 뒤 답한 영상 원본 bytes를 확인한다. coordinator AGENTS.md와 thread brief에 영상 문법도 확인한다. 앱 API 인증·sandbox를 풀거나 영상 객체·play를 가짜로 만들지 않는다.
- E2E53의 웹 `files_base`는 `/v/<48자리 hex>/`여야 한다. 답 내용·원문·사본 보존 검사를 유지하고, cookie 없는 읽기의 MIME·전체 바이트가 원본 그림과 같은지도 확인한다.
- 완료 사건은 host 보고 hash·DOM·loadedmetadata/timeupdate/seeked다. 실패 뒤 시한 연장·재시도 없이 원인을 조사한다. 관련 E2E21·59·69·74와 전체 `nice -n 15 bash scripts/ci-local.sh`·공개 검사로 회귀를 확인한다. headless WebKit은 실제 macOS Safari·iPhone의 출력 장치·UI를 보증하지 않는다.

### E2E88 실패 진단(t-0786)

- HTML 링크의 popup 대기와 실제 click은 `clickForPopup`에서 함께 관측한다. click이 먼저 실패한 뒤 정리 과정이 context를 닫아도 popup의 늦은 거부를 처리하므로 원래 오류가 남는다. `node --test web/test/popup.test.mjs`는 클릭 전 대기 등록·클릭 완료, 클릭 실패 뒤 context 종료, popup 실패 뒤 늦은 클릭 실패를 확인한다. 옛 순서의 구현을 별도 사본에 심으면 뒤 두 시험이 unhandled rejection으로 실패한다.
- 실제 WebKit desktop의 HTML 링크 클릭만 의도적으로 거부한 별도 사본에서도 대조했다. 옛 순서는 CI와 같은 popup 종료 unhandled rejection으로 죽고, 새 순서는 최초 클릭 오류·단계·본문/iframe 영상 상태를 `FAIL`에 남긴다. 두 사본 모두 실패로 끝나며 영상 재생과 sandbox 검사는 실제 제품을 쓴다. 오류 은폐의 재현이며 최초 CI 클릭 오류의 원인을 재현한 것은 아니다.
- 네 조합 각각의 실패에는 단계, 선택값, 주소, 영상 readyState·currentTime·paused·오류와 최근 click·hashchange·media 사건을 붙인다. 선택 보존과 주소 보존은 독립 단언이며 재생·탐색 실패에도 실제 상태를 남긴다. 영상 재생·시한·sandbox·제품 기대값은 그대로다.
- `212d8425`의 macOS CI는 WebKit desktop의 iframe 확인 뒤 popup 대기 거부로 종료돼 최초 오류를 남기지 못했다. t-0783 첫 gate의 선택/주소 복합 단언도 두 값을 기록하지 않았다. 로컬 macOS 27·Playwright 1.59.1의 캡처 포함 10회·캡처 없는 10회는 모두 통과했다. 최초 원인과 두 실패의 관련성은 미확정이며 제품 수리 완료로 해석하지 않는다. CI와 같은 캡처 없는 실행도 반드시 포함한다. 진단 반복은 실패를 통과로 바꾸는 재시도가 아니며 실패한 회차를 모두 보존한다.


## 본문 끝단 스크롤 격리(t-0766, plan §8.447)

- E2E87은 실제 격리 host·agent의 긴 질문(그림 16개·표), Markdown 보고서·thread 보고, 긴 할 일 상세와 단축키 설정을 연다. Chromium·WebKit 각각 1280×720·390×844, 밝음·어두움의 8조합을 검사한다. 질문의 lazy 그림은 각각 화면에 보인 뒤 decode 완료를 기다린다.
- 본문의 높이 제한·가로 넘침, 데스크톱에서 위·아래 끝의 실제 wheel 수신과 렌더링 사건 뒤 문서 좌표·visualViewport·앱·왼쪽 목록·본문 틀의 위치 및 목록 scrollTop 보존을 확인한다. 끝단에서도 본문 scrollTop은 끝에 남아야 한다. 휴대폰은 실제 overflow의 scrollTop과 scroll 사건으로 이동·양 끝을 확인한다. Playwright mobile WebKit은 wheel/swipe를 지원하지 않으므로 손가락 관성 스크롤을 재현했다고 주장하지 않는다.
- headless WebKit은 macOS Safari의 OS 고무줄 효과를 온전히 재현하지 못한다. 따라서 스크롤 전달 차단과 앱이 있는 html·body의 overflow 잠금도 함께 단언한다. ccc4a985 원본 번들에서 질문의 전달 차단 누락으로 RED를 확인한다. 로그인 화면에는 문서 잠금을 적용하지 않으며 기존 E2E01·06과 입력·키보드·viewport 회귀를 유지한다.
- 고정 sleep·시한 연장·실패 재시도 없이 DOM·그림 decode·wheel·렌더링 사건으로 끝낸다. `NODE_PATH=$(npm root -g) nice -n 15 node web/e2e/web.e2e.mjs <캡처 폴더> --spec 87 --workers 1`과 전체 `nice -n 15 bash scripts/ci-local.sh`·공개 검사를 수행한다.

## 설정 왼쪽 메뉴·목록 행 겹침(t-0771)

- E2E06은 설정 메뉴가 데스크톱의 공통 사이드바에 있고 오른쪽 본문 전체를 내용만 쓰는지 검사한다. Chromium 1280/980/320px·WebKit 1280/390px의 밝음/어두움에서 테마·색상·밀도 조작의 실제 경계와 hit target을 확인하고 캡처한다. 기존 주소·새 탭·새로고침·뒤로/앞으로·Esc·키 바·폰트·휴대폰 메뉴 → 내용 검사를 유지한다.
- 설정 메뉴가 본문 밖으로 이동했으므로 메뉴 locator는 설정 navigation을 사용한다. inList는 앞 패널을 숨긴 상태를 감지하고 실제 활동 막대로 전환한다. 제품 동작을 대신하지 않는다.
- E2E26의 긴 목록과 E2E34의 상태가 다른 자동화 행에서 자식 글·번호가 행 안에 있는지, 다음 행과 겹치지 않는지, 선택/hover 배경이 앞 행을 덮지 않는지 검사한다. 자동화 번호의 상대 시작 위치도 같아야 한다. 답할 것·보고서·할 일·기록·설정 메뉴도 같은 경계 검사를 사용한다.
- 고정 sleep·재시도·시한 연장 없이 DOM·host·scroll 사건으로 끝낸다. 전체 필수 검증은 `nice -n 15 bash scripts/ci-local.sh --tee`와 공개 검사다.
- 기존 빌드에서 E2E34의 자동화 행 세로 넘침을 재현했고 공통 행 높이 수리 뒤 E2E06·26·34가 통과했다. E2E06 마지막 단계는 초기 pane 선택 전 주소를 저장하는 경합이 있어 실제 terminal과 pane hash가 준비된 뒤 기준 주소를 기록한다. 복귀 주소 기대값·시한은 유지한다.
- 전체 웹 회귀의 E2E03은 iPad 820px에서 본문 안의 단축키 tab을 찾던 locator 한 곳이 남아 실패했다. 앱 drawer 기준에 따라 왼쪽 navigation에서 찾도록 갱신한 뒤 E2E03 전체가 통과했다. 키 전달·설정 저장·터미널 누출·크기 단언은 그대로다.

## 첨부 뒤 여러 줄 입력의 높이 안정(t-0772, plan §8.452)

- 전체 검증에서 기존 m13 `routines`가 DB 항목을 본 직후 아직 쓰는 중인 inbox Markdown을 빈 내용으로 읽어 실패했다. DB insert→파일 write→안내 action 완료 기록 순서를 확인했고, 파일 단언 전에 `routine runs`의 `done`/`coordinator 안내` 완료를 기다리도록 시험만 고쳤다. 명령 source 종료는 안내 action 종료보다 앞이므로 완료 신호로 대신 쓰지 않는다. 원래 파일 본문·승인·중복 출력 단언과 30초 상한은 유지한다.

- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 66-composer-wrap --workers 1`: 기존 줄 경계·게이지 폭 회수·창 폭 변경 검사를 유지하고, 실제 coordinator의 그림 업로드·경로 삽입·빈 줄·여러 줄 한글 입력을 추가한다. Chromium·WebKit 각각 1280px·390px에서 「맨 아래로」가 떠 있는 대화와 짧은 글/높이 상한을 넘는 긴 글을 검사한다.
- OS 입력기를 직접 조작하는 대신 두벌식 자모→음절→확정 사건을 재생하고 trusted 영문 키 입력도 실행한다. 입력 중 높이·화면 위치·내부 scrollTop·상자 높이·문서 스크롤을 기록한다. 원래 scrollHeight getter가 이미 강제하는 layout 직후를 관측하므로 화면 frame 사이의 일시적 축소도 검출한다. ResizeObserver·scroll 사건과 두 requestAnimationFrame 뒤 표본을 함께 보며 고정 sleep·실패 재시도·시한 연장은 없다.
- 수정 전에는 네 조합의 짧은 글과 긴 글 모두 측정 순간 높이가 40px로 줄어 실패한다. WebKit의 긴 글 진단은 240→40→240px와 화면 위치 왕복을 보였다. headless에서 최종으로 그려진 frame의 시각적 떨림까지 재현했다고 주장하지 않는다. 수정 후 같은 시험에서 높이·스크롤·focus·조합 글 보존·편집 중 전송 없음과 삭제 뒤 한 줄 복원을 검사한다.
- 화면과 JSON 표본을 캡처 폴더에 저장한다. E2E23의 첨부 전송·E2E35의 보내기 높이·기존 E2E66을 포함한 전체 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다. API와 E2E 번호는 유지한다.


## 작업 요약 카드의 실행 모델(t-0751, plan §8.445)

- `node --test web/test/summaryCard.test.mjs`: 관측 모델·생각 수준의 표시, profile과 실제 모델의 불일치, 미확인 값과 옛 host, 알려진 계열의 짧은 이름·알 수 없는 이름 보존을 검사한다.
- `nice -n 15 cargo test -p aaa-agent --lib model_observation --locked`: 실제 증분 JSONL reader로 Codex sol→astra·effort 변경·누락, 프로세스 시작 전 관측 제외, Claude 본 대화만 선택·생각 수준 추측 없음·하위 agent 제외를 검사한다.
- `nice -n 15 cargo test -p aaa-host --lib plan_events_set_counts_without_changing_the_activity --locked`: 실제 host 상태에 모델을 적용하고 idle 보존·미확인 사건·새 세션의 초기화를 확인한다. 기존 계획·컨텍스트·배경 수·상태 단언을 유지한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 28-thread-summary --workers 1`: 실제 pane 안 hook→agent JSONL reader→Agent link→host→열린 카드의 sol high→astra medium 변경과 같은 상세를 확인한다. /model 선택 결과는 harness 기록만 대신 쓰며 모델 판정·상태 전달·표시는 제품을 쓴다. 재시작의 새 profile·관측 없음, Chromium 데스크톱·320px Reader와 WebKit 390px Reader에서 밝음/어두움·가로 넘침을 검사하고 캡처한다.
- DOM·host·기록 사건으로 끝내며 고정 sleep·실패 재시도·시한 변경은 없다. 전체 필수 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다. FRONT_API 1.109·Agent link 1.11·기존 E2E 번호를 쓰고 착지 때 main의 번호를 다시 확인한다.

### t-0751 검증 환경 진단

첫 전체 실행은 load 97.60~103.38에서 계정 버전 확인·이미지 버전 확인 등 5초 상한의 준비 세 건이 실패했다. 같은 binary·기대값·시한으로 해당 세 건을 차례로 분리 실행해 3/3 통과했다(load 8.05). 첫 실행의 실패 원문은 보존한다.
설치된 Playwright 1.59.1/WebKit 26.4는 macOS 27에서 브라우저 초기화 중 `InspectorPlaywrightAgent`의 SIGBUS가 발생했다. thread 전용 폴더의 Playwright 1.64.1과 Chromium 156/WebKit 27.2로 비교 검사했다. 새 WebKit의 CSS font-family 따옴표 정규화가 기존 E2E06 문자열 검사를 실패시켰고, 지정된 1.59.1에서는 같은 E2E06이 그대로 통과했다. 기대값은 바꾸지 않고 최종 웹 회귀 검증은 지정된 1.59.1·worker 2개로 수행한다. 전역 설치·시스템 설정은 바꾸지 않는다.



## 공개 포트 변경·저장 링크 이전(t-0749, plan §8.444)

- `nice -n 15 cargo test -p aaa-host --lib web_publication --locked`: 실제 DB·원장·router를 사용한다. 옛 포트 해제/다른 서비스 보존, post 본문·refs/본문 SHA/원본 봉투 보존/원래 입력 재전송, inbox 파일 투영, 구판·옛 origin 구독 정리와 loopback 구독·기기 신원 보존을 확인한다. Serve 등록 실패 뒤 host 재개방·같은 대상만 재개, 다른 서비스 충돌·읽기 오류·권한 거부를 검사한다. DB의 구독 삭제에 실패를 주입해 저장 주소·원본 보존·답 상태·공개 주소·구독이 함께 롤백되는지 확인한다.
- `nice -n 15 cargo test -p aaa-host --lib tailscale:: --locked`: 기존 exact/LocalAPI/서비스 보존 owner와 함께 깨진 Serve JSON·없음과 `null`의 구별을 확인한다. 상태 읽기 실패로 후보 포트를 고르거나 기존 공개 주소를 지우면 안 된다.
- `nice -n 15 cargo test -p aaa --test m13 public_port_change --locked`: 실제 CLI→host→Serve 외부 명령→DB→HTTP로 자동 공개·명시 포트 변경·post/refs 이전·기존 cap의 새 주소에서 200·옛 Host 421·다른 매핑 보존·host 재시작 내구성을 검사한다. Python fixture는 Tailscale 실행 파일만 대신하고 제품의 전환·원장·공유 서명·HTTP 입구는 대체하지 않는다. 이 owner는 Unix m13이며 실제 daemon/TLS·Windows 실행의 증거는 아니다.
- CLI 인자 owner는 `public_port_change_parses_and_rejects_off_and_invalid_ports`다. 고정 sleep·실패 재시도·시한 연장 없이 명령 완료·실제 상태·HTTP/DB를 완료 사건으로 쓴다. 정본 전체 검증은 `nice -n 15 bash scripts/ci-local.sh --tee`·공개 검사이며 Linux 교차 clippy도 확인한다. 실제 운영 host/Serve는 변경하지 않는다.
- 결함 심기: `public_origin_in`의 관측 표지 검사를 제거하면 `failed_transition_survives_restart_and_resumes_only_the_same_target`가 「매핑 없음 확인 뒤 옛 포트 fallback 금지」로 실패한다. 원본 bytes를 복원한 뒤 정본 전체를 검증한다. 이전 전 원본 보존은 해당 post 본문 정리·inbox/답 기록 삭제 수명을 함께 따른다.
- 질문 본문 URL 변경은 해시도 바뀐다. `moving_a_question_body_preserves_its_ids_and_partial_answers`는 원래 q-ID 두 개·한 질문의 부분 답을 유지하고 새 본문 해시에 연결해야 한다. revision 이전 전에는 `q-1002/q-1003 != q-1000/q-1001`로 실패했다. 첫 전체 검증에서 `[]`를 struct의 빈 sequence로 읽은 Serve 판정과 새 DB 시험의 불필요한 clone lint도 잡혔다. JSON root를 object/null로 제한하고 같은 기대값·시한을 유지한다.

## 설정 본문·passkey 줄·긴 목록 스크롤(t-0748, plan §8.443)

- E2E06의 새 「설정 본문·passkey 긴 값」 단계는 실제 host·agent에서 설정이 main을 채우고 모달 배경·aria-modal 없이 표시되는지 검사한다. Chromium 1280px 밝음/어두움·980px 좁은 본문·320px, WebKit 390px 밝음/어두움에서 상태·설명 폭과 가로 넘침을 확인하고 캡처한다. 10분이 지난 상태와 긴 도메인의 등록 상태는 실제 `/me` 응답의 passkey 필드만 대체해 기다림 없이 재현한다. 인증·등록 동작은 기존 E2E79가 맡는다.
- 주소 복사·새 탭·새로고침·닫기·앞으로, 확인 창의 Esc, 같은 pane을 사이드바에서 다시 선택하는 경로, 모양/터미널/단축키/알림의 긴 값도 확인한다. 테마는 실제 설정 조작으로 바꾸고 적용 사건을 기다린다.
- 기존 설정 region·키 바 region 접근성 locator를 갱신한다. 휴대폰에서 아래로 밀면 본문에 남고 X로 나오는 새 계약을 검사한다. 닫은 뒤 focus는 데스크톱 톱니·터미널 또는 휴대폰 본문 목록 단추여야 한다. 폰트 다운로드·격자/PTY 크기·키보드 입력과 브라우저 뒤로의 기존 단언은 유지한다.
- 초기 회귀는 닫기 focus의 한 프레임 지연을 찾았다. 보이는 활동 막대에는 즉시 돌리고 가려진 본문은 commit 뒤에 돌린다. WebKit은 서비스 워커를 거친 `/me` 요청이 시험 route를 우회해 실제 loopback 응답을 받았다. 응답을 대체하는 새 단계만 기존 시험과 같이 serviceWorkers=block으로 격리하고 받은 passkey 필드도 확인한다. 제품 인증·서비스 워커 동작은 바꾸지 않는다.
- 전체 E2E04의 wake lock과 E2E14의 공유 목록 모양 검사는 설정을 닫으면 목록 drawer가 그대로 열려 있다는 옛 모달 가정을 사용했다. 본문 복귀 뒤 drawer를 직접 열도록 동작을 갱신하고 기존 요청/해제 횟수·저장·새로고침·Reader 단언을 유지한다. 새 Tab 검사는 특정 톱니가 아니라 실제 DOM 차례의 앱 목록에 focus가 나왔는지 확인한다.
- 첫 전체 병렬 gate에서 load average 76.31을 관찰했고 Rust m13 6개 준비 사건과 E2E78 계정 기동·E2E82 후보 점검이 실패했다. 원본 로그를 보존했다. 제품/시한/단언을 바꾸지 않은 Rust 6개 순차 실행과 E2E78·82 단독 파일이 모두 통과했다. 후속 전체 gate는 `--tee`로 단계를 순차 실행하고 Rust worker 3·E2E worker 3으로 모든 검사를 유지했다. 웹 E2E 88파일/99작업이 통과했다. 이 실행의 `RUST_TEST_THREADS=1`은 기존 aaa-common panic_log 자식 출력에 libtest 접두어를 붙여 파서 시험 하나를 실패시켰다. 자식의 성공·출력 형식을 직접 확인하고 저장소 기본값 2로 aaa-common 36개를 따로 검증했다. 기본 test thread 설정은 유지한다. 새 UI 변경 뒤에는 web/build와 해당 E2E를 검증한다.
- E2E26의 긴 목록 단계(`web/e2e/lib/panel-scroll.mjs`)는 실제 격리 host에 답한 것 79개·할 일 79개·기록 79개·자동화 40개와 앞 단계의 보고서 14개를 준비한다. DB fixture는 host를 멈춘 동안만 쓴다. Chromium·WebKit 각각 1280×800·390×844에서 제한된 높이·가로 넘침·스크롤 끝·hit test·항목 선택을 확인한다. 데스크톱은 실제 휠의 scroll 사건을 기다린다(scrollTop 변화는 MutationObserver가 알리지 않는다). 휴대폰은 native overflow의 scrollTop으로 끝을 확인한다(합성 touchmove를 실제 손가락 관성 스크롤로 주장하지 않는다). 기록은 더 보기가 없어질 때까지 페이지를 읽고 마지막 기록의 전후 차이도 연다. 머리·바닥과 문서는 움직이지 않는다. 기존 코드에서는 질문 목록 높이 3,484px가 800px 화면 밖으로 늘어나 첫 단언이 실패했다.
- 착지 전체 Rust의 `post_cli`에서 3초 응답 시한·가짜 서버 BrokenPipe가 관측됐다. ACK/종료 코드 fixture가 실제 `hooks::read_msg`와 달리 완성된 JSON 뒤에도 EOF를 기다렸다. 요청 쓰기 쪽을 열어 둔 socket pair로 이 추가 의존성을 RED 재현하고, 첫 JSON 완성에 답하도록 고쳐 GREEN을 확인한다. 기존 옛 agent/accepted/refused/queued의 종료 코드·응답 단언과 제품 3초 시한은 유지한다. 제품 코드는 바꾸지 않으며 고정 sleep 없이 읽기 사건으로 끝낸다.
- 고정 sleep·시한 연장·실패 재시도 없이 DOM·응답·history 사건으로 끝낸다. `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 06-settings --workers 1`과 전체 `nice -n 15 bash scripts/ci-local.sh`·공개 검사를 수행한다.

## 왼쪽 목록 고정 번호 칩(t-0739, plan §8.442)

- `nice -n 15 cargo test -p aaa-host --lib automation_ids --locked`: 실제 host·DB·파일로 기존 규칙 조회→제목 수정→형식 변환→삭제→다른 이름 번호 발급→복원→재시작·프로젝트 이름 변경·내보내기를 확인한다. DB의 동시 읽기와 서로 다른 이름 발급은 Barrier 사건으로 끝내며 프로젝트별 독립 순번·재열기·프로젝트 삭제를 검사한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 21-library-reports,34-automations,43-tasks,69-coordinator-questions --workers 1`: 실제 host의 r/d/q/a 번호를 공유 칩으로 보이는지 검사한다. 보고서 inline code 기준의 글꼴·크기·배경·색·여백·모서리를 밝음/어두움에서 비교한다. Chromium 데스크톱·휴대폰·320px과 WebKit 질문 목록 390/320px에서 번호 전체·행 넘침을 확인하고 캡처한다. 긴 제목을 DOM에 잠깐 적용해 제목만 줄고 번호가 남는지도 검사한 뒤 원래 DOM을 복원한다.
- 질문 행은 공통 button의 고정 높이를 덮어 내용만큼 커지고 flex로 줄어들지 않아야 한다. 추가한 세로 넘침 검사는 수리 전 실행 파일에서 네 q-ID의 행 넘침으로 실패하고 수리 뒤 통과했다. E2E69는 질문 본문과 sidebar의 선택 완료를 모두 기다린 뒤 다음 pane으로 이동하며, 단계 실패 때도 별도 WebKit을 finally에서 닫는다.
- 여러 질문의 모든 ID·coordinator 첫 질문 중복 없음·답한 기록 ID 보존, 자동화 변환 뒤 같은 ID와 기존 이동·편집·답·Reader 거부도 유지한다. 고정 sleep·실패 재시도·시한 연장은 없다. 전체 필수 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다. FRONT_API 1.107, Agent link 1.11이며 착지 때 main의 문서/API 번호와 다시 대조한다.
## 할 일 공통 검색줄·칩 분류·완료 체크(t-0737, plan §8.441)

- `node --test web/test/tasksModel.test.mjs`: 모델만 정한 대기와 담당·실행·예약·오류·완료 조합을 저장소별로 확인한다. 전체 외 칩의 합=전체, 각 칩의 실제 행 수=표시 수, 모든 ID의 중복/누락 없음을 검사한다. 예약 상위가 다른 칩에 있어도 하위는 자기 칩에 남는다.
- `nice -n 15 cargo test -p aaa-host --lib task_queue::tests:: --locked`: thread 없는 `[x]`/`[X]`를 하위부터 보관하고 미완료 하위가 남은 상위는 보존한다. 담당·메모·고정 ID·별도 완료 근거, 재시작 뒤 중복 없음, 복원 뒤 체크 해제·thread 없음·재예약 없음, 보관 저장 실패의 원본 보존과 파일 제거 뒤 확정 실패의 재시작 회수를 검사한다.
- `nice -n 15 cargo test -p aaa --test m13 thread_start_warns_about_similar_tasks_without_blocking --locked`: 완료 체크의 보관 tick을 확정한 뒤 활성 유사 task 경고·명시 연결·기동 비차단을 검사한다. 같은 제목의 완료 항목은 원래 ID로 보관되고 새 기동은 별도 ID와 열린 thread를 가져야 한다. 옛 시험의 완료 항목 활성 잔류 가정을 새 보관 계약으로 대체하며 경고·원본 머리·고정 ID 보존 검사는 유지한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 86-task-history --workers 1`: 실제 host·agent에서 Chromium 1920px와 Chromium 320px·WebKit 390px Reader의 탭/검색 공통 부모, 검색 위치·높이·스타일·간격과 좁은 화면의 동일 줄바꿈을 비교한다. 기록·보관 두 입구, 기존 검색·저장소·상태·더 보기·복원/Reader 권한을 유지한다. 실험실 파일의 모델 지정 대기·칩 합·직접 완료 체크·보관/복원도 확인한다. 운영 TASKS.md는 수정하지 않는다.
- DOM·응답·`ticker tick`을 완료 사건으로 쓰며 고정 sleep·실패 재시도·시한 변경은 없다. 배치는 board의 실제 hash와 Front 기능/권한 응답 수신 뒤 잰다. 수신 전에는 「기록」 탭이 없어 검색 시작 위치가 달라지므로 첫 응답 전 폭을 기준값으로 삼지 않는다. 기존 E2E43·46·47·67·76·85와 전체 `nice -n 15 bash scripts/ci-local.sh`·공개 검사를 수행하고 착지 창을 기다린다. API/E2E 번호는 유지한다.
- t-0753 착지 검증에서 E2E86의 완료 체크 복원 뒤 즉시 CLI 조회가 실패했다. 보관 목록은 복원 요청을 시작할 때도 비워지므로 행 없음은 완료 신호가 아니다. 해당 task ID의 실제 `task_archive` 복원 성공 응답과 `aria-busy=false`·빈 결과 문구를 확인한 뒤 기존 ID·체크 해제·연결 없음·owner·예약 없음 단언을 수행한다. 남겨진 실패 실험실에는 복원된 항목이 있었으며 제품 코드·시한·기대값은 바꾸지 않는다.



## 시스템 idle-audit의 웹 읽기 전용(t-0735, plan §8.440)

- `nice -n 15 cargo test -p aaa-host --lib automation:: --locked`: 실제 파일과 host 경계에서 Reader/Controller 원문·기록 조회, 웹 저장·토글·삭제·실행·승인·큐 변경·복원·옛 형식 변환의 거부와 파일 무변경을 확인한다. 파일 없음·오류에서도 이름 보호가 유지되고, 다른 운영 규칙은 웹 편집을 허용해야 한다. Local CLI 경계의 저장·토글·실행·삭제·꺼진 복원을 확인한다. 프로젝트를 생략한 일반 변환의 허용과 시스템 변환의 거부도 같은 경계에서 확인한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 55-automation-edit --workers 1`: 기존 일반 규칙의 만들기·편집·삭제·복원·실행·승인·Reader 검사를 유지한다. 시스템 규칙은 Chromium 1280px·WebKit 390px에서 목록/상세 표지·이유·꺼진 스위치·쓰기 단추 없음·원문·기록·가로 넘침을 확인하고 캡처한다. 실제 웹 API의 각 변경 거부와 CLI 토글·삭제·복원도 확인한다.
- 완료 신호는 host 응답·파일·DOM이며 고정 sleep·실패 재시도·시한 변경은 없다. 전체 필수 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다. FRONT_API 1.106·AGENT_LINK 1.11·기존 E2E 번호를 유지한다.

## agent 연결 종료 뒤 보고 복사 대기 해제(t-0734, plan §8.439)

- `nice -n 15 cargo test -p aaa-host --lib link::supervision_tests --locked`: 실제 Agent link reader에 잘못된 frame을 보내 끝내되 peer와 writer는 살려 둔다. 종료 전 요청과 이전 연결 사본의 FileRead·ReportFileRead·Input은 즉시 AgentDisconnected가 되어야 한다. 재연결 뒤 옛 generation의 종료가 새 요청을 끊으면 안 된다. 수정 전에는 후속 FileRead가 Timeout으로 실패하고 수정 뒤에는 가상 시계가 전진하지 않는다.
- 같은 묶음에서 실제 512칸 전송 대기열을 채운다. enqueue에서 기다리는 요청도 연결 종료로 즉시 풀리고, 연결이 살아 있는 포화 대기열은 기존 15초 시한 안에 Timeout·pending 해제로 끝나야 한다. reader panic 정리·감독자의 실제 시한 초과 경고와 이후 성공 응답 시험도 유지한다.
- `nice -n 15 cargo test -p aaa-host --lib orch::report_tests --locked`: 실제 보고 수집 owner가 manifest 뒤 첫 파일에서 연결을 잃는다. 두 번째 파일이 이전 연결을 재사용해도 파일마다 120초 기다리지 않고, 새 본문·실패 상태와 이전 검증 첨부를 보존해야 한다. 새 연결을 넣은 뒤 hash 변화 없는 다음 ticker에서 complete·첨부 복구·ack 보존을 확인한다.
- 고정 sleep·실패 재시도·시한 연장은 없다. link의 상대 I/O와 보고 전송 응답만 대체하며 연결 수명·보고 수집·원장·파일 저장은 제품 owner를 쓴다. 전체 ci-local·공개 검사와 Linux 대상 clippy를 수행한다. 외부 API·웹 동작을 바꾸지 않아 기존 API/E2E 번호를 유지한다.
- 전체 검증 중 E2E69의 질문 링크 연속 누름에서 본문 render 직후의 사이드바 전환 effect와 다음 목록 누름이 경합했다. 정확한 q-ID·카드·본문 단언에 「답할 것」 활동 단추 선택과 패널 표시 완료 단언을 더했다. 다음 목록 누름은 이 DOM 사건 뒤에 시작하며 기존 시한과 나머지 단언을 유지한다.

## 맨 q-ID도 질문 상태 칩으로 표시(t-0730, plan §8.438)

- `node --test web/test/appIds.test.mjs`: 문장·괄호·붙은 `·`/쉼표·줄바꿈에서 두 q-ID를 각각 인식하고 주변 글자를 보존한다. 주소·경로·없는 ID 제외 검사는 유지한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 69-coordinator-questions,77-app-ids --workers 1`: 실제 host·agent·Claude 대화에서 Markdown 링크·옛 참조·맨 ID를 비교한다. E2E69는 thread·coordinator의 네 질문을 문장·붙은 괄호·쉼표 안에 넣고 모든 칩의 아이콘·본문·상태와 밝음/어두움 배경·아이콘 색 일치를 확인한다. 각 맨 ID를 눌러 정확한 q-ID와 카드로 이동하는지도 검사한다.
- 열림→답함→새 보고 뒤 답 보존은 새로고침 없이 확인한다. 답 기록까지 삭제해 host 목록에서 사라진 맨 ID는 평문, 명시 참조는 중립 칩이라는 기존 규칙을 유지한다. E2E77은 coordinator·thread 대화와 보고서의 질문 칩, 코드 블록·외부 링크 안의 칩 제외, 질문 이동·답 기록과 나머지 ID의 기존 링크/가운데 누름을 확인한다.
- 고정 sleep·실패 재시도·시한 변경 없이 DOM·주소·host 사건으로 끝낸다. 전체 필수 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사이며 착지 창 전 병합·push는 하지 않는다. API와 E2E 파일 번호는 바꾸지 않는다.

## 활동 막대 자동화·할 일 아이콘(t-0727, plan §8.437)

- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 26-activity-bar --workers 1`: 막대의 자동화·할 일 단추가 새 아이콘(`lucide-aaa-automation`·`lucide-aaa-tasks`)이고 옛 Workflow·ListTodo가 막대에 없는지 본다. 명령 팔레트 「할 일 열기」도 같은 아이콘인지 확인하고 막대를 캡처한다.
- `--spec 48-automation-threads`: thread ⋯ 메뉴 「자동화 규칙」이 막대와 같은 자동화 아이콘인지 확인한다. 둘 다 DOM 상태로 끝나며 고정 sleep·재시도는 없다. 그림 자체는 데스크톱·휴대폰 × 어두움·밝음 화면으로 눈으로 확인한다.

## 작업 목록 메뉴의 요약(t-0721, plan §8.434)

- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 28-thread-summary --workers 1`: 실제 host·agent에 요약 있는 thread와 없는 thread를 만든다. 작업 목록 점 세 개 메뉴의 「요약」으로 thread 머리 단추와 같은 상세가 열리는지 확인한다.
- Chromium 데스크톱·320px Reader와 WebKit 390px Reader에서 메뉴 본문 미표시·다른 thread의 정확한 목표·요약 없음 안내·hover 겹침 없음·주소 보존·가로 넘침을 검사한다. 데스크톱 키보드 Enter·Escape, 휴대폰 메뉴 시트→요약 시트 교체 뒤 한 번 뒤로 가기를 확인하고 각 화면을 캡처한다.
- 기존 E2E28의 머리 단추·hover·고정·지시 전문·실시간 진행·Reader 쓰기 거부 검사는 유지한다. 메뉴 해제·DOM·애니메이션·history 사건으로 끝내며 고정 sleep·재시도·시한 연장은 없다. 전체 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다.
- E2E12의 thread 메뉴 전체 항목 검사도 「보고·요약·고정·모든 thread」 순서를 검증한다. 기존 고정·닫기·다시 열기·모든 thread 이동 검사는 유지한다.

## host Codex 계정 이미지 생성(t-0707, plan §8.433)

- `nice -n 15 cargo test -p aaa-host --lib image --locked`: 이미지/일반 thread의 공유 예약,
  주간 잔량 51%의 4·8회 연속 요청·medium 공존·짧은 창 부족,
  남은 15%에서 두 동시 이미지 claim의 한 번 배정, 명시 별칭에서도 다른 공유 별칭의 관측 유지,
  미관측 첫 실행 중복 거부, 종료 뒤 새 관측에서만 해제, 재시작 불명 예약 보존·잔여 인증 회수,
  출력 파일의 범위·symlink·형식·중복 item을 검사한다.
- `nice -n 15 cargo test -p aaa-host --lib image_rpc::tests::installed_codex_accepts_isolated_image_config_without_authentication --locked -- --ignored --exact`:
  설치된 Codex 0.163.0을 빈 격리 홈에서 initialize·config/read·modelProvider/capabilities/read로만 확인한다.
  로그인·인증 파일·thread/start·turn/start·이미지 생성은 없다. 설치판이 없는 CI에서는 ignored로 남긴다.
- `nice -n 15 cargo test -p aaa --test m13 image:: --locked`:
  실제 CLI·worker PID·hook token·로컬 IPC·TLS 원격 agent·host 원장·가짜 Codex·가짜 OAuth 제공자를 쓴다.
  두 계정의 선택·중앙 갱신 한 번·64KiB를 넘는 결과 전송·동일 SHA·인증/임시 홈 정리·토큰 합계,
  같은 ID 재전송·기본 변경·host/pane 재시작·파일 정리 뒤에도 재생성 없음,
  다른 thread·위조 token·세션 없음·허용하지 않은 계정/profile·지원하지 않는 CLI와 임의 도구 요청 거부를 검사한다.
  오류에 잘못 넣은 가짜 인증의 가림, worker 자기 목록·coordinator 소속 프로젝트 목록/정리도 확인한다.
  turn/start 응답보다 먼저 온 이미지 사건도 유지한다. 원문·토큰 조각·인코딩은 로그·DB·원격 파일에 없어야 한다.
- `scripts/fixtures/image-codex.py`는 제품의 격리 홈·환경·argv를 확인하고 이미지 사건만 만든다.
  인증 선택·원장·입력 검증·공유 예약·권한 검사는 대체하지 않는다. 외부 실계정·API 과금은 쓰지 않는다.
- `image_rpc::tests::refresh_waiting_on_the_central_gate_cannot_deliver_a_new_login_to_an_old_job`은
  실제 중앙 갱신 잠금·loopback proxy·금고를 쓴다. owner가 gate를 복제해 기다리는 사건 뒤 같은 별칭으로
  다시 로그인해, 이전 판에 새 인증을 주는 Served 응답이 옛 이미지 작업으로 나가지 않는지 확인한다.
  응답 뒤 세대 검사 전에는 200(새 인증), 수정 뒤에는 401/빈 JSON이다. 제공자·실인증·고정 sleep은 쓰지 않는다.
- 결함 심기: 실제 선택 owner에서 이미지 예약 목록만 비우면 공유 예산 시험이 두 번째 배정 단언으로 실패한다.
  원본 바이트를 복원한 뒤 같은 시험이 통과해야 한다. 시험은 응답·파일·pane 종료 사건으로 끝나며 고정 sleep·재시도·시한 연장은 없다.
- 전체 필수 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다. 웹 UI·Front 요청 변화가 없어
  FRONT_API 1.106·AGENT_LINK 1.11·웹 E2E 최대 번호 86(88파일)를 유지한다. Agent link의 imagegen.v1은 양쪽이 알렸을 때만 쓴다.

새 기능을 만드는 thread가 따르는 짧은 규칙이다. 근거는 참고 넷(herdr 0.9.5·herdr-projects 0.2.35·herdr-web-ui `aa624cb`·t3code `4b1024da`)과 이 저장소에서 실제로 난 간헐 실패다. 전략과 도구 목록은 [plan §13](plan.md#13-테스트-전략과-직접-만들-e2e-도구)에 있다.

## 대화 질문 칩의 q-ID 상태 색(t-0712, plan §8.432)

- `node --test web/test/asksModel.test.mjs`: 같은 thread의 새 q-ID는 답할 것, 지난 q-ID는 보존한 답으로 구별한다. coordinator 평문·구조화 질문의 열림·답함·기록 삭제 후 중립도 확인한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 69-coordinator-questions --workers 1`: 실제 host·agent·Claude 대화 기록에 Markdown q-ID 링크와 `aaa:question/q-ID`를 넣는다. thread·coordinator의 두 질문씩 열림→답함→새 보고에서 질문 비움 뒤 답 보존→기록 삭제 뒤 중립을 새로고침 없이 확인한다. class와 문구, 밝음·어두움의 실제 배경·아이콘 색을 CSS 토큰과 비교하고 화면을 남긴다.
- 기존 E2E53의 thread ID 링크·dock·보존본·지우기 단언을 유지한다. 고정 sleep·재시도·시한 연장 없이 host 응답·보고 수집·DOM 사건으로 끝낸다. 전체 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사를 수행하고 착지 창을 기다린다.


## 할 일 기록 검색·저장소·완료 상태(t-0703, plan §8.431)

- `nice -n 15 cargo test -p aaa-host --lib task_queue::tests:: --locked`: 실제 DB/파일로 4MiB 초과 보관의 개수·바이트 페이지와 모든 ID의 빠짐/중복 없음, trim/대소문자·메모/표시 ID·저장소·실제 완료 근거 조합, 같은 ID 복원·연결/예약 없음, 2MiB 이력의 목록 비전송/내구 보존, 일치하지 않는 100건 뒤의 기록 검색·삭제 복원을 검사한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 86-task-history --workers 1`: 격리 DB 130건/4MiB 초과에서 CLI의 전체 128건 출력, API 페이지의 frame 크기와 모든 ID, UI의 검색·저장소·완료 상태 조합/더 보기·완료/삭제 복원과 Reader 거부를 확인한다. Chromium 1280/320px·WebKit 390px의 활성/기록 화면과 필터 결과를 캡처하고 가로 넘침을 검사한다.
- 기존 E2E46의 삭제·차이·되살리기, E2E47의 완료 본문·결과·ID와 E2E67의 예약 실행·복원 이후 재실행 없음 검사를 유지한다. 완료 보관 입구가 기록 탭을 여므로 복원 뒤 활성 행은 할 일 탭에서 확인한다.
- 고정 sleep·실패 재시도·시한 연장 없이 실제 응답과 파일·DOM 사건으로 끝낸다. 전체 필수 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다. FRONT_API 1.106·Agent link 1.11.



## 일반 작업의 할 일 선행(t-0705, plan §8.430)

- `cargo test -p aaa --test m13 manual_threads_always_have_tasks_and_archive_the_result --locked`: 실제 host·agent와 가짜 harness를 사용한다. 배경 기동 응답 전에 할 일 연결·선택 모델/머신/저장소를 확인하고, Markdown 머리/체크 목록이 별도 task가 되지 않는지·원문 파일 바이트·완료 보관의 ID/보고를 검사한다. resolve 응답은 thread 종료만 보장하므로 `w.h.ticks(1)`로 그 뒤 시작한 ticker의 보관 판정을 확정한 뒤 조회한다. 원래 task ID·완료 근거·보고 경로·활성 목록에서 제거됨과 뒤 tick의 중복 보관 없음을 검사한다(t-0753, macOS CI의 resolve 직후 보관 조회 경합). 없는 Git ref의 준비 실패에도 연결된 할 일이 남고 `--link-task` 없는 기존 할 일 시작이 같은 ID를 쓰는지 확인한다. 보고 회수·명령 응답·ticker 완료가 끝 신호이며 고정 sleep·시한 변경·실패 재시도는 없다.
- host `account_pool::tests::waiting_direct_start_keeps_one_task_across_passes_and_request_replays`는 계정이 없는 실제 배정 대기 두 pass·같은 시작 ID 재전송·DB 재열기에서 task 하나/같은 ID·thread 0건을 검사한다. RPC 응답을 주지 않는 가짜 연결로 불필요한 재조회를 드러낸다. 기존 배정/취소 경합 시험의 대기·Cancelled·내구성 단언은 유지한다.
- 기존 m13 유사 task 경고 시험은 별도 task 생성과 기존 완료 task 보존, `coordinator_flow`는 연결 옵션 없는 자동 연결, `doctor_files_brings_the_coordinator_guide_to_existing_projects`는 새 프로젝트와 낡은 안내 갱신의 동일 문구를 검사한다. `automation_start_holds_approval_two_agents_and_restart_receipt`의 기존 실제 do.start 실행에서 할 일을 만들지 않는 예외를 검사한다.
- E2E47은 실제 일반 기동의 자동 연결을 확인한 뒤 그 할 일을 명시 삭제하여 옛 thread의 잇기/만들기 복구 단언을 유지한다. E2E48은 실제 승인된 do.start의 무생성·일반 작업 연결을 함께 확인한다. 전체 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사 뒤 착지 창을 기다린다.

## thread 보류·이어 하기(t-0689, plan §8.426)

- `nice -n 15 cargo test -p aaa-host --lib park:: restart_plan --locked`: 보류 대상(일반 Worker만)·닫기 판정(맡긴 뒤의 새 `[보류 준비됨]`이고 agent가 일·확인창을 기다리지 않음)·같은 대화 판정(대화 ID 자리와 UUID)·git 글, 보류는 `unpark`만 다시 띄운다.
- `nice -n 15 cargo test -p aaa --test m13 thread_park --locked`: 실제 host·agent·가짜 Claude(`resumable`)·TLS labB·로컬 origin. 고정 sleep은 auto-resolve 하루(시험 1초)를 넘기는 한 번뿐이고 그 밖은 host 기록·화면 출력·`ticker tick`으로 끝낸다. 결함 심기: brief 전달에서 `brief_line` 대신 brief.md 표지를 쓰면 「보류를 풀었다」 출력 기다림으로 실패한다.
- t-0720 인증 회귀: 같은 `thread_park` 묶음의 `account_unpark` 두 시험은 host 금고에 가짜 Claude 계정 둘을 등록하고 기본 계정으로 worktree를 시작한다. 보류 뒤 기본 계정을 바꿔도 같은 머신의 `--resume`·대화 ID·격리 홈 기록, TLS 다른 머신의 새 대화·같은 branch, 앞 머신 복귀의 원래 홈을 유지해야 한다. 계정 선택·실행 인증 원장·격리 홈은 제품 코드이고 harness만 대체한다. 기존 코드에서 두 시험이 모두 운영과 같은 `bad_request`로 실패하고 수리 뒤 통과하는지 확인한다. 운영 보류 thread·실제 로그인은 쓰지 않는다.
- `nice -n 15 cargo test -p aaa-host --lib account_policy --locked`: 보류 인증 참조 회수는 같은 실행·계정·harness이고 pane이 사라졌을 때만 허용한다. 기록 없음·다른 신원·준비 중·남은 pane 거부와 agent 응답 전 중복 기동 거부를 함께 검사한다. 고정 sleep·재시도·시한 연장은 없다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 85-thread-park --workers 1`: Chromium 1280px에서 「정리 맡기고 보류」·보류 띠·sidebar 제외·할 일 「보류」 칩과 거르기·「모든 thread」 보류 묶음·이어 하기 창, WebKit 390px에서 보류 창·바로 보류·이어 하기 창이 폭 안에 드는지 본다. 웹 유닛은 `operateModel`·`tasksModel`.
## 일반 post 기본 한도 확대(t-0688, plan §8.425)

- `nice -n 15 cargo test -p aaa-config --lib post_defaults_and_explicit_overrides --locked`: 파일 없음·빈 `[post]`·일부 생략에서 30/60초·200/시간·2000/일·pair 100·mute 20을 읽고, 명시한 옛 값과 local 덮어쓰기를 유지한다.
- `nice -n 15 cargo test -p aaa-host --lib post::tests:: --locked`: 실제 host DB와 전달 owner에 `now`를 주어 기본 30건 핑퐁 수락, 31번째 거부·19회까지 음소거 없음·20회 음소거, 분·시간·하루 경계와 24시간 내용 중복을 검사한다. 기존 ID 재전송·재시작 내구성과 작은 한도 설정도 유지한다. coordinator 사용자 알림은 별도 30/분·60/시간 한도와 자동 음소거 없음, 일반 한도 초과 뒤에도 수락을 확인한다.
- 기존 m13 `post_from_thread_reaches_coordinator_inbox`와 웹 E2E19 `post 음소거`는 전송·표시를 검사하도록 작은 한도(3/분·6/시간·30/일·거부 3회)를 명시한다. 제품 기본값에 기대지 않고 기존 단언·시한을 유지한다. 새 E2E 번호는 추가하지 않는다.
- 고정 sleep·실패 재시도·시한 연장 없이 원장 결과와 CLI 완료·DOM 사건으로 끝낸다. 전체 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다.
## 화면·고정폭·터미널 글꼴(t-0683, plan §8.423)

- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 06-settings --workers 1`:
  실제 host의 Chromium 1280px·WebKit 390px에서 세 글꼴을 바꾸고 새로고침 뒤 입력값·실제 CSS 목록을 대조한다.
  없는 글꼴만 고르면 기본 목록과 canvas 너비가 같아야 하며, 고정폭 목록은 아이콘 글꼴을 맨 앞에 둔다.
  화면·코드·터미널의 즉시 적용, 같은 기기 다른 탭의 storage 사건, 다른 기기 분리,
  세 「기본값으로」의 저장값 삭제·기본 목록 복원·새로고침 유지와 가로 넘침을 검사한다.
  설정의 모양·터미널 화면을 두 기기에서 캡처한다. 기존 face gate·설치 글꼴 거부·PTY 크기 검사는 유지한다.
  DOM·글꼴·storage 사건으로 끝내며 고정 sleep·재시도·시한 연장은 없다.



## 보고로 자동 갱신하는 작업 요약(t-0682, plan §8.422)

- `nice -n 15 cargo test -p aaa-host --lib progress --locked`: 실제 보고 수집 owner가 진행 목록·완료 수를 고치면서 목표·사용자 결정을 보존하는지 검사한다. 같은 보고 재수집·host 재시작 뒤에도 나중 지시 경고가 남아야 한다. 수집 시작과 수신 사이에 지시를 넣어 그 지시를 반영했다고 세지 않는 차례를 고정한다. 잘못된 목록·코드 예제·중복 절·항목/길이 상한은 마지막 정상 목록을 보존한다.
- 같은 host 시험은 실제 harness 사건으로 같은 활동의 진행률 갱신·정기 보고에서 활동 시작을 유지하고, 활동 변경·새 세션·대기 답변에서 초기화하는지 검사한다. 오래된 진행 사건도 새 활동 시계를 덮지 못한다.
- `node --test web/test/summaryCard.test.mjs`: 시작 후/활동 시간의 분리·진행률 미정·옛 host의 활동 시작 누락·끝난 thread 시간과 음수 경계를 확인한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 28-thread-summary --workers 1`: 실제 agent의 보고 감시→host 수집→열린 hover/상세 카드의 목록·완료 수·경고 갱신을 확인한다. pane 안 실제 CLI의 활동·진행률 갱신, 가상 시계의 경과 시간, coordinator 목표만 편집할 때 진행 보존, Chromium 320px·WebKit 390px Reader 표시·쓰기 거부를 함께 검사한다. 고정 sleep·실패 재시도·시한 연장은 쓰지 않는다.
- 결함 심기: 보고 수집 owner의 진행 전달 호출을 제거하면 실제 수집 시험이 진행 항목 `1 != 2`로 실패한다. 제품 원본 바이트를 복원한 뒤 전체 gate를 수행한다.
- 첫 전체 gate는 319초로 web·fmt·build·clippy·Rust 전체·public을 통과했고 E2E는 84/85 파일 통과다. 82 자동화 만들기의 저장 단추가 비활성 상태로 30초를 넘겼다(같은 실행 중 1분 부하 77.89). 저장 실패 때 주소·이름·입력값·비활성 상태를 남기는 진단만 더한 동일 제품의 단독 82 전체는 29초에 통과했다. 재시도·시한/기대값 변경으로 첫 실패를 지우지 않으며 원인은 미확정이다. 착지 통합 검증에서 원본과 진단을 함께 확인한다.
- 전체 필수 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 저장소 검사다. FRONT_API는 1.104, Agent link는 1.11을 유지한다.
- 착지 기준 `8ddca9dc` 통합본은 전체 ci-local 217초에 일곱 단계 모두 통과했다. 웹 유닛 471개·Rust 전체·의존성 경계·웹 E2E 85개 파일(96개 작업)·공개 검사를 확인했다. E2E 28 전체와 82 전체(20.5초)가 통과했고 저장 단추 실패는 재발하지 않았다. 이 실행의 기록한 1분 부하는 10.69~86.22이며 시한 초과나 단독 재실행은 없었다. 최초 82 실패의 원인은 여전히 미확정으로 보존한다.


## PR routine 재변환과 보존 사본(t-0684, plan §8.421)

- `nice -n 15 cargo test -p aaa-host --lib automation::tests:: --locked`: 영문 보존 사본+한글 routine+TOML 없음의 격리 파일로 기존 사본 충돌을 재현한다. 같은 사본 재사용·다른 판의 첫 빈 번호·실행 실패 복구에서 기존 사본 보존·symlink 거부·보존 위치 변경의 hash 및 무쓰기 거부·기존 TOML 충돌을 검사한다. 변환된 PR의 events·enabled·owner 메시지·본문도 원본과 비교한다.
- `archived_followup_without_toml_is_not_revived_by_doctor_or_create`는 변환 뒤 TOML이 없는 경우 `files_problems`가 옛 규칙을 만들 대상으로 판정하던 RED를 보존한다. 실제 host의 doctor와 create가 보존 사본과 원래 프로젝트 파일을 지키는지 검사한다. 기존 TOML이 있는 keeper도 유지한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs --spec 40-automation-detail --workers 1`: 실제 host의 미리보기 문법 오류·계획 변경 실행 오류를 화면 제목과 alert에서 확인하고, 원인을 고친 뒤 새 미리보기를 읽어 변환한다. 동일 사본 재사용·PR 두 판 보존·doctor 후 옛 규칙 부재와 Reader 미리보기 전용을 함께 확인한다. 고정 sleep·실패 재시도·시한 연장 없이 응답과 DOM 사건으로 끝낸다.
- 운영 프로젝트는 수정하지 않는다. 전체 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다.

## install macOS launchd owner(t-0485, plan §8.419)

- `nice -n 15 cargo test -p aaa --bin aaa --locked cli::tests::launchd_registration_authorization_arguments_parse_before_any_service_changes -- --exact`: 실제 등록 owner와 공유한 승인 argv를 실제 `Cli::try_parse_from`으로 검사한다. `--prompt plain`은 install 상위 옵션이므로 authorize 앞에 둔다. 서비스 등록 없이 잘못된 순서 RED→올바른 순서 GREEN을 남긴다.

- `nice -n 15 cargo test -p aaa --bin aaa install::engine --locked`: 실제 임시 파일 receipt와 서명 승인,
  launcher package 적용 뒤 codesign, 보존 launcher의 승인 뒤 편집 무쓰기 거부,
  같은 ID 재개·공유 instance·외부 편집/가짜 ready 거부를 검사한다.
  OS manager만 대체하며 파일 transaction·ready 판정·journal은 실제 owner를 사용한다.
- `nice -n 15 cargo test -p aaa --test install --locked`: 읽기 launchd 계획과 package 누락의 무쓰기 경계를 포함한다.
  실제 등록 시험 하나는 로컬에서 ignored이며 운영 LaunchAgent를 변경하지 않는다.
- `launchd_authorization_and_apply_preserve_the_reviewed_service_environment_without_registering`은 실제 PTY 승인 뒤 apply와 같은 명령 환경의 dry-run 해시를 비교한다. TERM 누락의 RED와 같은 TERM의 GREEN, 다른 TERM의 해시 변경·무쓰기를 검사한다. 실제 등록은 실행하지 않는다. 같은 canonical state에서 host의 agent/front listener 두 개도 실제 bind/drop한다. macOS의 긴 기본 임시 경로가 UDS 한도를 넘는 RED 뒤, 기존 backend owner처럼 짧은 `/tmp` 실험실 root로 GREEN을 남긴다.
- 승인된 기존 macos-15 CI 단계는 `cargo test -p aaa --test install --locked launchd::launchd_real_registration_ready_resume_and_second_instance_preserve_the_shared_launcher -- --ignored --exact --nocapture`를 필수 실행한다.
  고유 두 label의 실제 등록·ready·bootout 뒤 같은 ID resume·두 번째 instance와 launcher 보존·정상 해제, 실패 시 자기 label만 정리한다.
  GUI 도메인이 없으면 실패이며 skip/fake-ready로 바꾸지 않는다. 다른 OS에서는 Mac native owner만 cfg 제외한다.
- release producer/manifest는 `nice -n 15 python3 -m unittest scripts.test_release`, 실제 app producer→consumer는 위 파일 owner가 맡는다.
  package 판/CPU/binary 결속 제거·외부 plist 덮어쓰기 변형은 owner 실패를 남겨야 한다.
- 서명 RED는 번들 안에 영구 자원 lock/reference가 생겨 codesign이 거부한 것이다. 메타데이터를 번들 바깥에 두어
  파일별 journal·공유 소유권을 보존하며 서명 GREEN을 확인한다. 전체 ci-local·deps·공개 검사·Windows cross 뒤 후보를 보고하고 착지 창을 기다린다.


## 계정 배정 대기 취소의 경합(t-0700, plan §8.428)

- `nice -n 15 cargo test -p aaa-host --lib account_pool --locked`:
  `cancel_waits_for_allocation_and_cannot_be_overwritten_or_restarted`는 실제 `pool_run`을 사용량 cache 잠금의
  await에서 보류하고, 배정 잠금을 잡은 사건 뒤 실제 Front 요청으로 취소한다. 수정 전의 즉시 BadRequest를
  재현하며, 수정 뒤에는 취소가 기다려야 한다. 조회를 풀어 실제 Queued 저장이 끝나면 Cancelled를 기록하고,
  다음 pass·같은 ID 재처리·DB 재열기 뒤에도 취소와 thread 0건을 유지한다. 운영 코드에 지연 고리는 넣지 않는다.
- `cancel_rechecks_a_receipt_committed_by_the_inflight_allocation`은 기다리는 사이 배정 영수증이 생기면
  기존 「이미 시작한 thread는 thread에서 끝낸다」 conflict를 보존하는지 검사한다. Reader의 잠금 대기 전 쓰기 거부,
  없는 요청의 NotFound·중복 취소의 같은 결과도 확인한다. 고정 sleep·재시도·시한 변경은 없다.
- 기존 웹 E2E81의 취소 click·창 닫힘·Cancelled·다음 tick에서 추가 기동 없음 단언과 30초 시한을 유지한다.
  실패하면 창의 오류·대기 문구·시작 상태와 host의 배정 요청 목록을 로그에 남긴다.
  전체 `nice -n 15 bash scripts/ci-local.sh`에서 다른 spec과 병렬로 검사한다.

- 착지 때 Linux 대상 `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --locked --keep-going -- -D warnings`도 확인한다. main install 엔진의 `File` import는 실제 사용 범위인 Windows·macOS·시험 cfg에만 둔다. Linux 제품의 미사용 import 실패를 경고 억제 없이 수리하며 Linux 실행 검증과 교차 lint는 구분한다.

- Windows CI 정리 실패 수리(t-0709): 가짜 머신의 `agent_connected`가 예약한 `rewatch`는 `Arc<Host>`와
  SQLite 연결을 보유한다. 두 연결 시험은 전용 current-thread runtime을 Lab보다 먼저 종료해 예약 작업을
  해제하고, Lab 정리는 `Weak<Host>`로 실제 Host 종료를 확인한 뒤 `remove_dir_all`을 한 번만 수행한다.
  기존 `tokio::test` 수명에 종료 단언만 더하면 macOS에서도 취소 경합 시험이 RED다. 취소 순서·Queued와
  Cancelled·다음 pass·재전송·DB 재열기·thread 0건과 할 일 한 건의 단언은 유지한다.
  원본 시험의 추가 진단에서도 재열기 Host의 명시 drop과 Weak 소멸 뒤 최초 Host의 strong count는 2로
  남아 정리 단언이 실패한다. 재열기 연결이 남았다는 가설과 예약 작업의 수명 문제를 구분한다.
  고정 sleep·정리 재시도·오류 무시는 없으며, Windows 교차 build/clippy와 실제 Windows CI 결과를 구분한다.

## host 공통 계정 풀 자동 배정(t-0653, plan §8.416)

- `nice -n 15 cargo test -p aaa-host --lib account_pool --locked`: 창별 최소 여유·모델 창·동점·공유 한도,
  네 동시 transaction의 예약 두 건·영수증 재생, 미관측 첫 실행·실패/오래됨/reset/NaN/누락 제외,
  확정 준비 실패 해제와 spawn 결과 불명 보존, 사람 권한·설정판 충돌·삭제 시 풀 제외를 검사한다.
  다른 프로젝트의 과거 DB 기록도 공유 한도 셈에 포함하며, 현재 host의 단일 프로젝트 제한은 유지한다.
  그룹 변경 뒤 기존 예약, 종료 전/후 관측·다시 로그인 뒤 예약 교체와 프로젝트 이름 변경/삭제의
  참여·대기 이동 및 전역 풀·관측 보존도 검사한다.
- `nice -n 15 cargo test -p aaa-host --lib quota --locked`: 공식 RPC의 실제 가짜 자식에서 무작업 조회,
  누락과 잘못된 숫자 구분·인증 세대·backoff·임시 인증 정리와 A2의 창별 관측 계약을 유지한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs --spec 81-account-pool --workers 1`:
  실제 host·agent·session·PTY에 가짜 Claude 상태줄을 연결한다. 웹 풀 편집·프로젝트 참여·신규 계정
  미편입, 기존 기본/명시 우선, 실제 관측의 자동 선택·같은 영수증, 미관측 첫 실행 뒤 대기 한 건,
  host 재시작 뒤 관측·대기 보존과 새 관측 뒤 기동, 웹 할 일의 배정 대기·취소, WebKit 390px·Reader
  쓰기 거부·인증 비노출을 검사한다. 입력·상태줄 파일·host 응답·DOM 사건으로 끝낸다.
- 체크박스 E2E의 원본 실패는 비동기 저장 중 controlled checkbox가 즉시 옛 값으로 되돌아가는
  제품 결함이었다. 저장 중 선택값을 유지하고 실패 때 원래 값으로 돌아가게 고치며 같은 check 단언을 유지한다.
- 통합 gate의 CLI `large_enum_variant`는 시작 인자를 `Args` 구조체와 `Box`로 분리해 고친다.
  rollout의 연속 host 재시작 실패는 정상 연결이 10초 미만이면 이전 지수 backoff가 남아 원격
  agent가 30~45초 기다리는 것이 원인이었다. `aaa-agent`의
  `authenticated_short_connections_reset_backoff_but_rejections_preserve_it`는 실제 Welcome/replay
  사건과 Reject로 성공 뒤 250ms 초기화·인증 거부 뒤 기존 30초 보존을 확인한다. 초기화 제거
  변형은 `30s != 250ms`로 실패해야 한다. 기존 m13 `rollout_shared_host_agent_recovery_and_inflight_cancel_preserve_baselines`의
  30초 상한·두 agent 준비·복구/PID/파일 단언은 변경하지 않는다. main의 원격 fixture 재연결
  상한 1초도 보존한다. 이 시험 설정과 제품의 성공 사건 초기화는 별개이며 keeper는 상한과
  무관한 초기 30초를 사용한다.
- 고정 sleep·실패 재시도·시한 증가 없이 검사한다. 전체 필수 검증은 정본 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다.

## Windows 짧은 state 경로와 IPC 완료 수명(t-0485, plan §8.418)

- 기존 Windows IPC owner의 모든 주소/반복·DACL·peer PID·byte·동시 첫 instance 거부·동일 주소 단일 rebind를 유지한다. GetShortPathNameW가 돌려준 실제 부모/canonical 양방향도 검사한다. 8.3 이름이 없는 파일시스템에서는 API가 같은 긴 이름을 돌려주며, 해당 환경에서 8.3 차이를 검증했다고 부르지 않는다.
- EOF 뒤 Rust 값 drop은 IOCP 합성 readiness가 소유한 Arc 해제와 다르다. 연결을 소유한 전용 runtime을 종료해 completion drain을 끝낸 뒤 새 runtime에서 같은 이름으로 한 번 rebind한다. 재시도·sleep 없이 실제 첫 instance 획득을 단언한다. 기존 연결/EOF 3초와 BUSY250ms·승인60초는 유지한다.
- 3933e4cb 실제 Windows RED를 보존한다. 로컬 macOS install·전체 ci-local·Windows cross clippy/build·check-public을 확인하고, Windows 실제 통과는 승인 착지 뒤 main CI만 판정한다.

## Windows install pairing·IPC 해제 owner(t-0485, plan §8.415)

- 원문 c249e316 Windows run의 pairing 조기 종료와 IPC bind OS5를 보존한다. PTY 실패 끝은 answered/전체·needle·child_exit, IPC 실패는 조합/반복·경로·OS 오류/해제 단계로 구분한다. 기존 승인/수명/비밀 가림 단언과60초를 유지한다.
- IPC의 기존3주소 조합×3연결을 유지하고 mixed/canonical 양방향을 더한다. 각 연결에서 byte 일치 후 client를 닫고 server EOF를 관측한 뒤 server를 닫는다. listener를 닫은 뒤 같은 주소 첫 instance를 단 한 번 재bind한다. drop만으로 handle 해제를 추정하지 않는다. 기존 연결3초·BUSY250ms를 유지하고 EOF도3초 사건 한도 안에서 기다린다. 재시도/sleep/시험 제외/기대값 완화는 없다.
- Windows 실제 통과는 main CI에서 확인한다. 그 전 cross는 컴파일/lint, macOS는 해당 OS의 회귀 검증으로 구분한다. rollout 수리는 t-0654 소관을 유지한다.

## 보고서 목록 첫 화면(t-0656, plan §8.414)

- `cargo test -p aaa-host --lib library_links --locked`: 모아 읽은 목록과 기존 단일 보고 조회를 비교한다. 같은 이름의 두 판·r 첨부 보관·끝난/진행 중 thread·원격에만 있는 파일·실시간 변경/원본 소실·안 봄 해제·링크 폐기 후에도 크기·보호·판·공유 표시가 같아야 한다. 기존 삭제·보존·링크 경계도 유지한다.
- `cargo test -p aaa-host --lib report_ids --locked`: 예약/완료/삭제 판의 분리와 보관 첨부의 원본 보호를 확인한다.
- `node web/e2e/web.e2e.mjs <캡처 폴더> --spec 80-report-list,21-library-reports,73-report-ids --workers 1`: 실제 host의 격리 DB·manifest에 1,118개 사본 폴더·559개 원본·최대 954개 첨부·보관 120건을 넣는다. 원문 바이트는 만들지 않으며 운영 library는 읽거나 수정하지 않는다. 요청 시작부터 JSON 수신까지와 두 animation frame 후 첫 화면 시간을 `report-list-timing.json`으로 남긴다. 시간은 관측값이며 부하에 민감한 통과 시한으로 쓰지 않는다.
- 첫 50개·추가 50개·끝까지 보기·화면 밖을 포함한 전체 선택, 보존 판을 별도 줄로 세지 않음을 확인한다. Chromium 1280px·WebKit 390px Reader의 실제 용량 응답을 route gate로 보류하고 목록이 보이는 사건을 먼저 기다린다. 용량 실패 때도 목록을 유지하며 가로 넘침·Reader 조작 부재를 검사한다. 고정 sleep·재시도·기대값 완화는 없다.
- 통합 검증의 `turn_end_notices_follow_reports`는 현재 brief 턴의 Working을 읽은 사건 커서 뒤에서 Finished를 고른다. 원본 실패에서는 앞 trust 턴의 Finished(현재 Working보다 1.2초 이전)를 처음 기록부터 다시 골랐다. kind·thread·짧은 턴·발생 시각·그 뒤 보고/일반 턴의 단언과 시한은 유지하며 실제 host·agent로 확인한다.
- 전체 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사로 수행하고 착지 창을 기다린다. 운영 host 접근이 없는 실험실 결과를 운영 37GB 실측으로 부르지 않는다.


## 직접 병합한 thread의 할 일 완료 보관(t-0651, plan §8.412)

- `cargo test -p aaa-host --lib task_clean --locked`: 기존 PR·표지·중단·하위 task 판정과 명시한 병합 커밋 줄 파서를 검사한다. 일반 검증 SHA·코드 예제는 병합 근거로 읽지 않는다.
- `cargo test -p aaa --test m13 task_main_merge --locked`: 실제 host·agent·로컬 bare origin에서 `[착지]` branch 병합과 보고의 축약 병합 커밋으로 완료 보관한다. 오래된 origin/main·push 없는 로컬 main 병합·미병합 branch·보고 없음·사본 버림을 구분한다. 종료 응답 직후 attention 없음·active task 제거·archive 근거·coordinator inbox 한 건·중복 판정 없음과 로컬 ref/FETCH_HEAD 보존을 검사한다.
- `node web/e2e/web.e2e.mjs --spec 47-task-thread --workers 1`: 실제 Git 저장소의 직접 병합·미병합 thread를 끝내고, 병합된 할 일의 완료 보관과 「답할 것」 카드 부재, 미병합 할 일의 확인 카드 유지를 DOM 사건으로 확인한다. 기존 완료 복원·보고 없음·하위 task·시작 권한 검사를 유지한다.
- 고정 sleep·실패 재시도·시한 연장 없이 보고 회수·ticker·DOM 사건으로 끝낸다. 전체 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 저장소 검사다.



## install Windows Job·부모 종료·state IPC(t-0485, plan §8.411)

- main23328f7e Windows install의 원문6실패를 보존한다. 네 worker 실패는 OS5·Job0x2000이며 생산의 제한 거부는 맞는 동작이다. install 시험 실행기는 자기 nested Job만 breakaway 허용으로 만들고, worker 수명 owner는 별도 허용/금지 Job을 모두 검증한다. 바깥 runner 제한은 변경하지 않는다.
- 부모 종료는 소유 Job을 실제 TerminateJobObject(code1)로 끝내고 닫는다. 기존 부모 비정상 종료·worker 생존·worker 자신의 exit3 내구 기록·같은 ID 재개/읽기 status·금지 시 무쓰기 단언을 유지한다. timeout은 늘리지 않는다.
- 기존 Windows IPC ACL/peer/byte owner는 동일 경로3회에 일반/canonical 양방향3회씩을 더한다. 기존 주소를 먼저 열고 NotFound만 같은 실제 디렉터리의 별칭을 확인한다. 기존 BUSY·없는 pipe·첫 instance 보호도 계속 검사한다. Windows 실제 GREEN은 main CI만 인정한다.

## 계정별 Codex 대화 기록(t-0634, plan §8.407)

- `cargo test -p aaa-agent --lib chat_paths_tests --locked`: 저장된 CODEX_HOME·계정 홈 우선순위와 hook/rollout ID 불일치의 실제 JSONL을 검사한다. 같은 파일의 301줄 페이지와 ChatChanged 사건까지 확인하며, 중복 후보·다른 작업 폴더·하위 agent·이전 실행 기록·공유 홈 추측을 거부한다. 원래 경로 해석으로 되돌리면 세 keeper가 실패한다.
- `cargo test -p aaa --test m13 accounts::account_credentials_parallel_refresh_and_all_records_are_redacted --locked -- --exact`: 기존 로컬/원격 TLS 네 작업의 인증 회전·기록·재개·비노출을 유지하면서 서로 다른 hook/rollout ID를 넣는다. 실제 인증·운영 설정·모델 작업은 쓰지 않는다.
- `node web/e2e/web.e2e.mjs <캡처 폴더> --spec 78-account-chat --workers 1`: 서로 다른 계정의 thread별 홈에 같은 ID 기록을 만들고 Chromium 1280px·WebKit 390px에서 조회 실패 중 Codex 머리말, 실제 PTY F6 입력 뒤 JSONL 생성과 자동 갱신, 이전 301줄·새 글·계정 분리를 확인한다. DOM·PTY 출력·기록 감시 사건으로 끝내며 고정 sleep·재시도·시한 연장은 없다.
- `node --test web/test/session-cleanup.test.mjs`: 실제 세션 트리의 PID 소유 확인·자손 종료와 다른 트리 보존과 배포 감시 후보의 명령줄 대조를 검사한다. 전체 프로세스 표는 PID·부모·상태만 읽어 다른 작업의 긴 명령줄에 출력 한도가 소진되지 않게 한다.
- 통합 E2E77의 삭제·종료 후 대화 복귀는 원문 표시와 링크 없음을 함께 기다린다. 비어 있는 대화에서 부정 조건만 먼저 통과하던 원본 실패를 보존하고, 같은 원문 단언·시한·제품 동작을 유지한다.
- 전체 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 저장소 검사다.



## Git 자동화의 자기 착지 제외(t-0633, plan §8.406)

- `nice -n 15 cargo test -p aaa-host --lib git_watch --locked`: 격리 host DB와 GitQuery frame으로 실제
  git_repo·내구 outbox 전달을 끝낸다. 자기 규칙의 병합 여러 개와 내부 일반 커밋만 있으면 입력 0건·기록과
  routine list의 `자기 착지`, 사람 병합 혼합·다른 규칙 병합·두 규칙 혼합이면 입력 1건을 확인한다.
  승인한 hash·승인 상태 보존과 다음 같은 head 조회의 중복 입력·기록 없음도 확인한다.
- 병합 없음·없는 thread·번호 없는 병합·잘린 커밋 목록은 거르지 않는다. forced/deleted·다른 동작과 다른
  프로젝트의 출처도 거르지 않는 경계를 검사한다. 80자로 잘린 표시용 제목은 원문의 owners로 판정한다. 고정 sleep·재시도·시한 연장 없이 조회 응답으로 끝낸다.
- 자기 착지 판정을 제거한 변형은 자기 병합만 있는 경우의 큐 단언에서 실패해야 한다. 제품 바이트 복구 뒤
  전체 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사를 실행한다.

## 할 일 깊은 주소와 ID 자동 링크(t-0624, plan §8.404)

- t-0697: main CI run `39693201281`·`39699498898`의 원문과 artifact `e2e.log` 모두 깊은 주소 이동 전 `getByRole('menuitemcheckbox', { name: /^작업 폴더/ })`에서 30초를 넘겼다. 목록 도착 전 연 저장소 메뉴가 빈 항목을 복사해 둔 제품 경합이다. E2E77은 새로고침 전 실제 host의 `tasks` 응답을 gate로 보류하고, 「전체」만 있는 메뉴를 연 뒤 응답을 풀어 행이 나타나는 사건을 기다린다. 메뉴를 다시 열지 않고 「작업 폴더」를 선택하고 거르기 적용을 확인한다. 수정 전 같은 locator의 30초 실패를 재현했으며, 제품은 열린 저장소 메뉴의 항목·개수·선택 함수를 최신 board에서 만든다. 기존 깊은 주소·새로고침·다른 프로젝트·없는 번호·WebKit·Reader 단언과 시한은 유지한다. 고정 sleep·재시도·시한 연장은 없다.
- `node --test web/test/appIds.test.mjs`: 네 종류 ID·조사·문장 끝·5자리 번호·없는 번호, 주소·경로·낱말 경계와 프로젝트 있는/없는 할 일 주소를 확인한다.
- `node web/e2e/web.e2e.mjs --spec 77-app-ids --workers 1`: 실제 host에서 thread·할 일·coordinator 질문·보고서를 만들고 대화·보고서 본문의 네 ID를 누른다. 직접 주소·프로젝트 주소·새로고침·보조 키 새 탭·뒤로, 코드 블록/주소/없는 번호 제외, 답한 질문·삭제/복원·완료 보관을 확인한다. WebKit 390px·Reader도 첫 주소·상세 닫기·보고서에서 할 일 이동·가로 넘침을 검사한다.
- `cargo test -p aaa --test m13 doctor_files_brings_the_coordinator_guide_to_existing_projects --locked -- --exact`는 새 프로젝트와 `doctor files --fix`로 고친 기존 프로젝트의 ID만 쓰기·답할 질문 링크 규칙을 함께 검사한다. host의 `brief_text::tests::brief_order_and_memory_cap`도 새 brief의 규칙을 확인한다.
- 처음 구현의 thread 링크는 짧은 선택을 직접 쓰는 경로에서 hashchange와 정규화가 경합해 빈 화면에 머물렀다. E2E77의 네 종류 이동 단언이 이를 재현했다. 기존 thread 선택 함수로 내부 project/id를 고르고 같은 단언을 유지한다. Reader는 편집 폼 전용 표지 대신 실제 상세의 ID 글자를 대조한다.
- `d-9999`라는 이름으로 보고서를 발행해도 없는 할 일이 링크가 되면 안 된다. E2E77은 이 이름 충돌을 실제 publish로 만들고, 보고서의 실제 r 번호와 할 일 ID를 구분하는지 검사한다.
- 끝난 thread의 답하지 않은 질문은 보존 카드가 없으면 일반 글로 돌아간다. 기존 후보 binary에 새 E2E77을 실행해 해당 단언 RED를 보존했고, 열린 카드·답한 기록으로 존재 여부를 정한 후보에서 같은 단언 GREEN을 확인했다. 답한 질문 링크는 보존 기록으로 계속 열린다.
- 첫 전체 검증은 머신 부하 200~250에서 E2E72의 실제 SSH 감지가 시간 초과했다(host DB의 실패도 SSH 시간 초과). 부하가 내린 뒤 SSH 코드·시한·단언을 바꾸지 않은 단독 E2E72는 통과했다. 원본 실패를 보존하고 최종 전체 검증은 지원하는 worker 설정으로 동시 실행 수만 줄이며 모든 시험을 유지한다.
- 중간 검증에서 설치 승인 시험 실행(21:03:04~45)과 같은 target의 실행 파일 재빌드(21:03:10~27)가 겹쳤고, 해시를 묶는 승인 발급 두 시험이 종료 코드 4로 실패했다. binary를 고정한 두 시험과 후속 전체 설치 24개는 통과했다. 전체 gate가 도는 동안 같은 target의 aaa를 다시 빌드하지 않는다. 설치 제품·시험 시한·단언은 바꾸지 않는다.
- 고정 sleep·실패 재시도·상한 연장 없이 DOM·host 사건으로 끝낸다. 전체 정본은 `nice -n 15 bash scripts/ci-local.sh`다.

## install 종료 통지의 reap 독립성과 Windows 실패 진단(t-0485, plan §8.403)

- `authorization::terminal_exit::owner_reaping_after_subscription_preserves_the_child_exit_event`는 실제 shell 자식에 종료 구독을 준비하고, 실제 kill/wait로 먼저 reap한 뒤 같은 종료 통지가 와야 한다. 기존 waitid 변형은 errno10/ECHILD로 RED이고 구독 고정 후 GREEN이다. sleep·가짜 자식·production 허용 seam을 쓰지 않는다. Unix 전용인 까닭은 소유 reap과 waitid의 경합이며, Windows는 기존 실제 handle/ConPTY owner를 계속 돈다.
- `tests/support/child_exit.rs`의 구독은 driver callback 전에 고정한다. macOS kqueue/Linux pidfd는 reap하지 않으며, 이미 끝난 자식의 상태는 실제 소유 Child가 수집한다. Windows는 실제 process handle을 복제한다.
- Windows `input::password` console owner도 같은 구독을 쓴다. 실제 성공/취소/EOF·암호 일치/echo 없음/mode 복구·최종 출력을 유지하며, 기존20초를 늘리거나 사례를 빼지 않는다. install driver의 기존60초와 마지막300줄·종료7 owner도 유지한다.
- Windows 실패 단언은 redacted JSON·stderr·부모 종료 출력을 포함한다. worker spawn 실패의 OS 오류/Job 제한 비트와 human needs_user 요구가 CI 원인 판정 근거다. 기존 기대값/승인·private 표시·Job breakaway 정책은 유지한다. Windows 교차 clippy/build는 compile/lint 판정이며 실제 성공은 main CI로 구분한다.

## 원격 보고의 해시와 첨부 실패 격리(t-0638, plan §8.401)

- 후속 t-0654(plan §8.413)의 `orch::report_tests`는 `.claude`·`.reports` 하위와 경로 이탈을 거부하면서 최신 본문·불변 판이 도착하는지 검사한다. 판 저장 경로를 일반 파일로 막아도 본문·단계·정상 첨부를 갱신하고, 같은 hash의 ticker 복구가 ack를 보존하는지 확인한다. 수정 전 두 keeper는 각각 경로 거부·본문 미갱신으로 RED였다.
- `report_ids::tests::snapshot_`은 보존 판의 숨은 성분·symlink·크기 불일치를 계속 거부하고 깊은 일반 경로는 허용하는지 검사한다. t-0490의 크기 오류는 manifest 작성 후 바뀐 host 첨부에서 재현하며 크기 기대값을 완화하지 않는다.
- `cargo test -p aaa --test report_copy --locked`의 판 보존 사례는 실제 host·CLI·Agent link와 격리 SQLite의 판 공개 실패 trigger를 쓴다. 새 본문·경고 한 번·작업 상태·실패한 r 비공개, trigger 제거 후 새 hash나 변경 사건 없이 ticker로 같은 본문을 불변 판에 보존하는지 검사한다. 운영 DB·파일은 바꾸지 않는다.
- 판 복구는 보고 목록을 갱신하되 같은 본문·첨부의 sandbox 경고를 반복하지 않는다. 첨부 변경과 판 생성 신호를 합친 초기 후보는 실제 경고 2건으로 RED였고 신호 분리 뒤 같은 시험으로 확인한다.
- 후속 전체 검증에서 rollout의 마지막 host 재시작은 로컬 agent만 연결되고 원격은 준비 30초를 넘겨 실패했다. 원격 fixture의 `node()` 재생성이 기본 재연결 상한 60초를 적용했고, 10초보다 짧은 성공 연결·종료가 반복되며 16초 다음 32초 backoff로 커졌다. rollout fixture만 로컬과 같은 재연결 상한 1초를 명시한다. 제품의 backoff·요청/준비 시한·기존 rollout 단언은 유지하며 실패 로그를 보존한다.
- link의 사본 시험은 가짜 agent의 취소 완료와 host 연결의 정상 종료를 기다린 뒤 host를 놓고
  임시 폴더를 지운다(t-0647). 취소 요청만 하고 지우면 Windows에서 열린 DB 핸들 때문에
  정리가 os error 32로 실패할 수 있다. 재시도·sleep·정리 오류 무시 없이 기존 삭제 단언을 유지한다.
  Windows 실행 판정은 main CI의 해당 시험 결과로 하며 macOS 통과·교차 빌드와 구분한다.
- `cargo test -p aaa-host --lib report_ --locked`의 실제 Agent link keeper는 등록한 가짜 인증을 포함한
  원본 전체의 SHA와 가림 뒤 SHA가 다름을 재현한다. 원본을 변경하지 않은 채 512KiB 경계와 마지막 조각을
  전송해 먼저 검증하고, host의 현재 사본·불변 r 판에는 가린 바이트만 저장하는지 확인한다.
  원본 판 hash와 저장 본문 hash를 나누어 host 재시작의 단계 복구도 유지한다. 일반 파일 보기 가림 검사는 그대로다.
- `orch::report_tests`는 append prefix·rewrite 거부·이전 검증 파일 보존·저장 오류의 기존 검사를 유지하며,
  첨부 hash가 달라도 최신 보고 본문과 ack가 유지되고 실패 파일을 불변 판에 넣지 않는지 확인한다.
  같은 원본 판 재수집은 본문·저장 hash를 복구하면서 ack를 보존한다. 한 thread의 수집 lock을 잡은 상태에서
  다른 thread의 실제 수집이 끝나야 한다.
- `cargo test -p aaa --test report_copy --locked`는 실제 host·CLI·가짜 Agent link에서 manifest 실패와
  파일 하나의 rewrite 실패를 구별한다. rewrite 실패 중에도 새 보고·정상 첨부가 도착하고, 새 보고 hash나
  변경 사건 없이 ticker가 실패 첨부를 복구해야 한다. 경고 한 번·작업 상태 보존·기존 시한은 유지한다.
- `cargo test -p aaa --test m13 remote_report_with_registered_auth_bytes_copies_without_false_hash_failure --locked`는
  실제 TCP+TLS agent에 profile 인증 env를 등록하고, 바뀌지 않은 328KiB 로그와 보고를 회수한다.
  원본 보존·원본 판 hash·가린 최신/불변 사본·host 재시작 뒤 병합 대기 단계를 확인한다.
- 고정 sleep·시험 재시도·시한 증가 없이 수집 완료·보고 hash·연결 사건으로 끝낸다. 전체 검증은
  `nice -n 15 bash scripts/ci-local.sh`이며, 착지 때 main의 절 번호를 다시 맞춘다.



## 질문 카드의 강조·번호 목록·누락 경고(t-0618)

- `cargo test -p aaa-proto --lib --locked`: `m3-questions.md`는 host가 보존한 t-0578 답변 당시 Questions 원문이다.
  `1. **M3 순서**: …`, `- **A(추천)** …`, `2. **M3 끝까지 시험할 곳**`의 질문 2개·선택지 3/2개·추천과
  강조/간격 변형을 확인한다. 초기 구문 재현 fixture는 수정 전 첫 선택지 누락에서 RED였고 원문으로 교체했다.
- `t0621-questions.md`는 t-0621 Questions 원문이다. A~D와 각 들여쓴 그림 뒤에 E·F 설명·그림이 있어도
  질문 2개·선택지 4/3개·D 그림과 뒤 설명을 분리해 보존한다. 수정 전 같은 원문에서 첫 질문이 빠져 1개/3개로 RED였다.
  이때 누락 진단도 함께 발생한다. 원문에 없는 E/F 선택지를 만들지 않으며 줄을 깨뜨린 변형도 경고해야 한다.
  수정 전 제품 파서의 제어 대조에서 표를 네 행으로 줄이거나 들여쓴 그림을 지워도 누락됐다. 뒤 E/F 문단·그림만 지우면
  여섯 행 표와 들여쓴 그림을 둔 채 질문이 읽혔다. 실제 A~F 여섯 선택지도 읽혔다. 원인은 네 개 상한이 아니라
  뒤의 열 0 문단·그림에서 역방향 목록 탐색이 멈춘 것이다. 실제 상한은 9개이며 2~4개는 안내의 권장 형식이다.
- 파서 진단은 code fence 밖 A~Z 후보 줄 수·카드 수, 질문 수·선언 수와 번호/글자 순서를 비교한다.
  빠진 A·중복 A·A/C·인식 못한 강조·질문/선택지 상한·선택지 하나뿐인 질문은 경고다.
  code fence의 선택지 예시는 세지 않는다. `###` 본문의 일반 번호 목록을 보존하고 선택지 뒤의 `N.`만 다음 질문으로 읽는다.
  선택지가 하나뿐이거나 전부 읽히지 않아도 질문 제목과 경고를 남긴다. 9개 상한 초과도 제목과 경고를 유지한다.
- `cargo test -p aaa-host --lib question --locked`: 실제 보고 회수와 DB 경계에서 복사 성공·경고 유지,
  thread 전체 답과 q-ID 일부 답 거부(초안도 없음), 같은 보고 중복 경고 방지를 확인한다.
  이전 host가 다르게 읽은 미답 카드도 재기동 때 안전하게 잠그며 명시한 Questions를 기본 검수로 덮지 않는다.
- `node web/e2e/web.e2e.mjs --spec 53-thread-questions --workers 1`: 실제 host·agent·Chromium·WebKit에서
  M3 2개 질문·3/2개 선택지·추천, 누락 경고·전체 누락 카드·웹/CLI 거부, thread inbox JSON과
  coordinator 경고, 수정 뒤 `1C 2B`와 원래 선택지 설명 전달을 검사한다. 같은 파일의 기존 그림·답 보존 검사도 유지한다.
  같은 파일의 t-0621 원문은 글·표를 그대로 쓰고 그림 bytes만 실험실 PNG로 대체한다. A~D 그림, 뒤 E/F 설명·그림,
  질문 2개·선택지 4/3개, E 직접 답과 답한 카드의 뒤 그림 보존을 확인한다.
  스크린샷 폴더를 인자로 주면 정상 데스크톱/휴대폰 카드와 형식 경고·선택지 뒤 설명을 남긴다.
  고정 sleep·재시도·시한 연장은 없다.

- 첫 gate 도중 별도 Cargo 빌드를 겹치면 install 승인 검토 중 실행 파일이 교체될 수 있다.
  실제 install owner가 승인 발급에서 exit 4로 실패했고, 격리 사본을 질문 표시 사건 뒤 교체하면
  `installation_content_conflict`/4, 그대로 두면 0으로 제어 재현했다. 실행 시각과 실행 파일 교체 시각도 겹쳤다.
  최종 검증은 빌드를 끝내고 시험을 시작하며, 시험 중 같은 Cargo 실행 파일을 다시 빌드하지 않는다.
  승인·시한·기대값은 바꾸지 않는다.
- 병렬 전체 gate의 E2E68은 부하(load average 87 관찰) 중 안전 모드의 고정 3초 Refresh와 정상 listen이 경합했다.
  보관한 기록은 되돌리기 11:19:58Z·정상 listen 11:20:01.459Z·pane 20초 대기 실패다.
  다른 gate가 끝난 뒤 같은 시한으로 단독 실행해 8.5초에 통과했다. 관련 제품·시험 코드는 바꾸지 않았으며,
  당시 최종 전체 검증은 ci-local의 `--tee` 순차 단계를 써 Rust 시험과 웹 E2E의 부하를 나눴다.
  t-0797 이후 `--tee`는 기본 실행과 같은 병렬 경로를 쓴다.

## 입력 중 숨긴 게이지 자리 회수(t-0622, plan §8.398)

- 사용자 1A에 따라 숨긴 게이지의 폭과 보내기 단추 사이 간격을 글 칸에 돌려준다. 첫 입력·전체 삭제 때만 폭이 바뀌며, 글이 늘어 여러 줄로 접힐 때는 유지한다. 기존 §8.321의 자리 유지 단언은 이 선택으로 대체한다.
- E2E50의 실제 Claude 기록에서 보통(38%)·창 모름(84k) 게이지를 숨기면 폭이 0이고 글 칸이 게이지 폭과 간격만큼 넓어지는지 검사한다. 전체 삭제하면 원래 폭이 돌아오고 높은 사용량(86%)은 입력 중에도 폭을 유지한다. 기존 펼침·정리·iPad·휴대폰 및 700px 경계 검사를 유지한다.
- E2E66은 실제 usage가 있는 기록을 읽어 38% 게이지가 생긴 뒤 시작한다. Chromium·WebKit × 데스크톱·휴대폰 네 조합에서 빈 입력칸과 첫 입력의 폭 차이를 실제 게이지 폭·간격과 대조하고, 이전 폭에서 접히던 한글이 한 줄에 들어가는지 확인한다. 조합 입력 중·확정·긴 영문·붙여넣기·창 축소/회전·지우기의 native textarea 줄 수, 단추 옆 배치, focus와 전송 없음 검사를 유지한다. 지운 뒤에는 빈 입력칸의 폭과 줄 경계가 돌아와야 한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs --spec 50-context-gauge,66-composer-wrap --workers 1`로 실행한다. 입력·DOM·ResizeObserver 사건으로 끝내며 고정 sleep·재시도·시한 연장은 없다. 기존 E2E22·35·51과 전체 `nice -n 15 bash scripts/ci-local.sh`를 함께 확인한다.
- 수정 전 내장 웹에서 E2E66은 데스크톱 Chromium 약 73.89px·WebKit 약 74.67px의 미회수 공간과 이전 한글 첫 줄 경계의 줄바꿈으로 실패한다. 휴대폰에서는 이미 게이지가 없어 회수할 폭이 0이다. 수정 후에도 같은 폭·줄 경계 단언을 쓴다.

## 한도 초과 직접 시작(t-0619, plan §8.397)

착지 전체 검증은 E2E 정리 도구의 전체 `ps command` 출력이 Node의 1 MiB 버퍼를 넘는 ENOBUFS를 드러냈다.
전체 목록은 PID·부모·상태만 읽고, 명령줄은 실제 정리 후보 PID별로 조회한다. 기존 신원 재대조·부모 정지·자손 종료는 유지한다.
`node --test web/test/session-cleanup.test.mjs`는 관계없는 실제 프로세스 열 개의 argv 합계를 1 MiB보다 크게 만들며,
원래 도구의 ENOBUFS RED와 수정 도구의 session/writer 종료·관계없는 프로세스 보존 GREEN을 확인한다.
버퍼·대기 시한·시험 기대값은 늘리지 않는다.


- `cargo test -p aaa-host --lib over_limit_start_requires_a_direct_user_or_verified_coordinator --locked`는
  직접 CLI·웹과 다른 세션·작업자·자동화·빈/여러 줄/긴 근거를 구분한다.
- `cargo test -p aaa --test m13 explicit_over_limit_start_keeps_queue_waiting_and_requires_coordinator_user_request --locked`는
  실제 한도가 찬 상태에서 기본 시작 거부·CLI 강제 시작·coordinator pane의 근거 없는 거부와 명시 지시 시작을 검사한다.
  host 재시작 뒤 같은 요청자·시각을 보존하고, 강제 시작이 모두 자리를 반환할 때까지 예약은 시작하지 않는다.
- E2E47은 실제 host·agent와 Chromium 데스크톱·WebKit iPhone·iPad에서 추가 확인·돌아가기·실제 기동·출처와 표지·예약 대기를 확인하고
  `thread-<기기>-start-limit`·`over-confirm`·`over-started` 화면을 남긴다. 관측은 DOM·task 연결·ticker 완료이며 고정 sleep은 없다.
- 전체 필수 검증은 `nice -n 15 bash scripts/ci-local.sh`다. 기존 E2E 번호·시한·자동 예약 정책은 유지한다.
- 이 작업의 전체 실행에서는 기존 rollout transfer keeper가 원격 agent 교체 중 30초 사건 시한을 넘겼다.
  16코어에서 load average 139.27·126.31, 전송/사전 검사 99.382초였고 영속 작업은 오류 없이 Running이었다.
  같은 시한·단언의 단독 실행은 전체 100.04초·전송/사전 검사 65.354초로 통과했다.
  부하 근거와 단독 결과를 함께 기록하며 배포 로직·시험 시한은 바꾸지 않는다.

## thread 목록 상태 점 정렬(t-0620, plan §8.396)

- `node web/e2e/web.e2e.mjs <캡처 폴더> --spec 48 --workers 1`: 실제 coordinator·자동화·일반 thread를
  각각 선택하고 1280·820·390px에서 상태 칸 중심의 x 좌표가 0.5px 안에서 같은지 검사한다.
  실제 머신 연결은 유지한 채 표시 글만 짧고 긴 이름으로 바꾸어 내용이 상태 칸을 침범하지 않는지도 확인한다.
- 기존 자동화 막대·agent 표지 간격·휴대폰의 자동화 표지·규칙 이동 검사를 유지한다.
  선택 후 서랍과 DOM 사건을 기다리며 고정 sleep·재시도·시한 연장은 없다. `status-column-*.png`에 폭별 화면을 남긴다.
- 원래 데스크톱 CSS의 위치 기준을 다시 적용한 별도 실험실은 coordinator·자동화 305px, 일반 thread 339px로
  같은 정렬 단언에서 실패한다. 첫 수정의 휴대폰 선택 줄 10px 이동도 이 검사에서 잡아 메뉴의 flex 칸까지 고정했다.
- 모든 select가 같은 위치 기준과 상태 칸 여백을 가지며, 터치·좁은 창의 메뉴 칸은 선택 여부와 무관하게 확보한다.
  전체 검증은 `nice -n 15 bash scripts/ci-local.sh`다.


## install 승인 PTY의 종료 수명(t-0485, plan §8.395)

- `cargo test -p aaa --test install --locked`: 기존 승인·취소·worker 수명 단언을 유지한다. 실제 자식 종료 사건으로 master를 닫고 reader 완료까지 수집하며, 두 사건 모두 총 60초 안에 와야 한다. EOF만 온 뒤 살아 있는 자식도 무한 wait하지 않는다.
- `authorization::terminal_lifecycle::child_exit_closes_terminal_owner_before_eof_and_preserves_final_output`은 Unix의 실제 PTY 뒤에 소유자가 writer를 보관하는 실제 UnixStream을 잇는다. 실제 자식이 종료한 뒤에도 EOF가 안 오는 ConPTY 수명을 재현하며 종료 코드 7과 마지막 출력 300줄을 모두 검사한다. 자식 종료 때 master를 닫는 수리를 제거하면 60초 RED, 복구하면 GREEN이어야 한다. 이 keeper의 Unix cfg는 재현 fixture의 POSIX shell/UnixStream 때문이며 Windows의 기존 install owner들은 실제 ConPTY에서 같은 driver를 검증한다.
- Windows cross는 실행 증거가 아니다. macOS 로컬 통과와 macOS CI의 yes 뒤 정지·target 미상의 15분 종료를 구별하고, Windows/macOS 실제 CI 결과는 착지 뒤 감시 결과로 판정한다.

## 할 일 한 목록·예약 맨 위(t-0611, plan §8.393)

- `node web/e2e/web.e2e.mjs --spec 76-task-one-list --workers 1`: 자리 한도를 채운 실제 host에 CLI로 예약을 넣고, Chromium 1280px에서
  맨 위 예약 차례·하위 함께·확인 필요의 노란 표지와 상세의 까닭을 본다. 끌기로 예약 차례 바꾸기·일반 → 예약(내용 확인 뒤 그 자리)·
  예약 → 아래(풀기와 원래 자리 알림)를 하고 host `task queue` 차례와 대조한다. 「예약」 칩·저장소 거르기의 숨은 예약 줄, 거른 채
  Alt+↓, 새로고침 뒤 기억을 확인한다. 휴대폰(Chromium·WebKit 390px)은 맨 위 차례와 「순서」 ▲▼·⋯ 시트 풀기를 확인한다.
- `node --test web/test/tasksModel.test.mjs`의 「할 일 한 목록과 예약」은 행 배치(2A·crumb·숨은 예약 줄), 예약 순번(끌어 놓기·
  보이는 이웃과 바꾸기), 예약 푼 자리, 옛 `##` 틈의 옮기기 규칙을 검사한다. 43·46·67은 목록 머리 대신 한 목록 차례를 본다.
- DOM·host 기록으로 끝내며 고정 sleep·재시도·시한 연장은 없다.

## owner 목록 갱신과 저장소 선택 경계(t-0616, plan §8.392)

- main `35f6ce96`의 원래 E2E54 단독 실행은 3.7초에 통과했다(load average 약 6).
  같은 제품 실행 파일에서 실제 assignable 응답을 보류하고, 머신 목록 DOM 반영 직후 저장소 change를 보내면
  「owner 응답이 고른 저장소를 초기화했다」로 실패했다(load average 약 5). 부하만의 실패로 처리하지 않는다.
- `TaskDetail`의 owner 목록 effect는 예약한 render의 dirty 대신 상태 updater가 받은 최신 draft를 검사한다.
  저장소·제목 등 저장할 수정과 이름을 아직 안 쓴 사람 선택은 보존하고, 손대지 않은 폼만 다시 분류한다.
- E2E54는 실제 owner 응답·MutationObserver·두 렌더 frame으로 이 경계를 제어한 뒤 선택·저장 가능·파일 반영을 본다.
  고정 sleep·시한 증가·재시도를 쓰지 않는다. E2E46의 빈 사람 이름 보존과 E2E75의 연결 복귀도 함께 유지한다.

## 기동 선점·복구와 배포 진행(t-0606, plan §8.391)

- host `automation::start_tests::start_admission_owns_the_thread_before_recovery_can_observe_it`는
  실제 자동화 시작의 DB commit 직후, 첫 ThreadPrepare 전 복구기를 호출한다. 기존 코드는 새 Starting을
  중단된 기동으로 실패 처리했다. DB 행이 공개되기 전에 기동 task의 claim을 잡고, 실제 frame fixture의
  준비·기동 완료와 1회 실행까지 확인한다. 제어 고리는 host 단위 시험 빌드에만 있으며 고정 sleep은 없다.
- `task_queue::tests::recovery_reloads_under_the_claim_and_does_not_fail_a_completed_start`는
  복구기가 읽은 Starting 사본 뒤에 실제 기동 완료/송신이 확정되는 반대 순서를 검사한다. claim 아래에서
  최신 행을 다시 읽어 Open·pane·송신 기록을 보존한다. 기존 코드는 완료된 기동을 Failed로 바꿨다.
  `failed_start_admission_rolls_back_the_row_and_releases_the_claim`은 실제 INSERT 실패를 주입해
  새 행·자동화 영수증이 없고 claim도 반환됐는지 확인한다. 기존 중단·재기동 keeper도 유지한다.
- m13 rollout의 서비스 기록은 정확한 job ID의 `started_ms` 뒤에서 기존 30초를 잰다. host 확인 뒤의
  로컬 agent_restart는 host_restart와 같은 머신의 job을 고른다. 같은 phase의 마지막 항목은 원격
  agent일 수 있으므로 로컬 job 선택에 쓰지 않는다. 전송 재개·원격 복구·공유 host/agent 복구의
  네 로컬 기록 대기를 같은 경계로 맞추며, 순서·횟수·SHA·독립 PID·동일 job 재개 단언은 유지한다.
- host_update의 30초 진행 실패는 별도 계측에서 재관측했다. 당시 agent stack은 실행 파일 SHA-256
  검사 중이었다. 같은 약 194MB 파일의 debug 대조 3회에서 기존 경로 1.46~1.91초, ring 경로
  0.14~0.25초였고 해시는 모두 같았다. 샘플은 그 순간의 작업을 보여 주며 전체 지연의 유일한 원인이나
  OS 교착을 증명하지 않는다. `sample`을 붙인 실행의 시간과 일반 실행의 시간을 같은 표본으로 보지 않는다.
- `aaa-common::util::sha256_file`은 이미 사용하는 ring의 SHA-256으로 파일을 매번 64KiB씩 읽는다.
  agent 배포·독립 guard와 update/복구의 모든 기존 대조 지점을 유지하며 해시 cache·검사 생략은 없다.
  `sha256_file_matches_vectors_chunks_and_fresh_bytes`는 알려진 abc 값·빈 파일·블록 경계·끝 조각을
  독립 sha2 계산과 대조하고, 같은 경로의 같은 길이 변경·없는 파일 오류도 검사한다. 성능 합격 상한은 없다.

`cargo test -p aaa-host --lib --locked`, `cargo test -p aaa-common --lib --locked`,
`cargo test -p aaa --test m13 rollout:: --locked`, 기존 자동화 시작 m13 및 전체
`scripts/ci-local.sh`로 확인한다. 이 수리에서 추가 시한 연장·실패 재시도·m13 task_edit 및
resource_limits 변경은 하지 않는다.

고정 sleep 검사도 `cfg(test)`가 붙은 필드·초기화 뒤의 운영 코드를 시험으로 오인하지 않게
항목의 실제 범위를 읽는다. 필드·별칭·블록·여러 test 함수/모듈·generic/where 절 뒤의 경계를
회귀 시험으로 검사하며, 실제 시험 sleep은 계속 검출한다. 기존 파일별 기준값은 바꾸지 않는다.

## 할 일 로그·FD 회수의 CI 경쟁(t-0612, plan §8.390)

- `cargo test -p aaa --test m13 task_edit_keeps_bytes_conflicts_and_logs --locked -- --exact`: 직접 편집 fixture는
  같은 폴더의 임시 파일을 완성한 뒤 rename한다. 임시 파일 쓰기와 rename 사이에 실제 ticker tick을 끝내도
  마지막 변경은 「새 일」의 `파일 직접/update` 한 건이어야 하며, 원래 before bytes·충돌·권한 검사는 유지한다.
- 원래 `std::fs::write`의 truncate → ticker tick → write 순서를 제어하면 CI와 같은 「다른 일」의
  `파일 직접/add`가 첫 로그로 나와 RED다. 제품은 로그 저장 뒤 사건을 보내고 있었으며, 로그를 더 기다려도
  이미 생긴 삭제/추가 기록은 수정 기록이 되지 않는다. 제품의 기록 순서·감시 정책은 바꾸지 않는다.
- `cargo test -p aaa --test resource_limits --locked`: 실제 384개 HTTP 연결·OS fd 회수·EMFILE 복구를 유지한다.
  회수 대기의 여유 2개를 연결 수 단언에도 같은 상수로 적용한다. CI의 `400 → 18`은 원래 단언에서 RED,
  수정 뒤에는 `400 → 16·17·18`을 허용하고 `400 → 19`와 예전 256개 상한은 계속 거부한다.
- 고정 sleep·시험 재시도·시한 연장 없이 상태·사건으로 확인한다. 전체 검증은 `bash scripts/ci-local.sh`다.

## 사용자 지시의 task 모델·머신 변경(t-0609, plan §8.389)

- `cargo test -p aaa-host --lib task_queue::tests:: --locked`: 명시 지시의 빈 근거·제어 문자·RawSave·여러 동작 거부,
  기본 모델/자동 머신 변경과 예약 ID·순서·생성 시각·감사 보존을 검사한다. 승인 후 다른 내용이 바뀌었거나
  Starting/Running·기동 이력이 있는 Check 상태이면 거부하며 원문과 선택 출처를 보존한다.
  예약 뒤 기본 모델이 달라져도 머신만 변경하면 예약 당시 profile·model·effort·설정 판을 유지한다.
  새 기본 모델 회귀는 수정 전 claude → codex의 의도하지 않은 변경에서 RED, 수정 뒤 같은 단언에서 GREEN을 확인한다.
- `cargo test -p aaa --test m13 coordinator_user_request_changes_choices_without_losing_reservations --locked -- --exact`:
  격리 host·agent의 실제 coordinator pane에서 CLI를 실행한다. 근거 없는 모델 변경, 빈 근거, 다른 칸 혼합,
  모르는 profile, 시작/재시작 우회를 거부한다. 명시 지시 뒤 미기동 예약의 새 선택·실행 승인판과
  ID·생성 시각·순서·취소 상태·변경 전후 기록을 확인한다. host 재시작 뒤 기존 자리 resolve 사건만으로
  바뀐 profile·자동 머신의 실제 pane이 시작되어야 한다. 고정 sleep·재시도는 없다.
- 첫 전체 gate에서 E2E49는 host 기억을 본 직후 화면의 옛 본문을 읽었고, E2E74는 휴대폰 iframe에서 그림을
  누른 뒤 Escape가 크게 보기에 전달되지 않았다. 49는 같은 본문·thread 단언을 DOM 갱신 사건까지 기다리고,
  74의 휴대폰은 기존 E2E59와 같은 닫기 단추 tap을 쓴다. 데스크톱 Escape 회귀는 E2E59가 유지한다.
  원본 실패 로그를 남기고 두 keeper를 좁게 검증한 뒤 최종 전체 gate를 실행한다. 시한·기대값은 바꾸지 않는다.
- 착지 통합의 E2E46은 새 owner 응답이 아직 이름이 빈 사람 선택을 초기화해 입력칸이 사라졌다.
  load average 62~64였으나, 한 worker에서 owner 요청을 사람 선택 뒤로 보류해 같은 결함을 재현했다.
  이름을 쓰기 전의 사람 선택도 편집 중으로 보존한다. E2E46은 실제 응답과 두 렌더 frame 뒤 이름 입력·거부 검사를
  유지해 RED/GREEN을 확인한다. 부하만의 실패로 넘기거나 시한·기대값을 바꾸지 않는다.
- 기존 `task_selected_model_and_machine_are_changed_only_by_user`·직접 파일 수정·기본 모델 보호 keeper와
  coordinator 안내의 새 생성/doctor 갱신 검사를 유지한다. 전체 정본은 `nice -n 15 bash scripts/ci-local.sh`다.

## 할 일 머신 연결 목록(t-0600, plan §8.388)

- `node web/e2e/web.e2e.mjs --spec 75-task-owners --workers 1`: 실제 실험실 agent를 끊은 동안 할 일을 처음 열고,
  Chromium 1280px·WebKit 390px에서 `labA(끊김)`과 선택 불가를 확인한다. agent를 다시 붙이면 화면을 새로 열지 않아도
  머신 선택이 살아나며, 고치던 제목은 남아야 한다. 두 번째 끊김·복귀도 같은 상세에서 확인한다.
- 상세를 닫았다 열기·머신 칸 focus·바로 시작마다 실제 `assignable` HTTP 응답을 기다린다. 모델·머신 칸은 받은 목록만
  바꾸고 폼은 다시 만들지 않는다. 통계·pane 변화는 재조회 원인이 아니며, 새 조회나 화면 닫기 뒤에 온 옛 응답은 버린다.
- 연결과 DOM·HTTP 사건으로 끝내며 고정 sleep·재시도·시한 연장은 없다. 수정 전 코드는 첫 복귀의 머신 연결 단언에서 실패한다.

## 웹 알림함 숨기기(t-0590, plan §8.382)

- `node web/e2e/web.e2e.mjs --spec 26-activity-bar,53-thread-questions,56-operate,57-legacy-special-panes,62-asks-close`: 활동 막대에 알림함 단추·배지·패널이 없고, 예전 `inbox` 패널 기억으로 새로 열어도 「작업」이 보인다. 기존 26의 데스크톱·좁은 창·휴대폰 세로/가로·두 테마 배치 검사가 바닥 사용량 칸을 그대로 확인한다.
- main 통합으로 더해진 E2E23의 종료 문서 API keeper는 숨긴 알림함 대신 「답할 것」 조회를 쓴다. 정상 상태·복귀 뒤 두 실제 응답과 종료 뒤 전송 없음 단언을 유지한다.
- 53은 확인 필요 post의 「답할 것」 배지·본문·답함과 coordinator inbox 기록을 계속 확인한다. 56은 조작·보기 기기와 휴대폰의 알림함 숨김, 기존 CLI 조회·끝내기·끝낸 기록을 확인한다. 화면·host 응답을 기다리며 고정 sleep을 추가하지 않는다.

## 배포 조각의 개발 빌드 비용(t-0592)

- `cargo run --locked -p aaa-proto --example deploy_codec`: 실제 배포 응답의256KiB base64 JSON 조각을
  256번씩 encode/decode하고 원문 bytes를 대조한다. 성공 표본3개의 시간을 출력하며 성능 통과 상한·sleep·재시도는 없다.
- 기존 `sha2` 설정에 더해 dev/test의 `base64`·`serde_json`을 opt-level3으로 컴파일한다. debug assertions·release 설정·조각 크기·
  실제 배포 실행 파일·SHA·영수증·복구·재시작 owner를 유지한다. 프로토콜 crate 전체를 최적화하지 않는다.
- 전후 비교는 같은 머신에서 같은 `rollout::rollout_preflights_all_targets_and_resumes_same_jobs_after_host_restart`
  keeper를 실행하고 빌드와 시험 시간을 나눈다. codec 측정과 전체 gate·다른 머신의 결과를 같은 개선률로 쓰지 않는다.

## 웹 E2E 병렬 작업(t-0592)

- 37-chat-pages의 Chromium·WebKit ×1280·390·820px ×Claude·Codex 조합12개는 각각 새 실험실에서
  실행한다. source의 기존 단계 이름을 정확히 하나씩 선택하고 모든 조합의 위치·가상화·새 기록·복원 검사를 유지한다.
  `--grep`은 그 조합만 고르고 `--workers 1`은 이전처럼 한 실험실에서 모든 단계를 차례로 실행한다.
- 상태를 이어 쓰는23-composer-attachments 등의 spec은 분할하지 않는다. 공통 준비의 `always`도 유지한다.
  `node --test web/test/e2e-jobs.test.mjs`는 원 source의12개 조합이 반복마다 정확히 한 번 선택되는지,
  의존 단계 보존·grep·문장 부호·시간 합계를 확인한다. 실패 재시도는 없다.
- 기본 작업 수는 16 CPU 이상이면10개, 그 밖은 CPU 둘마다 하나다(t-0670). 명시한 `AAA_E2E_WORKERS`와 `--workers`는 그대로 적용한다.
  요약의 전체 시간은 wall time이며 분할 spec의 행은 독립 작업 점유 시간의 합계라고 표시한다.
  단계·전체 시한이나 실패 판정은 바꾸지 않는다. 같은 실행 파일을 고정한 전후 측정과 전체 정본으로 검증한다.

## 종료 시작에 완료된 조회(t-0592)

- E2E23의 「종료 시작의 완료 응답」은 실제 WebKit/App의 projects HTTP 응답을 받은 뒤 JSON 완료만
  beforeunload까지 보류한다. 실제 reload에서 pagehide보다 먼저 routines를 시작하는 경계를 검사한다.
  기존 코드는 `beforeunload → routines fetch → cancelled → pagehide`로 실패한다. 원래 간헐 실패의
  access-control 문구와 이 제어 재현의 cancelled 기록은 구별하며, 종료 중 실제 fetch 발생 자체를 단언한다.
- 공통 API는 beforeunload부터 새 일반 요청을 AbortError로 취소한다. 기존 pagehide·pageshow·명시 keepalive
  경계도 유지한다. 새 문서의 실제 routines 응답을 확인해 조회를 영구 차단하는 변형은 통과하지 못한다.
- `web/test/apiRequest.test.mjs`는 두 종료 사건부터 네트워크 시작0, 명시 keepalive, pageshow의 새 요청과
  취소한 요청 비재생을 검사한다. 일반 API 오류를 성공으로 바꾸거나 pageerror를 무시하지 않는다.

## 전체 검증 시간 기록

같은 머신·캐시 조건끼리 비교한다. 머신은 공개용 별칭이며 `mac-a`는 같은 16 CPU macOS 머신이다.
부하는 시작 직전 `uptime`의 1분 평균이다. 단계는 서로 겹치므로 합산하지 않는다.
`캐시 준비`는 같은 main의 전체 빌드·시험 산출물이 있는 상태다. 첫 실행의 준비 비용을 코드 개선 효과로 세지 않는다.

| 날짜 | main SHA·후보 | 머신 | 시작 부하 | 조건·결과 | 전체 | web | fmt | build | clippy | Rust | 웹 E2E | public |
| --- | --- | --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 2026-10-21 | `35ecad05` | mac-a | 미기록 | 이전 착지 보고·통과 | 195.99 | 5 | 1 | 22 | 12 | 166 | 165 | 1 |
| 2026-10-22 | `94d2081e` | mac-a | 10.73 | 새 작업 폴더·통과 | 203 | 4 | 1 | 27 | 15 | 137 | 169 | 1 |
| 2026-10-22 | `94d2081e` | mac-a | 20.41 | 캐시 준비·변경 전·통과 | 174 | 3 | 1 | 1 | 1 | 141 | 166 | 1 |
| 2026-10-22 | `94d2081e` + t-0670 첫 후보 | mac-a | 21.06 | 캐시 준비·E2E47 정리 경합 실패 | 179 | 4 | 1 | 2 | 2 | 127 | 170 | 1 |
| 2026-10-22 | `94d2081e` + t-0670 정리 수리·worker8 | mac-a | 12.81 | staging 뒤 재빌드·diff 임시 폴더 충돌 실패 | 203 | 4 | 1 | 25 | 13 | 127 | 171 | 1 |
| 2026-10-22 | `94d2081e` + t-0670 충돌 수리·worker10 | mac-a | 7.06 | 빌드 산출물 재사용·전체 통과 | 164 | 4 | 1 | 7 | 5 | 130 | 150 | 1 |

단위는 초다. 이전 기록 이후 웹 시험은 73→85파일로 늘었지만 첫 E2E 시간은 165→169초이고,
Rust 단계는 166→137초였다. 코드·캐시·부하가 다르므로 이를 특정 병합의 증감으로 단정하지 않는다.
새 작업 폴더의 준비(web·fmt·build)는 32초, 같은 코드의 준비만 다시 잰 값은 7초였다.
시스템 인덱싱·분석 프로세스 부하는 있었고 다른 thread의 무거운 빌드는 관측되지 않았다.

같은 index의 캐시 준비 기준선174초와 worker10 후보164초를 비교하면 Rust141→130초·웹166→150초다.
Rust 실행기 자체는131.24→120.09초, install 시작 대기는90.73→68.58초였다.
worker10에서도 Rust의 가장 긴 묶음은 m13-5(88.02초)이고, 가장 긴 단일 웹 spec은11-agent-panes(64.5초)다.
37-chat-pages의12작업 합계는 단일 spec의 wall time과 구별한다. 산출물21개·m13 107개·doctest를 유지하고,
Rust 시험은1057→1058통과(새 회귀 시험1개)·기존 ignored3개, Python19개 중18통과·기존 Linux 전용1skip,
웹85파일·96작업 전체 통과다. 새 작업 폴더의203초와164초 차이를 모두 코드 개선으로 세지 않는다.

E2E47 실패는 한도 초과로 웹에서 시작한 thread의 task 연결을 기동 완료로 간주하고 바로 정리한 시험 순서였다.
보존한 실패 DB는 `status=open`, `brief=pending`이고 시작 작업 잠금이 남아 resolve를 거부했다.
기존 출처·재확인·예약 대기 단언 뒤 실제 `brief=delivered`를 관측하고 resolve를 한 번 호출한다.
실패 재시도·시한 변경·단언 삭제 없이 정리의 선행 조건을 기다린다.

다음 gate의 agent diff 두 실패는 임시 index의 `PID + 순번` 이름이 이전 프로세스 잔여 폴더와 충돌한
`File exists`였다. 비교마다 새 ID를 써 한 번 생성하며 기존 폴더는 지우거나 재사용하지 않는다.
`cargo test -p aaa-agent --lib diff::tests:: --locked`의 새
`stale_pid_temp_index_never_blocks_a_new_process`는 격리 TMP 환경의 새 자식에 옛 PID·첫 순번 폴더와
보존 파일을 먼저 둔다. 원래 코드는 같은 오류로 실패하고, 수정 뒤 실제 diff와 보존 파일 바이트를 검사한다.
큰 patch·외부 callback·index 바이트/시각·ref/작업 파일 비변경 단언도 유지한다.

## Rust 전체 시험 병렬 실행(t-0591)

Unix의 `bash scripts/ci-local.sh --only test`는 Cargo `--workspace --no-run`의 시험 실행 파일 전체를 준비한 뒤
기본 여섯 프로세스로 실행한다. m13은 `--list`의 정확한 전체 이름을 rollout 단독과 나머지 세 묶음으로 나누고,
분할 전후 이름 집합·중복과 각 자식의 실제 실행 수를 대조한다. 작업 디렉터리는 각 package의 manifest 폴더다.
모든 binary와 workspace doctest를 유지하고 실패 뒤에도 나머지 작업을 끝낸다. Windows는 기존 Cargo 실행을 유지한다.
`AAA_RUST_TEST_WORKERS`로 프로세스 수를 정하고 `RUST_TEST_THREADS`가 없으면 CPU 수를 `min(프로세스 수, 4)`로 나눈다.
작업은 `min(libtest thread 수, exact 선택 시험 수)`만큼 CPU 자리를 사용하며 전체 합계를 CPU 수 이내로 제한한다.
16코어의 기본 m13 여섯 작업은 rollout 세 개 각1자리·나머지 세 묶음 각4자리로 함께 시작한다.
명시한 thread 수는 유지한다. `--logs` 아래 `rust/plan.json`·`summary.json`·작업별 로그에 실행 목록·시간·결과와
작업의 시작·대기 시간·CPU 자리 수가 남는다.

`scripts/rust-test-durations.json`은 같은 머신의 전체 검증에서 잰 binary 실행 시간이다.
m13 여섯 묶음을 먼저 시작하고, 나머지는 이 기록의 긴 binary부터 실행한다. 새 binary는 기록이 없으므로 먼저 실행한다.
시간 기록은 순서에만 쓰고 시험 선택·단언·시한에는 쓰지 않는다. doctest도 기존처럼 모두 실행한다.
부모가 CPU 자리가 있는 작업만 worker에 보내므로 큰 작업의 자리 대기가 worker를 점유하지 않는다.
앞의 큰 작업이 들어갈 수 있으면 우선하고, 들어갈 수 없으면 뒤의 작은 작업을 빈 CPU 자리에서 실행한다.
`test_waiting_large_job_does_not_block_a_fitting_job`은 실제 자식 A(2자리)와 D(1자리)가 만나야 열리는 socket으로
CPU 3자리·worker 3개에서 B·C(각2자리)의 대기가 D를 막지 않는지 검사한다. 기존 실행기는 이 사건에 도달하지 못한다.

`python3 -m unittest scripts.test_rust_tests -v`는 목록 완전성, 실제 두 자식의 동시 진입,
실패 뒤 후속 실행, 선택한 시험이 0건으로 빠지는 오류, Cargo 산출물부터 doctest까지의 실패 전파를 확인한다.
실제 자식 여섯이 CPU15자리 안에서 함께 만나는지, 실패 뒤에도 예산을 반환하고 다음 작업을 실행하는지도 본다.
가짜 자식은 socket 사건으로 만나며 고정 sleep을 쓰지 않는다.

`cargo test -p aaa --test m13 rollout:: --locked`의 macOS owner 세 개는 독립 사본 host·TLS agent를 사용한다.
전송·사전 검사·같은 job 재개, 거부된 갱신·원격 복구, 공유 host/agent 복구·진행 중 취소를 나눠 실행한다.
앞 시나리오의 배포를 반복해 준비하는 대신 필요한 서명 이미지로 시작하며 기존 단언·시한은 유지한다.
완료 rollout 뒤 새 기록의 진입·옛 이력·job ID 비재사용은 host 단위 시험
`completed_rollout_allows_the_next_record_without_reusing_jobs_or_losing_history`가 소유한다.

## 공개 검사 크기 조회 일괄화(t-0591)

`python3 -m unittest scripts.test_check_public -v`는 실제 임시 Git 저장소에서 기존 금지 확장자·이력·정확한 크기 예외를
유지하고, 공백·개행·탭·한글·선행 대시 파일명, index를 갱신하지 않은 작업 파일 크기, symlink 대상·삭제·끊긴 링크를
확인한다. 비 UTF-8 파일명은 이를 허용하는 Linux에서 바이트 그대로 검사한다(macOS 파일시스템은 생성부터 거부한다).
symlink 순환으로 실제 metadata 조회를 실패시켜 빈 크기 목록으로 통과하지 않는지도 본다.

크기 조회는 파일마다 `wc`를 띄우는 대신 Python 한 번의 `stat`으로 읽는다. NUL 파일 목록·작업 파일 기준·기존 크기 상한과
내용·커밋·세션 검사는 유지한다. 속도 비교는 같은 worktree·index·환경·기준 commit의 전체 스크립트를 전후 실행하고
종료 코드·출력을 함께 비교한다. 성능 수치로 시험 기대값이나 시한을 정하지 않는다. 전체 정본은 `bash scripts/ci-local.sh`다.

## 보고서 폴더 fetch·음원(t-0589, plan §8.381)

- `cargo test -p aaa-host --lib --locked`: `report_connections_have_one_exact_origin_and_capability_prefix`는
  loopback·HTTPS의 정확한 보고서 경로와 헤더 삽입 차단을 검사한다. `signed_report_http_boundary`는 기존
  폴더 밖·숨은 파일·symlink·위조 링크 거부를 유지하고 새 폴더 읽기 범위를 검사한다. 기존 connect-src none 단언은
  요구된 폴더 읽기 계약으로 바꾸며 범용 self·별표로 완화하지 않는다.
- `report_sandbox_warning_does_not_fail_copy_or_repeat_for_the_same_snapshot`은 실제 보고 회수로 제한 API 경고가
  inbox에 한 번 남고 복사 성공은 보존되는지 본다. 문자열 탐지는 별도 unit에서 상대 fetch·외부/동적 경로·제한 API를 검사한다.
  `publishing_reports_warns_for_restricted_apis_in_copies_and_live_sources`는 publish의 사본·실시간 등록 경고와 정상 상대 fetch를 검사한다.
- `cargo test -p aaa --test m13 library_view --locked -- --exact`: host 사본·agent 스트리밍·Range·내려받기·기존 경로 거부와
  Origin null의 CORS·쿠키 동반 거부·다른 Origin에 CORS 없음.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs --spec 71 --workers 1`: 격리 host·agent에서 390×844 Chromium·WebKit의
  앱 iframe과 인증 없는 공유 탭을 연다. 사용자 누름에서 WAV 둘을 fetch·decode·GainNode로 혼합하고 analyser 신호·running 상태·
  두 source의 ended를 확인한다. 출력 끝에는 gain 0을 두어 실제 스피커로 소리를 내지 않는다. HEAD·Range·하위 HTML의 상대 읽기,
  다른 보고서·정규화/인코딩 경로 탈출·앱 API·외부 서버·쓰기·credentials·쿠키·저장소·부모 접근 거부와 링크 폐기도 검사한다.
  외부 요청은 실험실 서버의 받은 수 0으로 확인한다. 고정 sleep·timeout 연장·실제 로그인·실제 iPhone 청취는 없다.

제품 변형 대조: connect-src를 host 전체로 넓히면 다른 보고서 읽기가 성공해 격리 단언에서 실패하고,
Access-Control-Allow-Origin을 없애면 음원 읽기 전에 fetch 실패를 검출한다. 두 변형 뒤 제품 파일 bytes를 복구하고 정본에서 다시 검사한다.

## 종료 문서의 앱 API(t-0485)

`node web/e2e/web.e2e.mjs --spec 23 --grep '종료 문서 API:'`는 실제 host/App/WebKit에서 정상 tasks/asks
조회 응답을 확인한 뒤 pagehide·WS 갱신 사건을 보내 일반 API 네트워크 시작이0인지 검사한다. pageshow 뒤 같은
두 실제 조회도 확인한다. 원래 코드에서는 tasks/inbox 두 fetch를 잡아 RED, 공통 apiFetch 뒤 GREEN이었다. t-0590에서 알림함 조회를 없앤 뒤에는 같은 경계를 tasks/asks 실제 조회로 유지한다.
개별 컴포넌트에 시험 export/guard를 붙이지 않는다. 새 web/aaa 빌드 뒤 E2E23 파일 전체와 전체 gate를 확인한다.

공통 경계는 post·인증·업로드·사용량·진단 API에 적용하며, 종료 뒤 취소는 AbortError다. 취소한 쓰기를 복귀 때
재생하지 않는다. 일반 숨긴 탭은 차단하지 않고 명시 keepalive는 유지한다. 떠날 때 설정 보존은 기존 E2E14가 맡는다.

## install I6 CI 시험 터미널(t-0485)

- `cargo test -p aaa --test install --locked`: 실제 승인·마법사·worker·TLS 등록 owner를 세 OS에서 실행한다.
  직접 연 시험 PTY도 aaa-vt로 ConPTY의 DSR/DA/색 질의에 답하며 기존30초 시한·승인 단언을 유지한다.
- `authorization::terminal_answers_cursor_query_before_application_prompt`: POSIX 실제 PTY의 자식이 DSR 응답 전에는
  승인 물음을 내지 않는다. 응답기 없는 원래 driver는30초 RED, 응답기 연결 뒤0.03초 GREEN이다. 셸/stty/dd fixture만
  Unix cfg이고 Windows 기존 owner는 실제 ConPTY 시작 질의로 같은 연결을 검증한다.
- `grant::verified_host_grant_uses_tls_pin_and_keeps_consumption_bound_to_same_machine`: TLS/grant/pin/재조회·폐기 단언을
  유지하고, host runtime을 내린 뒤 lab을 지운다. 종료를 제거하면 남은 host 소유자 단언이 실패해야 한다.
- Windows `install::engine::input::password::tests::console_masks_and_restores_after_success_cancel_and_eof`:
  같은 시험 터미널 응답기로 시작 질의를 통과하고 실제 console 암호 비노출·원래 mode·취소/EOF를 검사한다.

Windows 실행은 main push 뒤 CI로 판정한다. cross clippy/build를 실제 실행으로 부르지 않는다. macOS worker의
시한 초과는 출력 단계와 부하 근거를 함께 남기며 시한 연장·기대값 변경·시험 삭제로 통과시키지 않는다.

## 할 일 표시 ID(t-0575)

- `cargo test -p aaa-host --lib display_ids --locked`: 기존 ULID·CRLF를 가진 파일을 여덟 조회가 동시에
  이주해도 같은 `d-NNNN`을 받는지 본다. 삭제·로그 정리·옛 원문 복원·DB 다시 열기에도 대응을 유지한다.
- `cargo test -p aaa --test m13 task_display_ids_survive_rename_move_restart_delete_and_archive --locked -- --exact`:
  실제 host·agent·CLI에서 괄호 제목과 명시 owner의 시작/연결, 제목 변경·목록/하위 이동·기록 조회,
  삭제 복원·번호 비재사용·예약 순서/취소·host 재시작·완료 보관 복원을 검사한다. ID와 ULID를 모두 대조한다.
- `node web/e2e/web.e2e.mjs --spec 70-task-ids`: 실제 host의 첫 이주가 주석 외의 bytes를 유지하는지,
  웹 편집 뒤 CLI 선택자가 같은 할 일을 고치는지, host 재시작 뒤 320/390/768px에서 ID 검색·표시와
  가로 넘침을 검사한다. CLI 응답·DOM 사건·연결 준비로 끝낸다.

기존 원문 바이트 keeper는 이주가 끝난 고정 ULID fixture로 내용·충돌·이력을 검사한다. 기존 웹 내용 keeper는
식별 주석을 제외한 내용을 비교하고, 직접 편집 fixture에서는 주석을 보존한다. 식별 주석 자체의 정확한 bytes와
번호·내부 ULID 대응은 위 전용 keeper가 검사한다. `parent_attention_is_published_after_the_verdict_is_saved`는
자동 정리의 파일 사건 뒤 판정 확정 사건도 보내는지 검사한다. 파일 쓰기 직후의 읽기가 판정 전 원문을 받더라도
마지막 사건에서는 상위의 확인 필요 상태가 확정돼 있어야 한다. ID를 없애거나 재발급하는 변형은 전용 keeper에서 실패해야 한다.

## thread 재시작의 pane 교체(t-0567)

- `cargo test -p aaa --test m13 restart_replaces_exited_panes_locally_and_remotely --locked -- --exact`:
  실제 host·로컬 UDS/원격 TLS agent에서 kill 뒤 profile 변경, 자발적 exit 뒤 새 대화·resume·직접 pane resume를
  연속 실행한다. thread 결속, 옛 pane 제거, 이름·탭·화면 추적 ID, 다른 실행 중 pane 보존을 본다.
  host 재시작 뒤 두 agent의 Hello에도 옛 pane이 돌아오면 실패한다. host의 임시 TLS 포트는 재시작 전에 고정한다.
- `restart_spawn_failure_keeps_the_old_pane_until_a_successful_replacement`: 존재하지 않는 fixture 실행 파일로 기동을
  실패시켜 옛 pane·layout 보존을 확인한 뒤 설정을 고친다. 성공한 교체 뒤에만 옛 기록이 없어져야 한다.
- 기존 `accounts::host_login_defaults_pin_threads_and_coordinators_without_exposing_credentials`:
  로컬·원격의 고정 Claude 계정에서 Codex profile 변경은 기본 Codex 계정이 있어도 거부하고 brief bytes·profile·pane·계정을
  보존한다. 같은 harness의 다른 profile은 원래 인증으로 기동하고 옛 pane을 지운다. 가짜 인증·socket gate만 쓴다.
- 기존 `restart_with_another_profile`, `memory_and_restart`, `reboot_resumes_threads`, 계정 pane 재개·복원 keeper가
  실행 중 거부, brief 다시 쓰기, 저장 대화 이어가기와 재부팅 경계를 맡는다. 사건·출력·세션 상태로 기다린다.

수정 전 첫 keeper는 `labA/profile: 옛 pane이 기타로 남았다`에서 실패한다. pane 교체 누락·계정 사전 검사 누락·
일반 thread의 pane resume 우회를 각각 되심으면 해당 owner 단언이 실패해야 한다. 제품과 시험의 시한은 늘리지 않는다.

## 계정별 사용량(A2, t-0561)

- `cargo test -p aaa-common --lib quota`: 공식 camelCase RPC·Claude 상태줄 fixture, 누락·NaN·범위 밖
  수치·기간 모름·모델별 창·창별 관측 시각·2분/10분·리셋·미래 시각 경계.
- `cargo test -p aaa-host --lib quota`: 가짜 공식 실행 파일 여섯을 동시에 사용한다. RPC 기록은
  initialize·initialized·account/rateLimits/read만이며 thread/start·turn/start는 0회여야 한다.
  401·429와 재조회 가능 시각·깨진 응답·누락·stderr·알림·응답(버킷 이름 포함)의 canary 폐기·로컬 갱신 거부·임시 홈 정리를 본다.
  병합 규칙은 동시 관측 충돌·모델 창 분리·다른 계정/로그인 세대 거부·끊김의 원래 시각 보존·머신 시각 차이를 본다.
- `cargo test -p aaa-agent --lib quota`: 전용 pane 파일과 창별 시각·세대·경로 검사·0600·원문 폐기.
  기존 `accounts::encrypted_accounts_reject_tampering_and_lock_without_stopping_other_accounts`는
  세대 변조 거부·잠김·다시 등록·같은 별칭 삭제/재등록의 새 세대도 검사한다.
  `cargo test -p aaa-host --lib legacy_vault`는 A2 전 AAD로 만든 가짜 암호문을 읽고 첫 재암호화에도 로그인 세대가 유지됨을 확인한다.
- `node web/e2e/web.e2e.mjs --spec 65-account-quota`: 실제 host·agent와 가짜 공식 RPC 두 계정,
  관측 없는 Claude, 계정 pane의 주입된 상태줄 → agent snapshot → host 카드, 누락 주간 창의 원래 시각,
  401·429·잠긴 가짜 금고·agent 끊김·마지막 숫자·backoff 중 추가 RPC 없음·로그인 세대 교체·Reader 비노출·320/390/768px 카드.
  가짜 Claude pane에 F6 사건을 보내 창을 바꾸고 출력 표지를 기다린다. sleep·실제 로그인·실제 인증은 없다.
  `AAA_TEST_USAGE_REFRESH_MS=0`은 기존 수동 조회 하한 시험 고리이며 429의 시각은 줄이지 않는다.

`AAA_USAGE_HOME`을 지정한 기존 실험실에서는 새 수집기도 실제 Codex를 실행하지 않는다.
debug 실험실이 `AAA_TEST_QUOTA_CODEX`에 명시한 가짜 실행 파일만 허용한다. RPC 단위 시험은 실행 파일을
직접 주입하므로 전역 env를 바꾸지 않는다. 원격 숫자는 수신 시각 대신 원래 창 관측 시각으로 판정한다.

`quota_rpc::tests::refresh_refusal_reads_the_post_body_before_closing`는 가짜 갱신 POST의
`Expect: 100-continue` 응답을 받은 뒤 본문을 보내고 최종 401과 응답 끝까지 읽는다.
본문을 받기 전에 TCP를 닫던 서버는 첫 응답이 401이라 실패한다. 일반 갱신 fixture의
전송 오류를 HTTP 오류로 완화하거나 재시도하지 않는다. 여섯 모드의 실패에는 mode·snapshot과
가짜 Python 예외 종류/errno만 남기며 원문 인증·응답·stderr는 기록하지 않는다.

무작업 RPC keeper에는 실제 `thread/start` 전송을 추가하는 변형을 심어 RPC 기록 단언이 실패함을 확인한다.
변형 뒤 제품 파일 bytes를 원복하며 원복한 후보에서 전체 검증한다.

## host 로그인·기본 계정(t-0510)

`cargo test -p aaa --test m13 accounts::`는 실제 인증을 읽지 않고 가짜 공식 CLI·토큰·갱신 서버만 쓴다.
기존 keeper가 로컬/원격 네 Codex 작업의 기본 계정 전달·동시 회전·기본 해제 뒤 pinned resume와 모든 기록 비노출을 확인한다.
`host_login_defaults_pin_threads_and_coordinators_without_exposing_credentials`는 공식 로그인 argv·격리 홈·원본 출력 차단,
임시 인증 정리, 새 worker/자동화 평가/원격 작업의 기본 선택, 기존 로컬 작업 유지, 명시 계정 우선, 깨진 기본 거부,
coordinator의 host 재시작 뒤 refresh·재부팅 복원·pane 재개와 같은 기록 홈을 확인한다.
동시 coordinator pane 재개는 Barrier 뒤 두 실제 CLI를 경주시켜 성공이 하나인지 확인한다.
준비 중 중복 거부는 `cargo test -p aaa-host --lib account_policy::tests::preparing_account_resume_rejects_duplicates_before_agent_spawn -- --exact`가 맡는다.
실제 `SpawnAccount` 전송을 받은 뒤 응답을 보류하고, 둘째 기동 요청 없이 거부되는지와 첫 응답 뒤 새 pane 결속·옛 pane 재거부를 확인한다.
CLI 버전 확인의 제품 제한시간(5초)을 시험 제어로 소진하지 않는다. 이전 시험의 gate 해제를 정상 호출 종료 사건 뒤로 늦추면
CI와 같은 성공 0개·기동 실패/중복 거부가 재현된다. 준비 중 claim 검사를 제거한 변형은 새 host owner가 중복 기동 요청에서 실패해야 한다.
인증 worker의 pane 재개는 저장한 thread launch·identity를 이어 쓰고, 실행 중 thread restart가 같은 홈을 다시 쓰지 않는지 본다.
로그인 실패·취소·host 정상 종료는 SIGHUP 무시 자식·PTY를 가진 자손을 포함해 종료와 폴더 정리를 기다린다. 가짜 자식 안에서 계정 관리 명령을
실행해 aaa 세션에서는 로그인·기본 변경·삭제가 거부되는지도 확인한다.

`parallel_host_logins_finish_independently_and_cancel_returns_after_cleanup`는 가짜 Claude/Codex를 동시에 보류한다.
한쪽 성공/실패의 독립 종료, 이미 끝난 자식의 상태 정리, 같은 ID 중복 cancel과 다른 ID cancel의 동시 호출,
두 로그인 중 host 정상 종료를 검사한다. cancel 응답 시 최종 상태·자식 PID 소멸·임시 홈 정리를 확인한다.
원래 비동기 취소 응답은 waiting을 반환해 이 keeper가 실패한다.

완료는 socket gate·CLI 상태·세션 종료 사건으로 판정한다. token은 해시만 비교하고 마지막에 DB/WAL·journal·
상태 폴더·로그의 원문/8바이트 조각/인코딩 변형을 검사한다. 원래 코드에서 이 keeper는 로컬 profile의 공식 인증 env가
host 실행 기록에 평문으로 저장되는 경로를 잡았다. host가 실행 환경을 저장하기 전에 가림 규칙을 등록해야 한다.
기본 선택 제거·로그인 원본 출력 노출·coordinator 인증 없이 재개·준비 중 인증 참조 허용 변형은 각각 이 keeper를 실패시켜야 한다.
실제 host 브라우저 인증은 배포 뒤 사용자가 확인하며 이 가짜 시험이 대신 보증하지 않는다.

## host·agent 재시작과 상태 복원(t-0491)

`cargo test -p aaa-host --lib transitions::tests::hello_replay_does_not_publish_idle_between_panes -- --exact`
는 Hello의 첫 pane Working 사건을 처리하는 실제 대기자 콜백에서 ticker를 실행한다.
뒤 pane의 상태를 아직 재생하지 않았다는 이유로 `쉼(no agent)` inbox를 만들거나 조회 묶음을 쉼으로 바꾸면 실패한다.
로컬·원격을 모두 검사하며, Hello가 끝난 뒤 실제 Idle은 한 번 알린다. 복원 울타리를 재부팅 pane에만 치는
원래 결함을 되살리면 같은 거짓 쉼 알림에서 실패해야 한다. 시간·부하·새 production 주입 지점을 쓰지 않는다.

`cargo test -p aaa --test e2e restart::restart_preserves_working_pty_without_input_signal_or_resize -- --exact`
는 격리 host, 로컬 UDS agent, 원격 TLS agent를 띄운다. 별도 FIFO로 진행하는 raw PTY harness가
host → 로컬 → 원격 순서의 SIGTERM 재시작 중에도 checkpoint를 기록해야 한다. 자식·session host PID,
수신 바이트 전체, SIGINT·SIGTERM·SIGHUP·SIGWINCH, 실제 PTY 크기와 Working 상태를 비교한다.
재연결 뒤 의도한 `p` probe를 **PTY에서 읽은 사건**을 기다려 앞서 큐에 든 예상 밖 입력도 검사한다.
Screen 응답은 PTY writer 완료의 울타리가 아니다. 마지막 실제 Esc는 Idle과 정확한 바이트를 확인하는 양성 대조군이다.
session 재연결에 Esc를 넣는 변형은 `local-back: inputs` 단언에서 실패해야 한다.

이 시험은 연결·PTY 생명주기 계약을 지킨다. 실제 Codex의 네트워크 요청, hook 실패, TUI 표시까지
대신 검증하지 않는다. 턴이 중단됐다는 진단은 화면 관측과 rollout의 후속 실행·`turn_aborted`·턴 경계를 함께 대조한다.
끝 20줄의 `esc to interrupt` 부재나 짧은 화면 무변화만으로 중단을 확정하지 않는다. 긴 추론 요약이 그 줄을 대신하는
실측 화면에서도 도구 실행은 이어졌다. 연결 직후 `no agent`는 상태 복원이 끝났는지도 먼저 확인한다.

## agent 재연결 중 상태 전달(t-0508)

`cargo test -p aaa-agent --lib link::post_tests::harness_changes_while_waiting_for_welcome_reach_the_host -- --exact`
는 실제 link 세션의 Hello를 받은 뒤 Welcome을 보류한다. 그 사이 기억한 Idle이 연결 뒤 전달되어야 한다.
Hello를 읽는 host에는 바뀐 사건만, 옛 host에는 최신 기억 전체를 보내며 Ping/Pong으로 앞선 전달의 끝을 확인한다.
연결 뒤 같은 밀리초의 새 Working도 뒤에 전달한다. `harness_hello.v1`이면 변경분을 생략하는 원래 결함을
되심으면 Idle 누락 단언에서 실패해야 한다. 새 production 시험 주입점은 없다.

기존 `cargo test -p aaa --test e2e stale_working_left_by_hooks_turns_idle_from_the_screen -- --exact`
는 재시작한 실제 agent의 연결 하나만 실험실 UDS로 중계한다. Hello의 옛 Working을 확인하고, 실제 감지기가
`harness.json`에 새 Idle을 적을 때까지 Welcome을 보류한 뒤 연다. host의 새 Idle 사건과 파일 기억을 모두 검사한다.
시간 상한·감지 간격은 그대로다. 원래 제품과 변경분 전송을 제거한 변형은 같은 host Idle 대기에서 실패해야 한다.
전송 계약의 세부 표는 link 시험이 맡고, 이 E2E는 화면 감지·저장·프로세스 재시작 경계를 맡는다.

`link::post_tests::live_hook_waits_for_the_replay_to_finish`는 작은 실제 duplex 전송 버퍼와 여러 pane의
기억 fixture로 replay를 멈춘 동안 실제 `prompt_submit` 전달을 진행한다. 재생 뒤 live hook이 빠짐없이 오고,
없는 세션의 실제 Screen 요청도 NotFound 응답을 받아야 한다.
상대가 Ping 뒤 write half만 닫으면 읽기 half가 남아도 세션이 끝나야 한다. 잠금 제거·replay와 reader 직렬화 변형은
각각 live 누락·EOF 대기에서 실패한다. 디스크 저장을 이 시험에서 반복하지 않는다.
`tests::pending_host_send_lets_go_when_the_connection_changes`는 옛 bounded 큐를 가득 둔 채 연결을 바꿔
옛 전송 대기가 끝나고 새 전송이 가는지 본다. 연결 변경 시 취소를 빼면 옛 전송 대기에서 실패한다.

## 원칙

- **사건으로 끝낸다.** 시험은 정한 시간을 자지 않고, 기다리는 것이 왔다는 신호(흐름 사건·출력·화면 표지·fake 기록·tick 끝남)를 기다린다. 「시간 제한이 있어야 통과하는 시험은 틀렸다」(t3code `AGENTS.md`). 시한은 오지 않을 때 멈추는 안전 상한일 뿐이다.
- **시간 규칙은 시계를 흉내 내지 않고 `now` 인자와 설정 값으로 본다.** 판정 함수는 `now`를 받는다(herdr `*_at(now)`, herdr-projects `Outage::record(now)`, aaa `inbox::Outage::record`). 그 함수를 유닛 시험에서 여러 `now`로 부른다. E2E는 설정 값으로 시간 창을 줄이고(아래 표), 창이 지난 뒤 `ticker tick`으로 host가 판정했음을 확인한다.
- **실패를 다시 돌려 넘기지 않는다.** 재시도·시한 늘리기·기대값 완화·건너뛰기로 통과시키지 않는다. 실패의 증거(실험실 폴더·화면·로그)를 남기고 원인을 찾는다.
- **실험실은 남의 것을 건드리지 않는다.** 빈 port, 짧은 임시 경로, 자기가 띄운 PID만 정리한다. 운영 host·사용자 셸 설정·실제 clipboard·소리에 닿지 않는다.
- **웹 E2E 실행 파일은 병렬 단계 전에 고정한다.** `ci-local.sh`는 이번 빌드의 실행 파일을 임시 사본에 복사·바이트 대조한 뒤 `AAA_BIN`으로 넘긴다. Cargo의 fresh `test --no-run`도 공용 실행 파일을 지웠다 다시 연결하므로, 그 경로로 병렬 실행하면 짧은 ENOENT가 난다. 사본은 전체 검증 종료 때 지운다. 준비 실패는 E2E 실패이며 재시도하지 않는다.
- **실행할 파일은 Unix에서 자식이 복사한다.** 병렬 시험의 부모가 실행 파일을 쓰면 다른 fork가 쓰기 fd를 물려받아 복사 뒤 exec도 `ETXTBSY`로 실패할 수 있다([Rust #114554](https://github.com/rust-lang/rust/issues/114554)). 별도 복사 프로세스가 끝난 뒤 실행한다. 재시도·sleep·시험 전체 직렬화로 덮지 않는다. Codex 설치·hook 시험의 `Home::moved_bin`이 이 경계를 쓴다.
- **웹 실험실은 writer 자손까지 종료한 뒤 지운다.** host·agent 종료 뒤 `meta.json`과 명령줄로 확인한 session host를
  먼저 멈추고, 자손도 부모부터 멈춰 추가 fork를 막는다. 확인한 PID를 종료한 뒤 모두 종료 상태가 된 것을 보고
  `rmSync`를 한 번 부른다. `node --test web/test/session-cleanup.test.mjs`는 별도 process group에서 계속
  mkdir/write하는 실제 자손과 무관한 프로세스를 함께 띄워, writer 종료·무관한 PID 보존·재시도 없는 삭제를 본다.
  session host만 죽이는 원래 정리는 writer 생존 단언에서 실패한다. 웹 E2E 13의 재부팅 흉내도 같은 완료를 기다린다.

## 어디에 두나

Windows M4p(t-0613, plan §8.394)는 `cargo test -p aaa-common --lib windows_acl_tests::`와
`cargo test -p aaa-common --lib ipc::tests::`로 실행한다. 실제 state/TLS 폴더·키·원자 교체 파일과 첫 Pipe·accept 뒤
두 인스턴스의 OS DACL을 읽고, Authz의 그룹 조회 없는 SID context로 현재 사용자 허용과 Everyone·Authenticated Users·
Users·Administrators·별도 SID 거부를 판정한다. 파일의 SYSTEM 예외와 Pipe의 SYSTEM 거부도 구분한다.
공개 ACL fixture는 실제로 Everyone 접근이 되는지 먼저 판정한 뒤 기존 키 재읽기·신원 보존·권한 이행을 본다.
상위 폴더 ACL 보존·hard link·잘못된 종류 거부·같은 사용자 바이트 전송·peer PID·중복 bind 거부를 함께 검사한다.
계정·시스템 정책·실제 사용자 설정을 바꾸지 않고, 고정 sleep 없이 연결·읽기 완료로 끝낸다.
웹 `node --test web/test/machineDetail.test.mjs`는 연결/끊김 Windows에만 제한 안내가 나오는지 검사한다.
이 시험들은 기존 workspace test에 포함되어 GitHub CI windows-2025에서 실행된다. macOS의
`cargo xwin clippy --workspace --all-targets --target x86_64-pc-windows-msvc --locked --keep-going -- -D warnings`와
같은 대상의 `cargo xwin build --workspace --all-targets --target x86_64-pc-windows-msvc --locked`는 실행 결과가 아니다.

M4p의 전체 검증에서 발견한 E2E 49의 기억 본문 검사는 focus 이후 새 routines 응답의 DOM 반영을 기다린다.
기준 판의 실제 새 응답을 gate로 보류하면 host에는 새 기억이 있어도 기존 상세는 비어 있고, 즉시 검사하던 시험이
「기억 본문 없음」으로 실패한다. E2E 46은 다른 기기가 저장하기 전에 둘째 폼의 제목·저장 가능 상태로 편집 반영을 확인한다.
원래 본문·갱신 thread·충돌·파일 보존 단언과 시한을 유지하며 sleep이나 재실행으로 통과시키지 않는다.

Windows named pipe BUSY 회귀는 `cargo test -p aaa-common --lib ipc::tests::`로 실행한다. 실제 `LocalListener`를
한 연결로 점유하고 두 번째 연결의 OS 오류 231, 계속 점유할 때의 제한된 실패, accept 뒤 다음 연결과 바이트·peer PID를
검사한다. 없는 pipe 오류는 첫 poll에서 반환해야 한다. 고정 sleep 없이 accept 사건으로 연결을 푼다.
hook CLI 진단은 기존 `codex_integration`의 무음 시험이 기본 exit 0·빈 출력과 `AAA_LOG=debug`일 때만 나오는
전송 실패 원인, token·payload 비노출을 함께 확인한다.

| 무엇 | 위치 | 실행 |
|---|---|---|
| 순수 규칙·판정(Rust) | 소스 옆 `#[cfg(test)]` | `cargo test -p <crate>` |
| 여러 프로세스(host·agent·session·CLI) | `crates/aaa/tests/*.rs` + 실험실 `crates/aaa/tests/lab/mod.rs` | `cargo test -p aaa --test m13 <이름>` |
| 웹 순수 규칙 | `web/test/*.test.mjs`(`node --test`) | `cd web && npm test` |
| 웹 화면(실제 host·agent + Playwright) | `web/e2e/specs/NN-기능.mjs` + `web/e2e/lib/harness.mjs`(아래 「웹 E2E」) | `node web/e2e/web.e2e.mjs [--spec 07]` |

## 기다리기

### 부하 때 프로세스 시작·전달 대기(t-0606)

t-0578 첫 착지의 동시 검증에서 m13 다섯 시험과 웹 E2E 63의 아래 대기가 30초를 넘겼다.
실패한 m13의 머신 건강에는 CPU 10개에서 1분 부하 28.96(뒤 표본 24.89·21.06)이 기록됐다.
다음 전체 검증에서는 모두 통과했고 웹 후보 점검 단계는 1491ms였다. 현재 코드의 기존 상한 대조에서도
해당 m13 다섯 시험·task_queue·task_edit와 웹 63 전체가 통과했다.
추가로 Rust·웹 전체를 겹친 대조에서도 기존 30초의 m13 103개와 웹 63은 통과했다.
1분 부하가 74.13까지 올랐지만 원래 실패는 다시 나오지 않았다. 따라서 이 조정으로 재현한 결함을
고쳤다거나 60초면 어떤 부하에서도 충분하다고 주장하지 않는다. 원본 부하 실패 지점에만 여유를 둔다.

아래 지점에만 60초의 실패 상한을 쓴다. 기존 30초의 두 배로 시작·전달의 스케줄 여유를 두며,
정상 실행을 60초 기다리는 sleep이나 성능 합격선으로 쓰지 않는다. Rust는 `lab::LOAD_TIMEOUT`,
웹은 harness의 `LOAD_TIMEOUT_MS`에 모은다. 사건·파일·DOM이 오면 즉시 끝나고,
실패한 명령의 재실행·기대값 완화·전역 시한 변경은 하지 않는다.

| 시험 | 30초 → 60초인 대기 | 유지하는 경계 |
| --- | --- | --- |
| m13 `rollout::rollout_*` 세 시나리오 | 처음 사본 agent 둘 연결 | 사전 검사·설치·서비스·복구의 각 30초 |
| m13 `terminal_capture_keeps_output_after_stdio_close_before_reading` | 자식의 표준 FD 닫힘 영수증 | reader를 늦게 여는 순서·정확한 출력·수집 30초 |
| m13 `thread_adopt_existing_pane` | 처음 SessionStart의 대화 ID | 데려오기 거부·보고·재기동·권한 |
| m13 `thread_read_and_keys` | 처음 trust 질문 사건 | 뒤 권한 질문·응답·신뢰 창 거부 |
| m13 `thread_adopt_claude_started_in_a_shell` | 데려온 뒤 첫 brief 전달 | Claude 감지·보고·pane 안 CLI 결과 30초 |
| 웹 E2E `63-deploy` | 맞는 후보의 `deploy-sha` 표지 | SHA 거부·배포 중단·서비스 재시작·기본 locator 30초·단계 120초 |

다음 실패는 같은 방식으로 늘리지 않는다.

- E2E 47은 파일 사건 뒤 최종 DB 판정을 저장하고 갱신 사건을 빠뜨린 제품 결함이다. t-0573의
  결정적 재현·수정을 유지한다. `resource_limits`의 단발 Front EMFILE 미관측은 원인 미확정이며
  15초와 실패 진단을 유지한다.
- Ubuntu CI [39529369100](https://github.com/cLazyZombie/aaa/actions/runs/39529369100)의
  `task_queue_runs_possible_fifo_after_resolve_without_coordinator`는 `thread start`가
  「준비·정리 작업 중」으로 거부된 오류다. [39531630027](https://github.com/cLazyZombie/aaa/actions/runs/39531630027)의
  `task_edit_keeps_bytes_conflicts_and_logs`는 `파일 직접/update` 대신 `파일 직접/add`를 관측한
  단언 실패다. 둘 다 대기 시한 초과가 아니므로 별도 경합으로 조사한다.
- t-0575 rollout의 후보 SHA 거부 뒤 `recovering/restoring`·서비스 재시작 전 상태는
  사본 agent 시작 대기와 다르다. t-0587의 원격 교체 단계 진입 뒤 대기하는 수리를 유지한다.

검증은 기존 상한의 단독 실행과 `scripts/ci-local.sh --only test,e2e`의 동시 부하 대조를 남긴 뒤,
변경 후보의 `scripts/ci-local.sh` 전체로 한다. 부하 대조는 같은 조건에서도 항상 실패한다는 뜻이
아니다. 실패한 대기와 그때의 부하·관측 상태를 함께 기록하고, 부하 없는 실패나 상태 오류가 나오면
시한을 더 늘려 덮지 않는다. 진행이 없는 사건에 상한을 되풀이해 새로 시작하지 않는다.

| 기다릴 것 | 쓸 것 |
|---|---|
| 흐름 사건(thread 시작·보고·질문·병합) | `lab.follow(..)` → `Follow::kind`·`until`(`aaa events <p> --follow --json`, cursor 0부터라 놓치지 않는다) |
| pane 출력에 글 | `Node::wait_output`·웹 `waitOutput`(`aaa pane wait-output`, host가 출력 사건마다 다시 본다) |
| 「host가 보고도 아무것도 하지 않았다」(중복 항목·다시 알림이 없음) | `Node::ticks(n)`(`aaa ticker tick`: 이 명령 뒤에 시작한 tick이 끝나면 돌아온다) 뒤에 단언. herdr-projects 시험이 `ticker::tick_project`를 직접 돌리는 것과 같다 |
| 시간 창이 지난 뒤의 판정(idle·quiet·schedule·outage) | 설정 값으로 창을 줄이고, 창이 지난 것을 확인한 뒤 `ticks` |
| 상태 전이(머신 연결·pane 끝남) | `wait_for`(시한 있는 반복). 사건이 생기면 그것으로 바꾼다 |
| 웹 화면이 그렸다 | 제품 표지(`data-view-top`·`data-view-base`·`data-history-*`·`data-status`·`data-kind`)를 기다린다. 「몇 frame 뒤」를 어림하지 않는다 |
| 웹 화면이 판단을 끝냈다(그래서 하지 않았다) | 판단 자리에서 다는 제품 표지 뒤에 본다. Term `data-parsed-top`·`data-parsed-base`(받은 출력을 다 읽은 뒤 버퍼 자리: 올려 둔 동안 아래에 줄만 붙으면 그리기·`data-view-*`는 오지 않는다), `data-retry`(다시 붙기를 건 close code·ms), `data-fit`(맞추기 판단 off·released·other·keyboard·input·unsized·sent·same)·`data-fit-pending`(크기 바뀜 뒤 맞추기를 기다리는 동안), App `data-wake-lock`(wake lock effect가 돈 설정·걸지·보는 것). 크기·다시 붙기는 그 판단 안에서 바로 보내므로 표지 뒤 `sentFrames`가 비면 보내지 않았다 |
| agent·harness 사건을 host가 처리했다(버린 Stop 등) | 같은 agent 연결로 뒤따르는 표지를 기다린다: 진행 보고(`aaa report` → `thread show`의 `agent.progress`), 그 뒤의 출력. 연결 하나는 순서를 지킨다 |
| host가 일을 끝냈다는 사건이 있다 | 그 사건을 기다린다(`routine_ran`·`nudge_sent`, 새 점검 보고 `health.at_ms`, routine `last_run_ms`). 「몇 초면 돌았겠지」를 어림하지 않는다 |
| 프로세스가 끝났다(session host) | `Node::session_host_gone(pid, session)`(PID는 지우기 전 `session_host_pid`로) |
| 웹 화면이 보내지 않았다(reader 입력·조합 중 Enter·끌기의 마우스 보고) | `sentFrames(page, from)`·`sentText`(harness가 pane WebSocket으로 보낸 메시지를 기록한다). 키·마우스·휠 처리는 그 사건 안에서 보내므로 동작이 끝난 뒤 바로 본다. 타이머로 늦게 보내는 규칙(꾹 누름 되풀이)은 가짜 시계 유닛 시험(`web/test/keyRepeat.test.mjs`)이 본다 |

- **고정 sleep ratchet.** `web/test/fixed-sleeps.test.mjs`가 `web/e2e`·`web/test`·`web/src`의 `sleep(N)`·`waitForTimeout`·setTimeout Promise와 Rust 시험의 `thread::sleep`·`tokio::time::sleep`을 센다(herdr-web-ui `scripts/fixed-sleeps.ts` 그대로). 파일마다 `web/test/fixed-sleeps.json`의 수를 넘으면 실패하고, 줄면 그 수로 낮춰야 하며, origin/main보다 올릴 수 없다(새 파일로 옮긴 것은 합계로 본다). 다른 파일에서 import한 sleep helper(spec이 쓰는 harness `sleep`)도 센다. 시한이 있는 반복 안의 쉼과 `Promise.race`의 시한은 세지 않는다. 일부러 고정한 기다림(「아무것도 오지 않음」 증명)은 그 줄이나 윗줄에 `// fixed wait: <이유>`를 단다. 지금 수: `node web/test/fixed-sleeps.mjs`(`--list`면 줄마다).
- **tick만으로 증명되지 않는 것.** 시간 규칙이 막는 일(예: idle 2초 전에는 nudge하지 않음)은 tick을 백 번 돌려도 일어나지 않는다. 그런 「없음」은 창이 지난 뒤에 tick해야 결함을 잡는다. 순서를 틀리면 시험이 늘 통과한다. 창은 `// fixed wait: <이유>`를 단 sleep으로 지난다(그 시험이 보는 규칙 자체라 표시한 고정 기다림이다). nudge는 m13 `nudge_windows(idle, quiet)`(처음 보는 tick → idle 창 → 입력창을 보는 tick → quiet 창 → 보내는 tick, 창은 그 시험의 host.toml 값).
- **「없음」은 울타리 뒤에 단언한다.** 고정 sleep 뒤에 「일어나지 않았다」를 단언하면 그보다 늦게 온 결함을 놓친다(거짓 통과, 보안 감사 t-0340). 울타리는 위 표의 셋이다: host 판정(창 + `ticks`), 같은 연결의 뒤따르는 표지·끝남 사건, 화면이 보낸 것(`sentFrames`). 바꾼 시험은 그 「없음」을 깨는 결함을 제품에 심어 실패하는지 본다(plan §8.121).

### 시험에서 시간 창을 줄이는 설정

| 규칙 | 설정 |
|---|---|
| ticker 주기 | `host.toml [projects] tick_secs`(1 이상) |
| 머신 끊김 알림 | `[projects] outage_secs` |
| coordinator nudge | `[projects] nudge_idle_secs`·`nudge_quiet_secs` |
| 머신 건강 | `[health] every_s` |
| routine | routine 머리 `schedule = "every 2s"`(host에 `AAA_TEST_ROUTINE_SECONDS=1`이 있을 때만 받는다, lab이 붙인다) |
| 알림 묶음 | `[notify] debounce_secs`(초). 1초 밑은 `AAA_TEST_NOTIFY_DEBOUNCE_MS`(시험에서만, 웹 E2E 17·18 100. 설정 1초 길은 12) |
| 웹 상태 사건 ping | `AAA_TEST_EVENTS_PING_MS`(시험에서만) |
| thread brief를 치기 전 화면 멈춤(3초) | `AAA_TEST_BRIEF_SETTLE_MS`(시험에서만, 웹 E2E·m13 300. m13은 `spawn_host`로 띄운다) |
| 「쓰는 화면」 lease(25초, 앱 글이 그만큼 없으면 쓰는 화면에서 뺀다) | `AAA_TEST_PRESENCE_LEASE_MS`(시험에서만, 웹 E2E 18만 4000: 그 파일의 쓰는 화면은 presence를 1초마다) |
| 끝난 session host가 늦게 붙는 agent를 위해 더 응답하는 창(2초) | `AAA_TEST_SESSION_LINGER_MS`(시험에서만, 늘리는 쪽: m13은 8000으로 forget 한도 5초보다 길게 두어 지우기가 이 창을 기다리지 않는지 본다, plan §8.160) |
| 구독 사용량 억지 다시 읽기 간격(30초) | `AAA_TEST_USAGE_REFRESH_MS`(시험에서만, 웹 E2E 32 0: 가짜 구독 숫자를 바꾼 뒤 `?refresh=1`로 바로 읽는다) |
| harness 화면 감지 시작 유예(3초, 시작 화면을 상태로 읽지 않는다) | `AAA_TEST_DETECT_GRACE_MS`(agent, 시험에서만, 웹 E2E 11·19 0. 기본값 길은 15) |
| 긴 턴(60초, 웹 앱 안 알림은 그보다 긴 턴의 끝남만 띄운다, plan §8.185) | `AAA_TEST_LONG_TURN_MS`(host, 시험에서만, 웹 E2E 19 3000. 기본값 길은 m13 `turn_end_notices_follow_reports`) |

## 웹 E2E

### 두 controller의 전송·focus 순서(t-0488)

- E2E02는 입력줄 전송의 첫 resize를 완료로 보지 않는다. 글 뒤 Enter도 lease를 얻으므로,
  `submitted`를 처리한 입력줄(빈 값·다시 켜진 Enter 단추)과 해당 PTY 출력을 기다린 뒤 focus·blur·숨김으로 간다.
  host의 크기·lease와 두 화면의 제목·실제 행 수·`data-fit`·`data-fit-pending`을 함께 확인해 이전 state를 기준으로 삼지 않는다.
- 보이는 창의 blur는 페이지 시계를 멈춰 999ms에는 release 없음, 1000ms에는 release 있음을 확인한다.
  첫 `pauseAt`으로 설치와 정지를 함께 한다. 별도 `install` 뒤 벽시계로 멈추면 과거 시각을 지정할 수 있다.
  숨김은 `visibilitychange`를 보낸 같은 task에서 pane WebSocket 기록을 닫아 즉시 release를 본다.
  자동화 왕복 시간·700ms 벽시계 한도·Playwright `framesent` 도착 순서에 기대지 않는다.
- E2E03의 크기 되찾기도 500ms를 자지 않고 첫 화면의 새 grid·맞추기 판단을 기다린다.
  키보다 resize가 먼저 간다는 기존 전송 순서 단언은 유지한다. fixed-sleep 상한은 하나 낮춘다.
- 원본은 `AAA_E2E_THROTTLE=2`·`--spec 02 --grep '두 controller' --repeat 6 --workers 6`에서
  focus 실패 3/6을 재현했다. 고친 시험은 같은 조건 6/6, `AAA_TEST_SUBMIT_GAP_MS=1500`으로
  Enter를 늦춘 전체 02도 3/3 통과했다. 시한·허용 오차·시나리오는 줄이거나 늘리지 않는다.
- focus 되찾기 제거, blur의 1초 미만 release, 숨김 즉시 release 제거를 독립 변형으로 심어
  기존 keeper가 각각 실패하는지 본다. 제품 소스는 바이트로 복원하고 웹·실행 파일을 다시 빌드한 뒤 전수 gate를 실행한다.

### diff index와 입력줄 시계의 간헐 실패(t-0496)

- `cargo test -p aaa-agent --lib diff::tests::`의 기존 callback 시험은 같은 크기·mtime의 덮어쓰기와
  그 시각의 index를 만든다. `core.checkStat=minimal`·`core.trustctime=false`로 stat 정밀도의 우연을 없앤다.
  Linux에서 원본은 `+two` 단언에 실패했고 index 사본의 mtime 보존 뒤 통과했다. macOS의 원본 통과로
  Linux를 대신 판정하지 않는다. 원본 index의 바이트·mtime과 외부 callback 미실행도 그대로 검사한다.
  index mtime은 [Git의 racy entry 내용 재검사 기준](https://git-scm.com/docs/racy-git)이므로 사본을 만든
  현재 시각으로 바꾸지 않는다. 열린 원본 파일에서 내용과 시각을 함께 가져온다.
- E2E08은 `clock.install()` 뒤 별도 벽시계의 `pauseAt(new Date())`로 멈추지 않는다. 설치된 페이지 시계가
  벽시계보다 앞서면 과거로 진행하려다 실패한다. `install` 뒤 `runFor(1000)`을 넣은 결정적 재현에서
  CI와 같은 오류를 확인했다. 첫 `pauseAt`이 설치와 정지를 함께 하도록 하며 조합 취소·기한·PTY 단언은 유지한다.
  [Playwright Clock](https://playwright.dev/docs/api/class-clock)의 공개 API를 쓰고 타이머·자동화 왕복을 기다리지 않는다.

### 크기 기준값과 조합 확정 기한(t-0463)

- E2E06의 글꼴 기준값은 제목과 host 열 수가 같은 것만으로 정하지 않는다. 둘 다 이전 snapshot의 크기일 수 있으므로
  `data-fit="same"`이고 `data-fit-pending`이 없어 이 화면의 맞추기까지 끝났을 때 잰다. 제목은 state 수신 때 먼저 바뀌고
  xterm resize는 출력 큐 뒤에 실행되므로, 실제 행 수와 현재 글꼴의 영문 advance 대비 칸 너비까지 맞아야 기준값이다.
  영문 비교는 공용 `fontOk`의 0.02px를 그대로 쓰고 실패에는 기준값도 남긴다. 글꼴 응답은 route gate로
  붙잡고, 받기 전에는 기존 글꼴·칸 크기, 놓은 뒤에는 1.5배 칸 크기·PTY 크기를 확인한다. reload도 요청 도달 뒤 gate를 놓는다.
- E2E08의 합성 조합 확정은 페이지 시계를 멈춰 본다. 자동화의 왕복 시간이 300ms 확정 기한을 소비하지 않도록 하고,
  compositionend가 건 task와 이어지는 조합의 450ms는 `clock.runFor`로 진행한다. 취소·기한 뒤 보내지 않음은
  페이지가 보낸 frame 기록까지 확인한다. 실제 CDP IME·host의 보내기 확인·PTY 수신은 실제 사건으로 기다린다.
- E2E09의 IME 기록은 700ms 뒤 PTY를 읽지 않는다. 페이지가 보낸 전체 글을 기록으로 기다려 기대 문자열과
  비교하고, PTY의 GOT가 그 길이만큼 도착한 뒤 전체 문자열을 다시 비교한다. 다음 MARK는 그 수신 뒤에 보낸다.
  전송 지연이 앞 사례의 입력을 다음 사례에 섞지 않으며, 덧붙은 자모·중복 글도 그대로 실패한다.
- 두 시험은 `node web/e2e/web.e2e.mjs --spec 06-settings,08-input-line --repeat 5 --workers 1`로 좁게 돈다.
  느린 조건의 원본 실패와 수리 뒤 연속 통과는 별도로 남긴다. 기존 시간 상한·허용 오차·조합 수는 바꾸지 않는다.

보고서 열람은 `aaa-host`의 `report_navigation_records_seen_and_opened_but_not_previews`가 실제 HTTP 응답 뒤 DB의
안 봄·마지막 열람·배지 사건을 함께 확인한다. HEAD·미리보기·fetch·그림·실패 응답은 기록하지 않는다.
웹 E2E 21은 제목·공유 링크 클릭과 cookie 없는 Chromium·WebKit 탭의 최초 주소 탐색 뒤 앱 배지가 줄어드는지 본다.
HTML fixture에는 열람 기록용 스크립트를 넣지 않는다. 기록은 브라우저 문서 요청을 받은 host가 한다.
웹 E2E 44는 대화 안 찾기(plan §8.308)를 가상화된 400줄 기록에서 본다. 이동이 끝났는지는 「몇 frame 뒤」가 아니라 칸의 `data-find-index`·`data-find-revealed`로 기다리고, 그린 줄의 데이터 개수와 그린 글의 찾음 수(`data-find-count`·`data-find-dom`)가 같은지 본다. 「앞 기록도 찾기」(§8.310)는 칸의 `data-find-earlier`(idle·loading·failed)·`data-find-pages`를 기다린다. 「멈춘 뒤 앞 쪽을 더 부르지 않음」은 앞 쪽 요청을 route로 센 뒤, 기록에 말을 더해 같은 한 줄 차례(`ConversationRefresh`)의 최신 쪽 읽기가 그려진 것을 울타리로 삼아 단언한다. Codex 기록은 같은 웹 길을 iPad WebKit으로 본다.
웹 E2E 38은 대화에서 연 보고서(plan §8.300)를 WebKit iPhone 390·430 Safari 탭·홈 화면 앱 흉내로 본다. 「새 창이 없음」은 앱 안 보기(`file-viewer`)가 그려진 뒤에 보고, 휴대폰 sheet는 올라오는 움직임(`getAnimations().finished`)이 끝난 뒤 닫기 자리를 잰다. 헤드리스 WebKit의 휴대폰 꼴(hasTouch)도 `pointer: coarse`다. 같은 화면에서 파일 보기 머리가 한 줄(64px 이하, 칸 겹침·잘림 없음)이고 몸이 화면 높이의 85% 이상인지도 잰다(plan §8.302).
웹 E2E 42는 thread 변경 파일 카드와 diff 패널(plan §8.305)을 실제 git 저장소 thread로 본다. 가짜 agent가 보고를 쓴 뒤 worktree에 만드는 새 파일을 파일 존재로 기다리고 나서 worktree를 바꾼다. 카드는 `data-files`(받은 파일 수), 패널은 `data-diff-status="ready"`·`data-diff-lazy`·`data-diff-loaded`(큰 비교에서 읽은 파일 수) 표지를 기다린다. diff 줄은 CodeView shadow root 안의 글로 본다. agent 쪽 읽기 전용 경계(index·HEAD·ref·`.git` 안 파일이 바이트로 같음, 외부 callback 안 돎)는 `cargo test -p aaa-agent --lib diff::`가 실제 임시 저장소로 본다.

### 대화 찾기의 가상 줄 관측 묶음(t-0542, plan §8.337)

- E2E44 「앞 기록의 가상 줄 관측 묶음」은 기존 멈춤·이어 찾기 시나리오를 그대로 실행한다. 첫 찾음 줄의 실제
  IntersectionObserver 화면 밖 관측을 보류하고, Enter로 그 줄에 갈 때 생기는 실제 화면 안 관측과 시간순으로 함께 전달한다.
  보류 표지를 기다리고 전달 묶음 `false,true`도 단언한다. 원본과 주입 단계의 울타리 문구를 구분해 이번 append의 commit을 기다린다.
  가짜 좌표·entry·고정 sleep·시한 연장은 없다.
- [관측 규칙](https://www.w3.org/TR/intersection-observer/#notify-intersection-observers)은 모인 entry 전체를 전달한다.
  한 줄에 observer 하나여도 첫 entry만 쓰면 최신 화면 안 관측을 버려 본문이 빈 채로 남고 찾기 재개용 DOM 변화도 오지 않는다.
  ChatRow는 마지막 entry로 본문을 그릴지 정한다. 느린 통지에서 사용자가 찾거나 스크롤할 때도 해당하는 제품 결함이다.
- 수정 전 주입은 기존 20초 「앞 기록의 처음 1/7」에서 count=7·index=0·revealed=false·earlier=idle로 RED였다.
  첫 찾음 shell은 화면 안에 있지만 본문이 없었다. 같은 주입 GREEN과 주입 없는 기존 네 단계의 반복 통과를 별도로 보존한다.
  최초 착지 실패의 DOM은 남아 있지 않아 그 실행의 실제 entry 묶음까지 확인한 것은 아니다.
  원래 단언(강조 Range의 실제 보임·접힘 펼침·개수 일치·가상화)과 시한을 유지하며 전체 ci-local·check-public을 실행한다.

```
web/e2e/web.e2e.mjs        입구: spec 파일마다 lib/child.mjs 프로세스를 띄우고(worker N개가 함께) 결과를 모은다
web/e2e/durations.json     파일마다 기록한 초. 함께 돌 때 긴 파일부터 넣는다
web/e2e/lib/harness.mjs    실험실(host·agent·가짜 push·구독 서버·시험 프로그램)·Chromium·step·기다림·화면·IME·push 도우미, runSpec
web/e2e/lib/devices.mjs    기기 꼴과 그 기기 규칙의 가짜(iOS 소리·알림), 기록을 사건으로 기다림(아래 「실기와 headless」)
web/e2e/specs/NN-기능.mjs  기능별 단계(01-http-pwa … 19-turn-alerts). export default async function ({ ctl, page }) { … }
```

- **파일마다 새 실험실.** host·agent·Chromium을 파일마다 새로 띄운다(`/tmp/aaw-<pid>`, 준비 약 1.5초, 웹 port는 host가 고른다). 앞 파일이 만든 pane·질문·구독·설정에 기대지 않으므로 파일 하나를 혼자, 여러 번, 다른 파일과 함께 돌릴 수 있다. 한 파일 안 단계는 차례로 돌고 앞 단계가 만든 상태를 쓸 수 있다(그래서 `--grep`으로 뒤 단계만 고르면 앞 단계가 만든 것이 없을 수 있다: 그때는 `--spec`으로 파일을 돈다).
- **준비(`runSpec`).** host·agent(`labA`)·셸 탭 `t1`을 띄우고, 첫 화면 `ctl`·`page`(1280×800, 입력줄)를 t1에 연다. t1에는 `t1-42`가 찍혀 있다: 단계가 t1로 돌아왔는지는 이 글로 본다. 단계가 실패하면 그 파일의 나머지 단계는 돌지 않고 다음 파일로 간다. 파일 안 여러 단계가 같은 준비에 기대면 harness의 준비 도우미를 부른다(`pushLab`: push 서명 키·구독 하나·demo, 17·18). 준비는 `step(…, { always: true })`라 `--grep`으로 단계 하나만 골라도 돈다. 그 파일만 실험실 환경을 바꾸려면 `export const labEnv = { … }`(11: `AAA_TEST_DETECT_GRACE_MS`, 17·18: `AAA_TEST_NOTIFY_DEBOUNCE_MS`, 18: `AAA_TEST_PRESENCE_LEASE_MS`). 제품 시간을 줄이는 값은 그 기다림을 기본값으로 도는 파일을 하나 남긴다(위 표).
- **함께 돌기.** 파일은 worker 여러 개가 함께 돈다. 기본은 16 CPU 이상이면10개, 그 밖은 core 둘마다 하나다(`MAX_WORKERS`, t-0670의 같은 머신 전체 검증으로 비교), `--workers 1`·`AAA_E2E_WORKERS=1`이면 예전처럼 차례로 돌고 출력을 바로 보인다. 2 이상이면 파일이 끝날 때 그 파일의 `## specs/…`와 단계 줄을 한 덩어리로 찍는다. 함께 돌 때는 `durations.json`의 긴 파일부터 넣는다(LPT): 가장 긴 파일이 늦게 시작해 혼자 남지 않게. 적지 않은 새 spec은 가장 긴 것으로 보고 맨 앞에 넣는다. 파일 시간이 크게 바뀌면 `--write-durations`(모두 통과한 실행의 시간을 적는다)로 다시 적는다. 시간이 늘어도 재시도·timeout 연장으로 넘기지 않는다: 한 파일이 혼자 오래 걸리면 그 파일을 기능별로 나눈다(앞 단계 상태에 기대는 단계는 그 준비를 도우미로).
- **고르기.** `--spec 07`·`--spec 07-scroll-history,09`(파일), `--grep '앞 기록'`(단계 이름 정규식, `AAA_E2E_ONLY`와 같다), `--repeat 25`(고른 파일을 새 실험실로 25번, 끝에 통과 수와 중간·가장 긴 시간. 기본 worker 수로 함께 돈다: 혼자 돌 때 간헐 실패는 `--workers 1`), `--list`(돌지 않고 이름만). 스크린샷 폴더는 첫 인자.
- **새 기능.** 그 기능의 spec 끝에 단계를 더한다. 새 화면·새 기능 묶음이면 다음 번호로 새 spec을 만든다. 도우미는 두 spec 이상이 쓰면 harness에 두고 끝의 `export { … }`에 더하며, 하나만 쓰면 그 spec 안에 둔다. 단계 이름은 모든 spec에서 하나다. harness 이름을 쓰면 spec 머리 `import { … } from "../lib/harness.mjs"`에 더한다(빠지면 그 단계가 ReferenceError로 실패한다). 틀은 `web/test/e2e-layout.test.mjs`가 본다.
- **기다리기.** pane 출력은 `waitOutput`(host 사건), DOM은 `domIs`(MutationObserver), 주소는 `hashIs`, 그 밖은 `until`(시한 있는 반복). 새 시험은 앞 셋을 먼저 쓴다. 「이 화면이 보내지 않았다」는 `sentFrames`로 바로 보고, 「pane이 다 받았다」는 보낸 글(`sentText`)과 받은 글이 같아질 때까지 기다린다(03 키 막대 `sent`).
- **나누기 전 한 파일을 고친 branch.** 병합에서 `web/e2e/web.e2e.mjs`가 충돌하면 main의 입구를 받고 자기 변경을 옮긴다:
  ```
  git diff $(git merge-base <내 branch> origin/main) <내 branch> -- web/e2e/web.e2e.mjs > /tmp/mine.patch
  git checkout origin/main -- web/e2e/web.e2e.mjs
  node web/e2e/tools/port-patch.mjs /tmp/mine.patch   # hunk마다 같은 글이 있는 spec·harness를 찾아 고친다, 새 helper는 export·import까지
  ```
  단계 본문은 글자 그대로 옮겼다(바꾼 줄은 t1 표지 기다림 `"한글-8"` → `"t1-42"`과 `pushKey = …` → `setPushKey(…)` 둘, 도구가 같이 바꿔 찾는다). 도구가 못 옮긴 hunk(새 단계 등)는 찍어 주므로 그 기능의 spec에 손으로 둔다.

## 실험실

- Rust: `lab::Lab::new()`가 `/tmp/aae-<pid>-<n>`을 만든다. macOS UDS 경로 한도(104바이트) 때문에 worktree 안이 아니라 짧은 `/tmp`에 둔다(herdr `/tmp/hcli-…`도 같다). Windows는 Named Pipe라 임시 폴더다.
- `lab.node(name, host_toml)`이 설정·상태 폴더를 따로 만든다. host 여러 개·agent 여러 개를 같은 머신에 띄워 여러 머신을 흉내 낸다. listen은 `127.0.0.1:0`(OS가 빈 port를 고른다).
- 명령 없는 pane의 기본 셸은 `/bin/sh`다(`SHELL`을 lab이 정한다, herdr harness와 같다). 사람의 zsh 설정이 화면에 섞이지 않고 로컬과 CI가 같은 화면을 본다.
- 그 셸은 login이라 기계 rc를 읽는다(macOS `/etc/bashrc`는 prompt가 `\h:\W \u\$ `, hostname 길이가 기계마다 다르다). 그래서 lab은 노드 설정 폴더의 `shrc`(`PS1='$ '`)를 `ENV`로 주고 `TERM_PROGRAM`을 뺀다. sh는 rc 다음에 `ENV`를 읽어 모든 시험 셸 prompt가 `$ `다(`tab create -- /bin/sh`도). prompt 모양에 기대는 시험은 이 값을 쓴다.
- 띄운 프로세스는 `Child`로만 잡고 끝날 때 그 PID만 끝낸다. session host는 meta.json의 PID 중 명령줄이 이 실험실 경로인 것만 끝낸다. 시험이 실패하면 폴더를 지우지 않고 「실험실 기록을 남겼다: …」를 찍는다(CI는 그 폴더의 로그를 annotation으로 남긴다).
- Rust 실험실은 `AAA_USAGE_HOME`을 노드 안 빈 폴더로 고정한다. ticker의 주기 조회가 실제 CLI 로그인·keychain·외부 구독에 닿지 않는다. 사용량을 보는 host 유닛은 `UsageContext`와 loopback 서버를 직접 제공하고, 웹 E2E는 기존 가짜 구독 서버를 쓴다.
- ticker를 띄운 Host 유닛 시험은 runtime을 종료한 뒤 임시 폴더를 지운다. ticker·사용량 조회·자동화 task가 Host를 보유하므로 `drop(h)`만으로 SQLite가 닫히지 않는다. `ticker_refreshes_usage_without_a_browser_and_releases_pending_input`는 종료 뒤 `Weak<Host>`가 해제됐는지 단언한다. Windows SQLite 핸들은 삭제 공유를 허용하지 않아, 이 경계 전에 폴더를 지우면 공유 위반이 난다.
- 자주 쓰는 것: `Node::ok`·`json`(CLI JSON), `screen`, `new_pane`, `wait_output`, `connected`, `ticks`, `Lab::follow`, `kill`(SIGKILL로 갑작스러운 죽음), `kill_session_hosts`(재부팅 흉내).

## 선택자(웹)

1. 사람이 보는 역할과 이름: `getByRole("button", { name: "연결" })`, `getByLabel("보낼 글")`. 접근성과 같이 지켜진다(herdr-web-ui 시험이 가장 많이 쓴다).
2. 역할이 없는 자리는 `data-testid`(`pane-<label>`, `machine-<name>`, `terminal`).
3. 상태는 `data-*` 표지로 읽는다(`data-status`, `data-kind`, `data-phase`, `data-view-*`). 글자나 색으로 상태를 짐작하지 않는다.
4. class 선택자는 모양(크기·색·겹침)을 재는 시험에만 쓴다.

## 실기와 headless

- headless로 못 보는 것(실제 iOS 키보드 내림·노치·무음 스위치·홈 화면 앱의 잠듦)은 plan 각 절의 「실기 확인 필요」에 적고, 실기에서 본 사건 순서를 기록해(입력 기록, `web/src/inputTrace.ts`) headless 재생으로 옮긴다.
- 기기 능력(소리·알림·push·화면 키보드·안전 영역·wake lock·설치)은 실제를 쓰지 않고 기록하는 가짜로 바꾼다. 가짜는 그 기기의 규칙을 따라야 실기 결함을 잡는다. 헤드리스 Chromium의 진짜 AudioContext·Notification에는 iOS 규칙이 없다.
- **`web/e2e/lib/devices.mjs`.** `pairDevice(browser, code, "iphone" | "ipad" | "desktop", { standalone, silentSwitch, permission })`가 그 기기 꼴(크기·UA·platform·터치)로 pairing하고 iOS 규칙 가짜를 건다:
  - AudioContext는 suspended로 태어나고 사람이 누른 직후(1초 안)의 `resume`만 running이 된다. 홈 화면으로 나가면 interrupted이고 다시 누르기 전에는 그대로다. running이 아닐 때 낸 소리는 `silent <Hz>`로 남는다.
  - 무음 스위치(`silentSwitch`)를 켠 기기는 소리 범주(`navigator.audioSession.type`)가 playback·play-and-record가 아니면 `muted <Hz>`.
  - Safari 탭에는 `Notification`·`PushManager`가 없다. 홈 화면 앱(`standalone`)은 `navigator.standalone`이고, 알림 권한은 누른 직후에만 묻는다(아니면 `permission-blocked`).
  - 기록은 `exposeBinding`으로 Node에 바로 온다. `device.mark()`로 자리를 잡고 `device.next("무엇", (e) => e === "tone 880", mark)`로 기다린다(page를 다시 묻지 않는다). `device.skipAudio()`는 울리는 소리 끝 뒤로 소리 시계를 옮긴다(제품은 울리는 동안 온 알림을 겹쳐 울리지 않는다, t3code `TestClock.adjust`처럼 자지 않고 건너뛴다). `device.visible(false|true)`는 나감·돌아옴.
  - 쓰는 곳: `specs/15-ios-rules.mjs`. 결함 심기로 본 것: 「interrupted에서 다시 풀지 않음」과 「탭에서 없는 `Notification`을 부름」은 이 가짜만 잡고, 규칙 없는 옛 가짜(`12-threads-push` 「이 기기 알림」)는 둘 다 통과했다.
  - 새 기기 규칙(안전 영역·화면 키보드·wake lock)도 여기에 더한다. 단계 안에서 init script로 새 가짜를 만들지 않는다.


## post 검증(§8.199)

- host의 `post_*` 시험은 `now`로 rate 창 경계를 보고, durable ID·mute·inbox·fence·push를 검사한다.
- agent의 `post_*` 시험은 실제 hook socket과 메모리 Agent link를 쓴다. Named Pipe처럼 쓰기 반쪽을 닫지 않은 채
  응답을 기다려 Windows와 Unix가 같은 JSON 완성 계약을 검사한다. 무송신은 같은 writer의 뒤따르는 Pong으로 확인한다.
- hook 수신 시험은 열린 연결의 분할 JSON·UTF-8, 옛 EOF 방식, 문법 오류·크기 한도·미완성 EOF를 검사한다.
- m13 `post_from_thread_reaches_coordinator_inbox`는 실제 CLI를 로컬·TLS 원격 thread에서 실행한다.
  CLI Ack 뒤 `post-probe-done` 출력과 coordinator nudge 사건을 기다린다. 운영 프로세스·설정은 쓰지 않는다.
- 결함 심기 결과는 작업 report/library에 남긴다. 신호 동작 시험은 새로 만들지 않는다.

## 보고서 publish·등록·이전 검증(§8.200)

- host `library_links` owner 시험이 실제 Front API·HTTP router에서 덮어쓰기, 상한, 실시간 변화·탈출, import 건너뜀/접두사, prune(now), rotate와 pairing을 본다.
- 웹 E2E 21 `library-reports`는 실제 CLI·host로 목록·크기·만료·doctor·열기·복사·폐기·재발급·Reader를 1280/390px에서 본다.
- 기존 E2E 12의 top-level sandbox·SW 제외·형제 그림 검증을 유지한다. 시험 실험실을 정리하는 기존 자식 PID 처리만 쓰고 신호 동작 시험은 추가하지 않는다.
- Windows 공통 시험에는 예약 파일 이름을 만들지 않으며 symlink 생성은 Unix 갈래로 한정한다. 공통 계약 시험 자체는 Windows에서도 실행한다.

- hook 클라이언트의 실제 UDS 회귀는 peer가 정상 응답·종료한 뒤 write shutdown을 하도록 oneshot으로 순서를 고정한다.
  macOS ENOTCONN 뒤에도 정상 응답은 읽으며, 빈 응답·거부를 성공으로 치지 않는다. sleep·재시도·timeout 증가는 없다.
- post m13의 nudge는 기본 15초 ticker 두 주기와 30초 대기 시한을 경주시키지 않는다. 설정한 idle·quiet 창을
  `nudge_windows`로 지난 뒤 판정 tick을 끝내고 coordinator 출력을 본다.

## Codex 전역 연동 검증(§8.201)

- m13 `auto_resolve_and_reopen`의 상태 `resolved`는 정리 시작 때 저장되므로 inbox 완료 신호가 아니다.
  idle은 자동 종료를 끈 상태에서 먼저 확인하고 실제 설정을 켠다. kill 응답 도중 짧은 idle 창이 이미 지나도 중간 상태 관측을 놓치지 않는다. 기존 종료·재열기 assertion과 시한은 유지한다.
  auto-resolve를 실행한 ticker가 끝난 응답(`ticker tick`) 뒤에 `resolved` 항목 하나와 재개 안내를 검사한다.
  `idle`·새 보고 항목은 별개 사건이다. 그 항목들을 지워 기대값에 맞추지 않는다.
- `cargo test -p aaa --test codex_integration`: 임시 HOME/CODEX_HOME의 설치·제거·이동·멱등·백업·기존 hook 보존·doctor 수리.
  설치한 명령의 stdin 사건 → 실제 agent → 실제 host → Front 상태까지 본다. session host만 가짜이며 신호를 보내지 않는다.
  Windows에서는 Codex 기본 runner처럼 COMSPEC/cmd.exe `/C`로 설치 명령을 실행한다(PowerShell을 한 겹 더 감싸지 않는다).
  특수 문자·한글 경로에서 설치한 명령이 인증된 사건의 한글 안내를 UTF-8 JSON으로 돌려주고,
  거부 사건은 종료 0·stdout 0·stderr 0인지 확인한다. PowerShell 진행 CLIXML도 stderr가 있으면 실패한다.
  거부 시험은 실제 화면 감지의 Idle을 먼저 기다리고, 같은 agent link의 PaneRead 응답 뒤 두 pane의 AgentStatus가
  거부 전과 완전히 같은지 본다. 화면 감지는 hook 없이도 상태를 만들므로 `agent == None`으로 거부를 판단하지 않는다.
  안내가 원래 없는 옛 notify에도 잘못된 token·다른 pane peer를 보내, 출력 없이 host만 바꾸는 guard 결함을 잡는다.
  상태 사건은 현재 Panes를 다시 읽는 계기로만 쓰며, 모아 둔 옛 상태로 기다림 정리를 통과시키지 않는다.
  시작·압축·사용자 응답 안내는 올바른 사건 이름의 UTF-8 JSON·한글로 검사하고 진행 보고의 한글 활동도 확인한다.
  비-aaa hook은 stdin을 열린 채로 두어 EOF 전에 종료함을 확인한다. 결함이면 안전 상한에서 stdin을 닫아 정상 종료시킨 뒤 실패한다.
  Windows 설치 명령은 `cmd`의 `if defined`에서 세 필수 env를 먼저 검사해 비-aaa 세션에서 PowerShell을 띄우지 않는다.
  이 시험의 3초는 shell 기동도 포함한다. 실패 문구만으로 EOF 대기를 확정하지 않고, stdin을 닫은 뒤 걸린 시간과 출력을 함께 기록한다.
- `cargo test -p aaa-agent --lib hooks::`·`threads::`·`codex_start_before_pane_registration_waits_for_registration`:
  안내 간격·중첩 세션·사용자 notify·등록 순서를 본다. notify 실행 fixture는 Python 3.11+ 표준 tomllib를 쓴다(Unix).
  등록 순서 시험은 실제 attach가 등록 완료를 알리게 하며 시험이 알림을 대신 공급하지 않는다.
- `cargo test -p aaa --bin aaa integration::tests::`: Windows EncodedCommand를 복호화해 특수 경로·UTF-8·stdin 전달 계약을 검사한다.
  `cargo xwin check --target x86_64-pc-windows-msvc --workspace --all-targets --locked`와 같은 target clippy는 cfg 컴파일 검사다.
  Windows 실행 결과는 Windows runner에서 같은 codex_integration 시험을 실행한 결과로 구분한다.
  Mac의 `cargo xwin test --target x86_64-pc-windows-msvc --locked -p aaa --test codex_integration --no-run`은 실행 파일만 만든다.
  Windows 임시 폴더에 aaa.exe와 시험 exe를 옮긴 경우 `$env:AAA_TEST_BIN = 'C:\lab\aaa.exe'`로 CLI 경로를 지정해
  `& 'C:\lab\codex_integration.exe'`를 실행한다. 이 경로 재지정은 시험 전용이며 운영 설정을 건드리지 않는다.
- 신호 시험 금지 작업에서는 workspace 전체 test·기존 lab 기반 m13·웹 E2E를 실행하지 않는다(그 실험실의 정리가 신호를 쓴다).
  fmt·전체 target build/clippy·deps·위 경계 시험·공개 검사는 실행하고, 제한한 검증 범위를 보고서에 적는다.

## 제거된 특수 기능

임무 실행·reset·평가자·특수 패널 전용 시험은 A4에서 제품 계약과 함께 제거했다. 파일 전송과 A1/A2 keeper는 아래 「자동화 A4」의 owner로 옮겼다.

## Codex 신뢰·메시지 받음 회귀(§8.206)

- E2E 11의 휴대폰 신뢰 답은 실제 첫 `questions` HTTP 응답을 보류하고 질문 사건을 먼저 받게 한다(§8.207).
  자동 대화 선택(`chat-toggle`의 pressed·chat visible)과 카드 visible 뒤 응답을 풀고,
  App `data-questions-sync` 증가(목록 응답을 화면에 적용함)를 기다려 같은 카드가 유지되고 실제 답이 전달되는지 본다.
  HTTP·WebSocket 순서를 고정하므로 재시도나 CPU 속도·몇 frame에 기대지 않는다.
  `web/test/questionReload.test.mjs`는 늦은 목록의 답/메뉴 덮기와 옛 연결의 응답/실패가 새 연결을 지우는 회귀를 본다.
- `codex_integration::codex_trust_detection_question_and_answer_share_the_front_path`는
  0.163 화면 표본의 실제 감지부터 Agent link·질문·Front 답변·전송 키까지 본다. OS 자식·신호를 쓰지 않는다.
- host `harness::tests::codex_message_and_next_do_not_revive_previous_waiting_report`는
  hook 유무 × 메시지/Next 네 경우를 검사한다. 입력 받음 뒤 중간 idle에서 옛 기다림과 inbox 항목이 생기지 않음을,
  보고 확인 task 완료 알림과 실제 `thread_pass` 뒤 단언한다. 새 기다림과 실제 질문은 계속 보인다.
- 웹 E2E 11의 `Codex 폴더 신뢰:` 단계는 데스크톱·390px 카드의 답이 pane에 도착함을 출력 사건으로 기다린다.
  신호 금지 작업에서는 `AAA_E2E_ONLY='^(확인창 질문:|Codex 폴더 신뢰:)'`로 공식 ci-local e2e를 제한한다.
  두 단계 안에 신호 시험은 없으며 공식 실험실이 직접 띄운 자식 PID 정리만 실행한다.

## release·다운로드 계약(§8.210)

- `cargo test --locked -p aaa --bin aaa update::`는 임시 HTTP 서버와 실제 curl로 manifest 선택·SHA-256 거부·없는 OS/arch·전송 중단·같은 판 무동작·보관/rollback을 본다. 서버는 응답/연결 종료 사건으로 끝나며 운영 설정·키체인·신호·고정 sleep을 쓰지 않는다. 같은 시각에 version이 내려가는 보관 순서는 고정 `now`로 검사한다.
- `cargo test --locked -p aaa --bin aaa cli::tests::update_accepts_release_version_and_keeps_local_and_subcommand_modes`는 공개 CLI의 release 판·로컬 파일·rollback 선택과 충돌을 본다.

### 배포 후보의 바이트 보존(t-0525)

`cargo test -p aaa-host --lib link::post_tests::deployment_bytes_survive_auth_redaction_and_a_partial_tail -- --exact`
는 실제 host Agent link reader·요청 대기자를 작은 duplex 버퍼로 연결한다. 가짜 인증값의 원문과 인코딩을
온전한 청크와 마지막 3,176바이트 부분 청크에 넣고, 모든 조각의 offset·길이·EOF와 합친 바이트의 SHA를 검사한다.
EOF·요청 응답·연결 종료 사건으로 끝나며 고정 sleep, 실제 인증, 운영 서비스, 새 production 주입점은 쓰지 않는다.
같은 연결의 파일 보기 응답은 계속 인증 조각을 가려야 한다.
수정 전 host의 일반 JSON 가림은 배포 B64까지 디코딩해 바꾸므로 길이가 같은 채 EOF SHA 단언이 실패한다.
`DeployChunk`의 가림 예외만 제거한 변형도 같은 단언에서 실패해야 한다. agent의 청크 쓰기·sync·EOF 검사와
기존 rollout 설치·재시작 E2E는 그대로 둔다.

### 인증 가림의 B64·실패 처리·청크 경계(t-0525)

`cargo test -p aaa-common --lib redact::tests::`는 실제 protocol `B64`의 인코딩 글자에만 인증 조각이
겹치는 경우와 실제 바이트에 비밀이 있는 경우(8바이트 부분 포함)를 구분한다. 일반 글이 우연히 B64처럼 보여도
원문 비밀과 8바이트 조각의 짧은 인코딩도 가려야 한다. B64는 serde newtype 이름으로 구별하고 JSON wire·TS 모양은 그대로 둔다.
형식을 엄격히 검사하는 값과 직렬화 오류는 panic·원문 반환 없이 오류여야 하며, 오류에 비밀을 넣지 않고 기존
안전한 기록 파일도 덮지 않아야 한다. 실패 이유만 남기고 agent 송신 메시지·host 수신 연결·Front 요청을 거부하거나
Front 응답을 안전한 오류로 바꾼다. thread·journal 기록은 원문을 쓰지 않는다.

원래 B64 글자 가림은 `Invalid symbol 42` panic에서 RED였다. 원문 fallback·B64 타입 구분 제거·옛 스트림 복원
변형은 각각 이 keeper에서 실패해야 한다. 스트림은 비밀과 CSI ANSI를 섞은 입력의 모든 청크 경계에서 같은 가림과
끝 flush를 내야 한다. 원문과 가림 표시를 분리해 겹치는 인증 창의 끝을 보존하며, 단독 ESC도 다음 청크까지 잡는다.
기존 agent 연결 변경 시험은 snapshot 응답으로, host 배포 시험은 일반 파일 보기 응답과 뒤따르는 Pong으로
실제 송수신자가 B64 충돌 뒤에도 살아 있음을 검사한다. 실제 계정·로그인·갱신·기록 검증은 기존 m13 accounts keeper가 맡는다.

### macOS 시험 PTY의 마지막 출력(t-0525)

`cargo test -p aaa --test m13 terminal_capture_keeps_output_after_stdio_close_before_reading -- --exact`
는 작은 C 자식이 stdout·stderr에 글과 ANSI를 쓰고 표준 FD를 모두 닫았다는 파일 사건 뒤에 reader를 시작한다.
기존 helper는 exit 0·출력 0바이트로 RED다. macOS helper는 exec 전에 `/dev/tty`를 한 번 열어 controlling TTY의
참조를 잡고 닫는다. XNU의 종료 drain이 master 수집을 기다리므로 늦은 reader도 모든 바이트를 받아야 한다.
이 준비만 제거한 변형은 같은 단언에서 실패해야 한다. timeout·원래 profile 제거 출력 단언은 그대로다.
이 CLI 성공 출력은 `println!`→직접 PTY reader 경로이며 Agent link·session-host 가림을 지나지 않는다.
커널 경계 근거: [cttyopen의 참조](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/tty_tty.c),
[종료 시 drain·revoke](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_exit.c),
[ttyclose의 flush](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/tty.c).

### update 후보 서명과 사람 터미널 출력(t-0488)

- 기존 E2E `update_replaces_keeps_and_rolls_back`, m13 `coordinator_deployment_is_gated_and_survives_backend_restarts`, `rollout::`의 전송·원격 복구·공유 복구 owner 세 개가 실제 다음 실행·교체·보관·rollback과 재시작 수명을 계속 소유한다. macOS 후보는 바이트를 덧붙이지 않고 서로 다른 ad-hoc 서명 식별자를 써서 같은 version·다른 SHA·유효 strict 서명을 갖는다. 복사·서명은 자식 프로세스에서 끝내 병렬 fork에 쓰기 FD를 물려주지 않는다.
- m13 `web_pair_requires_a_person_outside_aaa_panes`의 사람 문은 그대로 검사한다. 터미널 helper는 직접 PTY를 열고 남은 출력을 EOF까지 읽은 뒤 자식을 수거한다. macOS `script`가 자식 종료를 먼저 처리하면 종료 0인데도 PTY의 끝 출력을 잃는다. 전달 프로세스 정지→자식 출력·종료→kqueue 종료 확인→전달 프로세스 재개 순서로 고정 sleep 없이 재현했다.
- helper의 cwd는 이전 `script`처럼 호출자의 작업 폴더로 명시한다. PTY의 HOME 기본 cwd를 쓰면 아직 없는 격리 HOME을 넘긴 service 시험이 실행 전에 실패한다. 기존 `service_stop_start_restart_wrap_launchctl`이 이 경계를 지킨다.
- ca58f1b0 macOS CI의 pair 실패는 outside 출력 전에 `pane 안 pairing 끝`에서 났다. worker의 brief 글과 Enter 사이에 probe를 보내면 두 글이 붙어 probe가 실행되지 않는다. 시작 응답을 brief 완료로 보지 않고, 시작 전 구독한 Thread 사건에서 brief 수신과 fixture의 보고 처리를 기다린 뒤 probe를 보낸다. 같은 typing 경계·느린 제출에서 수정 전 실패/수정 후 통과를 보고, 준비 대기를 제거한 변형도 실패해야 한다. 사람 문 단언과 30초 상한은 그대로이며 실패에는 역할·파일·pane·화면 진단을 남긴다.
- rollout의 서비스 재시작 기록은 해당 단계가 시작된 뒤 기다린다. 전송 완료만 기다린 뒤 두 사전 검사·설치·서비스를 한 30초 안에 기대하면 부하 아래 설치 중에 실패했다. 같은 rollout의 내구 cursor 전진/단계 시작을 `rollout_changed` 사건으로 관측하고 단계별 기존 상한을 적용한다. 같은 cursor·같은 시작 표지의 반복 사건은 상한을 새로 시작하지 않는다. 서비스 기록의 순서·횟수·실제 job/복구 단언은 그대로다. 재시작 대신 상태만 조회하는 제품 변형에서 기존 서비스 기록 keeper가 실패해야 한다. 자동 복구도 감시의 `restore_started`와 파일 복구 완료(`restarting`·`awaiting_old`)를 먼저 확인한 뒤 서비스 기록을 기다린다. 옛 판 프로세스 기동부터 SHA 대조·파일 복구·재시작을 한 서비스 대기에 섞지 않는다. 각 기존 30초 상한과 서비스 순서·횟수·정확 baseline SHA·독립 PID·새 프로세스 확인 단언은 유지한다.
- 구 inode 설치 경로, 같은 SHA의 후보 둘, PTY 출력 유실을 각각 심어 기존 keeper가 실패하는지 확인하고 바이트 단위로 원복한다. 시간 제한·사람 문·교체 기대값은 바꾸지 않는다.
- macOS 제품 update는 self-check에 사용한 staging과 별도로 실행 이력 없는 설치 파일을 독점 생성한다. 검증한 SHA와 복사한 바이트가 같은지 확인한 뒤 rename하고, 원래 실행 파일은 그대로 보관한다. 유효 서명 후보의 교체 완료 뒤 다음 실행이 SIGKILL 9(`EXEC/SECURITY_POLICY`)로 죽는 경계를 수리한다. [Apple의 갱신 원칙](https://developer.apple.com/documentation/security/updating-mac-software)은 파일에 붙은 서명 캐시 때문에 새 파일로 교체하라고 설명한다. 구 경로도 rename은 했으므로 이번 관측을 단순 제자리 덮어쓰기나 INVALID_PAGE라고 부르지 않는다. 구 staging inode 재사용을 심으면 기존 update keeper의 다음 실행이 다시 실패하고, 원복하면 전체 교체·rollback이 통과해야 한다.

### 흐름 사건의 동시 발행 제어(t-0488)

- m13 `flow_events_go_out_in_seq_order`는 기존 `AAA_TEST_FLOW_HOLD` 경로를 Unix 제어 연결로 쓴다. host가 DB 저장 뒤 `Hhold: 켬`, 다음 사건이 순서 잠금을 기다릴 때 `Wfree: 켬`을 보낸다. 시험이 해제 `R`을 보내야 첫 사건을 발행한다. 고정 sleep·파일 polling·시간 경과에 따른 조용한 해제는 없다. 30초 실패 상한은 그대로이며 단계·연결·사건 출처를 실패 출력에 남긴다.
- Front 요청·조회·구독 연결을 미리 열어 두고, 저장은 붙잡힘 알림 뒤 한 번 조회한다. 기존 contiguous seq 및 수신 순서 단언을 유지한다. 구 시험은 hold와 잠금을 함께 제거해도 통과했다. 새 시험은 hold 제거를 조기 응답으로, 잠금 제거를 실제 수신 순서 `[2, 1]`로 잡아야 한다. 변형을 바이트 단위로 원복하고 runtime worker 하나에서도 통과시킨다.
- 제보된 ci-local의 첫 저장 30초 실패는 좁은 동시 부하 대조에서 재현되지 않았다. 이 수리는 확인한 제어 수명·관측의 허점을 없애며, 당시 지연을 특정 OS·DB 오류로 단정하지 않는다. 최종 검증은 test·e2e·clippy를 함께 도는 ci-local 전체다.
- `python3 -B -m unittest scripts.test_release`는 release 도구를 실제 CLI로 실행해 tag/Cargo 판 일치·빠진 실행 파일 거부·생성 manifest와 checksum이 실제 바이트를 가리키는지 본다. `.github/workflows/distribution.yml`에서도 돈다.
- release workflow 실행·tag push는 착지 뒤 coordinator가 한다. actionlint가 없으면 YAML 문법 검사와 install-runbook 9.1의 herdr 차이표로 검토한다. 신호 금지 작업은 공식 ci-local에서 Rust 전체 test를 제외하고 문서의 확인창 E2E 범위를 쓴다. Windows는 웹 빌드가 끝난 뒤 target clippy로 별도 확인한다.


## 보고서 사본 자동 정리(§8.212)

- `cargo test --locked -p aaa-host library_links::tests::retention_`: 실제 임시 사본·manifest·DB·서명 HTTP 경계에서
  기한 삭제/401, 용량의 갱신 순서, 진행 중·실시간 보호와 합계 제외, 보호 초과 무삭제/중복 알림 없음,
  0으로 끄기, 6시간 tick 경계, dry-run 후보 사유·무쓰기를 본다. now를 인자로 주며 sleep·프로세스·신호를 쓰지 않는다.
- `cargo test --locked -p aaa-config library_retention`: 기본값·0/빈 문자열·값 오류 위치·환산 overflow를 본다.
- 기존 웹 E2E 21에 doctor 한도·CLI dry-run·웹 합계/기한 표시를 더했다. 신호 시험 금지 작업에서는
  `AAA_E2E_ONLY='^(확인창 질문:|Codex 폴더 신뢰:|보고서 전체 목록:)'`로 공식 ci-local e2e를 제한한다.
  이 세 단계는 신호 시험을 하지 않으며, 작업 지시가 허용한 공식 실험실의 자식 PID 정리만 실행한다.
  전체 Rust test에는 신호 시험이 있으므로 제외하고 위 host/config·proto·deps를 따로 실행한다.

## 대화 이미지와 Enter(§8.219)

- 웹 E2E 23 `composer-attachments`가 실제 host·agent의 coordinator pane을 열고 Chromium·WebKit 데스크톱·iPhone·iPad에서
  파일을 가진 clipboard paste 사건, drop 사건, 실제 파일 선택 단추를 거친다. OS clipboard와 iOS의 길게 누르기 메뉴 자체는
  headless가 제공하지 않으므로 그 메뉴 뒤 브라우저가 내는 `ClipboardEvent`를 재생한다. 실제 사람의 clipboard는 바꾸지 않는다.
- 업로드 완료 DOM 표지 뒤 프로젝트 `uploads/` 원본 bytes를 대조하고, 보내기 전에는 pane input·submit·attach가 없음을 본다.
  보내기 뒤 실제 PTY 프로그램의 출력 사건을 기다린 후 프로그램이 `@경로`로 연 파일의 SHA-256과 글을 대조한다.
- 프로젝트 밖 pane은 512KiB를 넘는 그림이 agent 세션 폴더에 조각으로 저장되는 경계를 별도로 확인한다. 삭제·형식 거부·Reader·미인증·다른 Origin도 확인한다.
- 휴대폰·패드 Enter는 여러 줄 내용, 데스크톱은 Shift+Enter 줄바꿈과 Enter 전송, IME는 조합 중 Enter 및 WebKit의 확정 keyCode 229를 본다.
  입력창 높이·rows·자동 늘림은 다른 작업 범위다. 새 시험에는 고정 sleep·polling을 넣지 않는다.
- Rust 실험실 `Node::cmd`는 바깥 aaa pane의 `AAA_SESSION_ID`·`AAA_HOOK_SOCK`를 상속하지 않는다. 세션 안 권한 거부 시험은
  각 명령에 세션 ID를 명시한다. 실제 세션 안에서 전체 gate를 돌렸을 때 사람 터미널·폴더 신뢰 시험을 오판하던 격리 결함을 막는다.
  운영 CLI의 사람 판정·host의 프로세스 기반 호출자 판정은 바꾸지 않는다.
- m13 `thread_read_and_keys`는 실제 report 실패 진단 뒤 spinner가 지워지고 권한 창을 화면 감지로 찾아 키로 답하는 경계를 유지한다. 진단 문구는 fixture가 지우므로 순간 화면 대신 CLI 종료값을 파일에 남겨 권한 창 사건 뒤 실패 실행을 확인한다. 예전 상대 4줄 복귀로 바꾸면 spinner가 남아 권한 창 검출이 실패해야 한다.

웹 E2E 12의 「특수 임무: 좁은 창·휴대폰·데스크톱」은 390/500/1280px에서 승인 메뉴·설명 글 조각·각 단추의 화면 경계와 실제 hit target을 확인하고 Controller 승인을 저장한다. 기존 승인 시험에 화면 경계를 더했으며 별도 가짜 위치 계산 시험은 없다. 화면마다 새 탭을 사용해 퇴장 sheet와 다음 크기를 섞지 않는다. `--spec 12 --grep '특수 임무: 좁은'`으로 이 경계만 확인할 수 있다.

## coordinator 전용 외부 푸시(§8.249)

- host `working_threads_publish_landing_reports_once`가 실제 사본·DB·inbox에서 완료·사람 필요 표지를 유지하면서 외부 대기열은 비우는 계약을 본다. 새 `[확인 필요]`와 옛 `[사람 필요]`의 앱 안 알림 의도도 함께 검증한다.
- `post::tests::coordinator_can_request_the_user_without_nudging_itself`는 host 정본 coordinator pane과 두 사용자 post 의도만 외부 대기열에 들어가고, 밖 CLI·위장·다른 프로젝트·웹·시스템 항목·다른 topic은 빠지는지 본다. `post_fence_push_matrix_and_privacy`는 post 본문·제목을 외부에 싣지 않는 계약, `push::tests::payloads`는 실제 암호문 4096바이트 한도를 지킨다.
- `m13::notify_webhook`은 실제 coordinator pane의 자식 CLI가 보낸 사용자 post만 webhook으로 묶이고, thread 보고는 inbox에 남으며 실패한 알림은 재전송하지 않는지 확인한다.
- 웹 E2E 12는 실제 암호문을 복호화해 coordinator 전체 완료의 고정 본문과 프로젝트 클릭 목적지를 확인한다. 17은 coordinator post 푸시의 쓰는 화면·기기별 설정, 18은 coordinator 푸시를 받은 기기에서도 별개 thread 보고가 앱 안에 표시되는 경계를 본다. DOM 반영은 실제 표시 완료로 끝낸다.
- 신호 금지 작업은 공식 ci-local의 Rust 전체 test를 제외하고 `AAA_E2E_ONLY='^(Web Push:|thread 보고서·결과물:|확인창 질문:|Codex 폴더 신뢰:|보고서 전체 목록:)'`로 고른다. 이 단계에는 신호 동작 시험이 없으며 실험실이 직접 띄운 자식 PID 정리만 허용한다.
- iOS의 `from <앱 이름>`과 Android/브라우저의 출처 줄 배치는 플랫폼 소유 UI다. headless 시험은 service worker에 전달한 title/body와 목적지를 확인하며 실기 OS 창의 줄 수를 증명하지 않는다.

## coordinator 배포 safety(§8.228)

m13 `coordinator_deployment_is_gated_and_survives_backend_restarts`는 별도 실행 파일 사본과 TLS agent에서 기본 차단·사람의 safety 허용/철회·coordinator ID 위장·env를 지운 worker 차단·실제 교체/rollback·host/자기 agent 재시작 뒤 동일 pane PID와 결과 조회를 확인한다. probe는 파일 기록 뒤 고유 출력 표지로 끝낸다. 실험실의 로컬·원격 agent를 다시 띄운 뒤에는 새 프로세스의 연결 로그와 host의 connected 상태를 함께 확인하고 coordinator를 연다. 이전 연결 상태나 원격 연결만 보고 로컬 준비를 추측하지 않는다. macOS에서는 격리 HOME/PATH의 launchctl 모형으로 typed host/원격 agent restart와 실패의 영구 기록을 확인한다. 운영 launchd에는 닿지 않는다. agent `deploy::tests`는 같은 ID 중복·동시 작업·다른 요청 충돌·중단 복구/실행기 중복 잠금/SHA 불일치를 담당한다. 기존 update E2E가 public release hash·self-check·pin·rename을 계속 소유한다.

배포 실행기 수명 회귀(§8.233): 기존 `jobs_are_durable_once_and_reconcile_only_after_the_executor_releases_its_lock`에서 실제 잠금 File을 `try_clone`해 fork와 같은 잠금 참조를 남긴다. Unix에서는 원본 FD의 close-on-exec도 확인한다. 실행기가 살아 있으면 중복 configure는 실패하고, drop 뒤에는 복제 FD가 살아 있어도 새 실행기가 즉시 잠금과 journal 복구를 맡아야 한다. dup/fork는 같은 flock을 공유하므로 자식·신호·sleep 없이 병렬 spawn의 fork~exec 창을 결정적으로 재현한다. close-on-exec만으로는 exec 전 잠금 참조가 사라지지 않는다.

local agent 재연결 중 배포 조회(§8.237): host `deploy::tests::coordinator_identity_survives_local_agent_disconnect`가 연결 상태와 독립적인 peer PID/pane 소속·권한 경계를 검사한다. CLI `deployment::tests::status_waits_for_reconnection_but_preserves_nonwait_and_forbidden_errors`는 Front IPC에서 AgentDisconnected 뒤 State 사건을 기다려 같은 ID만 조회하고, nonwait·Forbidden을 즉시 반환하는 경계를 검사한다. 머신 ID·생략을 포함한다. 고정 대기·새 자식·신호는 없으며, 기존 m13의 실제 재시작 검증을 대체하지 않는다.

### Ctrl+Enter 공통 판정

Ctrl+Enter는 agent가 작업 중일 때만 steering하고 줄바꿈을 넣지 않는다(plan §8.239 후속·정정). E2E 25는 감지가 알린 목록 상태(`data-status`)를 사건으로 기다린 뒤 누른다. `aaa-host/src/ctrl_enter.rs`의 유닛 시험이 입력 상자 스타일과 현재 steering 패널, Codex 작업 중·쉼을 판정하고 어느 경우에도 줄바꿈 키가 나오지 않는지 본다. 「보내지 않음」은 E2E 25가 같은 pane 연결의 뒤따르는 입력을 울타리로 삼아 그 조각만 닿았는지로 확인한다. `web/e2e/specs/25-ctrl-enter.mjs`는 실제 host·agent·PTY로 전달된 bytes를 출력 사건으로 확인하며 웹 물리 키·모바일 키 막대·입력줄·IME·반복 키의 연결을 본다. 셸 등 일반 프로그램의 키보드 모드는 기존 `03-keys.mjs`가 소유한다. 웹 입력창(대화 입력창·웹 입력줄)에 글이 있을 때의 Ctrl+Enter는 `submit`의 `steer`다(plan §8.239 「웹 입력창 Ctrl+Enter」). 끝 키는 유닛 `steered_text_ends_with_send_now_only_for_a_working_claude`가 본다. E2E 25 「대화 입력창 Ctrl+Enter」는 PTY에 닿은 bytes 수를 출력 사건으로 기다리고, 무엇을 보냈는지는 페이지가 보낸 frame(`sentFrames`)으로 본다. 쉬는 빈 입력창은 웹이 아무 frame도 보내지 않는지로 「보내지 않음」을 확인한다. 가짜 harness는 기록 위치를 알리지 않으므로 보내짐은 host 전달이고, 입력창이 비고 풀리는 것을 DOM 사건으로 기다린다.

## Mac 웹 터미널 Ctrl+V(§8.232 후속)

기존 E2E 11의 「그림 붙여 넣기·끌어 놓기」가 소유한다. Ctrl+V는 어느 OS에서나 브라우저 붙여넣기 몫이다(그림 읽기는 사용자 결정으로 되돌렸다). 실제 host·agent와 raw PTY에서 Mac Chrome·WebKit·Windows·iPad·iPhone의 Ctrl+V(누름·꾹 누름)가 clipboard를 읽지 않고, 뒤따르는 표준 글 paste가 `^V` 없이 PTY에 닿으며(같은 연결 순서가 「보내지 않았다」의 울타리), 그림 paste는 이번 경로·bytes로 닿는지 본다. 페이지의 Clipboard API만 읽기 수를 세는 가짜여서 사람의 clipboard나 OS 권한 설정에 닿지 않는다. `--spec 11 --grep '그림 붙여 넣기'`로 좁게 돌린다.

## 옛 임무 입력 안내·권한 시험

임무의 전용 입력 안내·추가 허용 목록·빈 입력 실행 순서는 A4에서 실행 코드와 함께 제거했다. 일반 파일 전송의 경로·hash·symlink와 구 Agent link 호환은 아래 A4 owner를 따른다. 과거 권한 실험은 plan §8.246·§8.251의 이력이다.

## resolve 뒤 종료된 pane 제거(§8.254)

- m13 `resolve_removes_exited_panes_even_when_the_worktree_is_kept_or_adopted`가 실제 CLI→host→agent 경계에서 dir·dirty worktree·adopted pane을 입력으로 exit 1 종료한다. 종료 상태 확인 뒤 resolve하고 pane 목록·layout에서의 제거와 폴더 보존을 단언한다. 새 신호·고정 sleep·production 시험 seam은 없다.
- 기존 `resolve_keeps_dirty_worktree_and_unmerged_branch`는 partial·skip-copy를 포함한 폴더/branch 보존과 별개로 pane을 닫는지 본다. `thread_adopt_existing_pane`은 복원된 살아 있는 pane도 닫고 원래 branch·파일은 남기는 경계를 본다. linger 시험은 기존대로 session host의 종료 유예를 기다리지 않는지 확인한다.

## 삭제된 TUI의 접속 거절

`cargo test -p aaa --test tui_removed`는 host 없는 로컬 `attach`·`tui`가 TUI 삭제 안내와 실패 코드로 끝나는지, 실제 host socket이 옛 `tui`·`tui@…` hello를 거절하고 CLI 상태 조회를 계속 받는지 본다. 폐지된 `mission`·`evaluator`·`evaluation`은 접속 전에 자동화 변환 안내로 끝나는지 같은 owner가 검사한다. 웹 E2E 01의 「옛 TUI WebSocket 거절·웹 API 유지」는 인증한 브라우저에서 옛 `/api/v1/front`에 접속해 분명한 Reject frame을 받고 일반 웹 API가 정상인지 본다. 웹 터미널·Ctrl+Enter·입출력 회귀는 기존 웹 E2E와 host·CLI 시험이 계속 맡는다.

TUI PTY·native clipboard·vendored crossterm 검사는 코드와 함께 제거했다. 목록과 보존 tag `tui-last`는 [삭제 기록](removed-tui.md)에 있다.

## 웹 입력 수신·Ctrl+Enter·사건 진단(t-0408)

- 웹 E2E 23은 실제 host·agent의 JSONL 읽기까지 지나, 같은 글의 옛 기록·다른 글을 수신으로 오인하지 않는지,
  전체 `pasted_content` 봉투가 있는 여러 줄 enqueue, 4천 자 표시 상한을 넘는 글, 표시 앞부분은 같고 끝이 다른 글,
  첨부·10초 경고·잠금 풀기·취소와 같은 보내기 ID의 진단 장부를 검사한다. 글 전문이 진단에 없는지도 본다.
- E2E 25와 host `ctrl_enter` keeper는 실제 Claude 2.1.303의 footer와 `paste again to expand` toast·spinner를 입력으로 쓰며,
  웹의 빈 입력 Ctrl+Enter가 host에 도착해 sendNow 키가 나오는지, 쉬거나 현재 상자를 모르면 키가 없는지 검사한다.
- `inputReceipt.test.mjs`·기존 `pastedContent.test.mjs`는 공용 전체 봉투 경계를 검사한다. host DB keeper
  `input_log_bounds_retention_and_queries_without_message_bodies`는 기간·행·20MiB 순환, 필터, 최신 1,000행의 순서를 본다.
  행 한도를 1 늘린 변형과 같은 ID의 별도 봉투를 걷는 변형에서 keeper가 실패한 뒤 바이트 단위로 복원됐다.
- 격리한 실제 Claude 2.1.303·Codex 0.163.0 pane에 로컬 합성 provider를 연결해 입력·대기·빈 입력 Ctrl+Enter·이미지 전달을 관찰했다.
  Claude는 소유한 FIFO의 실제 Bash 도구를 사건으로 기다리며, sendNow 뒤 enqueue가 흡수되고 다음 provider 요청이 오는 것을 본다.
  Codex는 pending steer의 Esc가 다음 요청을 시작하는 것을 본다. 고정 sleep·재시도·운영 설정·운영 pane 입력은 쓰지 않는다.
  이 실험은 CLI/PTY 수신 계약을 확인하며 실제 모델의 답변 품질을 검증하지 않는다.

조회 방법과 사건 해석은 [웹 입력 사건 보기](input-debug.md)에 있다.

Codex 실제 steering 화면에서 발견한 `<turn_aborted>` 내부 안내는 사람 글에서 제외한다. 기존 `chat::tests::codex_lines`에 실제 기록 형태를 더해 수정 전의 가짜 사람 말(RED)을 확인했고, 같은 keeper가 수리 뒤 통과한다.

E2E 23은 WebCrypto·randomUUID가 없는 브라우저에서도 짧은 글이 실제 pane에 가고 잠금이 풀리는지 검사한다. 새 hash·진단 ID 때문에 기존 일반 글 보내기를 막는 회귀를 수정 전 RED로 확인했다.

## 준비·정리 연결 유실 회귀(t-0412)

`cargo test -p aaa --test m13 preparation::`는 실제 Git의 응답을 socket 사건으로 붙잡는다. 준비·정리 각각 16초 창으로 예전 15초 상한을 실제로 넘긴다(총 약 35초). 연결 유실 시험은 Git이 자리를 만들거나 지운 뒤 응답 전에 실험실 host만 종료하고 새 host의 restart·resolve가 agent 기록을 회수하는지 본다. 별도 1초 host 설정 시험은 준비 deadline 전달·자기 Git 자식 종료·변경 파일 보존·깨끗한 자리 재사용을 본다. 시간 규칙을 보는 socket read 이외의 해제·종료는 사건으로 기다린다.

동일 keeper는 응답을 잃은 두 번째 준비를 restart 없이 바로 resolve하여 host가 agent journal의 경로를 회수하고 worktree가 실제 사라지는지도 본다. 이 확인은 journal 조회가 없던 후보에서 실제 고아 worktree로 먼저 실패했다.

구판 journal 없는 예상 자리가 남아 있으면 조회는 없음으로 확정하지 않고 명시적으로 거부한다. 변경 없는 자리는 안내한 `thread restart`가 소유권을 확인해 회수하고, 그 뒤 resolve하면 정리된다. 변경·다른 Git 신원은 기존대로 보존한다.

짧은 준비 설정 시험은 실패를 의도한 checkout 단계에만 1초를 적용한다. 그 뒤 실제 host 설정을 기본 600초로 되돌리고 실험실 host를 다시 띄운 뒤 정상 회수를 본다. 정상 회수의 Git 작업을 병렬 부하에 따른 1초 성능 시험으로 만들지 않는다.

## 가짜 harness로 agent 상태 만들기(§8.266)

- **새로 띄우는 pane.** 실행 파일 이름이 `claude`·`codex`면 그 harness 규칙으로 화면을 본다(harness `claudeSh`, 25·30의 node 가짜 harness). 대화 기록도 보여야 하면 가짜가 시작할 때 `aaa hook --agent claude`로 `SessionStart`(`transcript_path`)를 알리고, 기록 파일은 실험실 Claude 기록 폴더(`${root}/claude/projects/…`) 안에 둔다(밖이면 「기록 폴더가 없다」). Enter를 받으면 사람 말을 기록에 적어야 웹 보내기 확인이 끝난다(30).
- **이미 도는 셸 pane(thread의 fake profile).** agent는 셸 pane의 앞 프로세스 그룹이 바뀔 때만 앞 agent를 다시 본다(detect/foreground.rs, herdr 규칙). 같은 그룹의 자식(`sh 파일`)은 잡지 않으므로, post-probe 안의 셸이 `set -m`(job control)으로 이름이 `claude`인 script를 새 앞 job으로 띄운다(31).
- **알림이 이 화면에서 처리됐다.** 앱 안 알림(`.droplet`)이 DOM에 붙는 것을 MutationObserver로 세어 울타리로 쓴다. 창 focus는 `document.hasFocus`를 바꾸고 `blur`·`focus` 사건을 보낸다(19·31).

E2E30의 보내기 단계는 새 대화 DOM이 자동으로 끝에 간 직후 MutationObserver에서 위로 옮겨, 첫 native scroll 사건이 합쳐지는 순서를 고정한다. 자동 굴림 위치를 사건에서만 기억하던 제품 코드에서는 `5530 → 0` 이동을 놓쳐 「맨 아래로」가 숨었다. 즉시 실제 위치를 기억하는 수리 뒤 같은 keeper가 통과한다. 첫 사건을 기다려 경쟁을 피하거나 고정 대기·재시도를 넣지 않는다.

E2E17은 push 판정과 앱 안 알림을 확인한 뒤 화면을 정리한다. 알림은 3.6초 hold 뒤 저절로 접히므로, 이미 사라진 닫기 단추를 기다리지 않는다. 같은 브라우저 작업에서 현재 알림을 확인해 남아 있으면 닫고, 접힘·제거 사건으로 끝낸다. 첫 단계는 가상 시계로 자동 닫힘을 먼저 마쳐 조회 중 만료된 순서를 고정한다(실제 sleep 없음). 실제 사람의 카드·닫기 단추 조작은 E2E12가 계속 검증한다. 알림 개수·내용·push 허용/거부 단언이나 제품 수명은 바꾸지 않는다.

## Claude 내부 알림과 사람 글 경계(t-0408 후속)

`aaa-agent` chat의 origin keeper는 실제 Claude2.1.303의 user origin(task-notification·peer), 구조 표지(meta·compact summary·synthetic), human의 XML 인용과 수신 hash를 구별한다. 기존 pending keeper는 숨긴 알림의 remove/dequeue도 큐 자리를 유지하는지 확인한다. 웹 E2E23 「Claude 내부 알림」은 실제 host·agent가 같은 내용의 내부 알림으로 잠금을 풀지 않고, 뒤따른 human 기록에서만 풀며 휴대폰 말풍선에 그 인용 글을 남기는지 본다. JSONL의 뒤따른 assistant 기록을 처리 경계로 삼아 고정 대기를 쓰지 않는다.

## 빠른 답장과 공유 설정의 되돌리기(t-0413 실패·t-0408 후속)

preferences keeper는 전송 중 baseline 값으로 되돌린 설정·고정을 늦은 응답이 덮지 않고 다음 변경으로 남기는지 본다. E2E11 「대화 입력창」은 자동 전송 끔 요청의 실제 응답만 gate로 붙잡고, 다시 켠 뒤 해제해 다음 공유 쓰기를 확인하고 yes가 실제 프로그램에 도착하는지 본다. service worker가 route를 대신 처리하지 않도록 이 단계의 별도 기기만 serviceWorkers=block이다. 우연한 부하·반복 성공·기한 연장에 의존하지 않는다.

E2E14 「설정 되돌리기」는 host의 끔 events를 받은 뒤 HTTP 답만 유실시키고, 보관 문서를 반영할 때도 새 켬을 지켜 다음 켬 요청이 host에 적용되는지 본다.

최신 main의 auto-resolve keeper는 status=resolved가 정리 시작 표지이며 cleanup_pending 동안 group=working인 계약을 따른다. cleanup_pending이 사라진 사건까지 기다린 뒤 기존 group=resolved와 inbox 단언을 검사한다. 상태값 기대나 기한은 바꾸지 않는다.

## 「작업」 트리 작업 요약(t-0421, §8.280)

E2E28은 실제 host·agent의 pane→thread 대응을 거쳐, ⋯ 메뉴로 고정한 thread 줄(맨 앞 카드)도 폴더 안 줄과 같은 요약 카드를 여는지 본다. 「기타」의 thread 없는 일반 agent 줄에는 카드가 없고, 우클릭 메뉴를 열면 닫힌다. hover 지연은 브라우저 시계로 넘기며 고정 sleep은 없다. 휴대폰의 요약 단추·시트는 기존 단계가 맡는다.

## 「작업」 패널 통합(§8.280)

- 웹 유닛 `roster`: 프로젝트 안 coordinator → 작업 공간 폴더(이름순, 「작업 폴더」 끝) → 폴더 안 rank 차례, 임무 thread·그 pane 제외, 「폴더 안 정렬」 활동순. `workFolders`·`sidebarPanel`(옛 `projects`·`agents` 저장값 → `work`, 드러내기)도 같이 본다.
- E2E26은 막대 단추 셋(작업·머신·특수)과 「작업」 배지(확인 필요·검토 대기 thread + 답을 기다리는 agent pane의 합집합), E2E12는 프로젝트가 하나일 때 머리 없이 그리고 thread 줄 메뉴(보고·고정·프로젝트 thread 보기, §8.291부터 「모든 thread」)와 프로젝트가 여럿일 때 프로젝트 머리 안 폴더 들여쓰기, E2E11은 줄 끝 배경 작업 수·계획 n/m, E2E27은 머신 칸 agent 수가 트리의 도는 agent 줄 수와 같음을 본다.

## 한 host 한 프로젝트 시작 검사(§8.283)

- 유닛 `lifecycle::tests`: 보관하지 않은 프로젝트 0개(없음)·1개(그 이름)·멈춘 것은 셈·보관 섞임·2개(목록)와 거부 말.
- m13 `host_starts_only_with_one_project`: 보관한 둘째 프로젝트는 세지 않고 다시 뜬다. 보관을 풀고 다시 띄우면 host가 입구를 열기 전에 목록을 말하고 0이 아닌 코드로 끝난다. 폴더를 `.trash`로 옮기면 다시 뜬다. 시작 검사를 경고로 바꾼 결함을 심어 이 시험이 실패하는 것을 확인했다.
- 실험실(`tests/lab`)은 `[projects]`가 없는 host.toml에 실험실 root를 채운다. 그래서 시험 host가 실제 HOME의 프로젝트를 세지 않는다.

## 프로젝트 인자 없는 CLI(§8.285)

- m13 `cli_takes_no_project_argument`:
  - 인자 없이 `thread start`·`show`·`keys`, `inbox list`, `context`, `aaa set`, `aaa coordinator open`.
  - 옛 꼴은 같은 이름이면 표준 오류 경고와 같은 결과, 다른 이름이면 거부한다.
  - 다른 이름 거부를 뺀 결함을 심어 실패를 확인했다.
- 둘째 프로젝트 거부: m13 `coordinator_flow`이 거부 말을 보고, 폴더가 남지 않는지도 본다.
- 다른 프로젝트를 전제하던 시험은 한 프로젝트 뜻으로 바꿨다.
  - host 유닛: post에서 다른 프로젝트의 coordinator 줄을 뺐다. inbox는 한 프로젝트로 줄였고, pr은 경우마다 host를 하나씩 띄운다.
  - m13 7개: 옵션은 빈 host에서 보고, 한살이는 demo를 보관한 사이에 본다. safety·coordinator profile은 같은 프로젝트의 값을 바꿔 가며 본다. doctor는 실험실의 demo를 새 프로젝트로 쓴다.
  - E2E 12: 멈춤·빈 결과물·임무·coordinator 줄·작업 공간 폴더를 demo로 옮기고, 프로젝트 머리 셈 단계는 뺐다.
  - E2E 21: 보고서를 demo 하나에 두고 프로젝트 고르기 단계를 뺐다.
- 같은 초 삭제: 유닛 `deleting_the_same_name_twice_keeps_both_in_the_trash`. 겹침 검사를 뺀 결함을 심어 실패를 확인했다.

## 요청의 프로젝트 생략·safety 표 하나·aaa.toml(§8.288)

- host 유닛 `omitted_project_is_the_one_this_host_keeps`:
  - `{"op":"context"}`가 빈 `project`로 풀린다.
  - 프로젝트가 없을 때는 NotFound이고 `Instance`가 비어 있다.
  - 만든 뒤에는 `demo`로 채우고, 이름을 주면 그 이름을 쓴다.
  - 채우기를 뺀 결함을 심어 실패를 확인했다.
- m13 `post_from_thread_reaches_coordinator_inbox`:
  - coordinator 셸 pane의 `post user`(새 꼴)와 `post demo user`(옛 꼴, 경고 한 줄)가 모두 coordinator로 접수된다.
  - thread pane의 post는 여전히 `peer+token`이다.
  - `post_sender_is_judged_not_claimed`: 세션 env를 지운 thread pane의 post는 프로젝트를 채운 뒤 front 보기 권한으로 거부된다. thread가 아닌 셸 pane은 Front 길에서 목적지로, Front에 붙지 못한 hook 길에서 「열린 thread 세션에서만」으로 거부된다.
  - JSON 출력을 표준 오류와 함께 읽는 시험(m13 `coordinator_user_posts`, E2E harness의 coordinator 완료 post)은 새 꼴을 쓴다. 옛 꼴의 경고 줄이 JSON 앞에 붙기 때문이다.
  - 세션 안 Front 길을 옛 조건(프로젝트를 줄 때만)으로 되돌린 결함을 심으면 새 꼴이 hook 길로 가서 거부된다.
- safety:
  - 유닛 `one_safety_table_comes_before_the_project_table`: 읽기 차례, 버린 값·모르는 key의 경고, `--global`에는 닿지 않음.
  - 설정 유닛 `one_safety_table_mixes_values_and_tables`: 값과 표가 섞인 `[safety]`를 읽고, local이 덮고, 다시 써도 같고, 틀린 값은 그 줄을 가리킨다. 사용자 정의 읽기를 뺀 결함을 심어 실패를 확인했다.
  - m13 `safety_and_profiles_by_a_person`: 대상 없이 show·yolo·set을 쓰고, 옛 꼴 `safety set demo …`도 쓴다. `[safety] yolo`가 프로젝트 표를 덮고 그렇다고 알리는지 본다.
  - `coordinator_deployment_is_gated_and_survives_backend_restarts`도 대상 없는 `safety set coordinator_deploy on`을 쓴다.
- aaa.toml: 유닛 `aaa_toml_comes_first_and_keeps_workspaces`.
  - aaa.toml이 없으면 만들지 않는다.
  - 있으면 그것을 읽고, `[[workspaces]]`에 더하고 뺀다(`workspaces.remove`). project.toml은 그대로다.
  - 옛 이름 `[[repos]]`가 있는 글은 그 목록을 고친다.
  - 쓰기 자리를 project.toml로 되돌린 결함을 심어 실패를 확인했다.
- 안내: m13 `coordinator_flow`·`doctor_files_brings_the_coordinator_guide_to_existing_projects`·`project_lifecycle`이 `aaa post user`·`aaa project pause|resume`·`aaa project sweep`처럼 이름 없는 안내를 본다.

## 웹의 프로젝트 층 없애기(§8.291)

- E2E 12:
  - 「끝난 thread 보기」: 더 보기 → 모든 thread(`#threads`)로 간다. 묶음·거르기를 보고, 고르면 짧은 주소 `#thread=t-0001`이 된다. 뒤로 이름은 「모든 thread로」이고 뒤로 가면 `#threads`다. 옛 `#projects`·`#project=demo`도 그 화면을 열고 주소가 `#threads`로 바뀐다.
  - 옛 주소 바꾸기를 뺀 결함을 심어(웹을 다시 빌드하고 host binary도 다시 만든 뒤) 이 단계가 실패하는 것을 확인했다.
  - 줄 메뉴: thread 줄 메뉴는 「보고·고정·모든 thread」다. 짧은 주소 `#thread=<id>`로도 보고를 연다.
  - 멈춤은 모든 thread 머리 둘째 줄의 「멈춤」으로 본다. coordinator 열기는 모든 thread 머리에서 하고, 뒤로 가기는 `#threads`다.
  - push: coordinator 완료 push URL은 `/#threads`다.
- E2E 15: 모든 thread 화면의 ≡ 자리. E2E 19: post 음소거 앱 안 알림은 `#threads`로 간다. E2E 21: 보고서 화면에 프로젝트 고르기가 없다.
- host 유닛: push `payload`의 thread URL `/#thread=t-0001`, post 알림 URL `/#threads`.
- 낱말: brief 유닛(`# aaa`·`- 이름:`·memory 넘침 말), 임무 brief 유닛(「aaa 홈 폴더를 탐색하지 않는다」), m13 `worktree_thread_round_trip`·`memory_and_restart`(`# 지침`)·`coordinator_flow`·`doctor_files_brings_the_coordinator_guide_to_existing_projects`(`## 첫 턴`).

## E2E03 보낸 순서는 화면 안 기록으로

E2E03 「크기 되찾기」의 보낸 순서(resize가 키보다 먼저)는 화면 안 기록(`sentFrames`, init script)으로 읽는다. 예전에는 화면을 연 뒤 Playwright `framesent`를 달았다. 그래서 그 전에 열린 pane WebSocket을 놓치면 「보낸 순서 []」로 실패했다(ci-local에서 한 번). 예전 판에 2초 지연을 넣어 같은 실패를 재현했고, 고친 판은 같은 지연에서 통과했다.

## CI 감시(§8.274)

- `cargo test -p aaa-common ci::`: gh 304 exit 1·200/403 HTTP 해석, GET·머리 주입 거부, 응답 필드 축약, SHA의 일부 workflow 대기·취소·재실행·stuck, 실패 집합·병합 번호 계약을 본다. 실패 로그 표지는 실제 windows-2025 줄 모양(job·단계·시각 앞머리, `\` 경로)을 7바이트 조각으로 먹여 꼬리 밖 실패·panic 열 번호 제외·끝나지 않은 시험을 보고, 같은 job·단계의 다른 시험이 다른 실패로 갈리는지 본다(0a8c294f).
- `cargo test -p aaa --test m13 ci_conditional -- --nocapture`: 실제 host·agent와 가짜 gh, 승인된 임무를 띄운다. 기준 초록 → 304 무음 → 빨강의 CI 항목·임무 inbox와 깨움 한 번 → host 재시작 중복 방지·created pagination → 같은 실패 집합 무깨움 → 같은 job·단계의 다른 실패 시험은 같은 항목에 덧붙이고 다시 깨움(가짜 gh가 실패 줄을 32KiB 꼬리 밖에 둔다) → 꺼진 동안 실패 후 회복·CI fixed 무깨움 → 403 outage 한 건을 본다. 로그 잘림과 한 push의 여러 주인 번호도 이 경계에서 본다.
- 실험실의 기존 `AAA_TEST_ROUTINE_SECONDS=1`이 있을 때만 `ci_poll_secs` 하한을 1초로 낮춘다(운영은 30초). 고정 sleep을 넣지 않는다. 새 판정은 flow, 아무 일 없음은 다음 304 두 번(앞 CI pass 완료)과 ticker tick 뒤 확인한다.
- 결함 심기는 HTTP 상태보다 exit를 먼저 거부하기, 같은 실패 집합의 깨움 억제를 제거하기, 회복 자체가 임무를 깨우게 만들기 세 가지를 독립적으로 심는다. t-0685는 실패 시험 비교 빼기(「같은 실패 집합」으로 건너뜀)와 agent가 남긴 꼬리만 읽기(꼬리 밖 실패 시험 없음) 두 가지를 더 심었다. 해당 keeper가 의도한 이유로 실패한 로그를 보존하고 바이트 단위 복구 후 정상 통과를 확인한다.
- 운영 host·로그인·설정·실제 GitHub 쓰기는 건드리지 않는다. 배포 뒤 사용량·깨움 수 관찰은 coordinator의 별도 운영 단계다.

## 공용 watch 추출(t-0420 L1)

- 기존 `cargo test -p aaa-host ci:: --lib`와 `cargo test -p aaa --test m13 ci_conditional -- --nocapture`를 그대로 실행한다. E2E의 CI 사건·재시작·같은 실패·회복·outage 단언은 바꾸지 않는다.
- `l0_saved_state_survives_watch_extraction`은 L0 저장 문서 fixture를 실제 DB에 넣고 새 상태로 읽고 쓴 JSON 의미가 같은지 본다. cursor·SHA 판정 이력·미전달 outbox를 보존하고, 임무가 없을 때도 저장 입력이 남는지 확인한다.
- 결함은 각각 독립적으로 ① 프로세스 첫 조회의 따라잡기 누락 ② 전달 입력의 내구 key를 매번 바꿈 ③ source 상태의 최상위 펼침 누락을 심는다. 최근 poll 직후 새 host의 따라잡기 단위·pending 중복 단위·L0 저장 문서 keeper의 RED를 보존하고 바이트 복구 후 정상 gate를 실행한다.
- 첫 조회 표지 제거 변형은 기존 E2E에서 재시작 시간 자체가 단절 창을 넘어 통과했다. 이 감도 부족을 보존하고, 같은 DB의 새 host가 poll 1ms 뒤에도 따라잡는 keeper를 추가했다.
- 전체 gate는 `bash scripts/ci-local.sh`와 `bash scripts/check-public.sh`다. L1은 CI 동작·설정 변경 없이 구조만 추출한다.

## 배포 자동화 L0·L1(§8.276)

- host `rollout::tests::transition_receipts_require_success_hash_and_new_process`는 단계 전이 표, 실패·중단 중지, 영수증 ID/요청/소속, SHA·판과 새 프로세스 판정의 owner다.
- macOS m13 `rollout::`의 전송·원격 복구·공유 복구 owner 세 개는 격리 사본 host·TLS agent 둘과 기록하는 launchctl로 승인 거부, 사전 SHA 불일치 무교체, 모든 대상 dry-run 뒤 host부터 순차 실행, 실제 host/agent 프로세스 재시작 뒤 같은 job ID 유지, 안내 갱신·coordinator pane PID 지속을 확인한다. 서비스 호출 기록·연결 사건·출력 표지로 끝내며 고정 sleep은 없다. 실제 운영 launchd에 닿지 않는다.
- `python3 -m unittest scripts.test_deploy_build`는 임시 origin/main과 기록하는 빌드 도구로 조상 거부·격리·빌드 1회·salt·provenance를 확인한다. Unix 공식 ci-local test 단계에 포함한다.
- t-0547의 같은 시험은 기본 `target/deploy-build`가 `TMPDIR`·공용 Cargo target과 무관한지,
  명시 부모의 절대/호출 폴더 기준 상대 경로·공백·빌드마다 새 source/target을 확인한다.
  npm·Cargo·판 검증 실패를 각각 한 번 일으켜 worktree/중간 파일 정리·이웃 파일 보존과 기존 부분 출력 보존을 검사한다.
  부모가 파일이면 빌드 전 실패해야 하며 temp로 우회하지 않는다. 원래 temp 선택을 되살리면 기본 위치 단언에서 실패해야 한다.
  실제 macOS SIGKILL 사유는 가짜 도구 시험이 보증하지 않으며, 해당 머신의 빌드/OS 로그 대조와 구분한다.
- 기존 coordinator_deployment 시험이 worker·위장 coordinator·safety 철회·원시 rollback 권한을 계속 소유한다. L3의 실패 머신 자동 rollback과 host 감시는 이 단계에서 검증했다고 주장하지 않는다.
- 결함 심기는 재개 ID 변경, SHA 대조 누락, 실패 뒤 진행을 각각 독립 실행해 keeper 실패를 보존한 뒤 원복한다. 전체 검증은 scripts/ci-local.sh와 check-public.sh다.

- host 재시작 직후 로컬 agent의 pane 자료보다 CLI가 먼저 붙는 RED를 보존했다. wait 전용 handshake는 host가 local hello의 pane 자료를 기다린 뒤 peer PID를 판정한다. m13은 host 재시작 때 local agent도 잠시 내려 CLI의 대기 진입 사건을 확인하고 hello를 보내 이 순서를 고정한다. Forbidden은 즉시 종료한다. 기존 연결의 권한을 올리거나 세션 자기 신고를 믿지 않는다.

- 검토 회귀: release manifest keeper는 `x.y.z+커밋`에서 최신/명시 공개판 선택을 확인한다. m13은 후보가 staging self-check 도중 원본을 바꾸는 fixture에서 미검증 원본의 실행 표지가 없고 운영 사본이 그대로임을 확인한다. CheckedUpdate는 staging 판/SHA 영수증을 반환하며 원본 경로를 재실행하지 않는다.

## 요약 상세 창의 지시 전문(10-19 사용자 후속)

host summary keeper와 기존 웹 E2E 28이 긴 여러 줄 지시의 끝까지 API/화면에 남는지 확인한다. 상세 창은 줄바꿈·긴 토큰을 감싸고 전체를 스크롤해 읽는다. hover 카드의 짧은 줄 규칙과 최근 지시 개수는 유지한다.

## 설치 앱 아이콘 배지

`app_badge::tests::counts_shared_unseen_and_open_decisions_once_per_pane`는 host 정본의 보고 열람·미처리 요청·질문/thread 중복 제거를 본다. `web/test/appBadge.test.mjs`는 실제 숫자 전달·0 지우기·미지원/거부와 창 없는 worker push(최신 host 수·offline payload·로그아웃)를 본다. `33-app-badge.mjs`는 실제 host·agent의 보고 도착 1 → 다른 기기 열람 0 → 확인 필요 증가 → resolve 감소를 Chromium·WebKit의 가짜 Badging API 기록으로 기다리고, 허용된 coordinator push의 암호문을 독립적으로 풀어 같은 수인지 본다. OS 배지 모양은 [실기기 절차](app-badging.md)를 따른다.

## Git 감시와 thread 없는 사건 repo(t-0420 L2)

- `cargo test -p aaa-common git_watch::`: branch 옵션/패턴 주입 거부·정확한 ls-remote/삭제 해석·head 변화 종류를 본다. host `git_watch::tests`는 문서 예시와 잘못 섞은 명령/일정/사건을 검증한다.
- `cargo test -p aaa --test m13 git_push -- --nocapture`: 실제 host·로컬/원격 agent·로컬 bare 원격·다른 clone의 push를 사용한다. 기준/무변화 무음 → 프로젝트 사건/inbox 한 번·repo로 거른 임무 깨움 한 번 → 재시작 무음 → force·같은 SHA 전이의 재발·삭제 → 실제 ls-remote 연속 실패의 outage 한 건을 본다. HEAD/FETCH_HEAD 보존도 확인한다.
- 고정 sleep 대신 실제 DB의 새 poll commit과 Git trace2의 실패한 ls-remote 종료 두 번을 기다린다. 최초 시험의 잘못된 역할 DB 경로는 원인을 확인해 고쳤으며 첫 로그를 보존한다.
- 결함은 재시작 때 저장 head 무시(중복 사건), thread 없는 사건 repo fallback 제거(임무 깨움 없음), force를 보통 push로 판정(종류 틀림)을 각각 심고 해당 keeper의 RED 뒤 바이트 복구 PASS를 확인한다. 기존 CI E2E도 그대로 통과해야 한다.

- `inactive_watch_sources_release_tick_deadlines`는 실제 CI/Git 파일을 켠 뒤 한 source만 끄기·둘 다 끄기·프로젝트 삭제를 적용한다. 비활성 예약은 없어지고 다른 활성 source 예약은 남아야 한다. 수리 전 비활성 Git 예약 단언의 RED를 보존한다. 고정 sleep·CPU 임계값을 쓰지 않는다.

## 머신별 작업 디스크 L0 (t-0434)

`aaa-config`의 작업 경로 keeper는 설정 덮어쓰기·HOME 확장·기존 기본값과 잘못된 경로의 설정 오류를 본다.
`aaa-agent`의 `work_root` keeper는 없는 mount와 동명의 일반 폴더를 실제 파일시스템에 만들고, preflight·health·routine이
대체 작업 폴더/내장 여유를 만들거나 보고하지 않는지 확인한다. 새 m13 `work_root::agent_refuses_an_unmounted_work_volume_before_creating_a_workspace`는
CLI가 읽은 설정으로 실제 agent가 작업 폴더를 만들기 전에 실패하는 연결을 확인한다. subprocess 종료가 종료 사건이다.
운영 mount·서비스·전역 설정에는 닿지 않고 새 sleep·신호 시험·production 시험 전용 flag는 없다.
설정의 잘못된 경로 거부를 제거한 변형은 config keeper가 상대 경로 수용으로 실패했다. mount를 디렉터리 존재만으로
통과시키는 변형은 agent keeper가 대체 폴더 생성으로 실패했고, m13은 agent가 거부하지 않고 계속 실행되어 종료 사건
시한에서 실패했다. 각 변형의 소스를 바이트 단위로 복원한 뒤 정상 gate를 실행한다.
agent 시작이 작업 root의 쓰기도 점검하므로 공용 Rust 실험실은 기본 agent.toml에도 자기 작업 root를 명시한다.

## 자동화 기록·목록(t-0420 L3)

- proto `routine_view_reads_previous_front_response`는 기록 필드가 없는 N-1 host 응답을 읽는다. 누락 trigger의 실제 RED 뒤 기본값 읽기를 넣었으며 웹은 없는 기록 배열을 빈 것으로 그린다.
- host `routine_runs::tests::durable_bounded_history_keeps_daily_total_and_delivery_identity`는 실제 DB meta를 재오픈해 50건 상한·순서·동일 전달 중복 집계 방지·고리 밖 하루 합계·날짜 전환을 확인한다. 기존 CI outbox replay 시험이 없는 임무→입력 전달→재전달에도 기록 한 건/오늘 한 번을 확인한다.
- 기존 m13 `routines`가 실제 coordinator 없음·미처리 안내·명령 승인 필요 기록을 CLI에서 읽는다. CI E2E는 같은 실패 집합 까닭과 재시작 뒤 기록 불변을, Git E2E는 push 한 건 기록과 재시작 뒤 중복 없음을 확인한다.
- 웹 E2E 34는 실제 일정 판정의 건너뜀을 목록/최근 기록에서 보고, 임무 조건 출처·승인 전 임무의 승인 필요 표시·enabled만 토글(주석/일정/본문 유지)·Reader 서버 차단·390px drawer 폭을 확인한다. 고정 sleep 없이 CLI/DOM 사건으로 끝낸다.
- 결함 검증은 고리 상한 제거·실제 건너뜀 기록 제거·웹 토글 방향 고정을 각각 RED로 확인한 뒤 원본 복구·PASS를 요구한다. 최종 ci-local은 모든 복구 후 실행한다.

### 임무 확인창 막힘 복귀(L3 추가)

- 실제 host Lab의 Question→Working이 기존 코드에서 계속 막힘인 RED를 먼저 보존한다. 수리 뒤 Question→Working/Idle·입력 없는 Idle 무깨움·누적 입력 한 번 전달·DB/API 이유 해제를 확인한다.
- message_unknown·이전 host 깨움·brief 모름·restart 실패·이유 없는 옛 막힘은 Question/Working/Idle과 pause/resume 뒤에도 입력을 보존한다. 명시 restart는 이유를 지운다. Agent 응답을 기다리는 두 경계(상태 조회·입력 파일 쓰기)에서 실제 message_unknown flow를 넣어 merge/깨움 commit이 이를 덮지 않는지 본다.
- m13 `mission_question_answer_recovers_and_delivers_accumulated_input_once`는 승인한 임무의 실제 가짜 Claude 신뢰/권한 질문에 답하고, 확인창 중 routine 명령으로 쌓인 내구 입력이 Idle 뒤 한 번만 전달되는지 본다. 고정 sleep 없이 질문·상태·출력·tick 사건으로 끝낸다. 기존 restart/uncertain-wake 시험도 유지한다.
- 확인창 복귀 분기 제거와 전달 모름까지 자동 해제하는 변형은 각각 해당 회귀가 실패해야 한다. 웹 상태 말은 기존 specialModel 계약에서 모든 이유를 검증한다.

L3의 활동 막대 E2E26은 패널 개수를 고정하지 않고 기억한 머신 단추만 Tab 정지점인지 확인한다. PR 목록 E2E는 새 조건 → 동작·지난 결과·다음·오늘·건너뜀·출처 칼럼을 확인한다.

## Agent link 후보 전송 L2(§8.282)

- `cargo test -p aaa-agent deploy::stage::tests --lib`: 실제 파일/기존 Executor 경계에서 빌드 폴더·provenance·일반파일/symlink·조각 상한, EOF SHA 전에 실행 금지, offset 오류와 agent 객체 재생성 뒤 재개, 설치 시 프로젝트/ID/변조 거부, 최신 후보 보관·잘못된 keep 정리 무삭제를 확인한다. 새 test-only production seam은 없다.
- `cargo test -p aaa-host rollout::tests --lib`: 기존 전이 keeper에 모든 Transfer/Preflight 뒤 첫 교체, stage 영수증의 소속·SHA·완료 크기·판, UpdateStaged 해석을 더한다.
- 기존 m13 rollout keeper를 L2 정상 경로로 확장한다. 실제 TLS agent 앞의 시험 소유 TCP proxy가 다음 source 응답bytes를 channel 사건으로 붙잡은 동안 실제 저장 조각과 pane 입력/출력을 확인하고 host를 내린다. gate를 풀고 같은 rollout/job ID로 이어간다. 전송 뒤 원본을 치워도 상태 폴더 후보만으로 전체 교체가 끝나야 한다. --from의 SHA 실패/원본 변조/둘째 머신 실패 단언은 남긴다. 시간은 proxy gate부터 전체전송/사전검사, pane 왕복을 로그에 기록한다.
- 결함 심기는 재개 새ID·SHA누락·실패무시 필수3개와 전송 offset/EOF SHA/정리 보호를 각 owner 경계에서 확인하고 바이트 원복한다. 최종 gate는 ci-local 전체와 check-public이다. 운영 배포·실제 launchd는 건드리지 않는다.

## 머신별 작업 디스크 L1 (t-0434)

`cargo test -p aaa-agent recovery_tests --lib -- --nocapture`는 실제 Git을 쓰는 두 owner다.
구판 journal 가져오기·원본/정본 보존, A→B 뒤 생성 root의 placement/restart, 옛 mount 부재 거부,
전환 중 준비 거부·root 소유권을 본다. 별도 계약으로 잠긴 worktree 경로를 잠깐 옮긴 뒤 실제 Git prune에도
등록이 남는지, dirty/uncopied/사람 잠금 보존, 중단한 unlock의 재시작 복구와 대상별 remove를 본다.
임시 저장소에서만 실행하며 실제 mount나 서비스는 바꾸지 않는다. 두 owner는 약 1초다.

기존 m13 준비 keeper는 agent 상태 폴더의 journal을 읽는다. 연결 단절·15초 경계·dirty 거부 단언은 유지한다.
`interrupted_placement_and_resolve_resume_from_agent_records`는 준비 복구 뒤 같은 state의 agent를 B root로
다시 띄우고, 기존 A의 resolve 응답 유실·재시작 회수와 새 thread의 B 경로까지 확인한다. 연결/Git/종료 사건으로 기다린다.

결함은 생성 root를 현재 root로 재계산, Git 잠금 생략, 넓은 prune 재도입, 재시작 relock 누락,
사람 잠금 해제, 전환의 공유 잠금 생략을 각각 심어 해당 owner의 assertion RED를 확인했다.
각 소스를 바이트 단위로 복원한 뒤 정상 두 owner PASS를 확인했다. 전체 gate는 ci-local과 check-public이다.


## 머신별 작업 디스크 L2 (t-0434)

- `cargo test -p aaa --test m13 work_root:: -- --nocapture`: 실제 CLI의 mount 인자·시작 거부, 주석/타 설정 보존,
  멱등 set, 마지막 시작값과 다음 설정값 구분, 재시작 전후 새 thread 위치, 옛 pane PID·cwd 보존·resolve·sweep을 본다.
  별도 node는 구판 journal 가져오기와 실제 RootTransaction 등록 완료/config rename 전의 중단 checkpoint에서 이어 하기,
  읽기 전용·symlink·hardlink·잘못된 TOML 무교체를 확인한다.
- `cargo test -p aaa --bin aaa work_root::tests -- --nocapture`: 설정 snapshot 뒤의 외부 편집을 owner에서 만들어
  마지막 rename으로 다른 편집을 덮지 않는지 본다. race를 위한 production hook은 없다.
- L1의 실제 Git owner 둘은 그대로 실행한다. 새 마지막 시작 mount 필드는 구판 상태에서 기본값으로 읽는다.
- macOS 실제 두 mount 확인은 격리 APFS sparse image와 Data 볼륨의 별도 config/state/host/agent/repo만 사용한다.
  정상 전환 → 실제 detach 뒤 새 작업/정리/시작 거부·내장 대체 없음·Git lock 보존 → remount 뒤 명시 resolve 재개를 본다.
  운영 경로·서비스·로그인에는 닿지 않으며 소유한 image/device/프로세스만 정리한다.

L2 결함 검증은 이전 root 가져오기 생략, 재시작 전 applied 변경, 값 주석 유실, readonly 교체 허용,
동시 편집 덮기, 명시 mount 무시, JSON 오류 출력 누락을 각각 심어 owner assertion RED를 확인한다.
모든 소스를 바이트 단위로 복원하고 정상 CLI·snapshot keeper를 통과한 뒤 전체 ci-local/check-public을 실행한다.

## CLI 시작 스택 (t-0446)

- `cargo test -p aaa --test resource_limits cli_parses_and_dispatches_with_a_small_main_stack`는
  Unix 자식 셸의 main 스택을 768KiB로 낮추고 실제 aaa로 exec한다. 도움말의 정상 종료, 삭제된 TUI 명령의
  오류 처리, bash 완성 스크립트 생성까지 확인한다. 부모·운영 프로세스의 한도는 바꾸지 않는다.
- clap derive의 debug 명령 트리 생성은 중첩 프레임이 크다. 생성·파싱만 4MiB thread에서 끝내고
  실제 명령은 원래 main thread에서 실행한다. Box로 future나 CLI enum을 줄여도 이 빌더 프레임은 줄지 않는다.
- macOS에서 수정 전 도움말·완성 생성의 stack overflow와 수정 후 통과를 확인했다. 이 시험은 Windows 실행을
  대신하지 않는다. Windows의 codex_integration·smoke·tui_removed 결과는 GitHub Windows runner에서 확인한다.

## 실패 머신 rollback과 독립 복구 감시 L3(§8.289)

- agent guard policy 두 keeper는 새 실행 판/SHA/instance/시작시각, Activate/Recover 재전달의 deadline·ID 보존, unchanged 취소와 설치 중 취소/expiry, 복구 요청 뒤 old 프로세스와 로컬 host+agent 둘 확인을 소유한다. 제품 timer의 now 경계값으로 무응답120초를 확인해 실제 sleep으로 기다리지 않는다.
- proto `previous_rollout_records_keep_their_non_recovery_policy`는 N-1 JSON의 자동복구 off와 같은 ID를 확인한다. host `recovery_receipts_keep_the_failed_cursor_and_durable_busy_state`는 실제 DB 재오픈 뒤 Recovering의 busy와 terminal 실패 cursor/앞 완료 단계/영수증 보존을 확인한다.
- update `pinned_rollback_ignores_newer_backups_and_never_executes_the_broken_installation`은 실제 파일/실행 표지로 실패 설치·다른 최신 보관본 무실행, 정확 baseline SHA·멱등성, 일반 보관본 삭제 뒤 private pin 복구를 확인한다.
- 기존 macOS m13 rollout keeper는 정상 L2 경계 뒤 unchanged SHA 실패의 무재시작을 유지한다. remote가 옛 바이너리로 잘못 돌아오면 독립 PID가 살아 복구하고, 복구 중 host를 다시 띄워도 같은 IDs/실패 머신만 유지한다. host 실패는 host만 복구하며, 로컬 agent 실패는 공유 파일의 host/agent 둘을 복구한다. 복구 전 뜬 옛 프로세스는 완료로 세지 않는다. private token을 아는 다른 peer PID와 bootstrap 없는 내부 CLI도 거부한다.
- 실제 설치 config check는 시험 소유 Unix socket으로 멈춘다. safety 철회→Recovering→gate 해제→늦은 rename 뒤 baseline 복구→새 old host를 확인한다. self-check30초·primary120초와 기존30초 사건 대기 상한을 늘리지 않는다. 독립 guard는 기록 PID와 실제 실험실 명령 경로를 함께 확인해 Lab이 회수한다.
- 새 복구 ID·실행 SHA 누락·실패 뒤 다음 단계 진행의 필수 변형과 설치 중 취소/새 프로세스 확인 변형을 RED로 확인하고 정확 bytes 복구 뒤 GREEN/전체 ci-local·check-public을 실행한다. 독립 최종 read-only 리뷰는 같은 후보에서 한다.

SHA 거부 대기(t-0587)는 로컬 agent의 연결과 원격 `AgentUpdate` 단계 진입을 구분한다. 연결만 보고
`deploy status --wait`의 30초를 시작하면 로컬 재시작의 신원 대조·감시 해제 시간까지 함께 잰다.
원본 실패 기록에서는 원격 단계가 대기 시작 약 10.4초 뒤, Recovering이 약 27.1초 뒤에 시작했다.
기존 단계 진행 사건으로 원격 교체 진입을 확인한 뒤 원래 30초 상한으로 종료 표지를 기다린다.
SHA 오류·실패 cursor·앞 머신 SHA·서비스 호출 횟수 단언은 유지한다. 별도로 영속 guard의
`install_started=true`·`activated=false`·baseline SHA·Cancelled·무재시작을 확인하므로
BeginInstall 뒤 거부된 설치가 교체 lease 확인을 거쳐 Stopped로 끝나는 경로도 빠지지 않는다.
실제 설치 중 safety 철회의 늦은 rename·복구 keeper와 제품 시한·전이는 바꾸지 않는다.


## 순수 TOML·공통 자동화(L4)

`cargo test -p aaa-host automation::`는 실제 파일 파싱·스키마·안전한 본문 경로·이름 충돌·원본 보존·미리보기 hash·Reader 거부와 공통 큐의 예산·전달 모름을 검사한다.
CI/Git의 source outbox·중복 억제·재시작·outage는 기존 owner를 유지한다. 상주 실행과 reset은 A4에서 제거했으며 옛 파일·저장 형식은 변환 owner로만 읽는다.

## 웹 배포 조회 카드 L4 A안(§8.293)

- `cargo test -p aaa-host --lib rollout`: DB commit 뒤 변경 사건·표시용 자료만의 요약·재오픈, 종료 commit 직후 재시작·4가지 terminal 상태·중복 발행과 inbox 정리 뒤 재시작을 확인한다. DB owner는 실제 SQL trigger로 inbox 저장을 거부해 완료 정본만 남지 않는지 확인한다. 오류에 경로/PID를 섞어도 표시용 요약·실제 Reader inbox peek 응답에는 없고 정본에는 남는지, 옛 terminal에서 프로젝트/inbox를 재생성하지 않는지도 확인한다. `cargo test -p aaa-host --lib local_wire_excludes_web_only_rollout_notices`는 실제 front frame 전송에서 새 알림이 N-1 CLI에 닿지 않는지 확인한다. 운영 DB/프로세스를 건드리지 않는다.
- `node web/e2e/web.e2e.mjs --spec 35-rollout`: 실제 Reader pairing/API/상태 WS를 쓰고 rollout 사건만 주입한다. 전송률→해당 단계 복구→재연결의 같은ID→복구 완료와 실패 구분·전체 오류/SHA·Reader 시작 거부·390px 폭·앱 안 알림 이동을 확인한다. reload 뒤 이미 선택한 패널 단추를 누르면 drawer가 닫히므로 현재 drawer를 여는 사건으로 이어 간다.
- 변경 사건 누락·종료 중복 발행·정본과 inbox의 transaction 분리와 웹 사건 소비 누락을 각 owner에서 RED로 확인한 뒤 bytes 원복/GREEN을 확인한다. 같은 후보의 전체 ci-local·check-public·독립 read-only 검토를 마친다.

## 같은 Mac의 둘째 aaa(§8.294)

- 유닛:
  - 파생: 이름이 없으면 지금 이름 그대로이고 이름 규칙을 지킨다(`aaa-config` paths). launchd label·로그(`aaa` launchd), 쿠키 이름(`aaa-host` web).
  - init: 포트 쌍 고르기(다른 인스턴스·지금 쓰는 포트 건너뛰기), host.toml 읽기, 긴 상태 폴더 거부(`aaa` init).
  - tailscale: 같은 Mac의 다른 인스턴스 proxy를 제 것으로 보지 않는다.
- m13 `instance::second_instance_has_its_own_folders_host_and_cookie`(macOS): HOME만 실험실로 바꾸고 dir env를 비워, 실제 기본 경로를 본다.
  - dir env가 남으면 init을 거부한다. 이름 규칙(종료 2)을 본다. 터미널이 아니면 거부한다. 기본 인스턴스 프로젝트와 겹치는 이름도 거부한다.
  - init은 `~/.config/aaa-work/host.toml`(7430이 아닌 포트 쌍)과 `~/.aaa-work/projects`를 만들고 기본 폴더는 만들지 않는다. 두 번째 init은 덮지 않는다.
  - `--instance work host`·`agent`가 `Application Support/aaa-work`에서 뜨고 작업 root는 `~/aaa-work-work`다. 기본 인스턴스는 같은 HOME에서도 그 host를 보지 않는다. env `AAA_INSTANCE`만 준 CLI(pane과 같다)는 그 host로 간다.
  - 이름 없는 `project create`는 프로젝트 `work`를 만든다. 기본 인스턴스에서는 이름을 요구한다.
  - `service show`(가짜 `launchctl`, 읽기만)의 plist가 label `dev.aaa.work.host`, env `AAA_INSTANCE = work`, 로그 `Logs/aaa-work/`를 담고 plist를 쓰지 않는다.
  - 실제 HTTP pairing의 Set-Cookie가 `aaa-work=`이다.
  - 결함 심기 셋(쿠키 이름에서 인스턴스 빼기, 작업 root 기본값에서 인스턴스 빼기, plist env에서 `AAA_INSTANCE` 빼기)을 이 시험이 잡는 것을 확인했다.

## 자동 thread 시작(L5, plan §8.295)

- `cargo test -p aaa-host start_`: 실제 host/DB와 기존 agent frame fixture로 첫/넓힘/좁힘 승인, propose 무시작, Working 수 idlest, 명시 holds·상한·기다림 만료·변경 diff, 범위/동작 변경 취소와 본문만 변경 적용을 본다. `auto_start::tests::week_left_...`는 비교 가능한 주간 숫자와 모호한 계정/창/실패의 첫 profile fallback을 소유한다.
- 같은 사건의 thread+시작 key transaction과 DB 재접속, 생성 직후 최종 receipt INSERT를 SQLite trigger로 실패시킨 실제 inflight 복구를 검사한다. fake가 영수증/완료 상태를 대신 만들지 않는다. 일정형은 coordinator 없는 routine_pass부터 실행하며, cursor 저장 전 crash를 되감아 같은 예정시각 key와 실제 thread 한 건/완료 기록을 본다.
- `cargo test -p aaa --test m13 automation_start_holds`: 실험실 두 agent와 실제 CLI의 holds/PTY 사람 승인, 표지 해제 뒤 원격 worktree 생성, profile fallback, host 재시작 뒤 무중복, 앞 thread가 열린 상태의 다른 사건 시작을 본다. 보고 사건으로 진행하며 고정 sleep은 없다.
- 웹34 전체는 실제 Controller의 범위 확인/승인 반영과 Reader의 단추 없음/API 거부를 본다. 기존 파일 전제가 있으므로 개별 뒷 단계 대신 파일 전체를 실행한다.
- 변형은 승인 우회/ceiling 확대 허용, propose에서 직접 실행, idlest 순서, holds 후보/transaction 검사, DB 영수증 조회, 예정시각 key, 명시 기다림 만료/본문 변경 안내를 독립 제거한다. 각 keeper의 의도한 RED와 원본 bytes 복구 PASS를 확인하고 전수 gate는 정상 소스에서만 돈다.

## 사람 터미널·배포 제안 L5(§8.296)

- `cargo test -p aaa-host --lib automation::`: 실제 TOML의 deploy/main/pushed·min_commits1~100 계약과 schema 정본, 실제 SQL receipt 실패→inbox commit→done/prune→host 재오픈→동일 batch 재개를 확인한다. 제안은 한 번만 남고 rollout/thread를 시작하지 않아야 한다. Reader 본문에 raw source 경로/제목을 복제하지 않는다.
- `cargo test -p aaa --test m13 deploy_proposals_count_main_commits`: 실제 bare origin·조회 agent·push로 등록/한 커밋 무음→host 재시작→두 커밋 제안, 성공 정본 뒤 새 기준/최신 실패 정본 보존, forced/deleted 무제안과 rollout 무변경을 본다. 전체 inbox와 Git 사건에 경로/제목/보고서가 없는지, 이전 성공 기록을 둔 채 독립 history origin으로 교체·재시작한 뒤 새 누적 제안이 나오는지도 검사한다.
- `cargo test -p aaa --test m13 rollout_preflights`: 기존 두 머신 keeper에 실제 사람 PTY no·터미널 없는 CLI의 표준 입력 거부, yes 시작·coordinator safety off·host 재시작/같은 ID/bytes·사람 status를 더했다. 이후 coordinator 실패·safety 철회/독립 복구 keeper는 유지한다. 운영 서비스는 실험실 launchctl 기록 모형이다.
- 사람이 여는 문 제거/사람 승인을 coordinator safety로 재판정/제안 표지 누락/커밋 임계값 무시 중 해당 owner의 의도한 RED를 보존하고 bytes 원복 뒤 GREEN을 확인한다. 최종 후보의 전체 ci-local·check-public과 독립 read-only 검토를 마친다.


## 대화 기록 앞 쪽 읽기(t-0454, plan §8.298)

- `cargo test -p aaa-agent -p aaa-host -p aaa-proto --lib chat --no-fail-fast`: 실제 합성 JSONL reader가 Claude/Codex 수천 메시지, 한 줄 안 블록 쪽 경계, append 중 cursor/ID, 미완성 마지막 줄, 첫 메시지 도달, 17MiB blob 너머 진행, 17MiB 그림/문자열·배열 tool_result와 같은 줄의 사람 글 보존, frame 크기를 확인한다. 파일 원천은 agent에 있으므로 쪽 경계의 owner는 agent다. host 유닛은 Reader의 기존 pane 권한 아래 커서/처음/자란 최신 응답의 무변형 중계와 구 agent 기능 거부를 검사한다. proto는 선택 필드를 모르는 구 요청/응답의 양방향 호환을 소유한다.
- `node web/e2e/web.e2e.mjs --spec 37-chat-pages`: 실제 lab host/agent/JSONL에서 WebKit·Chromium × 1280·390·820 × Claude·Codex를 돈다. 사람/답 2400개(1200턴)에서 첫 메시지까지 이전 쪽을 받고 매번 보던 첫 줄의 viewport 좌표가 1px 안에서 같은지 단언한다. 처음 표지/생략 안내 종료, 300개 이하 본문 mount, 위를 읽는 동안 400개가 자란 최신 gap 보존과 맨 아래 따라가기도 검사한다. row40의 pane 왕복 복원 완료(aria-busy), 실제 ResizeObserver callback이 scroll 사건보다 먼저 오는 순서, 짧은 최신 쪽 자동 채움/위치 보존과 수동 End의 빈 공간 제거와 즉시 End 뒤 pane 왕복의 따라가기 기억도 단언한다. 응답/DOM/animation frame 사건으로 기다리고 고정 sleep은 없다.
- 기존 E2E11·29·30의 대화/입력/위치 기억 회귀를 유지한다. 수정 전 36의 이전 쪽 HTTP 요청 없음 RED와 수정 중 prepend 자동 보정이 기준 줄을 바꾸는 RED를 별도 로그로 보존한다. source owner 변형 RED 후 원본 bytes를 돌려놓고 전수 ci-local/check-public을 실행한다.

### 대화 앞 쪽 CI 회귀(t-0458)

- 웹 E2E37의 기존 390px Claude 짧은 쪽 시나리오에서, 긴 기록이 한 줄로 바뀌어 브라우저가 위치를 줄이며 내는 실제 scroll 사건을 앞 쪽 요청 시작 뒤에 전달한다. 응답 gate로 느린 전송을, frame당 25ms 작업으로 느린 렌더를 흉내 낸다. 고정 sleep 없이 scroll·HTTP 응답·DOM commit 사건으로 끝낸다. 보던 줄 1px 단언과 수동 End의 빈 공간 제거는 그대로 둔다.
- 수정 전 WebKit에서 CI와 같은 204.5419921875→755.7919921875 이동을 재현했다. 제품은 렌더 직후 브라우저의 자동 위치 축소를 기억해, 늦은 scroll을 사람의 이동으로 오인하여 끝 따라가기를 켜지 않게 한다. 회귀는 이 owner를 되돌리면 다시 실패해야 한다. 짧은 쪽 진단은 이전/축소 위치·줄 좌표·터미널 1006 횟수를 남긴다.
- Windows는 `cargo xwin clippy --target x86_64-pc-windows-msvc --workspace --all-targets --locked --keep-going -- -D warnings`로 cfg를 검사한다. 실제 Windows 실행 결과와 구분한다. `file_history`의 Unix identity 추가는 불변 변수 shadowing으로 처리하며 경고 allow는 넣지 않는다.

### E2E37 조합별 단계(t-0462)

- 12조합은 각각 독립된 기록·화면을 열고 같은 검사를 수행한다. 각 조합을 별도 `step`으로 실행하며 이름 끝에 브라우저·폭·harness를 붙인다. 공통 단계 상한 120초와 locator/HTTP 대기 상한은 그대로다. CI run 39346442455는 마지막 WebKit 820 Codex를 시작한 뒤 12조합 합계가 120초를 넘어 실패했다. 기본 1280px 화면의 진단을 정지 조합으로 해석하지 않는다.
- `AAA_CHAT_FRAME_MS=25 AAA_E2E_THROTTLE=2 NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs --spec 37 --repeat 3 --workers 3`으로 frame 점유(두 브라우저)·CPU 제한(Chromium) 아래 12조합을 반복한다. frame 점유는 시험 페이지 안에서만 돌며 context를 닫을 때 끝난다. 응답·DOM·scroll 사건과 기존 1px 단언, 300개 본문 상한, 2400개 처음 도달·400개 gap·복원·End 검사를 유지한다. 고정 sleep이나 재시도는 없다.
- 조합별로 최신 줄·처음 도달·row40 복원·최신 gap·끝 따라가기의 경과 시간을 남겨 locator 실패 위치와 합계 상한을 구분한다. 실패하면 수신한 행 수·끝 세 행의 본문과 좌표·굴림 위치·빈 공간·오류·화면을 남겨 수신 누락과 끝 따라가기 실패를 구분한다.

### E2E37 실패 산출물(t-0501)

- 성공할 때는 진단 파일·추가 출력·추가 HTTP 요청·대기가 없다. 앞 기록 요청/응답과 논리적인 DOM 전이는 256개까지 메모리에 모은다. DOM 전이는 기존 두 animation frame 대기의 반환값으로 가져오며 frame·브라우저 왕복을 더하지 않는다. 가상 본문 mount 때마다 행을 세거나 높이를 재지 않는다.
- 실패 지점은 기존 터미널 진단이 화면을 움직이기 전에 저장한다. 실험실 `diagnostics/pages-<browser>-<width>-<harness>/`의 `failure.json`에는 시작·실패 시각, 원래 오류, 요청 cursor·응답 before/history·첫/끝 ID·행 수·HTTP 수신 여부와 DOM 전이가 있다. `dom.json`은 실패 당시 HTML·위치·처음 표지 좌표와 남은 DOM 전이, `screen.png`는 화면, `host.log`·`agent.log`는 해당 조합 시작 이후 로그와 앞 다섯 줄이다. 인증 설정·DB·cookie는 복사하지 않고 실험실의 시작 fingerprint도 가린다.
- renderer가 응답하지 않아도 먼저 받은 요청/응답·DOM 전이와 로그는 남는다. DOM·화면 수집이 실패하면 각각 `dom-error.txt`·`screen-error.txt`를 남긴다. 이 수집에만 별도 상한을 둔다(DOM은 기존 `inTime`의 5초 인수·6초 Node 울타리, 화면은 5초). 시험의 기존 제한·단언은 바뀌지 않는다. 단계 전체의 120초 실패도 같은 진단을 실행한다.
- `bash scripts/ci-local.sh --only e2e --logs <폴더>`는 실패 진단을 `<폴더>/e2e-failures/aaw-<pid>/`에 복사하고 `e2e.log`에 경로를 남긴다. GitHub의 같은 명령(`--logs .`)은 `e2e.log`와 `e2e-failures/`를 `web-e2e-failure-<run>-<attempt>` artifact로 14일 보존한다. 실험실 전체는 업로드하지 않는다.
- 산출물 점검은 첫 조합의 `처음 도달` 직후 예외를 한 번 주입해 공식 ci-local의 해당 E2E만 실패시킨다. 요청/응답·2,400행/처음이다 전이·DOM·PNG·host/agent 구간이 로그 폴더에 복사됐는지 본 뒤 소스를 bytes로 복원한다. 이 변형은 진단 경로의 검증이며 원래 간헐 실패의 재현이 아니다. 복원한 소스에서 전체 ci-local·check-public을 통과해야 한다. 다음 실제 실패의 artifact로 별도 원인 수리 thread를 연다.

### macOS 27 WebKit 끝 이동 경쟁(t-0474)

- 별도로 남겼던 `기록 2800` locator 30초 실패는 제품의 끝 이동 경쟁이었다. 원본 전체 ci-local에서 WebKit 1280 Claude가 재현됐고, 2801행 수신·오류 없음·끝 세 행은 빈 가상 row인 상태였다. scrollTop 129322 / scrollHeight 334056 / clientHeight 794로 화면이 끝에서 이탈했다.
- 실제 scroll 추적은 smooth 요청 → 첫 이동 565px → row 실측의 즉시 끝 보정 337516px → 옛 애니메이션의 다음 scroll 23190px 순서를 보였다. 제품이 마지막 사건을 손의 위 이동으로 오인해 따라가기를 껐다. Safari에서 과거 대화를 읽다 「맨 아래로」를 누르면 새 답이 도착해도 본문이 화면 밖에 남을 수 있었다.
- 가상화하는 긴 대화의 끝 단추는 즉시 이동한다. 같은 가상화 판정을 row와 끝 이동에 함께 쓰며, 실측마다 실제 끝을 맞춘다. 목적지가 바뀌는 native smooth 이동과 즉시 보정을 섞지 않는다. 새 애니메이션 상태나 테스트 전용 제품 표지는 두지 않는다.
- 회귀 소유자는 기존 E2E37의 12조합이다. 2400행 처음 도달·400행 gap·새 메시지 본문과 끝 따라가기·prepend 1px·300개 본문 상한·pane 복원·End 검사를 그대로 쓴다. 원본 실패와 수리 뒤 같은 전체 ci-local 조건의 3회 연속 결과를 보존한다. 각 실행은 새 웹 번들·실행 파일을 확인하고 Rust 시험·clippy와 다른 E2E를 함께 실행한다. 한 번이라도 실패하면 반복을 멈추고 원인을 조사한다. timeout·재시도·허용 오차·조합은 바꾸지 않는다.

## 휴대폰 뒤로 기록(t-0466, plan §8.301)

- `node web/e2e/web.e2e.mjs --spec 39-phone-back`: WebKit iPhone 390·430 × Safari 탭·홈 화면 앱 흉내에서 pane 셋과 대화의 보고서를 연 뒤 앱 문서 칸(init script가 적은 `history.length`)까지 `history.back()`을 popstate 사건마다 하고 화면 차례를 단언한다. 왼쪽 가장자리 쓸기는 `document.createTouch`로 touchstart·touchmove(frame마다)·touchend/touchcancel을 내고, 겹친 뒤로는 손가락을 뗀 뒤 popstate를 기다린다. 닫힌 pane 칸·「모든 thread」 닫기의 기록 칸 수와 휴대폰·데스크톱 빈 화면 문구도 본다. 고정 sleep은 없고 popstate·DOM·animation frame·drawer transition 사건으로 끝낸다.
- t-0466 수정 전 코드로는 첫 단계가 「(주소 없음)+빈 화면」으로 실패했다. 당시 쓸기 겹침의 「목록 닫힘」 규칙은 t-0541에서 상태 유지로 바뀌었고, 사용자 10-22 결정(t-0780)으로 다시 항상 닫기로 바뀌었다. 뒤로 화면 순서·빈 화면 방지·기록 칸 수 단언은 유지한다.
- t-0780(plan §8.340): 휴대폰 뒤로·앞으로는 메뉴를 항상 닫는다. t-0541의 상태 유지 단언을 대체한다. 1→2→3에서 메뉴를 열고 가장자리 쓸기 + touchcancel + 뒤로는 2에 메뉴 닫힘이며, 매번 다시 열어도 1→2→3의 양방향 이동에서 닫힌다. 닫힌 메뉴가 이동으로 열리지 않는지도 확인한다. WebKit 390·430 × Safari 탭·홈 화면 앱 흉내에서 popstate·선택 줄 aria-current·drawer transition을 기다리고 기록 칸 수를 확인한다. WebKit 1280·820에서는 기존 사이드바 펼침·접힘 유지 검사를 보존한다.
- E2E39의 `history-destinations.mjs`는 실제 host·agent의 thread 질문·게시 보고서·할 일을 준비하고 Chromium·WebKit 390px에서 thread·보고서 상세/목록·할 일 목록·질문 카드/미선택 목록·설정 목록/쪽을 메뉴 열림·닫힘 각각에서 뒤로·앞으로 순회한다. `history.back/forward`의 popstate 뒤 주소·본문 DOM·React frame·drawer transition으로 끝낸다. hash가 그대로인 설정 목록↔쪽·본문 복귀도 포함하며 hashchange만 처리하는 수리가 빠지는 경로를 검사한다. 빈 주소를 보던 pane으로 정규화하는 조기 반환과 목록 줄 직접 선택·빈 화면의 메뉴 단추도 확인한다. 할 일 상세는 같은 주소의 시트 기록이므로 별도 순회한다. 실제 앱 단축키로 메뉴를 연 뒤 Back의 상세 닫기·Forward의 닫힌 시트 건너뛰기와 메뉴 닫힘을 확인한다. 고정 sleep·시한 연장·실패 재시도는 없다.
- t-0636(plan §8.408) 쓸기 자리: 왼쪽 끝(x 4)에서 시작한 쓸기는 목록을 열지 않고 주소·기록 그대로, 안쪽(x 60) 오른쪽 쓸기는 목록을 연다. 가장자리 쓸기 + touchcancel + 뒤로와 쓸기 끝난 뒤 뒤로는 앞 pane에 목록 닫힘이다. 왼쪽 끝 24px 경계와 메뉴 단추·안쪽 쓸기는 바꾸지 않는다.
- 헤드리스는 Safari 시스템 몸짓 자체를 실행하지 못한다. 오른쪽 쓸기는 합성 touchstart·touchmove·touchcancel 뒤 history.back으로, 일반 뒤로·앞으로는 history.go로 같은 문서의 기록 순회를 흉내 낸다. 실제 iPhone Safari·홈 화면 앱의 운영체제 몸짓까지 대신 보증하지 않는다.

## 자동화 상세·변환·막힌 큐(t-0468, plan §8.303)

- `node web/e2e/web.e2e.mjs --spec 40-automation-detail`: 상세의 기록 수는 host의 `routine runs`와 같아질 때까지, 원문은 파일 글과 같아질 때까지 기다린다. 변환은 미리보기 sheet가 그린 묶음(`data-group`) 뒤에 파일이 그대로인지 보고, 실행 결과 표지(`convert-archived`) 뒤에 보존 사본을 원본과 비교한다.
- 자동화 화면(t-0628): 34·40·48·49·55·64는 카드를 눌러 본문 설정 화면을 열고, 닫기 단추 뒤 그 화면이 숨는 것(`hidden`, 숨긴 채 그려 둔다)을 기다린다. 떠 있는 창(`.modal-scrim`)이 없는 것과 저장·표지 뒤 주소(`#automation=<project>/<name>`)를 본다. 숨은 설정 화면 머리에도 목록 단추가 있어 휴대폰 목록 단추는 `:visible`로 고른다.
- 막힌 큐는 실제 전송 실패로 만든다: fake thread의 pane을 `pane kill`로 끝내고(목록에서 끝남 확인) 1초 일정의 `do.message` 규칙을 둔 뒤 `ticker tick`으로 host 기록에 「입력 전달 모름 · 확인 필요」가 올 때까지 판정한다. 「다시 보내지 않음」은 tick 두 번(울타리) 뒤 같은 묶음 id·실패 기록 수로 단언한다. 「계속 보내기」 뒤에는 pane이 끝난 채라 다음 입력이 실제로 시도되어 실패하고 새 묶음으로 다시 막히는 것을 `ticker tick`·host 기록으로 기다린다. 그 묶음은 「버리기」로 닫고, 규칙을 끈 뒤 tick 울타리 뒤 큐가 빈 것을 본다. fake agent의 전송 지연(재시작 뒤 첫 글이 0~15초)에 기대지 않는다. 고정 sleep은 없다.
- host 쪽 묶음 상태(보낸 결과 모름·다른 동작 묶음·버리기)는 `cargo test -p aaa-host --lib automation::tests`가 DB의 큐 JSON을 직접 두고 본다.

### 파일 오류와 실행 실패·배포 중 기능 대기(t-0560, plan §8.362)

- `node --test web/test/automationModel.test.mjs`: 정상 파일의 `start:config`·`flow:error` 등 마지막 `failed`는 「실행 실패」,
  깨진 파일의 `error`는 「파일 오류」다. 파싱 실패는 `enabled=false`로 와도 꺼짐에 가려지지 않는다.
- `cargo test -p aaa-host --lib automation::start_tests::rollout_`: 실제 host·DB·agent frame fixture에서 `reuse=open` 승인 범위가
  끊김·기능 부족에도 같고, 흐름 입력이 수집되어 원인과 「다시 시도 가능·입력 보존」을 알리는 대기로 남는지 본다.
  host 재시작·새 agent 재연결 뒤 동일 사건 처리·receipt 재생·시작 예산을 확인한다. 혼재한 후보 중 가능한 머신을 고르는 경계도 본다.
  시작 완료는 `thread_events`의 실제 Open 전이로 기다린다. thread 생성 영수증은 배경 기동 완료를 뜻하지 않는다.
- `node web/e2e/web.e2e.mjs --spec 64`: 정상 TOML의 없는 대상에 실제 실행을 요청해 host 실행 실패를 만든다.
  CLI와 Front API의 오류 칸·지난 결과·원인을 비교하고, 목록 표지·상세 칩·요약이 실행 실패와 TOML 파싱 실패를 구분하는지 검사한다.
  휴대폰 WebKit(390×844)에서도 구분하고 두 화면을 촬영한다. 고정 sleep·운영 설정·운영 DB 쓰기는 없다.

## 자동화 만들기·고치기·지우기·지금 실행(t-0538 관리 GUI W2, plan §8.342)

- `cargo test -p aaa-host --lib automation::edit`: 폼 값 합치기의 바이트 보존(바꾼 칸만, 주석·순서 그대로), 조건 바꾸기와 기본값 칸 빼기, 새 파일의 칸 순서와 `#:schema` 머리, 첫 칸이 사라져도 남는 머리 주석, inline 표·`do = "deploy"`, 잘못된 폼·원문의 거부.
- `cargo test -p aaa-host --lib automation::tests`: 실제 Host로 Reader 검사 허용·저장 거부, 새로 만들기 중복·본 판이 아닌 저장·잘못된 글이 폴더 전체 스냅샷을 바꾸지 않음, 지운 뒤 낡은 판 저장이 NotFound(되살아나지 않음), 되살리기는 꺼진 채 한 번, 옛 `.md` 저장 거부, 삭제의 대기 입력 취소·처리 중 묶음 보존, 지금 실행의 같은 key 한 번·일정 차례와 따로 받기·거부 넷, 본문 파일의 판 대조와 automations/ 밖 새 경로 거부, 명령 승인의 세션 안·worker·evaluator·detached·Reader 거부와 Controller 허용.
- `node web/e2e/web.e2e.mjs --spec 55-automation-edit`: 폼으로 만든 원문이 검사 응답(TOML 탭)과 파일에 같은지, 「저장만으로는 돌지 않음」을 thread 사건 규칙 저장 뒤 `ticker tick` 두 번(울타리) 뒤 기록 0건·안내 0건으로 본다. 충돌은 파일 직접 편집과 둘째 브라우저 저장 둘 다로 만들고, 거부 뒤 파일 글이 그대로인지·차이 줄을 본다. 지운 규칙은 a 편집기를 연 채 b가 지운 뒤 a 저장으로 되살아나지 않는다. 지금 실행은 inbox 항목·`manual:` 기록을 기다리고, 같은 key 둘째 호출 뒤 tick 울타리 뒤 안내가 늘지 않는 것을 본다. 시작 범위 미승인 거부는 실제 labA 머신이 있는 규칙으로 본다. 고정 sleep은 없다.
- 「한 초안」 단계는 TOML을 고친 뒤 debounce를 기다리지 않고 폼 탭에서 칸을 고쳐 저장하고, 파일에 두 쪽 고침과 머리 주석이 모두 남는지 본다(탭 전환이 다른 쪽 고침을 먼저 검사한다).
- 결함 심기: host 저장의 판 대조(사전 대조와 `file_rev` closure)를 빼면 `save_writes_only_the_seen_revision_and_never_resurrects_a_deleted_rule`이 「본 판이 아닌 저장」 단언에서 실패한다. 탭 전환의 먼저 검사를 빼면 E2E 55 「한 초안」이 실패한다(library/t-0538/w2-fault-replant).

## 운영 화면: thread 관리·「이 aaa」·저장소·알림함(t-0538 관리 GUI W4, plan §8.343)

- `cargo test -p aaa-host --lib project::tests::repo_edit_replaces_in_place_and_settings_view_reads_it`: 저장소 제자리 고치기의 차례·다른 칸 보존, 머신 빼기, 이미 있는 값·없는 값 거부, 설정 읽기의 파일 이름·숫자·저장소.
- `cargo test -p aaa-host --lib library_links::tests::prune_apply_deletes_only_the_previewed_candidates`: 미리 본 뒤 기한을 넘은 사본이 늘면(시각 주입) `conflict`로 하나도 지우지 않고, 같으면 본 것만 지운다.
- `cargo test -p aaa-host --lib db::flow_tests`: 흐름 사건 `tail`이 최근 N개를 오래된 것부터 준다.
- `cargo test -p aaa --test m13 resolve_keeps_dirty_worktree_and_unmerged_branch`: 실제 agent·git으로 `thread resolve --dry-run`이 미커밋 1·base 대비 2·병합 안 됨·남기기 권함을 보이고, thread 상태와 흐름 사건 수를 바꾸지 않는다.
- `node --test web/test/operateModel.test.mjs`: 동작 가능 판정(도는 세션·데려온 thread·준비 중·끝냄·정리 중·받은 brief), 끝내기 기본값과 요청, 저장소 인자·묶음, 숫자 범위, 알림함 배지와 「답할 것」으로 보낼 post.
- `node web/e2e/web.e2e.mjs --spec 56-operate`: 저장소 더하기·제자리 고치기(파일 글과 차례)·빼기 확인 창·할 일 저장소 목록, 「이 aaa」 값 저장(파일 글)과 범위 밖 숫자, 멈춤·재개 확인 창, 위험한 thread(미커밋·branch 커밋·사본에 빠진 symlink) 끝내기 창의 위험 줄과 남기기 켬, 끝낸 뒤 worktree·branch 남음, 깨끗한 thread(새 파일·big.bin을 치우고 사본이 다 온 뒤)의 남기기 끔과 worktree 지움, 다시 열기·Next 더하기·보내기·요약 고치기·세션을 끈 뒤 다시 시작, 알림함 단추·배지·패널 숨김과 CLI 조회·끝내기 보존(`seen` 무변경), 정리 미리보기, 휴대폰 390 폭, 같은 「바로 시작」 두 번 → thread 하나, Reader에 쓰기 단추 없음. 사본 상태·세션 상태·파일 글로 기다리며 고정 sleep은 없다.
- 결함 심기: library/t-0538/w4-fault-replant.

## host 계정 화면(t-0538 관리 GUI W5a, plan §8.351)

- `cargo test -p aaa-host --lib account_meta::`: Codex id_token의 메일·plan과 깨진 값, Claude 「valid for N」 만료와 `.claude.json` 메일(없으면 모름), 상태 판정(거부 > 만료 > 7일 안), 웹 조작 기기의 로그인·기본 허용과 보기 전용·세션 안·worker 거부, 보기 전용·세션 안 목록에서 메일·plan 가림, 끊김·곧 만료 알림 한 번(메일 없음)과 로그인으로 끝냄·계정 지우기로 정리, 바뀐 메일의 최초 시각.
- `cargo test -p aaa --test m13 accounts::`: 가짜 공식 CLI(`install_login_fixture`, `.claude.json`도 쓴다)로 로그인 → 메일·plan·날짜·사람이 읽는 목록, Claude 3일 → 곧 만료 알림, 같은 메일 다시 로그인은 최초 날짜 유지, 다른 메일 → `mismatch`(금고 그대로, 같은 별칭 로그인 거부) → `--keep`·`--switch`, 실험실 전체(host DB·fixture 밖)에 `@example.com`이 없음. 병렬 갱신 시험은 제공자가 거부한 갱신 뒤 계정이 `rejected`이고 알림함 `account` 항목이 하나인지 본다.
- `node --test web/test/accountsModel.test.mjs`: 1.79 판정, 별칭 꼴, 상태 낱말(남은 날 올림), 날짜, harness 묶음과 기본, 로그인 진행 글(host Mac 브라우저 안내·이 메일·다른 메일).
- `node web/e2e/web.e2e.mjs --spec 58-accounts`: PATH 앞 가짜 `claude`·`codex`가 FIFO에 쓸 때만 끝난다. 새 로그인 진행 → 메일·plan·날짜·정상, 기본으로·기본 해제(host 기본), 진행 중 취소(계정 없음), Claude 3일 → 곧 만료 배지와 알림함 항목(메일 없음), 다시 로그인의 「이 메일로 로그인하세요」 → 다른 메일 → 금고 그대로 → 「새 메일로 바꾸기」, Reader에 메일·로그인 단추 없음, 휴대폰 390 폭. 화면 표지(`data-state`)와 host 기록으로 기다리며 sleep은 없다.

## 웹 머신 등록·공개·보고서 올리기(t-0538 관리 GUI W5b, plan §8.355)

- `cargo test -p aaa-host --lib library_upload::`: 올리기 권한(Reader 거부)·경로(숨은 이름·`..`·절대·역슬래시)·올리기 이름·없는 프로젝트, 합계 64MiB(16MiB 넷은 되고 1바이트 더는 `too_large`, 같은 경로 다시 올리기는 그 크기를 빼고 셈), 사용자 보고서의 맨 위 index.html이 없으면 거부하고 올린 것을 둠 → 더 올린 뒤 등록하면 하위 폴더까지 보고서가 되고 올리기 폴더가 지워짐, 세션 안 agent·Reader의 등록 거부, 웹이 올린 수와 다르면(올린 자리가 없어도) `conflict`. `a_stale_upload_goes_away_but_the_one_being_written_stays`는 하루 넘은 올리기 둘(표지 `<올리기>.put`의 시각으로 만든다: 폴더 시각은 OS마다 달라 쓰지 않는다, plan §8.358) 가운데 이어 쓰는 것만 남고 그 표지 시각이 새로워지며 다음 올리기의 치우기도 그것을 남기는지, `an_upload_under_a_thread_numbered_state_path_has_no_origin_thread`는 상태 폴더 경로의 thread 번호를 근원으로 읽지 않는지 본다.
- `cargo test -p aaa-host --lib pairing_tests::the_web_keeps_its_last_controller_and_the_host_machine`: 웹에서 host 머신 폐기 거부, 조작 기기가 하나일 때 그 기기 폐기 거부(host CLI는 됨), 둘이면 웹 폐기 허용. `trust_management_requires_a_person_outside_the_session`은 조작 기기가 등록 창을 열고 Reader는 거부하는지 본다.
- `node --test web/test/adminModel.test.mjs`: 1.80 판정, 대소문자·NFC/NFD만 다른 이름 막기, 확인 코드(공백·하이픈), 등록 창 남은 분, 마지막 조작 기기(보기 기기는 세지 않음), 공개 주소 판정과 다른 경로(tailnet 이름의 http 주소는 내지 않음), 보고서 이름(host `name_ok`와 같음), 폴더 올리기 경로(맨 위 폴더 빼기·숨은 파일 빼기)와 상한 까닭, 살아 있는 링크, 올리기 이름.
- `node web/e2e/web.e2e.mjs --spec 60-admin`: PATH 앞 가짜 `tailscale`(serve 상태는 실험실 파일, 실제 tailscale을 부르지 않는다)과 같은 Mac에서 다른 설정·상태 폴더로 띄운 둘째 agent(host `agent_listen`을 loopback에 연다). 등록 창 열기 → `aaa agent enroll`이 찍은 확인 코드 → 대기 목록(enroll 사건) → 틀린 코드의 말 → 맞는 코드로 승인 → enroll 끝 코드 0 → 연결(state 사건) → 창 닫기, host 머신에 폐기 단추 없음, 폐기 확인 창 → 목록·host에서 빠짐, 마지막 조작 기기의 폐기 단추 꺼짐, 공개 켜기(확인 창)와 loopback 화면의 끄기(다른 경로만), 공개 주소로 연 화면(실험실은 TLS 공개 주소를 열 수 없어 `web_status` 답의 주소만 이 화면 origin으로 바꾼다)의 「이 화면이 끊길 수 있음」과 확인 전 끄기 꺼짐, 폴더 올리기(맨 위 폴더·숨은 파일 빠짐, 사용자 보고서·제목, 올리기 폴더 지움)와 여러 파일(index.html 없는 사용자 보고서 막음 → 작업 보고), 키 교체 확인 창의 살아 있는 링크 수와 옛 링크가 열리지 않음, Reader에 등록·폐기·공개·올리기·키 교체 없음, 휴대폰 390 폭과 이 기기 폐기 확인 → pairing 화면. 화면 표지·host 기록·프로세스 출력과 끝으로 기다리며 sleep은 없다.
- 이 단계 ci-local에서 E2E 53(t-0540)의 「알림함 배지가 답할 요청을 세지 않음」이 5 → 6으로 실패했다. 실험실 DB를 보니 앞 단계 보고의 「report needs user」 항목이 요청 post 16ms 전에 들어와 앞뒤 배지 대조 사이에 끼었다. 대조를 「배지 = host의 끝내지 않은 항목 − 요청 1」로 바꿨다(늦게 온 항목은 둘을 함께 바꾼다). 배지가 요청을 세게 결함을 심으면 이 단계가 실패한다.

## 웹 profile·safety·작업 폴더·hook(t-0538 관리 GUI W5c, plan §8.357)

- `cargo test -p aaa-host --lib settings_admin::`: 웹이 바꾸는 safety key는 넷뿐(`*_agent_args`·`routine_commands`는 `forbidden`, 파일 그대로), 프로젝트 표·모든 프로젝트 표(`[safety.default]`)에 쓰고 「기본」은 그 key를 지움, 앞 판(`expected`)으로 보내면 `conflict`이고 파일 그대로, 주석 보존. custom profile 만들기·고치기·harness를 custom으로·지우기 거부, 목록 JSON에 env 값과 custom 실행 명령이 없음, 미리보기는 쓰지 않고 적용 뒤 실행 인자가 미리보기와 같음, 낡은 판 적용은 `conflict`. 허용 목록·기본값은 없는 이름 거부, 적용 뒤 host의 thread 허용·coordinator 기본이 바뀜. 세션 안 agent·worker·detached의 바꾸기 다섯 가지 거부와 Reader의 읽기 거부. 작업 root는 `~`·상대·`C:work`를 agent에 닿기 전 거부. `profiles_from_host_local_toml_stay_out_of_web_edits`는 host.local.toml의 custom 이름으로 더하기·local이 effort를 덮는 profile을 shell로 고치기·지우기를 거부하고 파일과 설정 읽기가 그대로인지, `global_edits_warn_when_the_project_table_wins_and_rows_tell_the_value_below`는 프로젝트 표가 이기는 모든 프로젝트 기본 고치기의 경고와 「기본」으로 yolo가 켜지는 `below`(host.local.toml이 같은 key를 정하면 그대로)를, `toml_errors_do_not_carry_the_broken_line`은 깨진 줄·형식(타입) 오류의 env 값이 오류 글에 없고 줄 첫 칸 오류도 그 줄 번호인지, `a_host_without_a_project_still_edits_global_safety_and_profiles`는 프로젝트 없는 host를 본다.
- `cargo test -p aaa-proto control_classification`: 설정 관리 읽기도 조작 권한.
- `node --test web/test/settingsModel.test.mjs`: 1.81 판정, yolo 켜기만 한 번 더 확인(아래 층이 on인 「기본」 포함), 고치기는 고친 칸만, 인자 줄마다 하나(빈칸 든 인자), 기본 profile 고치기는 같은 이름 더하기, harness를 바꾸면 받지 않는 model·effort 비움, profile 이름(host `validate_name`과 같음), 절대 경로(host `absolute`와 같음), 전환은 점검한 그 경로·mount일 때만.
- `node web/e2e/web.e2e.mjs --spec 61-settings-admin`: 실험실 `CODEX_HOME`(사람이 둔 다른 hook 하나). 「안전」에서 넷만 바꾸는 칸(인자·명령 목록은 「host 터미널에서」), start_threads auto → `aaa safety show` 값·출처, yolo 켜기 확인 창 → host.toml, 실험실 밖에서 host.toml을 고친 뒤 바꾸면 「그사이」 알림이고 파일 그대로 → 다시 바꾸면 사람이 고친 줄을 두고 씀. 「profile」의 custom `fake`에 단추·실행 명령 없음, 더하기 창(harness에 custom 없음) → 미리보기의 실행 인자(빈칸 든 인자 포함) → 미리보기만으로는 파일 그대로 → 적용 → `aaa profile resolve`. 허용 목록(실험실 기본 fake → 이 프로젝트는 e2e-opus만) → `aaa safety show`. 「머신」 작업 폴더: 상대 경로는 점검 꺼짐, 점검 전 전환 꺼짐, 「검사 파일을 만듭니다」 확인 → 없는 폴더를 만들고 통과, 점검 뒤 경로를 바꾸면 전환 꺼짐, 전환 → `agent.local.toml`에 새 경로·재시작 필요 표시, host.toml 그대로. Codex hook 설치 → 설치됨·aaa hook 세 사건·다른 hook 그대로 → 제거 → 없음·다른 hook 그대로. Reader에 안전·작업 폴더 없음, 휴대폰 390 가로 넘침 없음. 화면 표지·파일·host 답으로 끝내며 sleep은 없다.

## 웹 배포·재시작(t-0538 관리 GUI W6a, plan §8.361)

- `cargo test -p aaa-host --lib rollout::web_tests`: 웹 조작 기기의 `deploy_start`는 재시작만(교체는 `forbidden`), 재시작은 권한을 지나 `deploy:<id>`에 프로젝트·`web`·시각을 남기고 같은 ID 재전송은 처음 시각 그대로·다른 작업이면 `bad_request`. 세션 안 coordinator·worker·Reader의 `rollout_plan` 거부, 조작 기기는 상대 경로 `bad_request` → 끊긴 빌드 머신 `agent_disconnected`, 웹 rollout에 빌드 머신 없는 후보는 `bad_request`. 사람·웹 기기 actor는 coordinator_deploy가 꺼져도 승인 유지, coordinator·빈 `device:`는 아님. `rollouts`는 이 프로젝트만 새것부터, 조작 기기의 같은 ID 조회는 요약(actor·후보 경로·오류 원문·작업 ID 없음), Reader·다른 프로젝트 거부. 진행 중 rollout이 있으면 점검 거부, 같은 ID 재전송은 빌드 머신이 끊겨 있어도 기존 요약. `deploy_jobs`는 새것부터·옛 기록(시각 없음)은 맨 뒤·다른 프로젝트 빠짐·끊긴 머신은 상태 없음·coordinator 교체 작업은 종류만(후보 경로 없음), 작업 끝 `deployment_changed`가 `rollout` 사건을 보냄, 실패 영수증은 고정 까닭만(PID·결과 원문 없음). 끊긴 머신(기능만 남음)이라 agent에 닿기 전에 끝나며 시간을 기다리지 않는다. 시험 몸은 따로 만든 runtime에서 돌리고, runtime을 끝내 `agent_connected`가 띄운 task(한 번도 돌지 않은 `rewatch`)가 쥔 Host가 닫혔는지 본 뒤 상태 폴더를 지운다(Windows는 열린 host DB가 든 폴더를 지우지 못한다, os error 32).
- `cargo test -p aaa-agent --lib candidate_inspection`: 대기 중인 배포 작업이 있으면 점검 거부(거절 글에 작업 ID 없음, 실행기 staging 이름이 겹친다)와 점검 중에 들어온 작업 거부. 후보 읽기가 provenance의 SHA·커밋·빌드 시각과 dry-run 판을 답하고, 판의 커밋 표시가 다르거나 커밋이 40자리가 아니면 거부, 실행기 실패는 고정 글(경로·후보 출력 없음), provenance와 다른 파일은 실행하지 않고 거부, `aaa`가 아닌 이름 거부. 가짜 실행기를 쓴다.
- `node --test web/test/deployModel.test.mjs`: 1.82 판정, ID가 host `valid_id`(영숫자 26자), 후보 경로 형식, 대상은 host 맨 앞(다른 머신을 모두 풀면 host만), 확정 거절일 때만 보인 ID를 지움, 시작은 점검한 그 후보·대상이 모두 준비되고 진행 중 rollout이 없을 때만, 보낼 rollout의 SHA·커밋은 점검 결과 그대로.
- `node web/e2e/web.e2e.mjs --spec 63-deploy`: 실험실 집(`HOME`)과 PATH 앞 가짜 `launchctl`(재시작이 이 Mac의 실제 aaa 서비스에 닿지 않는다, 감독자 plist도 실험실 집). 후보는 허용 표지가 있을 때만 self-check를 통과하는 셸 파일과 provenance.json이다. provenance SHA가 다르면 점검 오류·시작 꺼짐 → 맞으면 SHA 전체·판·대상 「labA (host 먼저) · 준비됨」·시작 열림, 점검 뒤 경로를 바꾸면 시작 꺼짐. 표지를 지운 뒤 확인 창(SHA) → rollout ID 표시 → 기록에 `stopped`(대상 전송의 dry-run에서 멈춤), 실험실 실행 파일 SHA 그대로·launchctl 호출 없음. 화면을 다시 열면 같은 ID의 카드(「배포 중단」, 끝난 단계는 전송뿐, 후보 경로 없음). agent 재시작 확인 창 → 작업 「완료」·가짜 launchctl의 `kickstart -k gui/<uid>/dev.aaa.agent`. Reader는 기록·작업만 보고 시작·재시작 단추 없음, 휴대폰 390 가로 넘침 없음. 화면 표지·파일·host 사건으로 끝내며 sleep은 없다.

## 안전 모드(t-0538 관리 GUI W6b, plan §8.370)

- `cargo test -p aaa-host --lib safe_mode::`: 원문 가림(env 표·인라인 env·비밀처럼 보이는 key·표 머리의 비밀 낱말·webhook·그런 낱말이 든 값·주석으로 꺼 둔 줄, 줄 수 그대로, 다른 한 줄 값과 표 머리는 남음, 가린 값은 오류 글에서 뺄 목록에). 여러 줄 값의 이어지는 줄은 모두 가림: 닫히지 않은 문자열은 끝까지, 줄마다 하나씩 쓴 인자·배열 안 `["…"]`, 배열이 닫힌 뒤는 다시 보임, `]`를 빠뜨린 배열은 뒤의 표 머리(공백 없이 주석이 붙어도)에서 끝나 그 env 값을 가림, 주석으로 꺼 둔 비밀 배열·여러 줄 문자열은 이어지는 주석 줄을 가리고(사이 빈 줄은 건넘) 주석이 끝난 바로 뒤 값·표는 보임, 비밀이 아닌 배열의 원소는 화면에서만 가리고 오류 글에서 지울 목록에 넣지 않음(닫히지 않은 문자열 안에 표 머리가 나온 뒤의 값은 넣음). 마지막 정상 설정은 설정 폴더 `last-good/`에 둔다(상태 폴더에 env·인증 값이 남지 않는 것은 m13 `secret_k0`·`accounts`가 본다). 오류 글은 TOML 원문 조각(`|` 줄)과 큰따옴표 값·가린 값을 뺌. 정상 설정 보관(폴더 0700·파일 0600, 시작 뒤 깨진 파일은 보관하지 않고 지난 보관본 유지)과 되돌리기(지금 파일을 `.broken-<UTC>`로 남김, 보관 때 없던 local은 남긴 뒤 치움, 같은 이름의 남긴 파일을 덮지 않고 그때는 먼저 만든 남기기도 치우고 아무것도 바꾸지 않음, 바뀐 게 없으면 남기지 않음, 보관이 없으면 거부). 뜰 주소는 마지막으로 연 주소(실제 `web::start`가 적음) → 보관 설정의 listen, 둘 다 없으면(업그레이드 직후) 짐작하지 않음, 웹을 끈 설정으로 시작하면 없음. 조작 기기만 오류 줄 표시·가린 원문·되돌리기(Reader·짝 없음·모르는 token은 안내만), API는 503 `unavailable`, 다른 Host 거부. 되돌리기·다시 읽기는 조작 기기와 이 origin만(다른 Origin·Origin 없음·Reader·짝 없음은 파일을 건드리지 않음), 아직 깨져 있으면 안전 모드에 남고, 되돌리면 남긴 파일·감사 기록과 함께 끝을 알린다. 실험실 DB와 router를 바로 부르며 시간을 기다리지 않는다.
- `node web/e2e/web.e2e.mjs --spec 68-safe-mode`: Reader·휴대폰 기기를 먼저 짝짓고 host를 한 번 다시 띄워 고정 port 설정이 보관되는 것을 본다. host.toml 끝에 env 값과 닫히지 않은 표를 붙이고 다시 띄우면 같은 주소의 `/api/v1/me`가 503 `unavailable`(「안전 모드」) → 조작 기기 화면에 `host.toml:<줄>:` 오류·그 줄 표시·`E2E_LABEL = «가림»`, 페이지에 env 값 없음. Reader는 안내만(원문·되돌리기 없음), 휴대폰 390 가로 넘침 없음. 「마지막 정상 설정으로 되돌리기」 → 안내 화면, host.toml이 보관 설정 그대로·깨진 파일이 `host.toml.broken-*` 하나로 남음 → 안내 화면이 스스로 다시 열어 정상 앱(t1 pane)이 뜨고 labA가 다시 붙으며, 안전 모드를 연 host 프로세스가 그대로 정상 host가 된다. 파일·HTTP 답·화면 표지로 끝내며 sleep은 없다.

## 대화의 도구 호출 묶음(t-0477, plan §8.304)

- `node --test web/test/toolGroups.test.mjs`: Claude·Codex 도구 이름 분류, 요약 문구(두 가지까지·우선순위·서로 다른 수정 파일·생각), 이어진 도구·생각 줄 묶기, 앞 쪽이 묶음 앞에 붙은 뒤에도 옛 첫 줄 id로 같은 묶음과 펼침을 찾는지 본다.
- `node web/e2e/web.e2e.mjs --spec 41-chat-tool-groups`: 실제 lab host/agent/Claude JSONL에서 데스크톱 Chromium 1280·패드 WebKit 820·휴대폰 WebKit 390을 열고 묶음 요약·접힌 동안 보이는 도구 줄·펼친 줄을 단언한다. 끝이 아닌 곳에서 펼치고 접은 뒤 머리 좌표가 1px 안에서 같고, 끝에서 펼친 뒤에도 끝인지 DOM·animation frame 사건으로 본다. 스크린샷 폴더를 주면 기기마다 끝·첫 묶음·펼침을 찍는다(전후 캡처). 고치기 전 코드로는 묶음 요약이 비어 실패했다. 둘째 단계는 실험실 `claude`(harness `claudeSh`)가 SessionStart hook으로 기록을 알리고 `pane run` 한 줄마다 작업 화면·입력 상자를 바꾸며, 사이드바 `data-status` 사건을 울타리로 맨 끝 묶음의 글과 `is-live`를 본다. 작업 중 판정을 끄면 「작업 중 마지막 도구 줄」에서 실패한다.

## 웹 할 일 보기(t-0480, plan §8.306)

- `cargo test -p aaa-host --lib tasks::`: 운영에서 나온 오독 꼴 셋(같은 꼴의 가짜 글)이 owner가 아니라 `problems`로 나오고 기존 `lines_owners_notes` 기대가 그대로인지, 메모만 바뀌면 그 task의 `block_sha256`만 바뀌는지 본다. 오독 판정을 끄면 `misread_owner_shapes_become_problems`가 실패한다.
- `cargo test -p aaa --test m13 task_board_reads_tasks_and_follows_file_edits`: `aaa task list`의 목록·형식 어긋남·task 없는 열린 thread, worker 세션(보기 권한) 읽기, `--from-task` 거부 까닭, 「나중」 경로 거부·폴더 밖 symlink·없는 파일. 「고치지 않은 tick은 사건이 없다」는 sleep 없이 본다: 원본 목록 읽기 → 사건 연결 → 초기 파일 쓰기 → tick 뒤 초기 sha 사건의 위치를 기억한다. 그 위치 다음에서 고치지 않은 tick → 파일 고침 → tick 뒤 첫 `tasks` 사건이 고친 sha인지 확인한다. 초기 사건을 다시 읽는 경쟁을 재현하며, 수정 사건을 sha로 걸러 기다리지 않아 불필요한 사건도 잡는다.
- `node web/e2e/web.e2e.mjs --spec 43-tasks`: 막대 단추로 열기, 행 상태·배지·상세·thread 칩 이동, 파일 직접 편집 + `ticker tick` 뒤 `data-tasks-sha` 갱신, 거르기·검색, Reader, 휴대폰 전체 화면 시트와 Back, 패드 세로 시트·가로 두 칸, 「나중」 탭 있음·없음·「없음」. 가짜 thread가 곧 턴을 끝내 앱 안 알림이 도구줄을 덮으면 Playwright가 알림이 시간으로 사라지기를 기다리므로(6초), 첫 단계에서 그 기기의 「앱 안에서」 알림을 끈다.

## 웹 할 일 편집(t-0482, plan §8.311)

- `cargo test -p aaa-host --lib task_edit::`: 순수 함수 `apply`. 바꾼 블록 밖 바이트(CRLF 포함) 보존, 대상 블록만 충돌(다른 task 변경은 통과, 지운 task 고치기는 충돌), 저장 검사(`)` 끝 제목·`· t-` 끝·task 꼴 메모·중복 제목), 하위 묶음째 지우기·옮기기, 목록 동작, 되살리기, 파일 직접 차이 기록, 한 요청 안 고치고 옮기기(hash 따라가기), 원문 저장 홀로. 대상을 블록 hash 대신 제목으로 찾게 하면 `block_anchor_conflicts_only_on_the_target`·`update_then_move_in_one_request`가 실패한다.
- `cargo test -p aaa-host --lib file_rev::`·`tasks::tests::suggestions_for_misread_lines`: 공용 저장 helper(판 대조·다시 읽기), 정리 제안 후보.
- `cargo test -p aaa --test m13 task_edit_keeps_bytes_conflicts_and_logs`: CLI 더하기의 바이트와 `tasks` 사건, `--block` 충돌, 지운 task 고치기 충돌, 쓰기에 묻힐 뻔한 직접 편집 기록, 되살리기 한 번, 저장 검사, 다음 tick의 「파일 직접」 기록, 권한(worker 세션 보기 권한 거부, 확인되지 않은 셸·확인된 사람 셸 pane은 「그 프로젝트 coordinator만」, coordinator pane 허용). 확인된 pane을 허용하게 느슨히 하면 실패한다.
- `cargo test -p aaa --test m13 task_memory_forgets_deleted_project`: 지운 프로젝트를 같은 이름으로 다시 만들면 첫 tick은 기억만 하고 「파일 직접」 기록을 지어내지 않는다(지울 때 기억을 남기면 실패).
- `node --test web/test/tasksModel.test.mjs`: 메뉴·끌기 → host 동작, 폼 → 동작(상위는 고른 판의 블록 고정점, owner 목록 전 폼은 동작 없음), 「나중」 항목, 기록 차이.
- `node web/e2e/web.e2e.mjs --spec 46-task-edit`: 폼으로 더하기·고치고 옮기기(파일 글 전체를 기대값과 바이트로 비교), 사람 이름 owner 거부, 정리 제안, 행 메뉴·끌기·Alt+↓·목록 이름 바꾸기·빈 목록 지우기·새 목록, 두 브라우저 충돌 창과 「최신으로 다시 열기」, 폼을 고치는 동안 다른 task가 파일에서 바뀌어도 저장됨(대상만 충돌), 지우기·되살리기 줄·기록 거르기·기록에서 되살리기, 저장만으로 thread가 시작되지 않음(`ticker tick` 울타리 뒤 흐름 사건·thread 수), 보기 권한 기기에 편집 UI 없음(host 판 판정이 끝난 `data-can-edit="false"` 뒤에 본다. 판정 전은 `pending`), 명령 팔레트 「할 일 더하기」(할 일 화면을 연 적 없는 기기), 휴대폰 「+」·전체 화면 폼·메뉴 시트, 패드 세로 오른쪽 시트(상자 좌표로 단언), 「나중」 할 일로 옮기기(「나중」 파일 그대로), 원문 편집의 판 대조. 저장이 끝났는지는 시간이 아니라 화면 `data-tasks-sha`가 새 판이 되고 `data-task-saving`이 없어진 뒤 그 판이 파일 sha와 같은지로 본다.
- 같은 E2E 46의 「터치 입력 글자」는 Chromium·WebKit의 iPhone 세로/가로에서 검색·제목·메모·선택·새 목록·사람 이름·첨부 붙여넣기·원문·목록 이름의 실제 계산 글자가 16px 이상인지 본다(t-0530). 폼의 13px 덮어쓰기를 복원하면 실패한다. headless는 iOS 키보드의 초점 확대를 재현하지 않으므로 실제 iPhone에서 입력·키보드 내림 뒤 배율 유지 확인은 별도다.
- 결함 심기: 웹 폼이 편집을 시작한 판 대신 최신 판 hash로 저장하게 하면 E2E 46 「충돌」 단계가, 팔레트 요청 수를 mount 때 값으로 두면 「명령 팔레트」 단계가 실패한다. 웹 결함은 `npm run build`가 dist를 다시 만든 뒤 `cargo build -p aaa`로 binary에 넣어 돈다.

## 일을 맡기기 전 task 찾기(t-0539, plan §8.336)

- `cargo test -p aaa-host --lib tasks::tests::start_warning`: 제목의 대소문자·구두점 정규화, 낱말 겹침 경계, 첫 열린 후보 선택, 완료·빈 제목·약한 겹침 제외와 제안 명령의 셸 인용을 확인한다. 형태소·동의어는 추측하지 않는다. 완료 task 제외를 제거하면 음성 시험이 실패한다.
- m13 `thread_start_warns_about_similar_tasks_without_blocking`: 실제 CLI 응답에서 경고 한 줄과 시작 성공, `--from-task` 시작·완료 task에 경고 없음, 기존 task 보존과 직접 시작의 별도 할 일 생성·자동 연결을 확인한다. 명령 응답이 끝이라는 신호이며 고정 sleep은 없다.
- 기존 m13 `doctor_files_brings_the_coordinator_guide_to_existing_projects`의 안내 문구 목록이 새 프로젝트 생성과 낡은 안내 갱신 모두에서 사전 검색·기존 thread 재사용·메모/모델/머신/저장소 갱신·`--link-task` 안내를 검사한다.

## 웹 할 일 thread 연결과 자동 정리(t-0493, plan §8.316)

- `cargo test -p aaa-host --lib task_clean::`: P2 판정표(병합은 표지와 무관하게 완료, `[완료]`만 완료, 그 밖 표지·보고 없음·사본 없이 끝냄·하위 남음은 확인 필요)와 확인 필요 까닭.
- `cargo test -p aaa --test m13 task_thread_link_and_auto_clean`: `--link-task` 줄 끝 번호, 열린 동안 판정 없음, `[완료]` 지움과 「host 자동 정리」 기록, 보고 없음·`[결정 필요]` 확인 필요(`task list`·`context`·inbox), 상위 올림, 되살린 task를 다음 tick이 다시 지우지 않음(한 번 판정 거르기를 없애면 실패). 판정은 `ticker tick` 울타리 뒤에 본다.
- `cargo test -p aaa --test m13 merged_thread_cleans_its_task`: 가짜 gh로 병합한 thread는 보고가 `[착지]`여도 그 task를 지운다(병합 판정을 끄면 실패). 병합 끝내기와 판정은 같은 tick에서 돌아 `thread_resolved` 사건 뒤 `ticker tick` 하나가 울타리다.
- `node web/e2e/web.e2e.mjs --spec 47-task-thread`: 바로 시작 확인창(`data-start-state`)과 줄 끝 연결, 확인창을 연 뒤 블록이 바뀌면 시작 거부, 자동 정리 줄·되살리기, 확인 필요 단추, 상위 올림, task 없는 thread 잇기·만들기, coordinator 맡기기(입력칸 초안을 localStorage로 확인), 휴대폰·패드 한도 확인.

`task_clean::tests::parent_attention_is_published_after_the_verdict_is_saved`는 실제 thread 기록 잠금으로 마지막
판정 저장만 보류한다. 하위 삭제·상위 연결의 파일 사건을 받고 같은 hash의 board를 읽으면 아직 확인 필요가 없다.
잠금을 풀어 판정을 저장한 뒤에는 같은 파일 hash의 `tasks` 사건이 다시 와야 하고, 그 board의 상위는 확인 필요다.
판정 저장 뒤 사건을 빼면 사건 누락 단언에서 실패한다. 시간 지연 주입·sleep·화면 재시도는 쓰지 않는다.

## 파일 한도 시험 실패 진단(t-0573)

`cargo test -p aaa --test resource_limits --locked`는 자식 host의 한도만 낮춘다. hard64 시험의 Front EMFILE
관측이 실패하면 그 실행의 host 로그, 단발 Front 요청의 stdout·stderr, 실제 host fd 목록과 두 PID의 상태를 남긴다.
Front 요청이 이미 성공했는지, 연결 전에 실패했는지, 여전히 기다리는지 구분할 근거이며 원인을 미리 단정하지 않는다.
원래 15초 상한과 EMFILE·복구·경고 한 건 단언을 유지한다. 실패한 실험실 폴더는 기존 Lab 정책대로 보존한다.

## 키 관리자 K0 카나리 찾기(§8.307)

- `cargo test -p aaa --test m13 secret_k0`: 실험실 host·agent를 `AAA_LOG=trace`로 띄운다. 무작위 가짜 값(카나리)을 셋 둔다.
  - A·C: host.toml profile env. 웹 새 세션처럼 `pane_run --profile`로 띄운다. C에는 `/`·`"`·`+`·공백·`&`를 넣어 URL·JSON escape 꼴이 원문과 달라지게 한다.
  - B: thread launch env. `thread start --profile`로 띄운다.
  - 받은 pane은 값의 sha256만 실험실 상태 밖 파일에 쓴다. 시험은 그 hash로 값이 실제로 갔는지 본다(가림 때문에 전달이 끊긴 것을 통과로 착각하지 않으려는 장치다).
- 찾는 꼴: 원문, 무작위 부분의 8자 조각, base64(표준·URL, 앞 0~2바이트 정렬마다 끝 4자를 뺀 가운데), URL 인코딩, JSON escape.
- 찾는 곳:
  - A·C: host DB 파일·WAL·shm 전체.
  - A·B·C: host·agent stderr 로그, `project delete --force`의 `aaa-records.json`, 감사 기록 `pane_run` 행, 상태 폴더 전체.
  - 상태 폴더에서 빼는 K2 범위: agent `sessions/`·`thread-preparations/`. host DB의 B도 뺀다(`threads.data`).
- 기록의 모양도 본다: 감사 기록·export의 env는 이름 목록이다. 평문 경고는 host 로그에 이름으로 나온다.
- 마지막으로 pane 안 `printenv`에 값이 보이고 `ulimit -c`가 0인지 본다. 이 출력은 pane 화면 저장물(K2)에 남으므로, 그 뒤에는 host DB만 다시 찾는다. agent는 core soft 한도를 hard까지 연 채 띄운다. 그래야 끄기를 빼면 이 단계가 실패한다.
- 모두 출력·파일 사건으로 기다리며 고정 sleep은 없다.

## 공개 검사 파일명·크기 경계(t-0483 후속)

- `python3 -m unittest scripts.test_check_public`: 실제 check-public CLI를 임시 Git 저장소에서 실행한다. 1MiB 경계·초과, 공백·개행·마지막 파일의 금지 확장자, 현재 트리에서는 지워졌지만 이력에는 남은 금지 확장자를 검사한다. 정상 현재/이력 입력도 함께 통과해야 한다. 모든 키 fixture는 빈 파일이다.
- 크기 예외는 정확한 경로와 상한을 지킨다. plan 2MiB와 버전 고정 Bold·Regular의 기존 바이트 상한까지 허용하고, 각각 1바이트 초과 및 다른 문서·폰트는 차단한다. 해당 세 파일은 삭제·축소하지 않는다.
- 기존 NUL 변수 경유로 돌아가면 세 keeper가 실패하고, 예외 상한을 없애면 크기 keeper가 실패해야 한다. 바이트 복원 뒤 ci-local 전체·check-public을 실행한다. ci-local의 Unix test 단계가 기존 배포 빌드 계약 시험과 함께 실행한다.

## 대화 정리(compaction) 중 감지·보류·표시(t-0479, plan §8.309)

- agent 감지는 실제 화면 fixture(`crates/aaa-agent/src/detect/fixtures/*compact*.txt`, 10-20 Claude 2.1.304·Codex 0.163 `/compact` 실측)로 본다: `cargo test -p aaa-agent --lib detect`. 「(0s)」 Claude 화면은 참고 규칙으로 쉼이라 덧댐이 없으면 시험이 실패한다(결함 심기로 확인).
- `node web/e2e/web.e2e.mjs --spec 45-compacting`: 가짜 Claude가 F7(작업 중)·F5(정리 시작)·F6(정리 끝)을 `aaa pane send-keys`로 받아 실제 정리 화면(spinner 「Compacting conversation… (3s)」, 참고 규칙으로는 쉼)과 실제 기록 차례(compact_boundary → isCompactSummary → 대기열 dequeue)를 그린다. 상태 전이는 가짜가 그린 「mode …」를 `waitOutput`으로, 화면은 `domIs`로 기다린다. 「보내지 않았다」는 보관 직후 `sentFrames`(그 사건 안에서 끝나는 보관)와 pane이 받은 bytes로 본다. 휴대폰·패드는 WebKit `pairDevice`. 같은 브라우저 탭 둘은 같은 context의 page 둘이다: 「한 번만 갔다」는 그 글을 품은 사람 말이 꼭 하나인지(두 탭이 함께 붙여 넣으면 한 말로 붙는다)와, 다른 탭이 다음 글을 보낸 뒤에도 그대로인지(입력창은 앞 글이 보내질 때까지 잠긴다)로 본다. 고정 sleep은 없다(넘겨받기 전 다른 탭에 묻는 500ms는 제품의 기다림이고, 시험은 기록에 오는 것을 기다린다).
- 웹 단위 `node --test web/test/messageQueue.test.mjs`: 정리 뒤 보낼 글의 저장 왕복(넣은 탭 `by`), 표지 떼기·넘겨받기, 정리 뒤 차례(`compactionStep`: 넣은 탭만 보냄, 앞의 보통 글을 앞지르지 않음, 자동 전송을 껐으면 표지를 뗌).
- 결함 심기로 확인한 것: agent 덧댐 끔(detect 시험 둘 실패), 옛 host에 compacting 그대로 보냄(`compaction_goes_to_an_old_host_as_working` 실패), host 표지 안 씀, agent가 청하지 않아도 정리 줄을 냄, 웹 보류 없음·「지금 보내기」 정리 중 켬·정리 중 줄 없음·정리됨 구분선 없음·끝나도 안 보냄·탭 주인 무시(「두 탭 하나두 탭 하나」)·넘겨받기 없음(각각 E2E 45 실패). Codex 답 글 오탐·긴 초안에 밀린 표지는 detect 시험이 고치기 전에 실패했다. 웹 결함은 `npm run build`(tsc 포함)가 실패하면 dist가 그대로라 옛 번들로 돈다: 심은 뒤 binary의 `index-*.js` 이름이 바뀌었는지 먼저 본다.


## PR 추적 gh 실행·인증·회복(t-0487)

- host는 thread 머신 agent에 `PrQuery`를 보낸다. CI는 별도로 정한 조회 머신 agent에 `CiQuery`를 보낸다. coordinator 셸의 로그인은 다른 머신의 인증을 증명하지 않는다.
- PR·CI는 agent PATH에서 실행 가능한 gh를 절대 경로로 찾고, macOS에서는 PATH에 없을 때 Homebrew 표준 경로를 본다. 사용자 PATH가 우선이다. URL·`--repo`·API 경로로 대상을 지정하고 gh의 cwd는 실행 파일이 있는 볼륨의 root로 둔다. origin은 계속 원래 저장소에서 읽으며, 이미 사라진 저장소는 별도 폴더 오류다.
- 인증 파일·토큰은 읽거나 복사하지 않고 gh의 기존 HOME·GH_CONFIG_DIR·XDG_CONFIG_HOME·토큰 환경을 그대로 전달한다. gh 종료 코드 4는 인증 실패로 안내한다. GH_TOKEN은 기존 gh 로그인을 대신할 수 있는 선택지다. 실패 stderr 원문은 inbox·로그로 넘기지 않는다. 운영 설정·서비스·키체인을 자동으로 바꾸지 않는다.
- PR outage는 한 pass의 모든 thread 조회가 끝난 뒤 한 번 판정한다. 실패가 남아 있으면 다른 thread의 성공으로 회복하지 않으며, 실패 안내에는 thread와 조회 머신을 붙인다. 실제 조회가 하나도 없으면 회복을 만들지 않는다. 재시도·시간 상한 변경은 없다.
- `cargo test -p aaa --test m13 pr_environment_failure`: 실제 host↔agent↔가짜 gh에서 한 thread 인증 실패와 다른 thread 성공, 실패를 포함한 뒤 pass의 시작을 울타리로 한 회복 없음, 인증 진단·stderr 비노출·설정 경로 전달·gh cwd, 인증 조건 해제 뒤 회복 한 번, worktree가 사라졌을 때 폴더 진단을 확인한다. 고정 sleep 없이 호출 기록·flow로 끝낸다. 기존 PR·CI 시험이 상태 변경·병합·HTTP 응답 처리를 계속 맡는다.
- `cargo test -p aaa-agent --lib gh::tests::`: 사용자 PATH 우선·짧은 서비스 PATH의 fallback·실행 파일 없음은 실제 임시 파일의 실행 권한으로 판정한다. 별도 실행 프로세스나 운영 gh 로그인은 필요 없다.
- 수정 전 두 thread 시험에서 실패와 회복이 교대로 기록되는 RED를 보존했다. 거짓 회복·인증 코드 오분류·worktree cwd 의존·fallback 제거를 각각 심어 keeper의 의도한 단언 RED를 확인하고 소스 bytes를 복구했다. 최종 gate는 전체 `bash scripts/ci-local.sh`와 `bash scripts/check-public.sh`다.
- 과거 세 사건의 저장 오류에는 조회 머신·thread·환경 snapshot이 없었다. 인증 안내/ENOENT까지 구분할 수 있지만 당시 HOME·PATH·삭제 경쟁 중 하나를 확정할 수는 없다. gh 자체의 빈 설정 PR 조회는 종료 코드 4, PATH에서 gh가 없거나 spawn cwd가 사라졌을 때는 ENOENT로 재현했다.

## aaa install I0 계약(t-0485, plan §8.312)

- `cargo test -p aaa-config --lib install::tests`: 질문과 자동화 DTO의 같은 순수 검증기·고정 계획/hash,
  role별 필수/상충·OS 서비스·한국어 Windows 경로·출력/비밀 입력 모드·허용 env와 오류의 값 비노출을 검사한다.
  파일·HOME·프로세스·포트·OS 설정을 사용하지 않는다. I1 이후의 실제 탐지/CLI/설치 증거와 구분한다.
- `cargo test -p aaa-proto --lib install::tests`: 공개/비공개 fixture의 typed 왕복, 모르는 key/schema 거부,
  status/exit와 생성 schema drift를 검사한다. TS drift는 기존 `typescript_bindings_are_current`가 소유한다.
- `docs/install/agent-input.json` → `agent-plan.json`/`agent-plan.sha256`은 공개된 canonical JSON/SHA-256 계약의
  독립 fixture다. 해시는 compact JSON의 key를 재귀 정렬하고 UTF-8/BOM·끝 LF 없이 계산한다.
  테스트 실행 중 owner가 기대 hash를 갱신하지 않는다.
- SSH arch는 문자열 fixture `x86_64`의 역직렬화/직렬화를 검사한다. enum의 kebab-case 자동 변환으로
  `x86-64`가 되던 실제 계약 결함은 이 fixture가 수정 전 RED, 명시 snake_case 수정 뒤 GREEN이다.
- 새 보안/hash 검사는 허용 env 거부 제거, 대상 context를 hash에서 제외, 입력의 모르는 필드 허용을 각각 심어
  해당 owner 시험의 단언 실패를 본다. 각 변형은 원본 bytes로 복원한 뒤 다음 시험/전수 gate를 실행한다.
- I0는 승인서/등록 허가의 schema만 만든다. 서명·pin·nonce·W0 executor·일회 grant·실제 감독자/SSH 수명은
  후속 단계의 구현과 시험 전까지 통과했다고 표시하지 않는다. 가짜 fixture 서명은 승인으로 쓸 수 없다.

## PR 추적의 선택 실행과 CI 장애 분리(t-0492, plan §8.313)

- `cargo test -p aaa --test m13 pr_environment::`: 실제 host↔agent↔가짜 gh에서 미게시 branch의 호출 0, 게시 branch 탐색, 미게시 branch의 명시한 보고 PR 추적, 미인증의 inbox 0·머신 상태·자동 사용 재개, 실행 파일 ENOENT의 조용한 생략, 일반 API 오류의 장애·회복 경계를 본다. 앞의 미게시 thread가 사용 불가 상태 때문에 뒤의 인증 회복 조회를 막지 않아야 한다. 호출 기록의 뒤 pass 시작, 보고 수신, 머신 상태로 기다리며 고정 sleep은 없다.
- gh 없는 경계는 `cargo test -p aaa-agent --lib gh::tests::`의 실제 실행 가능 파일 검색(사용자 PATH·서비스 fallback·아무 실행 파일도 없음)과 위 ENOENT 통합 시험이 나누어 맡는다. ENOENT는 시험 gh의 없는 interpreter로 만들며 운영 gh·로그인·시스템 설정에 닿지 않는다.
- 기존 `project_lifecycle`의 PR 청소·`pr_followup`·`merged_pr_resolves_the_thread`·`merged_resolve_waits_for_the_routine_prompt`·`resolve_looks_up_the_pull_request_once_more`는 가짜 GitHub가 게시된 branch라고 답할 때 그 원격 참조도 fixture에 둔다. 기존 PR 상태·후속 메시지·병합·정리 단언은 유지한다.
- `cargo test -p aaa --test m13 ci_conditional_failure_restart_catchup_and_outage`: 기존 HTTP 실패·중복 억제·복구에 이어 별도 인증 실패(gh 종료 코드 4)의 CI outage를 확인한다. PR 생략 때문에 CI 실패를 숨기면 안 된다.
- 수정 전에는 미게시 branch의 불필요 호출과 미인증 PR outage가 각각 실패했다. 게시 조건 제거·사용 불가 오류를 다시 실패로 전달·머신 상태 저장 제거·CI outage 전달 제거를 각각 심어 keeper 실패를 확인하고 원본 bytes로 복원한다. 마지막 검증은 전체 ci-local과 check-public이다.


## branch 변경 뒤 agent 재시작과 준비 기록 격리(t-0495, plan §8.315)

- `cargo test -p aaa --test m13 agent_restart_preserves_threads_after_branch_changes_and_isolates_invalid_preparations`: macOS 실제 host·agent·Git worktree 실험실에서 현재 볼륨을 명시한 휴대용 root를 쓴다. 준비 뒤 branch 변경·dirty 상태와 정상 thread를 함께 두고 agent만 재시작한다. 연결 상태·pane 읽기·기존 session host PID·Git 잠금으로 확인하며 고정 sleep은 없다.
- 별도 thread의 `.git`을 다른 repo로 바꾸되 원래 repo의 worktree 등록은 남긴다. JSON이 깨진 기록도 함께 둔다. 두 기록은 원인과 파일명이 경고에 남고 journal bytes는 보존된다. 정상 두 worktree만 aaa 소유 잠금을 복구해야 한다.
- 수정 전에는 agent가 repo·branch 불일치 오류로 종료했다. branch 거부를 다시 심으면 변경한 thread의 잠금 단언, 실제 repo 대조를 빼면 다른 repo의 잠금 금지 단언이 실패해야 한다. journal의 repo 칸만 바꾸는 입력은 Git 등록 검사도 거부하므로 그것만으로 repo 대조 제거를 검증했다고 하지 않는다.
- 기존 `work_root::recovery_tests`와 m13 `configured_prepare_timeout_recovers_only_unchanged_worktree`는 준비 재진입·dirty 거부·정리 잠금 계약을 계속 소유한다. 전체 ci-local·check-public 뒤 착지 창을 받는다.

## aaa install I1 읽기 CLI(t-0485, plan §8.317)

- `cargo test -p aaa --test install --locked`: 실제 binary/argv와 임시 HOME/config/state/root/repo를 소유한다.
  문법·전역 instance·필수값 오류의 정제된 JSON/exit, JSON/NDJSON의 같은 계획 hash, dry-run/오류 전후 파일 snapshot 불변을 본다.
  공통 여섯 시험은 OS cfg로 빼지 않는다. Unix 실행 권한/harness 무실행 하나는 chmod·shell 의미를 검사하므로 Windows에 없다.
  여기에 OS별 native owner 하나를 더해 Mac/Linux 8개, Windows 7개다. Mac은 기존 LaunchAgent 파일,
  Linux는 stat mount 관찰·systemd system 조회/booted marker·XDG user unit 충돌·상대 경로,
  Windows는 실제 drive mount·SCM 읽기 조회·drive 상대/불완전 UNC 거부·ACL unknown을 검사한다. 등록/daemon-reload는 하지 않는다.
  snapshot은 내용·목록뿐 아니라 파일 종류·권한·수정 시각을 포함해 같은 내용 재쓰기/권한 변경도 잡는다. I0 순수 입력 표를 반복하지 않는다.
  install 앞의 malformed --json=value/모르는 전역 옵션과 malformed stream flag도 정제된 envelope여야 한다. 실제 fake secret stderr 유출 RED 뒤 명령 트리 metadata 기반 판별로 수리했다. 다른 명령의 값 install은 기존 오류 계약을 유지한다.
- 기존 base/local 설정·CLI에 없는 reconnect 변경의 계획 무효화, fresh 충돌, 없는 mount/남은 일반 폴더,
  binary SHA/형식·선택 파일 무실행, 실제 Git rev-parse 전후 무변경, profile import의 secret env/자유 실행 거부를 확인한다.
  모든 child 출력에서 fake secret canary가 없어야 한다. snapshot에는 선택한 events/lock/probe 경로도 들어간다.
  profile owner는 실제 빌드 볼륨에 정상 파일을 두어 macOS 외장 볼륨에서도 읽기 계획이 성공하는지 본다.
  storage owner는 기존 일반 파일 work-root를 exit 2로 거부한다. 두 실제 회귀는 수정 전 RED·수정 후 GREEN으로 남긴다.
- 무변경 owner에 mkdir, 문법 오류 owner에 argv 출력, hash owner에 실효 context 무시,
  profile owner에 secret/shell guard 제거, harness owner에 --version 실행을 각각 심으면 의도한 단언에서 실패해야 한다.
  원본 source bytes를 복구하고 같은 owner GREEN 뒤 전수 gate를 실행한다. 의도적인 leak 양성 대조 로그와 정상 출력 검사를 구분한다.
- 공식 gate는 전체 ci-local·deps·check-public이다. Windows cross-check는 native SID/volume API의 컴파일 증거다.
  Windows·Linux 실제 실행: main push 뒤 CI(windows-2025·ubuntu-24.04)의 cargo test가 판정한다.
  사전 branch/PR push·별도 runner/VM은 쓰지 않는다. 착지 전은 macOS 전체 gate와 Windows cross clippy/build이며,
  I1을 완료로 부르지 않는다. 서비스 등록·boot/login/PTY는 후속 단계의 별도 증거다.

## 자동화 A2: 이어 보내기·기억·짧은 끝내기(t-0504)

- host의 `automation::start_tests` fixture로 최근 열린 thread, propose의 이어 보내기, 33건의 32+1 전달, receipt, Codex 거부와 보고 첨부의 64KiB와 단일 입력의 512KiB 초과 거부를 검사한다. 종료 owner 판정표는 표지 6종·보고/입력 시각·활동·내구/파일 입력·검토/쉼을 교차한다. 기존 report boundary keeper는 자동화 완료의 앱 안 알림 없음과 확인 필요 알림 유지를 함께 본다.
- m13 `ci::automation_ci_reuses_open_thread_and_closes_only_after_new_report_with_memory`: 서로 다른 fake CI 두 건을 batch에 보존 → 한 thread → 셋째 Queue 전달 → 옛 완료로 끝내지 않음 → 새 완료 뒤 resolve/기억 사본 → 다음 thread 기억 → 전달 모름/무재전송. FIFO로 화면 진행을 허용하며 host가 Working/Idle를 관측한 뒤 다음 단계로 간다.
- 자동 완료의 첫 확인은 보고 hash·Idle와 함께 `acked_hash == report_hash` 기록을 기다린다.
  보고 hash 저장과 자동 ack 저장은 별도 DB 갱신이므로 앞 둘만으로 확인 완료를 추정하지 않는다.
  t-0679는 자동 ack 직전에 임시 FIFO를 삽입하고 hash·Idle·ack 미기록을 실제 조회한 뒤 해제해,
  기존 단언의 `Null != 보고 hash` 실패와 수정한 시험 전체의 통과를 확인했다.
  주입 코드는 제품에 남기지 않으며 기존 30초 한도·hash 일치 단언을 유지한다.
  같은 시험의 입력 파일/묶음 수는 기동·전달 기록, 옛 완료의 미종료는 ticker 완료,
  기억·충돌 사본은 마지막 회수 뒤 resolved, 다음 기억은 brief 전달,
  무재전송은 막힘 기록과 후속 ticker 완료를 각각 관측한 뒤 검사한다.
- `AAA_TEST_AUTOMATION_CLOCK_FILE`에 Unix ms를 쓰면 자동화 batch·close_idle의 판정 시계만 움직인다. 실제 보고/입력 시각과 agent 활동 시각은 바꾸지 않는다. m13 Lab의 설정 폴더 안 파일만 사용한다. 고정 sleep·timeout 연장은 없다.
- E2E49 `49-automation-memory`: 실제 report/Idle 사건으로 기억/크기/마지막 갱신 thread와 끝남 예정 표시를 본다. 데스크톱·390px에서 상세를 열고 캡처하며 가로 넘침과 자동화 완료 droplet 없음도 검사한다.
- 결함 되심기: close_at의 마지막 입력보다 뒤인 보고 조건(시각과 입력 전 hash)을 제거하면 m13의 옛 완료 보호 단계에서 open 대신 resolved로 실패한다. 원본을 바이트 단위로 복구한다.

### 자동화 기억의 동시 편집(t-0548, plan §8.350)

- `cargo test -p aaa-host --lib memory`는 실제 host·DB와 새 work_files/N-1 Mission frame fixture로 전달 뒤 host 덧붙이기와 thread 요약 변경을 겹친다. host 줄·양쪽 보존 사본·history 두 쪽·충돌 inbox 한 건을 검사하며, 저장한 공통 판을 새 host owner가 읽고 오래된 ThreadRec으로 반복 회수해도 원문을 덮지 않아야 한다. 각각 상한 아래인 양쪽 history를 합쳤을 때 1000개를 넘으면 회수를 거부하고 양쪽 원문을 유지해야 한다. 원래 제품은 「열린 동안 host에서 쓴 줄이 회수로 사라졌다」에서 실패했다.
- 순수 판정은 독립 추가·삭제, 같은 변경, 수정/삭제 충돌, 기준 없는 옛 실행, 파일/폴더 경로 충돌, 되풀이한 압축, host만 고친 뒤 나중에 thread도 고치는 순서와 수동 해소를 검사한다.
- 기존 m13 `ci::automation_ci_reuses_open_thread_and_closes_only_after_new_report_with_memory`가 실제 host→agent→PTY의 FIFO gate를 잡고 마지막 보고 전에 host 줄을 더한다. thread는 기존 기억을 history로 옮긴 뒤 요약한다. resolved 사건 뒤 host 원문·4KiB 이하 두 요약·history·양쪽 사본·중복 보존 없음·inbox를 확인하고, 다음 실행이 host의 보존된 기억을 받는지 검사한다. 고정 sleep과 시한 변경은 없다. 이 실험실은 운영 자동화 파일을 읽거나 고치지 않는다.

## host 계정 전달과 동시 갱신(t-0500, plan §8.320)

`cargo test -p aaa --test m13 accounts:: --locked`는 빈 포트·임시 폴더·가짜 인증만 사용한다.
CLI→host→UDS/TLS agent→session pipe→PTY의 실제 경계에서 같은 Codex 계정 네 작업(머신당 둘)을
동시에 띄운다. 이전 refresh token을 재사용하면 거부하는 가짜 제공자가 두 회전을 처리하며,
각 회전의 제공자 호출은 한 번이다. 부분 응답의 이전 토큰 보존, 갱신 거부 시 우회 없음, 대화 조회·
같은 기록으로 재개, auth 파일 권한·정상 종료 청소를 검사한다. Claude 동시 작업·기존 profile 인증·
계정 미지정 환경도 함께 확인한다. host 단절 중 outbox를 직접 검사하고 DB/WAL·post·spec·준비
journal·로그·화면에 원문·8바이트 조각·base64/URL/JSON 변형이 없는지 찾는다.

금고의 암호문/AAD 무결성·키 잠김은 `cargo test -p aaa-host accounts:: --locked`가 소유한다.
이 시험은 실제 로그인·실제 인증 파일·운영 host/agent·기본 설정에 접근하지 않는다.

## 대화창 컨텍스트 게이지(t-0502, plan §8.321)

- `cargo test -p aaa-agent --lib plan::tests::`: 대화 기록 꼴(Claude assistant `usage`·`requestedModel`·`compact_boundary`, Codex `token_count`·`compacted`)을 임시 파일에 이어 적고 Reader를 직접 돌린다. 값은 실제 harness 실측(Claude 43,922 → 7,910, Codex 18,861 → 12,274)이다.
- `node --test web/test/contextUsage.test.mjs web/test/inputReceipt.test.mjs`: 표기(%·k·1050k), 치는 동안 숨김 규칙(80% 미만·창 모름만), harness 명령 한 줄 판정.
- `node web/e2e/web.e2e.mjs --spec 50-context-gauge`:
  - 가짜 Claude가 SessionStart hook으로 기록 자리를 알린다. F7(작업 중)·F5(쉼)·F6(정리 끝: `compact_boundary` pre 163,600/post 28,400과 요약)과 「/compact」 Enter(정리 화면)를 받는다.
  - spec이 기록에 assistant usage 줄을 더하면 게이지 글이 바뀔 때까지 `domIs`로 기다린다(agent는 2초마다 읽는다).
  - 패드·데스크톱의 치는 중 숨김은 `data-hidden`과 게이지·글 칸 폭이 같은지로 본다. 정리 단추는 pane이 받은 bytes와 입력칸 잠금 풀림으로 본다. 고정 sleep은 없다.
  - t-0570(사용자 10-21): WebKit iPhone에서 빈 입력칸·focus만 있음·치는 중·80% 이상·창 넘음·창 모름 모두 게이지가 보이지 않고 폭도 0이며 접근성 트리에서도 빠지는지 본다. 320·390·700px는 숨고 701·768px는 보인다. 실제 iPad 꼴(820px)에서는 기존 표시·치는 중 숨김·펼침을 유지한다.
  - 첫 인자로 캡처 폴더를 주면 데스크톱·iPhone 밝음/어두움과 기존 데스크톱 펼침·정리 중·정리 뒤, iPad 펼침 그림을 남긴다. 테마는 실험실 명령 팔레트로 바꾸고 DOM 반영을 기다린다.

## aaa install I2 파일·상태 owner(t-0485, plan §8.322)

- `cargo test -p aaa --bin aaa install::engine::transaction --locked`: 실험실 실제 파일을 stage/commit하고
  rename 뒤 journal 전 중단, 같은 ID 재개, 외부 편집/같은 내용의 외부 inode 교체, 두 프로세스 OS 잠금,
  다른 instance, 반복 unchanged의 bytes/권한/mtime 불변, 공유 마지막 참조 제거/원본 hook 복원,
  제거·rollback 중단, missing repair·reinstall 데이터 보존, 과거 ID 재개 거부, 같은 config의 별도 파일/job 잠금,
  root 보존 partial/remaining, 소유 stage 정리·외부 stage 보존, 프로젝트 뼈대/기존 repo를 검사한다.
  실패 주입 flag/export 없이 실제 파일 작업과 production 단계 경계를 사용한다.
- 일반 journal/공개 결과에는 fake secret이 없어야 하고 원본 backup은 별도 private 파일이어야 한다.
  status는 실제 파일·root 소유 기록을 읽으며 lock/probe/기록 보정 없이 전후 snapshot이 같아야 한다.
  Unix owner는 mode/소유권/nlink/symlink를, Windows 전용 owner는 실제 NTFS hardlink와 열린 DACL을
  사용한다. 공통 파일 owner는 OS cfg로 빼지 않는다. Unix 전용 1개는 Unix mode/symlink를 검사하며
  Windows는 그 대신 handle link 수와 현재 SID+SYSTEM DACL을 실제 CI에서 검사한다.
- `cargo test -p aaa --test install --locked`: I1 owner에 status routing과 `--yes`/임의 승인 JSON의
  쓰기 차단을 더한다. 자동·SSH·외부 TTY apply/resume은 검증된 I6 승인 경로 전까지 needs_user/3이다.
- baseline/rename 소유 증명·잠금·private 읽기·마지막 참조·uninstall 편집 대조·reinstall TOML 보존·active job·
  repo 보존·root 읽기 대조·일반 journal 비밀 비노출의 결함 주입이 해당 owner에서 실패해야 한다.
  변형은 bytes로 복원하고 원본 owner 통과 뒤 전체 gate를 돌린다. Windows/Linux 실제 실행은
  main push 뒤 GitHub CI이며 cross clippy/build를 실제 실행 통과로 쓰지 않는다.

## 입력칸 「/」 목록과 명령 줄(t-0502, plan §8.323)

- `cargo test -p aaa-agent --lib -- slash:: chat::tests::claude_commands plan::`: 임시 폴더에 명령·skill 파일과 실측 기록 줄을 두고, 대화 줄은 실제 쪽 읽기(`read_path`)로 읽는다. 창 크기 시험은 사용자 설정 파일 자리를 임시 경로로 바꿔 실제 `~/.claude`를 읽지 않는다.
- `node --test web/test/slashCommands.test.mjs`: 여는 자리, 순위, 숨김, 확인창·알림 판정.
- `node web/e2e/web.e2e.mjs --spec 51-slash-commands`:
  - 실험실 `CLAUDE_CONFIG_DIR`의 `commands/`·`skills/`와 pane 폴더(`.git`이 있는 git root)의 `.claude/commands/`를 agent가 읽는다.
  - 가짜 Claude는 F7(작업 중)·F5(쉼)과 Enter 글을 받는다. `/context`는 실측 꼴의 명령·출력 줄을 적고, `/model`은 선택창을 그렸다가 Esc에 결과를 적고, `/clear`는 새 기록으로 SessionStart(clear) hook을 보낸다.
  - 「보내지 않았다」는 pane이 받은 bytes로, 상태 전이는 가짜가 그린 「mode …」를 `waitOutput`으로, 화면은 `domIs`로 기다린다. 휴대폰은 WebKit `pairDevice`. 고정 sleep은 없다.
  - 지연 커서 회귀(t-0536, plan §8.338)는 다음 RAF를 보류한 채 Tab 명령 넣기·↑ 기록 부르기 뒤 실제 키로 전체 선택하거나 중간에 글을 치거나 focus를 옮긴다.
    보류한 콜백이 실제 frame에서 모두 끝난 뒤 글·선택·focus가 그대로인지, 전체 선택 뒤 Delete가 비우는지 검사한다.
    원래 RAF 커서 복원을 되살리면 새 전체 선택이 끝 커서로 바뀌어 실패해야 한다. 글과 커서는 같은 React commit에서 반영한다.
    이 scheduler keeper와 별도 transport 지연 진단은 따로 실행한다(transport의 Tab frame 대기를 keeper가 보류하면 시험 도구끼리 막힌다).

## 자동화 A4: 특수 실행 제거(t-0507)

- A4 뒤 남은 종료 pane(t-0545, plan §8.346): `web/test/roster.test.mjs`는 옛 평가자·임무의 `evaluator` pane 18개가 「기타」·단축키에서 빠지고, 실행 중·중단·유실·사용자 종료 기록과 연결된 thread 줄은 남는지 본다. `web/e2e/specs/57-legacy-special-panes.mjs`는 격리된 실제 agent의 옛 세션 파일을 복원해 화면에서 18개가 빠지는지, 실제 실행 중인 사용자·평가자 pane과 사용자 종료 기록이 남는지 검사한다. t-0540·W4의 막대 단추 다섯과 「답할 것」→「알림함」→「작업」 전환 뒤에도 같은 숨김을 확인한다. 숨긴 기록의 파일 바이트·CLI 읽기·주소로 직접 보기를 함께 확인한다. 종료 기록은 지우지 않으며 새 변환 명령 없이 목록에서만 숨긴다. 준비 완료·종료·출력·DOM 사건으로 끝낸다.
- `cargo test -p aaa --test m13 automation_start::legacy_mission_files_are_inert_and_convert_with_memory_and_pending_input -- --exact`: 실제 host·agent·CLI·Front API에서 옛 파일 비실행, 완료 처리 뒤에도 한 번인 제거 안내, doctor 진단, mission_list 빈 목록/mission_start 제거 오류, 열린 옛 evaluator 표시, 미리보기 무쓰기, 기억·이력 hash 회수, 원본 보존·대기 입력 이전·tick 뒤 무복제를 확인한다.
- `cargo test -p aaa-host previous_agent_keeps_input_and_memory_transfer_with_legacy_file_frames --lib`: AGENT_LINK 1.9/mission.v1 fake가 입력 쓰기와 기억 주입·회수·처리 여부 조회를 받는다. 새 WorkFile 태그가 옛 상대에 닿으면 실패한다. 실제 agent의 파일·hash·symlink keeper는 `threads::work_file_tests`가 계속 맡는다.
- E2E26은 패널 셋과 저장된 special→work를, E2E40은 승인 규칙+막힌 규칙 배지와 기존 변환/Reader/큐 풀기를 검사한다. E2E48·49는 기존 자동화 줄과 기억을 유지한다. 캡처는 데스크톱·390px, 실제 host·agent를 사용한다.
- `Role::Evaluator`와 구 Front/Agent DTO는 한 판 호환 경계에만 남는다. 옛 outbox의 모르는 칸과 ThreadRec의 원본 데이터는 변환 전까지 보존하며, L0 snapshot keeper의 기대 문서는 바꾸지 않는다.

I2 Windows 수리(plan §8.325): 같은 파일 owner를 세 OS에서 유지한다. `resource_locks_serialize_processes`는
실제 자식 process 둘(같은 작업/같은 config의 다른 파일)을 거부하면서, 잠금 중 유효 status와 파일 snapshot이
읽히고 바뀌지 않으며 해제 뒤 같은 ID 재개가 되는지 함께 본다. Windows는 읽기 공유만 허용하는 handle
예약을 사용한다. stage/reverse flush는 Windows 쓰기 handle로 수행하고 원본 ACL/hardlink/복구 owner를 뺄 수 없다.
Windows 실행이 없는 현지 cross 결과는 native GREEN이 아니며 다음 main CI 결과와 나눠 보고한다.


## 웹 할 일 첨부(t-0506, plan §8.327)

- 웹 E2E 52 `task-attach`가 실제 host·agent에서 Chromium 데스크톱·iPhone·iPad(밝음·어두움)로 붙여 넣기(⌘V·붙여넣기 칸), 고치는 중 초안,
  떼기, md 올리기·보고서 문서 고르기·주소, 파일 보기 열기(md는 그린 Markdown, html은 sandbox 페이지), 「바로 시작」 복사, 「할 일에 추가」
  (보고서 머리·thread 결과물 줄 → 새 task·있는 task에 링크), 보고서 주소 알아보기(공유 링크·앱 보고서 주소·풀 수 없는 주소), 보기 권한을
  본다. 올린 html은 GET 응답의 sandbox CSP·`nosniff`·글 종류를 본다. 끝은 화면 표지(`data-tasks-sha`·`data-count`)와 TASKS.md·작업
  폴더 파일로 정한다. 시간은 기다리지 않는다.
- iOS 길게 누름 메뉴와 클립보드 허용 말풍선은 headless가 그리지 못한다. 그 메뉴 뒤 브라우저가 내는 `ClipboardEvent`를 붙여넣기 칸에
  재생한다. iPhone 실기기 확인은 배포 뒤 사용자가 한다(보고의 절차).
- 휴대폰 단계는 상세 시트 위 파일 보기를 닫기 단추로 닫은 뒤 상세가 남고, 다른 화면으로 가도 파일 보기가 되살아나지 않는지 본다.
  상세 시트가 파일 보기 동안 기록을 내주지 않으면(`usePhoneSheetBack(…, !fileOpen)`을 빼면) 이 단계가 실패한다.
- 웹 순수 시험 `web/test/taskAttach.test.mjs`는 첨부 줄 규칙·입력칸 글·붙인 글과 파일 판정(주소의 괄호는 풀어 쓰고 괄호 든 보고서
  경로는 받지 않는다)·「할 일에 추가」 단추를 둘 문서·보고서 주소 꼴을 본다.
- host `task_attach::tests::upload_rules`는 보기 권한·폐기한 기기를 파일을 쓰기 전에 거부하고(옛 순서는 폐기 기기의 파일을 남겼다),
  signature 없는 그림·NUL·UTF-8 아닌 문서·모르는 문서 종류를 거부하며, 같은 내용은 파일 하나, 읽기는 이름 꼴만·symlink 거부인지 본다.
- host `library_links::tests::task_link_resolves_report_urls_and_reads_live_docs`는 공유 링크·앱 보기 경로의 cap 풀기, 보기 권한·
  다른 프로젝트·없는 cap·폐기 거부, live 공개 문서 사본 읽기를 본다. `task_attach::tests::old_agent_is_refused_before_a_record`는
  `work_attach.v1`이 없는 agent에 thread 기록을 만들기 전에 거부하는지 본다(앞 확인을 빼면 실패한다).
- m13 `task_attachments_reach_a_remote_agent`는 TLS 원격 agent(다른 상태 폴더 labB)에 `--from-task`로 띄워 그림·보고서 문서를
  그 작업 폴더 `attachments/`에 같은 바이트로 쓰고, 할 일 글이 그 경로만 가리키며 host에 없는 첨부는 경고로 알리는지 본다. 이어서
  host 원본(올린 그림·보고서 문서)을 지운 뒤에도 `thread restart`가 되고 첫 사본이 남는지 본다. 첫 전송의 `sent` 표지를 저장하지
  않으면 이 다시 시작은 「아직 쓴 적 없는 첨부」로 보고 멈추므로 실패한다(표지 저장을 빼고 확인했다).

## task 저장소 칸(t-0506, plan §8.330)

- m13 `task_repo_field_picks_machine_and_path`는 labA(host 로컬)·labB(TLS 원격)에 이름이 같은 저장소(경로는 다름)를 project.toml repos로
  두고, owner 머신이 정한 task는 그 머신 경로의 worktree로, 칸만 있는 task는 열린 thread가 적은 머신(labA)으로 띄우는지 본다.
  덜 붐빔 차례를 뒤집으면 labB를 골라 실패한다(결함을 넣어 확인했다). 어긋남(`--repo`·`--kind`·없는 머신·repos에 없는 이름) 거부,
  모르는 칸의 형식 어긋남, `task add|edit --repo`(경로 → 이름, 빈 글 비움, 모르는 이름 거부), 같은 머신에 같은 이름 저장소가 둘일 때의
  거부와 끝 `/`를 붙인 `--repo` 경로로 띄우기도 본다.
- 웹 E2E 54 `task-repo`는 labA 하나로 폼 「저장소」 고르기와 칩, 칸이 있는 task의 고정된 저장소·머신과 worktree 시작, 칸이 빈 task의
  「고르기…」(시작 막힘) → 작업 폴더 시작과 줄에 적지 않음(iPhone)을 본다. 두 머신 사이 고르기는 m13과 웹 순수 시험이 맡는다.
- 칸이 빈 task를 웹에서 시작하는 다른 spec(47·52)은 「작업 폴더」를 고른 뒤 시작한다(결정 4A).

I2 Windows 잔여 수리(plan §8.331): `unsafe_targets_readonly_and_untrusted_journals_preserve_originals`의
ParentDir 입력은 canonical Windows prefix에서도 `..`가 남도록 native separator를 OsString에 raw로 붙인다.
fixture의 ParentDir 전제조건을 선행 단언한 뒤 target의 conflict 거부·escape 미생성·전체 tree 불변을 확인한다.
`Path::join`은 Windows verbatim prefix에서 `..`를 먼저 정규화하므로 이 부정 입력을 만드는 데 쓰지 않는다.
기존 invalid journal status의 bytes/권한/mtime·비밀 비노출 검사를 유지하며, 거부 분기를 허용으로 바꾼
결함 변형은 conflict 단언에서 실패해야 한다. Windows 실제 통과 판정은 main CI가 맡는다.

## 비밀 없는 panic 위치(t-0533)

- `cargo test -p aaa-common --lib panic_log::`는 실제 hook을 별도 시험 프로세스에서 설치한다.
  CLI·host·agent·session host 네 종류마다 비밀 String·비문자열 payload·동적 thread 이름·환경변수를 두고,
  `RUST_BACKTRACE=full`이어도 stdout/stderr와 알림용 snapshot에 비밀이 없는지 검사한다.
  같은 소스 위치를 서로 다른 thread에서 세 번 실행해 횟수 1·2·3, 다른 위치는 1,
  실제 Tokio task의 숫자 ID와 파일·줄·열·빌드 SHA를 확인한다. stderr를 실제로 잠근 동안에도
  감시자가 `latest()`를 읽는 사건을 받아야 한다. 종료·join·관측 사건으로 끝내며 sleep은 없다.
- `panic_log::latest()`는 마지막 안전한 `Snapshot`을 반환한다. hook은 알림을 전송하지 않는다.
  단일 실행 파일 진입점에서 hook을 설치하고, 파싱한 명령으로 프로세스 종류를 정한다.
  등록용 `agent enroll` 등 일반 명령은 CLI, 복구 감시는 agent로 분류한다.
- 임의 thread 이름은 `other`로 숨기고 런타임 숫자 ID로 구분한다. 고정 이름(main·aaa-cli·Tokio worker)만 허용한다.
  Tokio의 안정 API에는 task 이름이 없어 숫자 task ID를 쓴다. payload·환경·backtrace는 읽거나 포맷하지 않는다.
- build script는 Git HEAD SHA와 추적된 Rust 파일 목록만 생성물에 넣는다. HEAD·현재 ref·index·packed-refs를 감시한다.
  절대 경로는 허용된 저장소 상대 경로로 줄이고, 모르는 의존성·생성 파일 위치는 `external`로 숨긴다.
  Git 없는 소스 배포는 SHA `unknown`, 경로 `external`로 숨긴다. 줄·열과 원본 위치별 횟수는 계속 남긴다.
- 본문 출력 복원·원본 경로 출력·반복 횟수 초기화 결함은 각각 keeper의 비밀 비노출·경로·횟수 단언에서 RED여야 한다.
  stderr 쓰기 중 상태 잠금을 잡는 결함은 감시자가 출력 전에 위치를 읽는 계약에서 RED여야 한다.
  각 변형의 원본 bytes를 복구한 뒤 GREEN과 전체 ci-local·deps·TS 최신·check-public을 확인한다.

## aaa install I5 Windows 감독자(t-0485, plan §8.333)

- `cargo test -p aaa --bin aaa install::engine::supervisor --locked`: 공통 owner 7개는 세 OS에서 실행한다.
  실제 I2 파일/receipt/잠금·감독자 journal로 등록 직후 중단/실제 manager 대조·같은 ID 재개·기존 암호 보존·
  외부 서비스 보존·credential 중단/이전 process 미채택·보존 제거·읽기 status를 확인한다.
- lifecycle owner는 별도 process의 실제 고정 host/agent backend와 실제 ready producer를 쓴다.
  host가 없는 agent 초기화 → 실제 host 연결/Welcome → 단절/재연결 → 각각 stop을 사건으로 기다린다.
  유효 proof를 받아 계정/context/SHA/PID/생성 시각/boot/session/연결/elevation을 변조해 거부를 본다.
  실제 TCP listener 충돌은 RUNNING/ready를 낼 수 없다. 외부 SCM/token 관찰만 모형이고 준비 영수증을 fixture가 만들지 않는다.
- Windows 추가 owner 4개는 실제 현재 token/profile/image/SCM query·EventLog 읽기 구독/해제 ABI·CommandLineToArgvW,
  종료 process handle 거부·실제 configured I2 receipt와 context/active ID 대조다. Win32 API가 없는 Mac/Linux에서 이 ABI owner만 제외한다.
  lifecycle Windows 자식은 CREATE_NO_WINDOW지만 실제 SCM Session0 logon 증거는 아니다.
- `cargo test -p aaa --test install --locked`는 숨은 서비스 진입의 console fallback 거부와 무변경을 포함한다.
  apply/resume 승인 차단·I1 읽기 keeper를 유지한다. agent link owner는 Welcome/replay 완료 전 연결을 공개하지 않는지 본다.
- 실제 process 대조 제거·proof context 무시·credential 이전 process 허용·manifest 결속 제거·LSA Deny 무시·
  Removed에서 실행 파일 강제 요구·restart stop 생략·listener 전 host ready 변형은 각각 기존 owner에서 실패해야 한다.
  원본 bytes 복구 뒤 공통 owner GREEN을 확인하고 전수 gate를 실행한다. 비밀 canary는 private tree UTF-8/UTF-16와
  정상 owner 출력에서 검사한다. 시험 허용 production flag/export·실제 SCM/LSA 쓰기·고정 sleep·재시도는 없다.
- 전체 macOS ci-local·deps·check-public과 Windows workspace/all-targets cross Clippy/build를 구분해 기록한다.
  Windows·Linux 실제 실행은 main push 뒤 CI(windows-2025·ubuntu-24.04)다. 실제 서비스 logon·부팅·권한·암호·
  ConPTY/공식 인증은 Windows bootstrap 뒤 사용자 실기 인수의 준비물/명령/되돌리기/로그 항목으로 남긴다.

## host 핵심 작업 감시(t-0532, plan §8.334)

- `cargo test -p aaa-host --lib supervisor::tests --locked`: 실제 task의 panic/조기 종료 사건과 가상 backoff 시각으로 1·2·4초 뒤 세 번 재실행, 상한 중단, 한 번 알림을 검사한다. loopback HTTP 수신은 앱을 닫았을 때 이상·회복 각각 한 번, 앱을 쓰는 동안은 notice만 확인한다. 이상/회복 판정 저장 뒤 inbox commit 전 재시작, SQLite INSERT 실패 뒤 재개방, failed INSERT만 거부해 뒤 recovered가 기다리는 순서, 이미 발행한 이상의 재시작 무중복·회복 이어 알림도 같은 owner가 검사한다. 판정의 `now`로 30초 안정 경계·희소 시간 초과 만료·회복 중 새 시간 초과를 확인한다.
- `cargo test -p aaa-host --lib link::supervision_tests --locked`: Hello/Welcome/Ping을 마친 실험실 link의 전송체가 읽기 중 panic한다. pending의 즉시 AgentDisconnected·세대별 연결 정리·재연결 처리·네 번째 종료 뒤 거부와 독립 감시기의 inbox/notice 한 번을 확인한다. 다른 keeper는 실제 AgentConn 요청을 받되 응답을 보류하여 다섯 deadline 사건을 만들고, 이후 같은 reader의 실제 응답을 확인한다. ticker는 띄우지 않으므로 전달이 ticker에 기대면 실패한다. panic 감지 경계는 공통 `panic_log::latest()` API를 호출하고, 안전한 위치·payload 비노출의 정본 keeper는 t-0533의 `panic_log::` 시험이다.
- 감시 제거·pending 정리 제거·재실행 상한 변경·시간 초과 관측 제거·회복 판정 제거·중복 억제 제거를 각각 되심으면 owner 단언이 실패해야 한다. 변형마다 원본 바이트를 복원하고 원본 keeper를 통과시킨 뒤 전체 ci-local/deps/TS/public gate를 수행한다. 실제 OS/Web Push 제공자의 휴대폰 전달·host 프로세스 전체 정지는 이 실험실 keeper의 보증 범위 밖이다.

## 계정 thread 폴더 신뢰(t-0543)

`cargo test -p aaa --test m13 accounts::account_threads_trust_only_their_cwd_when_coordinator_answers -- --exact`
는 실제 host·로컬/원격 agent·session·PTY와 임시 홈의 가짜 Claude/Codex를 사용한다.
두 harness × 두 머신 × worktree/일반 작업 폴더에서 `trust_screens=coordinator`일 때 신뢰 기록은
정규화한 자기 cwd 한 곳뿐이고, `user`일 때 기록이 비고 실제 신뢰 화면이 남는지 확인한다.
Claude SessionStart가 알린 격리 홈의 대화는 pane chat으로, TaskCreate 계획은 thread 상태로 확인한다. 완료는 socket gate와 종료 사건으로 판정한다.
실제 인증 파일·키체인·제공자 네트워크는 사용하지 않는다. host의 정책 분기나 session의 기록 생성을
빼면 시작 증명의 trusted 단언에서 실패해야 한다. 정책을 무시하고 늘 기록하면 user 단언에서 실패해야 한다.
`cargo test -p aaa-session --lib account::tests::`는 격리 Claude의 환경설정 보존,
재개 시 자동 신뢰 제거, 도입 전 Claude 대화의 원래 홈 유지도 확인한다.

## aaa install I2 파일 조상 경로(t-0485, plan §8.341)

- `unsafe_targets_readonly_and_untrusted_journals_preserve_originals`는 세 OS 공통 owner다.
  기존 readonly stage/원본 bytes, 불신 journal 읽기의 exit·비밀 비노출·tree 불변, raw ParentDir 거부를 유지한다.
  파일 아래 직접 자식/여러 단계 proposal은 거부하고 정상 미생성 디렉터리 아래 proposal은 tree 변경 없이 허용한다.
  유효 proposal 뒤 부모를 일반 파일로 교체한 실제 `Transaction::create`는 journal과 다른 자원 잠금 생성 전에 거부한다.
- 기존 Windows CI 실패는 native RED다. Unix의 NotADirectory를 Windows식 NotFound로 임시 분류한 진단은
  native Windows 실행으로 부르지 않는다. 시험용 production flag/export를 만들지 않고 원본 bytes를 복구한다.
- snapshot 부모 검사·가장 가까운 조상 검사 생략은 오류 분류 진단에서 proposal 거부 단언을 실패시켜야 한다.
  실제 create의 쓰기 전 확인 생략은 native owner의 전체 tree 보존 단언을 실패시켜야 한다.
  각 변형 복구 뒤 엔진 owner·전체 ci-local·Windows cross와 비밀 canary 비노출을 확인한다.
- 동반 Windows native owner `process_observation_rejects_completed_process_with_its_handle_still_open`은
  기존 cmd/인수/환경/stdio/creation flags와 success·signaled process·유효한 열린 handle의 거부 단언을 유지한다.
  실패 메시지에 종료 코드(Debug/십진수/32-bit hex)만 더해 main CI가 원인 진단 값을 남기게 한다.
  native는 진단만, 수리는 다음 착지다. macOS와 cross 통과를 이 Windows owner의 실제 GREEN으로 부르지 않는다.

## project 명령의 프로젝트 이름 인자 정리(S5, §8.344)

- CLI 유닛 `project_lifecycle_help_omits_legacy_project_arguments`: 여섯 명령의 도움말에서 프로젝트·호환 인자를 숨기며, 새 꼴과 옛 꼴만 받고 넘치는 인자는 거부한다. rename의 새 이름은 여전히 필수다.
- m13 `project_lifecycle`: 이름 없는 pause·resume·archive·rename·delete·sweep으로 실제 상태 전이·거부·이름 변경·기록 이동·휴지통·worktree와 병합된 branch 청소를 본다. 보관한 프로젝트를 직접 지정하는 호환 경로도 유지한다.
- m13 `project_cli_keeps_legacy_names_with_a_stderr_warning`: 옛 꼴 여섯 명령은 경고 한 줄을 표준 오류에만 쓰고 JSON은 그대로 파싱된다. 다른 이름은 모두 거부하며 상태를 바꾸지 않는다. 새 꼴 sweep은 경고 없이 같은 결과, 옛 꼴 rename은 실제 이름을 바꾸고 새 꼴로 되돌린다. 보관 뒤 이름 생략은 기존 오류이며 폴더를 보존한다.
- pause에서 주어진 이름을 무시하고 현재 프로젝트를 쓰는 결함을 심으면 다른 이름 거부 단언에서 실패한다. 원본 바이트를 복원한 뒤 같은 keeper를 다시 확인한다.
- m13 `coordinator_flow`·`doctor_files_brings_the_coordinator_guide_to_existing_projects`·`project_lifecycle`: 새로 만들거나 doctor로 갱신하거나 이름을 바꾼 AGENTS.md에 이름 없는 pause·resume·sweep 안내가 들어간다.
- 실험실 host·가짜 agent만 쓰며 운영 프로젝트·설정·host는 건드리지 않는다. 고정 sleep이나 새 시험 전용 제품 경로는 더하지 않는다.


## 계정 Claude 첫 실행 우회 경고(t-0551, plan §8.347)

- `cargo test -p aaa --test m13 accounts::fresh_account_claude_initializes_only_authorized_bypass_consent --locked -- --exact`는 실제 host·로컬/원격 agent·session·PTY와 가짜 Claude를 사용한다. yolo 켜기/끄기, profile의 두 permission-mode 표기와 일반 모드로 새 홈 10개를 시작한다. 안내 완료와 우회 수락 키, 첫 경고 부재, 가짜 원본 홈의 인증·다른 설정 비복사와 원본 불변을 확인한다. 폴더 신뢰는 별도 정책이므로 실험실에 `trust_screens=coordinator`를 명시한다. 기본값을 coordinator로 가정했던 첫 시험 실패는 제품 결함으로 분류하지 않는다.
- 가짜 시작 화면은 설치된 공식 Claude 2.1.304의 시작 조건과 저장 키를 따른다. 실제 제공자 CLI·키체인·인증 파일·외부 로그인은 실행하지 않는다. 완료는 socket gate·harness 종료 사건으로 판정하며 고정 sleep·재시도는 없다. 이 시험이 실제 제공자 인증이나 이후 새 버전의 추가 안내를 보증하지는 않는다.
- `cargo test -p aaa-session --lib account::tests:: --locked`는 명시적 우회/허용 옵션, 마지막 permission-mode, `--` 뒤 문자열 제외, 일반 실행의 파일 미생성, 기존 환경설정 보존, 비우회 재개 시 자동 수락 제거, 인증 비기록, Unix 0600·symlink 대상 보존·깨진 JSON 거부를 확인한다.
- 수락 준비 제거와 비우회에도 무조건 수락하는 변형은 각각 위 m13의 settings 단언에서 RED여야 한다. 매번 원본 bytes를 복구한 뒤 정상 owner와 전체 ci-local·check-public을 실행한다.


## install I5 Windows 종료 process fixture(t-0485)

`install::engine::supervisor::native::tests::process_observation_rejects_completed_process_with_its_handle_still_open`은
현재 시험 실행 파일을 `--list`로 직접 실행해 시험 본체의 실행·재귀 없이 성공 종료시킨다(§8.356).
cmd 인수 분리(§8.352)는 실제 Windows CI에서도 실패했으므로 셸 해석에 기대지 않는다.
stdout은 null로 두고 stderr는 reader thread가 읽어 자식 종료를 막지 않으며, 실패하면 status·code·32-bit hex·stderr 첫 줄을
panic 앞 `eprintln!` 한 줄로 남긴다. `wait_with_output`으로 Child를 소비하지 않아 같은 process handle이 남는다.
자식의 성공 종료·아직 열린 유효 handle의 `WAIT_OBJECT_0`·제품 `service_process_unverified`를 모두 검사하며
고정 sleep·재시도·timeout·production 변경은 없다. 같은 native/상위 supervisor의 현재 image·SID·profile·SCM 읽기와
Unicode argv 왕복 owner도 Windows cargo test에 남긴다. 이들은 Win32 전용이며 실제 실행은 main push 뒤 windows-2025,
macOS gate·Windows cross compile 결과와 별도로 보고한다. SCM 실제 logon·UAC/LSA는 실기 인수 경계다.

## 보고서 그림 크게 보기(t-0552, plan §8.353)

- 웹 E2E `59-report-images`: 실제 host에 index.html·그림 넷(하나는 링크 안)·data: 그림을 publish한다. host 단계는 `node:http`로 보낸다(Node fetch는 `Sec-Fetch-Mode`를 cors로 덮어 브라우저 탐색 머리를 흉내 내지 못한다). fetch·Range·`?download=1`·그림은 원본 바이트, iframe 탐색은 원본 + `<script>` 하나와 맞는 `Content-Length`, sandbox CSP 그대로, 최상위 문서 탐색에도 붙는다.
- 데스크톱(Chromium): 보고서 안 그림 누름 → 크게 보기(`data-at`·`data-count`·`data-scale`·그림 주소), ←→·앞뒤 단추, 두 번 누름 2.5배·맞춤, 그림 누름은 닫지 않음, Esc·바깥 누름으로 닫고 보고서는 남음. 누른 곳이 iframe이라 키는 크게 보기가 focus를 가져온 사건(`focusIs`) 뒤에 누른다(앞서 누르면 키가 보고서 문서로 간다).
- 흉내 낸 알림: 보고서 제 script(API 주소·다른 origin의 같은 경로·data:)와 앱 창 자신(그 보고서 그림 주소)이 보낸 넷을 iframe 표지 `data-messages`로 본 뒤 `data-opened`·크게 보기가 그대로인지 단언한다. 진짜 누름 뒤에 보면 React가 한 번에 그려 흉내가 연 상태를 덮어 결함을 놓친다(처음 쓴 판이 출처 검사 결함에도 통과했다). 흉내 주소는 iframe의 실제 주소로 만든다(publish가 준 공유 링크 cap은 앱이 연 cap과 다르다).
- 휴대폰(WebKit iPhone): `tap()`으로 열고, 손짓은 touch PointerEvent를 무대·그림에 직접 보낸다(옆으로 쓸어 넘김, 짧은 끌기는 그대로, 두 손가락 90→190px = 2.11배, 남은 손가락 끌기는 옮기기이고 넘기지 않음, 두 번 누름 맞춤). 뒤로(popstate)는 크게 보기만 닫고 주소·보고서 그대로, 아래로 쓸기는 닫고 기록 칸(`aaa:sheet`)을 거둔다. headless WebKit은 이 iframe 안 그림을 그리지 않아(plan §8.353 「남은 것」) iframe 그림 읽힘은 데스크톱에서만 기다린다.
- 결함 심기(되돌렸다): iframe 출처 검사·origin 검사·`/v/<cap>/` 경로 검사를 빼면 데스크톱 흉내 단언, host가 fetch에도 붙이면 host 단계, 그림마다 빈 click listener를 빼면 휴대폰 첫 누름에서 실패한다.

## 「답할 것」 닫기·끝난 배포 접기(t-0559, plan §8.359)

- `node web/e2e/web.e2e.mjs --spec 26-activity-bar,35-rollout,39-phone-back,53-thread-questions,57-legacy-special-panes,62-asks-close`로 관련 화면 계약을 함께 확인한다. 26의 버튼 순서와 Home·End, 57의 패널 목록 단언은 사용자가 지정한 새 순서(작업부터, 답할 것은 보고서 바로 위)를 검사한다. 57의 옛 세션 숨김·실제 실행과 사용자 종료 기록·파일 바이트 보존·직접 읽기 검사는 그대로 유지한다.
- 62는 실제 격리 host·agent에 확인 필요 post, 셸과 Claude 대화 pane을 만든다. WebKit iPhone 390·430 × Safari 탭·홈 화면 앱 흉내와 Chromium 1280에서 질문 → 대화 → 뒤로 → 질문 → 뒤로 → 셸을 본다. 본문 위·가운데·아래 세 점의 맨 위 요소가 같은 화면에 속해야 하며 숨긴 「답할 것」은 visibility:hidden·position:absolute여야 한다. 수정 전 휴대폰은 두 화면으로 갈라졌고 데스크톱도 숨긴 화면이 공간을 차지했다.
- 닫기 단추는 앞서 본 대화로, 빈 「답할 것」에서 닫기는 앞서 본 보고서로 돌아가야 한다. 가장자리 합성 touchcancel 뒤 popstate를 내는 경우 목록이 열리지 않는지 검사한다(왼쪽 끝은 브라우저 뒤로 몫, plan §8.408). iOS 시스템 몸짓 자체는 헤드리스로 보증하지 않는다.
- 대기는 data-shown·aria-current·popstate·drawer transition 완료 사건으로 끝낸다. 숨긴 채 그려 둔 화면에도 drawer 단추가 남으므로 공용 inList는 보이는 화면의 단추를 고른다. 목록 행 클릭도 inList로 drawer 닫힘이 끝난 뒤 본문을 잰다.
- 35는 배포 실행기를 돌리지 않는다. 실제 Reader 세션의 WebSocket에 배포 상태만 주입해 WebKit 390·Chromium 1280에서 성공한 배포만 접기·펼치기, 높이 절반 미만 축소, 같은 배포 갱신·패널 전환 유지, 새 ID의 성공한 배포 재표시와 running·recovering·stopped·stopped_rolled_back·recovery_failed 상시 표시를 확인한다. 갱신 처리는 카드 data-updated로 기다린다. 기존 Reader 조작 거부·복구 재연결·긴 실패 원인 검사는 유지한다.

## 대화 그림의 task 첨부 전달(t-0558, plan §8.360)

- 웹 E2E23의 「대화 이미지」는 Chromium·WebKit 데스크톱·WebKit iPhone·iPad에서 붙여넣기·드롭·파일 선택으로 올린
  원본 바이트의 SHA-256 앞 16자와 저장 이름이 같은지 본다. iPhone 파일 선택은 MIME이 빈 `IMG_0001.PNG`도 보낸다.
  같은 그림을 task 업로드 API로 다시 보내면 같은 경로·파일 하나여야 한다. 각 기기의 대화 경로를 task 메모에 붙이고
  `thread start --from-task --link-task` 뒤 실제 작업 폴더의 첨부 바이트·task/brief의 사본 경로·보고를 확인한다.
- E2E23의 「옛 대화 첨부」는 무작위 이름의 fixture가 시작을 막으며 다시 첨부하라는 안내를 내는지, 원본을 다시 올려
  메모 링크를 바꾸면 thread에 같은 바이트가 전달되는지 검사한다. 거부와 복구 모두 옛 원본의 이름·내용을 보존해야 한다.
- m13 `task_attachments_reject_unverified_uploads_without_changing_them`은 옛 무작위 이름과 hash 이름의 원본을 실제로 바꾼
  경우를 각각 거부하고 원본을 보존하는지 본다. 기존 원격 TLS 전달·첫 사본으로 다시 시작 시험도 함께 실행한다.
- host `task_attach::tests::upload_rules`는 hash 이름의 기존 파일을 같은 길이의 다른 바이트로 바꾼 뒤 다시 올려도
  성공하거나 덮어쓰지 않는지 본다. 대화도 이 저장 함수를 쓰므로 두 화면의 보호 규칙이 같다.
- 수정 전 E2E23은 첫 Chrome 붙여넣기의 무작위 이름에서 실패했다. 시간 상한·재시도·해시 검사를 완화하지 않는다.

## 대화 첨부 미리보기 복원(t-0583)

- E2E23의 「대화 이미지」는 Chromium·WebKit 데스크톱·WebKit iPhone·iPad에서 붙여넣기 뒤
  대화 → 터미널 → 대화 → 보고서 → 대화로 이동하고, 드롭·파일 선택 뒤에는 새로고침한다.
  초안의 글·경로 보존, 복원한 썸네일 하나의 실제 이미지 decode, 전송 뒤 원본 SHA-256·경로와 미리보기 제거를 본다.
- 복원한 그림 뒤 새 그림을 붙여 하나씩 삭제하고, 새로고침 뒤 지운 경로·썸네일이 돌아오지 않는지도 확인한다.
  blob URL을 저장하지 않고 초안의 경로를 기존 pane_file 권한 검사로 읽는다.
- 원래 제품에서는 첫 Chrome 대화 → 터미널 → 대화 뒤 경로는 남고 썸네일이 없어 decode 대기에서 실패했다.
  시한·재시도를 바꾸지 않고 DOM 표시·입력 준비·이미지 decode·실제 입력 수신 사건으로 끝낸다.

## 입력칸 줄바꿈 폭 고정(t-0563, plan §8.364)

- 사용자 1A에 따라 글이 길어져도 그림·맥락·보내기 단추를 옆에 둔다. 한 줄/여러 줄 표지는 실제 글 칸의 줄 수이며 배치를 바꾸지 않는다. E2E 22의 단추 아래 줄 조건과 펼친 폭 표지만 이 선택에 맞춰 바꿨다. 짧은 글 높이·단추만 있는 밑 줄 금지·10줄 상한·focus·아래끝·잘림 검사는 유지하고 모든 길이의 글 칸 폭 불변 검사를 더했다.
- 새 `66-composer-wrap`은 실제 host·agent의 Claude pane에서 Chromium·WebKit × 데스크톱·iPhone 네 조합을 돈다. 한글 168음절의 자모 변화·음절 확정 사건을 재생해 매 조합 입력 뒤 크기를 기록한다. 같은 계산 CSS와 폭의 독립 native textarea로 줄 수와 첫 줄이 넘는 위치를 대조한다. OS 입력기를 직접 조작하는 시험은 아니다.
- 영문 단어·긴 영문 토큰·줄바꿈이 있는 붙여넣기·창 축소/휴대폰 회전과 복원·지우기를 검사한다. plain text paste 사건을 제품 handler에 보내고 취소되지 않으면 브라우저 편집 명령으로 기본 삽입을 실행한다. 시스템 clipboard는 건드리지 않는다. 창 변경은 ResizeObserver 사건으로 끝내고 고정 sleep·재시도는 없다. 편집 중 pane input·submit·bin 전송이 없어야 한다.
- `AAA_E2E_SHOTS`에는 같은 입력의 단계별 PNG와 `wrap-geometry.json`이 남는다. 변경 전 네 조합 모두 실제 글 칸 폭과 첫 줄 경계가 바뀌어 RED였다. 변경 후에는 조합 입력 336회씩 폭과 첫 줄 경계가 일정하고 native 줄 수와 일치했다. E2E 35의 전송 중 높이와 51의 지연 커서 keeper도 통과했다.
- 초기 머신의 `cargo build -p aaa --locked`는 빌드 스크립트 SIGKILL로 종료됐고 같은 시각 AMFI 서명 실패와 syspolicyd의 파일 열기 한도 오류를 기록했다. 사용자 지시로 시스템 서비스를 바꾸지 않고 다른 머신의 새 clone·target으로 검증을 옮겼다. 구현 후보 a5100fbb의 정본 ci-local 전체 7단계(웹 유닛 403개·Rust 863개·Python 8개·웹 E2E 65개 파일 전체)와 별도 공개 검사가 통과했다. 기존 수동 ignored 3개는 정본 설정 그대로다. 후보 내장 웹을 시험하며 초기 진단용 Node loader는 쓰지 않는다. 착지 때 main 통합 후보도 새 실험 폴더에서 ci-local 전체와 실제 이름 목록을 적용한 check-public을 요구한다.
- main의 `63-deploy`·`64-automation-failure`·`65-account-quota`와 겹치지 않게 `66-composer-wrap`으로 옮겼다. FRONT_API·AGENT_LINK나 기존 다른 E2E의 번호·시한·단언은 바꾸지 않는다.
- 통합 후보의 첫 전체 실행에서 E2E 51 준비가 실패했다. 입력칸은 그려졌지만 새 pane의 WebSocket이 아직 readonly를 해제하기 전이었고 가짜 agent의 수신은 0바이트·대화 기록도 비어 있었다. 준비에서 첫 Enter 전에 제품 보내기 단추의 disabled 해제 사건을 기다린다. 입력은 한 번만 보내며 답과 38% 게이지·지연 커서 단언·20초 상한은 유지한다. 연결 전 Enter가 글을 두고 보내지 않는 제품 계약은 기존 E2E 08이 계속 검사한다.
- 수정 후보에서 51은 통과했으나 E2E 25의 빠른 pane 전환도 같은 준비 누락을 드러냈다. Codex 수신은 앞 단계의 6바이트 그대로였고 새 입력의 submit은 없었으며 키 입력 뒤에 새 snapshot·readonly 통지가 왔다. 터미널은 현재 pane 이름·data-readonly=false, 대화는 기존 23처럼 그림 첨부 단추의 disabled 해제 사건을 기다린다. 두 대화 진입 helper를 함께 정리하며 Ctrl+Enter 키·기대 바이트·host 판정·시한은 유지한다.
- 이어진 전체 실행의 E2E 61은 safety 충돌 뒤 새 판 읽기가 끝나기 전에 입력을 다시 여는 제품 경합을 잡았다. SafetyPage는 재조회 Promise가 끝날 때까지 busy를 유지한다. 기존 충돌 시험에서 실제 safety_show 답만 gate로 보류해 그동안 선택이 꺼져 있는지 검사하고, 해제 뒤 원래 user 값 저장·사용자 주석 보존을 확인한다. 재조회 대기를 제거하면 새 판을 읽기 전 입력 금지 단언이 실패해야 한다. host의 판 대조·권한·기대값·시한은 바꾸지 않는다.

## 질문 카드와 확인 필요 post 묶기(t-0565)

- `node --test web/test/asksModel.test.mjs`는 제목의 `aaa:question/<thread>`가 열린 카드와 일치할 때만 post 여러 개를
  질문 한 줄로 묶는지 본다. 답한 카드의 늦은 알림도 묶는다. 카드 없음·보고서 참조·본문에만 참조가 있는 post는 그대로 남긴다.
- `cargo test -p aaa-host --lib asks::tests --locked`는 카드 대신 post를 처리하는 API 거부, 답 전송 경로의 거부 뒤
  질문·post 보존, 연결 post만 처리, 전송 중 새 알림 보존, `request handled`와 답한 질문 하나를 검사한다.
  `post handled`는 원래 발신 ID·화면 p-번호로 같은 항목을 처리하고 반복해도 한 번 기록한다. `inbox done`도
  post 목록·status의 처리 상태에 반영돼야 한다. 둘 다 질문 답·선택·승인을 만들지 않는다.
  옛 버전이 남긴 post는 새 보고로 질문이 닫힌 시각이나 성공한 답 기록으로 정리한다. 임시 답 claim은 성공으로 보지 않는다. 끝난 카드에 늦게 도착한 알림은 즉시 닫고 다음 질문의 알림은 유지한다.
- `node web/e2e/web.e2e.mjs --spec 53-thread-questions`는 실제 host·agent에 질문 카드와 연결 post 두 개를 만든다.
  수정 전은 빨강 배지 1개 검사에서 실패한다. Chromium 데스크톱, WebKit iPhone 390 Safari·430 홈 화면 앱에서
  한 줄·한 배지, 카드 답 뒤 post 둘의 inbox done과 `request handled`, 「답한 것」 한 줄을 확인한다.
  실제 coordinator post에만 있는 C는 `thread answer --text`로 원문을 전하고 A/B 선택지 수·선택하지 않은 picks를 보존한다.
  `post handled`만으로 카드 답이 생기면 실패한다. 새 보고로 질문이 닫히면 post도 닫히고 답 없는 닫힘 기록을 남겨야 한다.
  원문·그림 보존·새 질문·보기 권한·독립 post의 웹 처리·다른 기기에서 기록 지우기도 기존대로 검사한다.
- `cargo test -p aaa --test m13 worker_session_cli_is_read_only --locked`는 실제 worker pane의 새 `post handled` 명령도
  다른 쓰기와 같이 거부하는지 본다. 새 Front 요청은 조작 권한이 필요하고 승인·배포 요청과 연결하지 않는다.
- 외부 푸시의 참조 해석은 그대로다. `cargo test -p aaa-host --lib post::tests --locked`의 기존 사용자 post 시험이
  coordinator 검증·푸시 큐 유지·제목의 참조 제거와 `/#question=<thread>` 목적지를 확인한다.
  iOS 실제 푸시 수신과 시스템 Safari 자체는 헤드리스 시험이 보증하지 않는다.

## task 예약·완료 보관(t-0564)

- host `task_queue::tests`는 ID의 원문/CRLF 보존·편집 후 유지·중복 거부, coordinator 선택 변경 거부와 사용자 직접 변경,
  12개의 동시 DB 선점에서 6자리 상한·resolve_pending 보유, 내용 변경 뒤 확인 필요·취소의 재개방 보존을 검사한다.
  완료 보관은 실제 SQLite INSERT 실패 trigger에서 활성 파일·미판정 상태가 보존되는지, 정상 저장 뒤 같은 ID·본문·결과·첨부,
  이력 prune 뒤 보관 보존·같은 ID 복원·기동 연결 해제·재예약 없음도 확인한다. 완료 결과와 메모의 보고서 파일은 실제 기한 정리/삭제에서 보호되고, 관련 없는 사본은 정리되는지도 검사한다. 고정 sleep은 없다.
- m13 `task_queue_runs_possible_fifo_after_resolve_without_coordinator`는 실제 host·agent·CLI·PTY로 6개의 자리를 채우고,
  보고 [완료]·host 재시작에도 세 예약의 새 기동이 없는지 본다. 가장 늦게 등록한 예약을 CLI `queue-move`로 1번에 옮기고,
  저장된 차례·생성 시각 보존과 재시작 뒤 차례를 확인한다. resolve 완료 사건 뒤 coordinator/웹/수동 tick 없이 옮긴 1번의
  실제 pane 기동과 다른 예약의 대기를 확인하고, 그 thread도 resolve한 뒤 고정 머신이 끊긴 앞 예약을 남기고 가능한 뒤 예약의
  실제 pane·같은 task ID 연결을 기다린다.
- host `slot_released_during_queue_scan_does_not_bypass_the_moved_front`는 앞 예약의 `자리 대기` DB 기록 직후
  다른 thread가 자리를 반환하는 경합을 임시 SQLite trigger로 고정한다. 같은 순회의 뒤 예약이 머신 선택·기동으로 넘어가면
  실패하며, 다음 순회에는 앞에서부터 새 자리를 판정한다. 원래 코드는 뒤 예약만 머신 가용성 판정으로 넘어가 실패한다.
  기동 중 예약의 상태 대조·내용 검사는 계속하며, 끊긴 머신 등 시작 조건이 막힌 앞 예약을 건너뛰는 계약은 유지한다.
- 기존 m13 `parallel_threads_enforce_capacity_before_launch`는 한도 초과 직접 요청의 실패·thread 수·pane 수를 함께 본다.
  `task_thread_link_and_auto_clean`은 기존 확인 필요·부모 보호·옛 delete restore를 유지하며 완료 보관의 ID·본문·보고서·복원을 관측한다.
- 웹은 실제 host의 예약 상태와 완료 보관만 읽는다. 실패의 상한·재시도·기대값을 바꿔 통과시키지 않는다.
- 추가 source owner는 기본 모델 칸이 비어 있어도 승인된 선택을 coordinator가 바꾸지 못하는지, 직접 파일 수정 뒤
  coordinator의 재예약이 거부되는지, 사용자 API의 선택 변경만 새 출처로 기록되고 명시 갱신 전에는 과거 승인으로 실행되지 않는지 본다.
  기동 전 중단의 자리 회수/옛 송신 불명 보존·실패 예약 취소 후 tick 무변경·파일 제거 뒤 DB 확정 전 중단의 복구도 사건으로 확인한다.
- 웹 E2E `67-task-queue`는 데스크톱 예약 확인·두 기기의 같은 차례·휴대폰 맨 앞으로/취소·새로고침 보존·resolve 뒤 실제 기동·
  완료 보관/같은 ID 복원을 본다. 기존 47은 한도 우회 단추 부재와 완료 보관의 본문/결과/ID를 검사한다.
- 시작/재시작은 같은 기동 오류 판정을 쓴다. 송신 결과 불명은 Starting으로 자리를 유지하고, 확실한 새 기동 실패는 원인과 함께
  Failed로 남긴다. 이미 가진 Open 자리를 재시작할 때는 기동 전 중단에도 그 원래 자리를 유지하며 중복 선점하지 않는다.
- 완료 보관을 복원한 뒤 제목을 바꾸고 다시 완료하는 경우도 검사한다. 일반 이력이 정리된 뒤에도 이전 완료의 본문·thread와 그 결과물 참조는 보관 이력으로 이어진다.

## 요약 전체 작업 진행상황(t-0574, 사용자 1A)

- `cargo test -p aaa-host --lib summary:: --locked`: 완료·진행·대기가 섞인 다섯 작업의 순서, 대문자 체크, 잘못된 표지·빈 이름·여섯 항목·30자 경계와 옛 요약의 빈 진행 목록을 확인한다.
- `cargo test -p aaa-proto --locked`: 옛 host 응답의 누락된 `progress`는 빈 배열이고, 기존 now·done은 보존됨을 확인한다. Rust 정본과 생성 TypeScript도 검사한다.
- 기존 m13의 대화·확인창 흐름은 실제 CLI의 요약 쓰기/읽기로 순서·세 상태의 JSON을 확인하고, 새 지시를 보내도 저장된 목록이 바뀌지 않음을 확인한다.
- `node web/e2e/web.e2e.mjs --spec 28-thread-summary`: 기존 옛 요약·hover 위치·닫힘·고정 줄·휴대폰 표시를 그대로 실행한다. 새 단계는 실제 host·agent·CLI가 쓴 다섯 작업을 hover에서 모두 읽고, 열린 카드 중 요약 갱신→새 완료 개수와 상태→새 지시 경고의 DOM 사건을 기다린다. 전체 완료와 목록 없는 요약으로 바꾸어 열린 상세의 캐시가 바뀌는지도 본다.
- 같은 E2E의 실제 Reader 기기로 Chromium 320px·WebKit 390px 상세를 확인한다. 긴 이름은 hover에서 한 줄 생략, 휴대폰에서는 줄바꿈한다. 시트 움직임은 `getAnimations().finished`로 끝을 기다리고 가로 넘침을 잰다. Reader의 직접 쓰기 요청도 거부하고 원본을 보존해야 한다. 고정 sleep·시한 연장·상태 추측은 없다.

### 착지 검증의 파일 링크 좌표(t-0574)

E2E 16은 첫 snapshot의 글이 보인 직후 링크 좌표를 재면 뒤늦은 초기 resize와 경주할 수 있다.
xterm은 buffer resize에서 현재 링크를 지우고 다음 mousemove에서 다시 찾는다. 실제 pane resize 요청을
소켓 gate로 보류한 진단에서 `xterm-cursor-pointer`가 resize 뒤 사라지고 새 마우스 이동 뒤 돌아옴을 확인했다.
기존 실패는 출력이 모두 있는 상태에서 `src/long.ts`의 링크 커서만 20초 안에 오지 않았다.

`clickLink`는 이 화면의 `data-fit="same"`·pending 없음과 실제 그린 행 수까지 맞춘 뒤 좌표를 잰다.
원래의 커서·클릭·문법 강조·매체·폴더·설정·가상화 단언과 20초 시한은 유지한다. 실패 재실행이나
시한 연장으로 넘기지 않으며, 진단과 좁은 E2E 16 뒤 수정한 통합 후보의 전체 ci-local을 실행한다.

## coordinator 질문 카드·짧은 질문 ID(t-0576, 1단계)

- `cargo test -p aaa-host --lib question_ids::`: 프로젝트별 q-1000 시작, 같은 대상의 동시 발급,
  다음 보고 판의 새 번호, DB 재개, 답한 것 삭제 뒤 순번 보존을 검사한다. 구조화 post는 본문 hash·선택지 검증,
  ID별 부분 답과 긴 자유 답의 원문·선택 보존·library 사본, 두 기기의 동시 제출 한 건, 평문 post의 ID 처리 표시를 본다.
  완료한 부분 답 초안은 비우고 ID 원장만 남긴다. DB 종료의 `done=0` 조건을 제거한 변형은 중복 답 단언에서 실패해야 한다.
  변형 원본 bytes를 복원한 뒤 정본 전체를 검증한다.
- `cargo test -p aaa-host --lib short_question_ids_reach_push_titles`: push 제목의 q-ID,
  기존 thread 링크와 새 q-ID 링크, 앱 참조가 알림 문구에 남지 않는 것을 검사한다.
  기존 post privacy keeper는 새로 추가된 ID까지 정확히 비교하며 제목의 경로·token 가림과 평문 기본 링크를 유지한다.
- `node web/e2e/web.e2e.mjs <캡처 폴더> --spec 69-coordinator-questions`: 실제 host·agent·CLI를 쓴다.
  두 종류의 카드는 같은 QuestionCard에서 렌더링하며 질문 DOM 구조를 비교한다. 데스크톱·WebKit 휴대폰 각각
  밝음·어두움 캡처, Markdown 표·그림, 추천·선택·직접 답, 다른 기기의 답 보존, Reader의 두 답 경로 거부,
  `thread answer q-ID A`·`question answer q-ID B`·`post handled q-ID`, 평문 호환을 본다.
  닫히거나 지워져 찾을 수 없는 q-ID를 열 때는 다른 질문을 대신 열지 않는다.
  그림은 실험실의 SVG와 publish 사본뿐이다. 이미지 decode·DOM 표지·host 상태로 끝나며 고정 sleep을 쓰지 않는다.
- 기존 E2E 53은 thread 질문 알림 여러 건을 한 카드로 묶기, thread로 답 전달, 늦은 알림·다음 보고,
  평문 post와 보존·삭제를 계속 검사한다. `web/test/asksModel.test.mjs`는 q-ID 링크와 기존 thread 참조의 같은 카드 연결을 본다.

전체 정본은 `bash scripts/ci-local.sh`이며 공개 검사는 새 파일을 추적한 상태에서 `bash scripts/check-public.sh`로 확인한다.
운영 host·agent·설정·인증에는 닿지 않는다. 보고서 r-ID는 별도 2단계다.

### 페이지 종료 뒤 예약 조회 재개(t-0584 착지, 사용자 1A)

E2E23의 「재로딩 중 예약된 자동화 조회」는 WebKit의 첫 `projects` 요청을 route gate로 붙잡고 events 연결·첫 snapshot의
갱신을 합친 뒤 reload한다. 종료로 첫 요청이 취소된 뒤 `finally`가 예약된 조회를 다시 보내던 원래 제품은
`Fetch API cannot load … due to access control checks` 페이지 오류에서 실패한다. `pagehide`에서 후속 조회를 막는 제품은
오류 없이 새 문서를 연다. WebKit의 요청 취소는 `pagehide` 전에 올 수 있어, 실패한 읽기의 `catch`도 예약을 비워야 한다.
종료한 문서의 갱신 사건은 fetch를 보내지 않고, `pageshow`는 실제 새 `projects` 요청으로 끝난다.
route gate는 service worker를 차단한 실험실에서만 쓴다. 고정 sleep·시한 연장·페이지 오류 무시는 없다.
기존 E2E23의 그림 전송·복원·해제·권한·Chrome/Safari/iPhone/iPad 검사는 그대로 함께 돈다.

## 머신 상세(t-0578 웹 머신 추가 M0, plan §8.372)

- host 유닛 `cargo test -p aaa-host machine_view_tests`: 실제 host DB를 연 실험실에서 hello(CPU 종류·판·SHA·기능·시작 점검)가 `MachineView`에 실리는지, 다른 판으로 다시 붙으면 `software_differs`, 모르는 꼴의 점검은 버리는지 본다. runtime을 끝내 Host가 닫힌 것을 확인한 뒤 같은 상태 폴더로 다시 열어 판·SHA·CPU 종류는 DB에서 남고 기능·점검은 빈 것을 본다.
- 웹 유닛 `node --test web/test/machineDetail.test.mjs`: 줄 요약(옛 host는 OS만), 판 주의·SHA 고정폭·「모름」, 연결·끊김의 마지막 본 때, 건강은 연결 중일 때만, 시작 점검.
- 웹 E2E `60-admin`의 「머신 상세(M0)」: 등록해 붙인 둘째 agent의 `aaa machine list` SHA가 실행 파일(`AAA_BIN`)의 SHA-256과 같고, 줄 요약·「자세히」의 SHA·판·연결·기능 목록이 그 값을 보인다. 보기 기기도 「자세히」를 열고, 휴대폰 390은 상세를 펼친 채 가로 넘침이 없다. 기다림은 화면 표지와 host 답이다.

## aaa install I6 승인·실제 executor owner(t-0485, plan §8.374)

- `cargo test -p aaa --test install --locked`: 공통 실제 CLI/PTY owner는 세 OS에서 같은 마법사·plain/inquire·뒤로·초안·정확 yes·EOF130·자동/세션 거부·private 발급·실제 worker·같은 ID status/resume를 검사한다. 실제 TLS host 등록/SAS/응답 유실 조회·외부 pin 편집 보존, require unknown→새 defer 승인/파일 job 재사용, 첫 Controller 실제 redeem/폐기·코드 비공개를 포함한다. 운영 서비스/계정/암호/LSA는 사용하지 않는다.
- `cargo test -p aaa --bin aaa install:: --locked`: 실제 private 파일·서명/pin/만료/nonce·파일 scope·execution/job 결속·I2/SCM 상태·보호 입력·PATH 정책을 검사한다. 독립 v1 status/exit validator가 needs_user3와 cancelled130을 소유한다. 논리 clock은 승인 만료에만 쓰고 실제 쓰기는 OS 파일 transaction이다.
- `cargo test -p aaa-host --lib installation:: --locked`, `cargo test -p aaa-host --lib tailscale:: --locked`: 실제 host DB/서명/private 입력과 TLS peer·grant 일회 소비/폐기, 보호 publication의 PID/image/nonce/phase/실제 owned listener, exact 포트/외부 mapping 보존, bounded LocalAPI wire의 ETag/If-Match/412, 현재 host HTTPS proof와 일반 200 거부를 검사한다. publication 소비자의 입력 fixture는 파일 엔진 생산 완료 증거로 쓰지 않으며 실제 CLI owner가 생산 경계를 검사한다.
- `cargo test -p aaa-host --lib install_agent_query_keeps_the_handshake_key_after_same_id_rekey --locked`: 첫 메시지 전 key 교체 때 실제 연결의 immutable peer를 유지하고 다른 key의 결과를 내주지 않는다.
- Windows 전용 owner는 실제 ConPTY 암호 mode/echo/성공·취소·EOF, Job의 breakaway 허용/금지와 부모 소멸, Win32 ready 파일 변경 사건, 임시 HKCU Software lab key의 registry transaction rollback/동시 편집이다. Windows API 차이 때문에 cfg(windows)이며 공통 CLI/권한 owner는 제외하지 않는다. Unix 부모 감시는 macOS kqueue/Linux pidfd를 사용한다. SSH 신뢰 unit은 봉인된 경계만 실험실에서 대체하며 실제 OpenSSH 설치 실행 성공을 주장하지 않는다.
- 서명·계획/소비·파일 scope/job 연결·worker 분리 수명·정확 Serve의 If-Match/선택 포트·HTTPS instance proof·TLS rekey/소비/PATH 보존 변형을 owner에서 RED로 대조하고 바이트 복구 뒤 검증한다. 실패 재실행·timeout 증가·기대값 완화는 하지 않는다.

Windows/Linux 실제 실행 판정은 main push 뒤 GitHub CI windows-2025/ubuntu-24.04다. 실제 SCM logon/LSA/Session0/UAC/재부팅/실제 서비스 harness 인증·Tailscale HTTPS는 사용자 Windows PC 사용자 인수로 따로 확인한다. 교차 clippy/build와 file/PTY owner를 이 실제 설치 증거로 부르지 않는다.

## 자동화 thread의 별도 실행 자리(t-0586)

- host `task_queue::tests::automation_threads_have_atomic_separate_slots_and_restart_limits`는 Barrier 뒤 12개 DB 선점을 경주시켜 사용자 자리 1개·자동화 규칙 자리 1개만 허용한다. 같은 actor인 사용자도 세고 정리 중 자동화·실패 재시작·상한 초과 재시작·자리 반환을 확인한다.
- host `automation::start_tests`는 다음 사건을 기본 동시 1개에서 내구 대기하고 명시 2개로 넓힌 뒤 처리하는지, 같은 사건 영수증·0의 기본 상한·DB 최종 선점·기존 holds를 검사한다. 기존 reuse keeper는 열린 thread로 이어 보내기·기억·완료 뒤 쉼을 그대로 검사한다.
- m13 `automation_start::automation_threads_leave_user_capacity_and_queue_slots_available`은 실제 host·agent·CLI로 사용자 한도가 차도 자동화를 시작하고, 사용자 직접 시작은 막으며, 사용자 resolve 뒤 자동화가 열린 채 다음 예약이 실제 pane으로 시작하는지 확인한다. CLI list/show/context 표지·출처, host 재시작 뒤 보존·자동화 재시작도 본다. 기존 자기 사건 거르기 keeper는 명시 상한 2개로 두 원인을 비교한다.
- 웹 E2E `48-automation-threads`는 기존 실제 규칙 기동·막대·규칙 이동을 유지하며 데스크톱·휴대폰에 같은 「자동화 thread」 표지, 모든 thread 목록, thread 상세의 규칙/일정/자리 제외 설명을 검사한다. 보고·DOM·기동·연결 사건으로 끝내며 고정 sleep·시한 연장은 없다.
- 일반 자리에서 자동화 제외를 제거한 변형과 규칙 상한을 제거한 변형이 새 DB keeper를 실패시키는지 확인한 뒤 원본 bytes를 복구한다. 정본 전체 검증은 `bash scripts/ci-local.sh`다.

착지 통합 후보는 t-0584의 `slot_released_during_queue_scan_does_not_bypass_the_moved_front`를 자동화 thread 없음/열림 두 경우에 실행한다. 사용자 자리가 순회 도중 반환돼도 뒤 예약은 추월하지 않고, 다음 순회에서는 자동화 thread를 자리에 넣지 않아 앞 차례부터 조건을 다시 확인해야 한다.

## 「직접 설치」 안내(t-0578 웹 머신 추가 M1, plan §8.383)

- host 유닛 `cargo test -p aaa-host enroll_guide`: 연 주소(모든 주소·tailnet 주소·LAN·loopback·이름)별 주소 후보와 순서.
- 웹 유닛 `node --test web/test/enrollGuide.test.mjs`: 1.92 판정, 이름·주소 글자 규칙, 명령에 주소·이름·판·지문, 쓸 수 없는 값은 자리표시.
- 웹 E2E `60-admin` 첫 단계: 안내를 펼쳐 실험실 host가 연 loopback 주소가 후보로 오고 「host 머신 안에서만」 경고, 이미 있는 이름(labA) 알림, 둘째 단계 명령이 `~/.local/bin/aaa agent enroll <주소> --name labB`인지 본 뒤 실행 파일 자리만 실험실 binary로 바꿔 그대로 실행해 등록한다. 안내가 보인 host fingerprint 앞 16자가 새 머신 `agent enroll` 화면의 것과 같아야 한다. Reader에는 안내가 없고, 휴대폰 390은 안내·상세를 펼친 채 가로 넘침이 없다.

## SSH로 머신 추가(t-0578 웹 머신 추가 M2, plan §8.384)

- 실험실 sshd: 시험이 임시 폴더에 host key·사용자 키·`authorized_keys`·`sshd_config`(loopback 빈 포트, 비밀번호·PAM 끔, `StrictModes no`)를 만들고 `sshd -f <config> -D -e`를 자식으로 띄운다. stderr의 `Server listening` 줄이 나오면 시작한 것이다. host는 `[host] ssh_config`의 실험실 설정(별칭 `lab`, 닫힌 포트 `lab-down`, `IdentitiesOnly`)만 쓴다. 같은 사용자 로그인뿐이지만 「자기 자신을 머신으로」 접속하기에 충분하다. 시험이 띄운 sshd 자식만 끝낸다. sshd가 없으면 건너뛰고 그렇게 적는다.
- host 유닛 `cargo test -p aaa-host machine_setup`: 순수 규칙과 실험실 끝까지(처음 키 → 세션 안·Reader·다른 지문 거부 → 믿음 → aaa known_hosts 한 줄 → 감지 = 이 빌드의 OS·CPU → 다시 하면 아는 키 → 닫힌 포트 까닭 → known_hosts를 다른 키로 바꾸면 changed로 멈춤 → Reader 목록·지우기). 단계 대기는 host `enroll` 사건이다.
- 웹 유닛 `node --test web/test/machineSetupModel.test.mjs`, 웹 E2E `72-machine-add`(데스크톱: 비밀번호 꼴 막음, 지문이 `ssh-keygen -lf`와 같음, 믿음 확인 창, 준비됨·감지 줄, aaa known_hosts 한 줄, 닫힌 포트 까닭 / 보기 기기: 조작 없음 / 휴대폰 390: 폭·지우기).
- Windows: `windows_runner_reports_whether_an_openssh_server_exists`가 CI 로그에 sshd.exe 유무만 적는다(실패하지 않는다).

## 병합 대기·병합 중 실행 자리(t-0594, plan §8.385)

- `cargo test -p aaa-host --lib report_mark::tests --locked`: 첫 줄·공백·BOM/CRLF·중복/붙임 표지·본문·완료/다음 단계 경계.
- `cargo test -p aaa-host --lib report_stages --locked`: 같은 SQLite의 여덟 동시 요청이 마지막 한 자리만 선점하는지, 병합 대기·착지 한 자리만 제외하며 자동화의 착지는 그 한 자리를 빼앗지 않는지, 착지 실패 수리·작업 복귀 초과·정리 중/완료를 검사한다. 옛 JSON과 실제 보고 사본으로 Host를 다시 열어 marked 보고는 이행하고 unmarked/완료·hash 불일치·유실은 보수적으로 세는지도 확인한다. 운영 폴더는 읽거나 쓰지 않는다.
- `cargo test -p aaa --test m13 thread_stages:: --locked`: 격리 host·agent·CLI·가짜 harness에서 한도가 찬 상태의 예약 두 건을 순서대로 준비한다. 병합 대기 보고 회수 사건이 첫 예약을, 그 예약의 착지 보고가 둘째 예약을 시작해야 한다. 단계를 알기 위해 ticker를 반복 호출하지 않는다. CLI 세 출력·예약 DTO·ack 보존·다음 단계 복귀 시 새 기동 거부/기존 작업 보존·host 재시작 뒤 같은 단계를 검사한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs --spec 67 --workers 1`: 기존 예약 순서·취소·다른 기기·완료 보관을 유지하고, 실제 보고 파일→host 단계→예약 기동→데스크톱/390px 할 일의 연결·미연결 표지와 다른 색·사이드바·모든 thread·예약 상세를 확인한다. 스크린샷은 시험 산출물에 남긴다. DOM·보고·기동 사건으로 끝내며 고정 sleep·시한 연장은 없다.

병합 대기 제외를 제거한 제품 변형은 DB 선점 keeper가 실패해야 한다. 변형 뒤 제품 bytes를 복원하고 정본 `bash scripts/ci-local.sh` 전체와 공개 검사를 실행한다. 실제 동시 착지 명령을 강제로 중단하는 기능은 아니며 coordinator가 착지를 하나씩 시작한다.


## 보고 판 r-ID·실행 입력 이름(t-0576, 2단계)

- `cargo test -p aaa-host --lib report_ids`: 프로젝트별 동시 예약·재개·삭제 후 순번·실패 예약의 비노출, 옛 r 이름과 충돌 방지, 첫 기동 등록, q/r의 프로젝트 이름 변경 보존을 본다. 같은 이름을 두 번 발행하고 옛 r의 본문·그림·CLI 열람·공유 링크·삭제·Reader 거부를 본다. thread 두 판의 옛 Markdown·그림·inbox r 번호와 미복사 파일의 최신 agent 대체 금지도 본다.
- `cargo test -p aaa-host --lib library_links::`: 기존 서명·범위·권한·만료·기기 폐기·삭제·보존 규칙을 지킨다. host 용량은 불변 판까지 정확히 세며 live 원본의 나중 증가는 상한 밖, 등록 당시 사본은 보호 용량임을 구별한다.
- `automation::runtime::input_names_tests`: 옛 inflight와 새 run 이름을 직렬화·재개해 같은 processed 영수증 이름을 유지한다. E2E 49는 실제 자동화 시작의 run 파일(64자 상한)과 processed.txt를 확인한다.
- `node web/e2e/web.e2e.mjs <캡처 폴더> --spec 73-report-ids`: 실제 host·agent·CLI의 같은 이름 재발행·thread 보고 변경, 이전 판 원문·그림, 프로젝트 없는 r hash·기존 이름·t 번호·판 선택·inbox·Reader 거부와 390/1280px 밝음·어두움 머리를 본다. 특정 hash의 thread 사건·DOM·이미지 decode를 기다리며 고정 sleep을 쓰지 않는다.


## 질문 안 자료 보기·기본 검수 카드(t-0576, 3단계)

- `node --experimental-strip-types --test web/test/questionMaterials.test.mjs`: 본문·목록·표의 같은 보고 r 링크를 한 번만 세고 그림을 중복 자료로 만들지 않는다. 음원·HTML은 안전한 질문 파일 경로로만 풀며 code·API·로컬 절대 경로·실행 scheme·경로 이탈은 거부한다.
- `cargo test -p aaa-host --lib report_mark::`: Questions 없는 검수 대기만 기본 카드로 만들며 명시한 질문을 보존하고 A를 추천하지 않는지 본다. report_ids keeper는 기존 검수 보고를 한 번만 q로 등록하고 B 글 누락 거부 뒤 답·부분 답을 선점하지 않음을 본다.
- `node web/e2e/web.e2e.mjs <캡처 폴더> --spec 74-question-review`: 실제 publish·coordinator post·thread 보고를 쓴다. 카드 안 보고서·그림·음원과 자동 재생 없음·MIME·원래 질문 주소·390px WebKit/1280px 밝음·어두움, 선택/글 답과 inbox·새 보고 뒤 답한 r 판의 옛 그림을 확인한다. 소리 출력·운영 자료·고정 sleep을 쓰지 않는다.
- 보고 판 등록의 격리는 독립 keeper로 지킨다. 일반 Host::open에 사본 등록을 되넣으면 등록 전 비어 있어야 할 ID 단언이 실패하고, DB 소유 필터를 빼면 다른 state의 원문을 채택하지 말라는 단언이 실패해야 한다. 기본 경로의 프로젝트를 시험 fixture로 사용하지 않는다. 실제 host 기동의 실험실은 설정에 임시 projects root를 명시한다.

E2E 74의 headless WebKit iframe 그림은 기존 E2E 59·plan §8.353과 같은 이중 sandbox 제약을 적용한다. iframe의 그림 노드를 확인하고 같은 cap 주소의 실제 이미지를 앱 크게 보기에서 decode한다. Chromium iframe과 질문 본문의 그림은 직접 decode한다. 정책·시한을 바꾸거나 성공 여부를 추측하지 않는다. WebKit을 별도로 띄운 실험실은 실패 때도 finally에서 그 browser를 닫는다.

`report_ids::tests::archived_r_attachment_protects_its_entire_report_family_from_delete_and_prune`는 완료 보관의 옛 r 첨부가 같은 이름의 모든 판을 명시 삭제·기한 정리에서 보호하는지 검사한다. 기존 m13 library_view의 최신 t 별칭 전체/Range 스트리밍을 유지하면서, 같은 파일의 r 보기만 미복사 파일의 기존 보기 경로 오류(502)와 명시한 까닭을 확인한다. 최신 별칭과 불변 판의 읽기 계약을 구분한다.

전체 캡처를 켠 E2E 06은 설치 글꼴 검사에서 시작한 가짜 face를 아직 gate로 붙잡은 동안 screenshot이 document.fonts.ready를 기다려 막혔다. 보류 전의 크기·값·거부 단언은 그대로 두고, 캡처를 face gate 해제와 그리드 정합 확인 뒤로 옮긴다. 시한·기대값·제품 로딩 정책은 바꾸지 않는다.

착지 main 통합 뒤 ThreadView의 단계·자동화 출처·r 번호가 합쳐져 FrontEvent의 enum 크기 차이가 Clippy 상한을 넘었다. Thread 사건·응답 본체를 Box에 두어 JSON/생성 TypeScript 형식을 유지하고, 전체 WS/E2E·Rust 사건 수신 시험과 Clippy를 다시 검증한다.

## 배포 감시의 웹 응답·재시작 rollout(t-0538 관리 GUI W6c, plan §8.402)

- `cargo test -p aaa-agent --lib deploy::`: `web_probe`는 가짜 웹(TcpListener)으로 짝 없는 기기 답(401 `unauthorized`)만 답으로 보고 요청 줄·Host `127.0.0.1:<port>`를 확인한다. 안전 모드 503·421·JSON 아닌 401·잡음, 닫힌 port, 받고 답하지 않는 웹(한도 200ms로 줄여 사건으로 끝냄), 주소 아님은 답이 아니다. `0.0.0.0:<port>`는 loopback으로 묻는다. `guard`의 웹 판정은 실행 신원이 맞아도 웹 주소가 있으면 AwaitingNew·`pending_web`, 웹이 답한 뒤 NewReady·확정, 같은 한도에 복구(늦은 답은 복구를 되돌리지 않음), 주소 없음·빈 주소·agent 감시는 실행 신원만, 한도를 늘리지 않음을 순수 전이로 본다.
- `cargo test -p aaa-host --lib rollout::`: 웹은 개별 `deploy_start`(교체·재시작)를 모두 `forbidden`, coordinator 개별 재시작은 `bad_request`. 재시작 rollout은 끊긴 대상에서 아무것도 남기지 않고, 붙은 agent의 hello 실행 신원을 단계 baseline·rollout SHA·판으로 고정하며, 같은 ID(대상을 비우면 host 머신)는 같은 기록, 다른 서비스는 거부, 진행 중이면 새 재시작 거부, 두 머신 거부, 신원을 알리지 않는 agent는 `unsupported_version`, host 재시작은 host 자신의 신원. 가짜 agent(duplex link)가 감시 세우기에 답한다: 세운 영수증 뒤에 `guard_ready`, 설치 파일이 실행 판과 다르다는 `conflict`면 웹은 `conflict`이고 기록은 재시작 없이 중단, 같은 ID 동시 요청은 기록과 감시가 하나, 첫 세우기의 답이 끊기면 진행 루프가 같은 정본으로 다시 세우고 그때 `conflict`면 다른 요청(재시작) 없이 중단하며 그 감시를 취소(rollout 변경 알림으로 기다린다). agent `guard` 순수 전이: 재시작만 하는 감시(기대 판 = 배포 전 판)는 설치 파일이 바뀌어도 띄우기 전 취소·시한이 취소, 띄운 뒤 시한은 복구.
- `cargo test -p aaa --test m13 rollout_preflights_all_targets_and_resumes_same_jobs_after_host_restart`(macOS): 웹을 연 실험실. 첫 rollout의 새 host를 `AAA_TEST_WEB_HOLD_ME`(debug 빌드에서 `/api/v1/me`가 답하지 않음)로 띄우면 host 감시가 그 host의 웹 주소(`status`의 web_listen)를 `pending_web`로 두고 awaiting_new에 머문다 → 같은 판을 그대로 띄우면 이어 간다. 뒤의 모든 host 교체·복구도 웹 응답을 거친다. 끝에 coordinator `aaa service restart --role host`가 재시작 rollout을 만들고, 가짜 launchctl 기록 뒤 같은 실행 파일로 host를 다시 띄우면 done·감시 verified_new·실행 파일 그대로·재시작 한 번. 실험실은 agent port만 고정한다(웹은 시작마다 OS가 고른다).
- `cargo test -p aaa --test m13 coordinator_deployment_is_gated_and_survives_backend_restarts`(macOS): coordinator `aaa service restart --role agent --machine labB`는 재시작 rollout, 기록 뒤 같은 실행 파일로 agent를 다시 띄우면 done. 감독자 재시작 실패를 넣으면 작업을 다시 보내지 않고 감시의 복구 재시작 한 번 뒤 recovery_failed(오류에 주입한 실패). 이어 개별 update로 설치 파일을 바꾼 뒤 `service restart`는 stderr에 「감시 밖 개별 재시작」을 남기고 개별 재시작 작업(같은 ID, `update status --wait` completed, 재시작 기록 한 번)이며 그 ID의 rollout은 stopped(오류에 `aaa update status <ID>`).
- `node web/e2e/web.e2e.mjs --spec 63-deploy`: 「서비스 재시작」 확인 창(감시 안내) → 「배포 기록」에 「agent 재시작 ·」 카드 → 가짜 launchctl 기록. 옛 agent 그대로면 끝나지 않고, spec이 감독자 대신 agent를 다시 띄우면 done. 보기 전용 기기도 그 카드를 본다. 실험실 정리는 감시 프로세스를 명령줄의 `deployment-guard`와 실험실 상태 폴더로 확인한 PID만 끝낸다.

## 조작 기기 passkey(t-0538 관리 GUI W7, plan §8.410)

- `cargo test -p aaa-host --lib passkey::`: ring으로 만든 소프트웨어 인증기(ES256·EdDSA 키, 손으로 만든 COSE·attestation object CBOR)로 짝짓기 → 10분 안 등록 → 공개 주소의 승인(`web_pair`·다른 기기 폐기)을 끝까지 돌린다. loopback 화면은 passkey 상태가 없고 바로 승인, 공개 주소는 passkey 없으면 `passkey_required`(등록 안내), 읽기·제 기기 폐기는 묻지 않는다. challenge가 op ID와 요청 hash만 묶는지, 같은 서명을 다시 보내면 실행하지 않는지, UV 없음·다른 화면 주소·다른 RP·다른 challenge·남의 키 서명·모르는 credential은 거부하고 그 op도 버리는지, 다른 기기가 op를 마치지 못하는지 본다. 등록은 loopback·보기 기기·UV 없음·다른 credential ID·10분 지남(시작 뒤 지남 포함)을 거부하고, 다른 기기의 credential을 가져가지 못하며, 기기 폐기 때 지운다. CBOR는 길이 없는 값·잘린 값·남은 크기보다 큰 길이·너무 깊은 중첩을 거부한다.
- `cargo test -p aaa-host --lib web::guard_tests::only_a_direct_loopback_screen_approves_without_a_passkey`: 라우터 전체로 loopback(직접)은 코드가 나오고, 공개 Host와 serve를 거친 loopback Host(`X-Forwarded-Proto`·`X-Forwarded-For`)는 `passkey_required`.
- `node --test web/test/passkey.test.mjs`: base64url 왕복(채움 없음)과 등록 단추를 여는 규칙.
- `node web/e2e/web.e2e.mjs --spec 79-passkey`: debug host를 `AAA_TEST_WEB_LOCALHOST`로 띄워 `http://localhost:<port>`를 공개 주소처럼 쓴다(브라우저 passkey는 IP에 묶이지 못한다, RP ID `localhost`). Chromium 가상 인증기(CDP `WebAuthn`)를 단 휴대폰 화면이 짝지은 뒤 안내 줄로 등록하고, 설정 → 폰 및 기기에 등록됨·기기 목록 「passkey」 표지, 「기기 페어링」 코드가 서명 한 번(인증기 signCount)으로 나온다. 누름이 지난 뒤(`navigator.userActivation.isActive`를 거짓으로 고정)는 「passkey로 승인」 창을 한 번 더 누른 처리기에서 서명하고, 그 창을 취소하면 서명도 요청도 없다. 휴대폰 390에서 다른 확인 창(공유 링크 키 교체) 위의 그 창을 취소해도 아래 창과 안내가 남는다(그 창은 「뒤로」 기록을 쓰지 않는다). 인증기 없는 공개 주소 기기는 거부·등록 안내, 실험실 첫 화면(127.0.0.1)은 같은 동작을 passkey 없이 한다. UV 표지 없는 서명 거부는 host 유닛이 본다(확인하지 못한 가상 인증기는 브라우저가 제 창에서 기다린다).
- 결함 심기(library `w7-fault/README.md`): gate 열기·UV 검사 빼기·op 다시 쓰기·10분 시한 빼기·화면 주소 검사 빼기·서명 검사 빼기는 host 유닛이, 웹 `call`의 gate 빼기·웹의 UV 다시 보내기 빼기·「passkey로 승인」 창을 휴대폰 sheet(ConfirmDialog)로 되돌리기는 E2E 79가, serve를 거친 Host 판정 빼기는 guard 유닛이 잡았다.

## 「CLI 0회」 인수(t-0538 관리 GUI W8, plan §8.417)

- `node web/e2e/web.e2e.mjs --spec 82-cli-zero`: 설치(프로젝트 만들기, 첫 짝짓기) 뒤의 운영 한 바퀴를 웹 화면만으로 돈다. 새 머신 등록 → 설정 「안전」 `start_threads = auto` → 자동화 ①(사건마다 새 thread: 폼 → 시작 범위 승인 → 지금 실행) → 자동화 ②(열린 thread에 글: ①의 thread 번호 → 지금 실행 → 그 thread 터미널에 글) → 배포 점검·시작(대상 dry-run에서 멈춤)과 agent 재시작 rollout(가짜 launchctl) → agent·host를 죽이고 다시 띄우면 화면이 끊김을 말하고 저절로 돌아옴 → 깨진 host.toml의 안전 모드와 되돌리기 → thread 끝내기·자동화 지우기·머신 폐기. 설치 뒤 spec의 `cli()`는 보기 명령만 통과시키고 다른 aaa 명령은 「CLI 0회 위반」으로 멈춘다(첫 단계가 그 거부를 직접 본다). agent 연결 주소는 고정 port다: 다시 띄운 host에 새 머신이 다시 붙는다.
- 실제 머신 점검표(사용자가 웹과 화면 공유만으로, 터미널 입력이 한 번이라도 있으면 미완료):
  1. 휴대폰을 공개 주소로 다시 짝짓고 10분 안에 passkey를 등록한다. 이 뒤 승인 필요 동작은 passkey 확인 한 번이다.
  2. 새 머신을 더한다: 설정 → 머신 → 등록 창의 안내 명령을 새 머신에서 치고(그 머신의 설치), 확인 코드로 승인한다. 「연결됨」을 본다.
  3. 설정 → 안전에서 `start_threads`를 `auto`로 바꾼다.
  4. 자동화 ①: 새 자동화 → 「새 thread 시작」(profile·머신) → 저장 → 「승인 검토」 → 승인 → 「지금 실행」. 작업 목록에 자동화 thread가 서고 보고가 온다.
  5. 자동화 ②: 새 자동화 → 「thread에 글」(4의 thread 번호) → 저장 → 「지금 실행」. 그 thread 터미널에 글이 온다.
  6. 설정 → 배포: 빌드 머신에 있는 후보(coordinator나 thread가 빌드해 둔 것)를 점검하고 배포를 시작해 기록에서 끝까지 본다. 「agent 재시작…」으로 한 머신 agent를 다시 띄운다.
  7. 장애: 화면 공유로 agent 머신의 aaa agent 프로세스를 활동 모니터에서 끝내고, 웹의 끊김 표시와 감독자가 다시 띄운 뒤의 복귀를 본다. host Mac에서도 같게 한다.
  8. 정리: 4의 thread를 「thread 관리 → 끝내기」, 두 자동화를 지우고, 2의 머신을 폐기한다.
  9. 화면 공유 밖에서 터미널을 연 일이 있으면 그 단계와 까닭을 적는다(미완료).
- `node web/e2e/web.e2e.mjs --spec 83-cli-zero-gaps`: 인벤토리의 작은 빈칸 다섯(답 1A). t1 줄 메뉴 「세션 끝내기」 → 확인 창 → 세션만 끝나고 「다시 열기」로 다시 뜸, coordinator 메뉴 「하나 더 띄우기」(맨 위 profile 이름을 단다) → coordinator pane 둘, 보고가 오면 host가 낸 30일 링크를 thread 화면을 연 채 설정 「이 aaa」 「낸 링크」에서 보고(앱 안 보기가 마지막 열람으로 남는다) 폐기하면 열려 있던 링크 줄도 「폐기됨」이 되고, 7일을 골라 새 링크(만료 − 발급 = 7일), 「나중」 파일은 폴더 밖 경로를 host가 거부한 말을 보이고 `memory/later.md`를 더하면 host `project show`의 `task_notes`에 들어가며 빼면 비운다.
- 결함 심기: 「CLI 0회」 검문 끄기는 첫 단계가, `do.message`가 받는 thread를 잃으면 자동화 ② 단계(전달 기록 없음)가, thread 관리 메뉴의 「끝내기」 빼기는 정리 단계가 잡았다. `do.message` 본문에서 규칙 글만 빼는 결함은 잡지 못한다: 실행 자료(`memo`)에도 같은 글이 들어가 화면에 남는다. 81은 「세션 끝내기」가 `pane_close`를 보내는 결함(「다시 열기」 없음), 「하나 더 띄우기」가 `new`를 뒤집는 결함(coordinator 하나), 링크 수명을 보내지 않는 결함(30일), 폐기 뒤 host가 reports 사건을 내지 않는 결함(열린 화면이 active로 남음)을 잡았다.

## 질문 dock 칩 정렬·답한 뒤 숨김(t-0687, plan §8.329)

- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 53-thread-questions --workers 1`: 실제 host의 질문 보고와 휴대폰 답변을 사용한다. Chromium 2800px·1280px와 WebKit 휴대폰에서 미응답 칩의 왼쪽 끝·열 오른쪽 끝이 입력칸과 1px 이내이고 입력칸 위에 있는지, 가로 넘침이 없는지 단언한다.
- 같은 세 크기의 답변 뒤 dock 칩·빈 dock 부재를 검사하고 두 상태의 스크린샷을 남긴다. 기존 대화 칩의 답함·보관된 질문 원문·다른 기기 카드·새 질문 회귀 검사는 유지한다. DOM·host 답 사건으로 끝내며 고정 sleep이나 시한 연장을 쓰지 않는다.

## SSH로 설치·등록(t-0578 웹 머신 추가 M3, plan §8.420)

- host 유닛 `cargo test -p aaa-host machine_setup`:
  - aaa 전송: 실험실 sshd로 보낸 파일이 원격 `mktemp -d` 폴더(0700)에 같은 SHA로 놓이고 `--version`이 돈다. SHA·판이 어긋나면 그 폴더를 지운다(보낸 파일과 같은 내용의 폴더만 세어 나란히 도는 시험과 섞이지 않는다).
  - 원격 install 대본: 실제 ssh로 보낸 shell 대본이 `install-probe`·`install-bootstrap`·`install --resume`에 미리 만든 답을 내고 받은 인자·stdin을 남긴다. 계획은 host가 보낼 choices로 실제 install planner를 돌린 것이다. 적용은 FIFO가 열릴 때까지 기다리고, 그사이 시험이 둘째 package의 grant로 host `install_enroll`을 부른다(실제 원격 worker 대신). 고정 sleep은 없다.
  - 원격 install이 이 SSH를 거부(exit 3)하면 까닭을 보이고 원격 임시 폴더·계획 파일을 남기지 않는다.
  - launcher(install I3): 보낸 choices가 `--service launchd`·`--start now`이고 원격 실행 파일 옆에 host 실행 파일 옆의 launcher package가 같은 내용으로 있다(대본이 계획 때 그 옆 파일을 남긴다). 다른 실행 파일 SHA에 묶인 package는 원격 계획 전에 멈추고 원격 임시 폴더를 지운다. 개발 판 host 옆에 없으면 보내지 않고, 원격이 `installation_launcher_asset_required`를 사람 일로 알리면 host가 준비할 것(package 이름·release 판 host)을 덧붙인다. release 번호 판인데 그 release에 launcher가 없으면(옛 release) manifest만 읽고 보내지 않으며 그 까닭을 덧붙인다. 대본 원격은 OS를 보지 않으므로 Linux CI에서도 감지 OS를 macOS로 둔다.
  - release 받기: 임시 HTTP 서버(받은 경로를 남김)로 정상·같은 SHA 캐시(asset 다시 안 받음)·SHA 다름(남기지 않음)·manifest 판 다름·그 판 release가 아닌 주소(manifest만 읽음)·없는 종류·옛 manifest의 launcher 없음. 감지 CPU를 다른 종류로 바꾼 작업은 release 실행 파일과 launcher(manifest `launchers`, 캐시는 실행 파일 옆 0600)를 받아 원격에 나란히 보내고 그 SHA를 원격 install executor로 준다.
  - 끝 확인·로그인: 끝 확인 스크립트를 이 머신에서 실제로 돌려 표지 사이 줄을 읽는다. 가짜 agent(duplex 연결)가 Spawn argv·cwd를 남기고, 그 스크립트 실제 출력을 pane 화면으로 넣으면 host가 작업에 줄을 남긴다. `codex login` pane, 끝나지 않은 작업·보기 기기·머신 없음·연결 안 됨 거부.
- 웹 유닛 `node --test web/test/machineSetupModel.test.mjs`(1.103 판정, macOS만 설치, 기본 설정, 설정 앞 검사, 단계).
- 웹 E2E `72-machine-add` 마지막 단계(harness가 `AAA_TEST_RELEASE_BASE`를 닫힌 loopback 포트로 줘 개발 빌드 host가 같은 판 launcher를 찾으러 실제 GitHub에 가지 않는다): 설치 설정 폼(기본 이름·작업 root, 잘못된 이름 알림) → 「계획 보기」가 실제 host 실행 파일을 보내고 실제 install이 비특권 실험실 sshd를 거부하는 까닭(시스템 SSH 서버)을 보인다. 원격 임시 폴더가 남지 않는다. 보기 기기에는 설정 폼이 없다.
- 실제 install SSH 입구(root 소유 시스템 sshd 계보)는 이 Mac 실험실에서 열 수 없다(시스템 설정을 바꾸지 않는다). 끝까지의 실제 경로는 CI 필수 단계 「SSH로 설치·등록 실제 root sshd·LaunchAgent」(macos-15)가 본다:
  - `scripts/ci-root-sshd.sh`(GitHub Actions가 아니면 멈춘다)가 실험실 host key·사용자 키·`sshd_config`(loopback 빈 포트, 비밀번호·PAM 끔, `AllowUsers` 자기 계정, `SetEnv HOME=<실험실>/h`. 실험실은 agent socket 길이 때문에 짧은 `/private/tmp/ar.XXXXX`)를 만들고 `sudo -n /usr/sbin/sshd`로 띄운다. 끝에 그 PID만 끝낸다.
  - ignored 시험 `cargo test -p aaa --test m13 ssh_install::ssh_install_through_the_system_sshd_starts_a_launch_agent_that_connects_and_checks -- --ignored --exact`(`AAA_ROOT_SSHD_LAB`): 시험 aaa 사본 옆에 I3 시험과 같은 방법으로 만든 launcher package를 두고 그 사본으로 host를 띄운다 → 「ci」 접속 → 지문(`ssh-keygen -lf`와 같음) 믿음 → 감지(원격 HOME = 실험실) → 계획(Review) → 적용 → 설치·등록됨 → agent 연결 → 끝 확인 줄(작업 폴더·PATH).
  - HOME 분리: 원격 셸·install·LaunchAgent(plist의 HOME)가 모두 실험실 HOME을 쓴다. 시험이 실험실 HOME 아래 plist·`.local/bin/aaa`를 보고, runner 실제 HOME의 같은 자리(plist·실행 파일·launcher·상태 폴더)가 전후 같은지 본다. gui domain label `dev.aaa.agent`는 계정 전역이라 적용 전 없어야 하고 시험이 끝에 bootout한다.
  - 스크립트는 시험 전에 그 sshd로 연 원격 명령의 조상(pid·ppid·사용자·image)을 적는다. install 검증 SSH 입구가 root `/usr/sbin/sshd`를 찾지 못하면 그 사슬로 원인을 본다.
  - 이 Mac에서는 같은 시험을 비특권 실험실 sshd(`SetEnv HOME` 포함)로 `env -u AAA_SESSION_ID`로 돌려 계획 단계의 검증 SSH 입구 거절까지(launcher 만들기·host·지문·감지 HOME·전송) 확인한다.

## 연결 안내의 0.5초 지연(t-0686, plan §8.424)

- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 84-connection-banner --workers 1`: 실제 host·agent와 2,400행 Claude JSONL을 Chromium 1280px·WebKit 390px에서 연다.
- 시험 안 WebSocket의 open/message 전달을 gate로 보류한다. 실제 연결·기록 조회는 유지하고 Playwright 시계를 499/500ms로 전진시켜 지연 경계와 빠른 연결에서 순간 표시도 없음을 MutationObserver로 확인한다.
- pane 왕복의 지연 초기화, 실제 socket 닫힘 뒤 499ms 미표시·500ms 표시, 재시도 시 표시 유지, 회복 즉시 제거·옛 타이머 취소를 확인한다. 고정 sleep·재시도·시한 연장은 없다. 정본 전체 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다.


## Windows 최소 다운로드 bootstrap(t-0485)

- `cargo test -p aaa --test windows_bootstrap --locked`: Windows에서 PowerShell 5.1과 7을 모두 실제로 실행한다. 네트워크 요청만 외부 경계에서 대체하고 실제 PE/파일/hash/--version/native argv/종료 코드와 표준입력 상속을 검사한다. 빈 문자열·공백·따옴표·끝 역슬래시·한글·셸 메타문자를 보존하고, 같은 bytes의 mtime·다른 instance·충돌 파일을 보존한다. manifest 판/URL/hash·PE/CPU/실행 판·파일 부모·junction·NUL 거부 뒤 실행/배치가 없어야 한다. 실제 aaa dry-run은 state를 만들지 않고 --yes는 쓰기 권한이 아니다.
- 이 owner만 `cfg(windows)`다. PE/CreateProcess·PowerShell 5.1·NTFS junction이 대상 OS 계약이기 때문이다. macOS/Linux에서 통과를 주장하지 않는다. 공통 Rust install owner는 각 OS의 기존 Cargo 시험에 남는다. 실제 Windows 판정은 승인된 hp branch의 기존 windows-2025 CI에서 한 번 확인하고 branch를 지운다.
- `nice -n 15 python3 -m unittest scripts.test_release`: 같은 source의 bootstrap만 게시하며 manifest/체크섬이 실제 script bytes를 포함해야 한다. ci-local의 test 및 distribution workflow에도 묶었다. 검토하지 않은 script/누락 파일을 허용하는 변형이 이 owner를 실패시켜야 한다.
- CI는 임시 사용자 폴더만 쓴다. bootstrap 자체는 SCM/registry/PATH를 변경하지 않는다. 실제 SCM logon·Session 0·부팅·LSA/UAC·harness 인수는 [Windows 사용자 절차](windows-bootstrap.md)로 분리한다.


## install supervisor 잠금의 owner 종료(t-0485)

기존 `service_phase_keeps_file_resource_locks_and_removal_and_credentials_resume_preserve_context` owner는 Unix에서 supervisor File의 실제 복제 descriptor를 owner 종료 뒤까지 남긴다. fork와 같은 open file description이 남아도 원래 Job 종료 뒤 같은 ID의 credentials/removal 재개가 즉시 가능해야 하며, 옛 사본을 닫아도 새 Job 배타성은 유지해야 한다. Windows는 실제 share reservation 방식과 원래 전체 owner를 유지하며 Unix flock 복제 대조만 제외한다. 원래 File close만 쓰는 코드에서 installation_busy RED, install private::Lock guard drop의 명시 unlock 뒤 GREEN을 확인한다. 잠금 생존 중 거부·계정/설정/준비 검증·제거/보존/조회 단언과 기존 시한은 그대로다.

같은 Unix guard는 `preserving_a_launcher_binds_its_bytes_to_the_approval_before_any_transaction_write`의 짧은 발급 scope 뒤 실제 복제 descriptor를 남겨 issuer.issue 재획득 RED→GREEN을 확인한다. `consumption_survives_reopen_and_only_explicit_same_id_resume_is_allowed`는 Lease 복제 descriptor가 남아도 같은 ID만 재개되고 소비/서명/만료/읽기 보존 단언이 유지됨을 본다. 세 owner의 기존 단언·시한은 바꾸지 않는다. Windows는 복제 대조만 cfg 제외하고 원래 실제 share reservation·owner 전체를 유지한다.


### 배포 binary의 production 의존성 검사

ci-local build는 Windows에서 `cargo build --release --locked -p aaa --bin aaa --target x86_64-pc-windows-msvc`를 독립 호출한 뒤 기존 workspace/all-targets build를 이어 간다. 기존 windows-2025 CI도 이 단계를 실행한다. 배포와 같은 release profile·Windows target·production binary만 컴파일·링크하며 dev feature 합치기로 Win32 API 누락이 가려지는 것을 막는다. v0.1.3 release의 JobObjects dev-only 누락이 실제 실패 사례다. 필요 feature를 dev-only로 돌린 변형은 같은 production release 빌드에서 실패해야 한다. macOS의 `cargo xwin build`는 교차 컴파일·링크 증거이며 실제 Windows 실행 판정은 기존 CI가 맡는다. source 목록 검사·새 runner·시험 전용 export·시한 변경은 없다. 다른 OS의 기존 전체 build와 시험은 유지한다.

## Windows 한 줄 입구·부분 설치 재개(t-0485, plan §8.446)

- `nice -n 15 python3 -m unittest scripts.test_release`는 실제 producer 출력의 install.ps1 URL/파일/manifest/SHA256SUMS를 대조한다. Windows의 기존 `--test windows_bootstrap` owner는 PS5.1/7의 실제 IRM octet-stream→IEX·argv/exit·구판/부분 config 보존·내장 hash 거부를 검증한다.
- `nice -n 15 cargo test --locked -p aaa --test install recovery::`는 닫힌 실제 host 등록 창과 실제 PTY/서명/파일/journal에서 status ID/안내·명시적 재동의·같은 transaction을 확인한다. Windows 전용 같은 묶음은 실제 powershell.exe/pwsh.exe의 한 줄 입구가 외부 콘솔을 native 검토/승인/파일 적용까지 유지하는지 검사하며 서비스는 none이다. 새 production flag/export나 실제 운영 서비스 등록은 없다.
- `nice -n 15 cargo test --locked -p aaa --bin aaa install::engine::execution::tests::legacy_hook_alias_approval_keeps_its_signature_id_and_scope_through_status_and_resume -- --exact`는 서명 원문/ID/범위를 유지하는 옛 lexical hook 호환과 다른 target/SHA 거부를 검사한다. `install::engine::transaction::references::tests::`는 명시 단독 repair의 최초 원본 복원과 다른/공유 owner의 무쓰기 거부를 검사한다. 암호/issuer key가 포함된 실험실 전체를 로그에 출력하거나 library로 복사하지 않는다.
- 최초 RED와 수리 GREEN을 보존하고 canonical 비교/owner 제한을 각각 망가뜨린 변형 실패를 확인한다. 전체 ci-local·공개 검사와 Windows/Linux strict cross, dev 없는 Windows release build 뒤 착지 창을 기다린다. Windows 실제 판정은 CI이며 교차 빌드나 로컬 PowerShell로 대신하지 않는다.

## pane 책임 경로와 launcher 보존 재시작(t-0763, plan §8.449)

- `cargo test -p aaa --test restart --locked`: macOS에서 실제 launcher·host·agent·session-host·CLI를 쓴다. launchctl 호출만 격리된 자기 launcher에 신호를 보내는 script로 대체하며 운영 launchd 등록은 하지 않는다. fixture launcher는 기존 C 소스를 그대로 빌드하고, `AAA_RESTART_TEST_LAUNCHER`를 주면 그 기존 파일을 읽기 전용으로 사용한다. 등록된 launcher 바이너리를 바꾸지 않는다.
- 서로 다른 SHA인 agent 사본으로 바꾼 뒤 CLI 재시작으로 launcher·worker PID와 기존 pane이 유지되는지 확인한다. 기존 pane·새 pane에서 고유 코드를 넣은 C·Rust 실행 파일과 cargo run·build.rs의 실제 책임 PID를 확인하고 첫/재실행 시간을 보존한다. 시험 도구 `scripts/pane-responsibility-probe.py`의 출력은 작업 저장소 `target/restart-probes` 아래에 둔다(임시 OS 폴더의 Cargo target은 쓰지 않는다). 권한 미등록 CI에서는 속도 상한을 합격 조건으로 삼지 않는다.
- 잘못된 worker 생성 시각·승인과 다른 실행 파일은 launchctl 변경 호출 전에 거부한다. launcher를 직접 종료하는 별도 단계는 pane을 남기고 새 agent의 재연결에서 `detached`가 갱신되는지 확인한다. 시험은 준비 기록·host 사건·명령 완료 출력으로 끝낸다. 셸 화면은 끝 공백을 잘라 내므로 `$ ` 문자열을 준비 사건으로 기다리지 않는다.
- `cargo test -p aaa-agent --lib responsibility --locked`는 실제 경로 분류·조회 실패를 확인한다. 기존 직렬화/TypeScript 계약 검사는 선택 필드의 이전 판 호환을 유지한다.
- E2E89은 실제 host·agent·thread를 사용하고 관측값만 응답 경계에서 바꾼다. Chromium 1280/320px, WebKit 390px Reader의 경고·접힌 경로 상세·정리 창·가로 넘침·보기 권한·보류 무변경을 확인하고 캡처한다. 보고서 iframe이 필요 없는 이 시험은 「사람을 기다림」 fixture로 sandbox/ServiceWorker 주입을 분리한다. 실제 보류·저장·이어 하기는 기존 E2E85와 함께 검증한다. 목록과 터미널 표지도 직접 확인하며, 응답 경계의 route handler는 context를 닫기 전 `unrouteAll({behavior: "wait"})`로 완료를 기다린다.
- 최초 회귀에서 준비 기록 상위 폴더 누락을 재현해 수정했다. 새 fixture launcher가 시작하지 않은 실행은 해당 PID의 syspolicyd YARA 오류·종료와 직전 DetachedSignatures EMFILE을 기록했다. 시스템 설정·시한·기대값을 바꾸지 않고 기존 등록 launcher를 사용하는 별도 경로에서 책임·첫 실행을 검증한다. 준비 기록 신원은 파일 경로 문자열 대신 커널 생성 시각·부모·UID·boot로 대조한다(원자 교체된 옛 실행 파일의 경로 조회를 신원으로 쓰지 않는다).
- 전체 검사에서 기존 task 경고 유일성 시험이 새 책임 안내의 `경고:` 접두어 때문에 실패했다. 책임 안내를 별도 `개발자 도구:` 줄로 표시해 기존 task 경고 계약·단언은 유지하며, 새 restart 회귀가 `pane list`의 기존 무인자 JSON 출력에 책임 관측값이 포함되는지 확인한다.
- 추가 CLI 시험의 최초 문구 단언은 pane 명령의 기존 JSON 전용 계약을 잘못 가정했다. 불필요한 pane 글 출력 분기를 없애고, `--json` 없는 실제 CLI 출력도 JSON 한 줄이며 책임 관측값을 포함하는지 검사한다. thread 글 출력의 별도 진단과 웹 표지는 유지한다.
- 전체 필수 검증은 `nice -n 15 bash scripts/ci-local.sh`·공개 검사·Linux 대상 clippy다. 착지 검증은 자기 worktree target만 쓰며 운영 host·agent 재시작은 배포 때 coordinator가 한다.
## Windows 한 줄 입구 첫 CI 수리(t-0485, plan §8.450)

- 기존 `--test install recovery::`는 실제 OS architecture 환경을 보존한 PS5.1/7 입구를 실행한다. 값을 AMD64로 꾸미지 않는다. 닫힌 실제 등록 창→파일/status ID/안내→같은 ID 새 nonce/job 하나를 모두 검사한 뒤 runtime·Host를 종료하고 lab을 제거한다. Windows OS32를 무시하거나 재시도하지 않는다.
- 기존 `legacy_hook_alias_approval_keeps_its_signature_id_and_scope_through_status_and_resume`는 Windows canonical verbatim 경로에서 일반 Win32 lexical 경로를 구성한다. 기존 lexical/canonical 차이·서명 원문/ID/scope/nonce·다른 target/hash 거부·status 무쓰기·재개 job 하나 단언을 유지한다. Windows/Unix 모두 실행하며 Windows만 scope 구성을 달리한다.
- `--test windows_bootstrap`의 staged executor exact argv 비교는 그대로 두고 실패 시 시험 argv의 actual_hex/expected_hex를 한 줄로 남긴다. 이 진단은 비밀 입력을 받지 않는 native fixture에만 적용한다. PS5.1에서 실패했다고 PS7까지 통과한 것으로 쓰지 않는다. 로컬 PowerShell의 조립 구문 대조나 Windows cross build를 실제 Windows 성공으로 보지 않는다.
- 최초 Windows RED는 main 4fe5937c의 run 39761138515/job 119765308652다. 새 exact main CI로 Windows GREEN 또는 staged argv의 실제 차이를 판정한다. 공식 전체 ci-local·공개 검사·양 OS cross clippy와 Windows release build 뒤 별도 착지 창을 기다린다. 시한/기대값/조합 수는 바꾸지 않는다.

## 완료 탭과 웹 기록 제거(t-0775, plan §8.453)

- host의 기존 `archive_search_pages_bound_bytes_keep_all_matches_and_restore_without_queue` owner를 확장한다. 4MiB 초과 원장을 실제 DB에 저장하고 전체 페이지·검색·저장소·완료 근거·복원 불변식을 유지하면서 모델·머신과 완료 기간의 시작 포함/끝 제외, 미지정과 전체, 필터 전 전체 선택지를 검사한다.
- E2E86은 웹 기록 대신 완료 탭을 검사한다. 실제 host 원장 130건의 페이지 분할·검색·저장소·모델·머신·기간·완료 근거 조합, 상세 메모·되살리기·Reader 쓰기 거부, 직접 체크 보관을 본다. Chromium 데스크톱/320px·WebKit 390px와 밝음/어두움을 캡처한다. E2E26 긴 목록의 기록 부분도 완료 79건의 끝 행·상세·다음 페이지로 바꾼다. E2E46은 삭제 직후 되살리기 인수를 유지하고 없어진 웹 기록 동선만 제거한다.
- 완료 사건은 API 성공 응답·aria-busy·DOM·파일 상태다. 고정 sleep·재시도·시한 변경은 쓰지 않는다. 전체 ci-local·공개 검사를 수행하고 착지 창 전에는 병합·push하지 않는다.

- E2E86은 실제 복원 쓰기의 응답만 gate로 보류한 뒤 다른 완료 검색을 열고 응답을 푼다. 이전 검색을 다시 읽던 코드에서는 결과 0건으로 실패했고 최신 load를 참조하도록 고친 뒤 새 검색 1건을 유지한다. 최초 시험 자료의 저장소 불일치를 바로잡은 뒤 RED→GREEN을 확인했으며 시한·제품 기대값은 완화하지 않았다. E2E47은 상세의 메모와 실제 r-ID 보고서 연결도 검사한다.

## 계정 gate 수신 진단(t-0777)

`t-0775`의 기존 `accounts::host_login_defaults_pin_threads_and_coordinators_without_exposing_credentials` 실패는
`expected value`, line 1, column 1이다. 같은 잠금 파일의 serde_json에서 빈 입력은
`EOF while parsing a value`, line 1, column 0이고, HTTP 요청 등 JSON이 아닌 첫 바이트는 원본 오류와 같다.
원본 로그에는 받은 문자열·peer가 없어 빈 연결이나 송신 프로세스 종료로 확정할 수 없다.

- Gate는 EOF를 별도로 단언하고 JSON 오류에는 예상 phase, listener·peer, 받은 바이트 수와 첫 8바이트의 hex를 남긴다. 입력 본문 전체는 로그에 쓰지 않는다.
- `POLICY_HARNESS`는 실험실 `proof/gate-trace.log`에 JSONL로 PID·thread·phase·클라이언트 주소와 연결/전송 완료를 기록한다. 토큰과 인증 홈은 이 진단에 넣지 않는다. 실패하면 기존 Lab 정리가 실험실을 보존하므로 peer를 이 기록과 대조한다. CI 실패 요약은 `*.log`·`*.json`만 모으므로 확장자를 `.log`로 둔다.
- 기존 phase·계정·토큰·pane 수·종료 단언과 30초 상한을 유지한다. EOF나 잘못된 JSON을 건너뛰거나 연결을 다시 시도하지 않는다. 진단 추가를 원인 수리 완료로 보지 않는다.

### 정해진 20회의 동시 부하 재현 조사

- 전체 Rust에 포함된 대상 owner 1회와 별도 프로세스 19회를 합쳐 총 20회로 고정했다. 별도 실행은 최대 4개를 동시에 돌리고, 정본 `scripts/rust_tests.py`의 전체 workspace·doctest 28묶음(worker 6·libtest thread 2)과 겹친다. 먼저 binary를 모두 준비하고 실행 중 같은 aaa 실행 파일을 재빌드하지 않는다. 실패를 재실행하거나 횟수를 늘리지 않는다.
- macOS·논리 CPU 10개에서 대상 20/20과 전체 Rust 28/28묶음이 통과해 원본은 재현되지 않았다. 별도 19회의 시작/끝 시각 모두 전체 Rust 실행 구간 안에 있었다. 전체 Rust는 219.15초, 별도 대상 실행은 각 33.03~39.75초였고 시작/끝의 1분 부하는 15.58~43.19였다. PID·시각·종료 코드·로그를 각각 보존했다. 이 결과로 원인을 고쳤다고 판단하지 않는다.
- 이후 CI 수집 대상에 맞게 trace 확장자만 `.jsonl`에서 `.log`로 정리했다. 기록 형식·송신 순서·기존 단언·시한은 그대로다. 제품 코드는 바꾸지 않고 진단 변경만 준비하며 다음 CI·ci-local의 재발 자료로 송신자를 확인한다.
- 실제 `POLICY_HARNESS`를 TCP gate와 시험용 hook/거부 CLI에 연결해 두 phase의 연결/전송 기록 4개, PID·peer 일치, 인증 값 없음과 기존 CI `find` 수집 조건에 포함됨을 확인했다. 대상 owner의 추가 실행은 하지 않았다.

## Windows bootstrap profile 표기 수리(t-0485, plan §8.454)

- 기존 `cargo test --locked -p aaa --test windows_bootstrap` owner의 staged argv exact 비교를 유지한다. profile directory 생성 뒤 입력을 OS GetFullPath로 정규화해 PS5.1의8.3별칭 확장과 fixture 기대target 표기를 맞춘다. actual을 바탕으로 expected를 만들지 않는다.
- RED는 main36f3e935의 Windows job119808659146에 남긴 actual_hex/expected_hex다. 세 인자가 동일하고 profile ancestor만 긴 이름/별칭인 것을 전체 로그에서 확인했다. 원래 argv/exit/hash/구판·부분 config 보존·거부 조건·PS5.1/7·실제 IRM/IEX 범위와 시한은 유지한다. production 변경은 없다.
- macOS parser/release producer·전체 ci-local·공개 검사·양 OS strict cross와 Windows release 조건 build는 호환 검증이다. 같은 owner의 실제 Windows GREEN은 후보 착지 뒤 main CI에서만 판정한다. CI rerun/단언 완화로 넘기지 않는다.


## 저장소 workspace·기존 결과 지표(t-0784, plan §8.455)

- `nice -n 15 cargo test --locked -p aaa-agent --lib workspace_metrics`: 기존 형식의 fixture만 읽는다. 전체/단계 경과·개수·E2E 파일/작업 분리, 묶음을 개별 테스트 시간으로 쓰지 않음, 반복 E2E의 중간값 제외, 손상·누락 자료의 미수집을 확인한다. rc·엔진은 기존 개별 exec_time·시나리오 단계 합계와 이전 실행 제외를 검사한다. rc 시간표를 엔진 입력으로 오인하지 않고, 읽기 예산을 넘겨도 이미 확인한 전체 경과를 보존하는지 확인한다. 수집 뒤 원본 로그를 지워도 같은 입력 판은 저장한 요약을 재사용한다. 실제 Git 커밋 객체로 최초/증분·rename·삭제·dirty 파일 제외·혼합 cfg(test) 합계와 host가 가진 커밋의 재집계 생략을 검사한다. 실패한 동일 커밋을 다시 스캔하지 않고 다음 커밋에서 회복하는지도 검사한다. Unix에서는 fork가 물려받는 것과 같은 열린 파일 참조를 남겨도 소유자 종료 때 수집 잠금이 풀리는지 검사한다.
- `nice -n 15 cargo test --locked -p aaa-host --lib workspace_metrics`: SQLite 재시작·중복·명시적 workspace 묶음의 이력 이전, 프로젝트 이름 변경·삭제, API Reader 조회·기간/머신/profile·빈 결과·미수집을 확인한다. 날짜별 p50은 머신·profile 경계를 지키고 실패·빈 날짜를 넣지 않는다. 자료가 없는 등록 경로로도 조회할 수 있어야 한다. 조회가 원본을 읽거나 agent에 요청하면 실패하는 경계를 유지한다.
- `NODE_PATH=$(npm root -g) nice -n 15 node web/e2e/web.e2e.mjs <캡처 폴더> --spec 90-workspace-metrics,26-activity-bar --workers 1`: 실제 격리 host·agent·보고 감시·전송·원장·웹을 사용한다. fixture는 기존 runner 종료 산출물만 준비한다. 수집 완료 사건으로 열린 화면이 갱신되는지, 전체·개수·TOP 5·dirty 코드 제외, URL 복원·필터·키보드 상세·목록 접기·로그 삭제 뒤 조회를 확인한다. Chromium과 WebKit, Reader, 390/320px, 밝음/어두움에서 drawer·가로 넘침·오류 때 마지막 성공 값을 검사하고 캡처한다. 데스크톱은 1280px이다. 실제 iPhone 검증은 아니다.
- 완료 사건은 보고 hash·원장 저장 뒤 WebSocket 사건·DOM·응답이다. 통계를 얻으려고 추가 검증을 실행하거나 검증의 옵션·병렬 수·시한·성공 조건을 바꾸지 않는다. 필수 전체 검증은 `nice -n 15 bash scripts/ci-local.sh --tee`와 공개 검사·의존 경계 시험이다.
