## 답보다 늦은 첫 신뢰 질문 관측(t-1212, plan §8.529)

- `harness::tests::a_late_first_trust_observation_keeps_the_answered_brief_question`은 실제 Host·SQLite·`thread_brief`·`answer`를 쓰고 가짜 agent 연결의 Screen/Input 응답만 소유한다. Idle → brief 화면 질문 → yes 저장 → 같은 첫 Question → Idle 순서를 직접 실행해 기록 하나의 Answered·yes와 열린 중복 없음·Gone 기록 없음, 막힘이 끝난 뒤 같은 질문 재등장과 `ask_anew.v1`/옛 agent의 재질문 차이를 확인한다. 필요한 이유: 첫 Question을 먼저 주입하는 기존 늦은 사건 회귀만으로는 brief가 앞선 경쟁을 지킬 수 없기 때문이다. 수리 전 같은 회귀는 첫 관측의 중복 열기로 실패했다.
- `a_different_first_observation_is_not_an_answered_screen_question`은 답한 신뢰 창과 종류가 다른 권한 창, 같은 종류지만 본문이 다른 신뢰 창을 즉시 열고 답 전 반복 관측은 한 질문에 묶는지 검사한다. 필요한 이유: 첫 관측을 무조건 버리거나 옛 답 표지가 새 질문을 가리는 수리를 막아야 하기 때문이다.
- 부분 검증은 Windows에서 BelowNormal 우선순위로 `cargo test --profile verify --locked -p aaa-host --lib -- harness::tests:: brief::tests::`, host strict clippy, 의존 경계·fmt·공개 검사다. 원래 m13 `claude_thread_states_questions_and_messages`는 Unix 전용이므로 전체 ci-local·cross와 함께 정규 착지 차례에서 확인한다. 원래 m13 기대값·시한, agent의 800ms 경계는 바꾸지 않으며 고정 sleep·재시도·시한 확대·새 E2E 번호는 없다.


## 착지 검증

- 사용자 승인에 따라 macOS 전용 worktree에서 최신 원격 main을 통합하고 no-ff 병합한다. FRONT_API 1.142·plan §8.529를 사용하며 E30·35의 두 파일 분할과 기존 기대값·시한을 유지한다.
- 전체 검증은 `nice -n 15 bash scripts/ci-local.sh --logs <회차 로그> --tee`로 실행한다. 전역 Playwright는 바꾸지 않고 착지 폴더 안 1.64.1 패키지와 기존 공용 Chromium·WebKit 캐시를 쓴다. 시작·실패 시 uptime과 최초 원문·단계별 결과를 보존한다.
- 공개 검사 뒤 `nice -n 15 bash scripts/ci-linux-clippy.sh`와 Windows MSVC workspace/all-targets strict clippy를 실행한다. Windows는 기존 LLVM clang-cl·cargo-xwin을 쓴다.
- Windows production은 별도 Cargo 호출의 salt 없는 release binary를 확인하고 exe·PDB를 보존한다. 전체 gate에 AAA_BUILD_SALT를 넣지 않는다.
- 번호·충돌 표지·개인정보 검사를 통과하면 원격 main과 ff push 결과를 대조한다. 실제 착지 SHA의 Windows salt 실행 파일을 별도로 빌드해 격리된 전달 폴더에서 SHA-256·크기·`--version`을 확인한다. 설치·웹 기기 페어링·GitHub CI 대기는 하지 않는다.
- 첫 실패 원문을 보존하며 재시도·시한 확대·기대값 완화로 통과시키지 않는다. main 유래 다른 E2E가 실패하면 수리 전에 원문과 가설만 보고하고 멈춘다. 실제 실행 결과와 착지 SHA는 thread 보고 및 library 영수증에 둔다.
