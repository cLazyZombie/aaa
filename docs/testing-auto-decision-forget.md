## 자동 결정 지우기와 대화창 줄(t-1223, plan §8.531)

- host 유닛 `auto_mode::tests::forgetting_decisions_clears_every_list_but_never_reopens_an_auto_resend`는 실제 Host·SQLite로 질문 두 개를
  자동 답한 뒤 보기 기기·coordinator 세션·worker의 지우기 거부, ID 없는 요청 거부, 하나 지우기 → `Asks` 응답의 목록·「답한 것」·`asks`
  사건·질문 사본 폴더, 답한 질문의 자동 재답 거부, 전달 여부를 모르는 결정을 모두 지운 뒤 같은 ID 예약 거부를 본다. 필요한 이유: 지우기가
  모든 목록에서 함께 빠지면서 「자동 재전송하지 않는다」 계약을 깨지 않아야 한다.
- 웹 유닛 `asksModel.test.mjs` 「자동 결정 첫 쪽은 …」은 `freshAuto`가 첫 쪽에서 빠진(다른 기기에서 지운) 결정을 남기지 않고 「더 보기」로
  읽은 이전 쪽만 잇는지 본다. 필요한 이유: 이전 병합은 옛 목록을 늘 합쳐 지운 결정이 다른 기기에 남았다.
- E2E100 마지막 단계 「대화창 접은 줄의 확인·지우기와 …」는 실제 host·agent·Chromium 1280·390px 두 기기에서 대화창 자동 결정이 접힌
  한 줄(60px 미만)로 시작하고 펼친 목록이 화면 절반을 넘지 않는지, 「괜찮음」으로 확인한 결정이 없는지, 줄의 확인·지우기가 다른 기기
  대화창에서도 빠지고 host 기록이 바뀌는지, 카드 지우기, 「모두 지우기」 뒤 목록 0·`auto_next` 없음·자동 답만의 「답한 것」 없음·새로 읽은
  대화창 줄 없음, 보기 기기의 `auto_decision_forget` 거부를 본다. 기존 Chromium·WebKit 반복 단계는 접힘 시작과 펼친 줄의 폭·한 줄 높이를
  본다. 고정 sleep·재시도·시한 확대는 없고 DOM 상태와 host 기록으로 끝낸다. 단계 약 1.5초, 파일 약 21초(studio1 단독)다.
- 부분 검증: `cargo test -p aaa-host --lib`, `cargo test -p aaa-proto`, `AAA_UPDATE_TS=1 cargo test -p aaa-proto typescript`,
  `cargo clippy -p aaa-host -p aaa-proto --all-targets -- -D warnings`, 웹 `tsc`·build·`npm test`, E2E100(Playwright 1.64.1 패키지는 작업 폴더
  밖 임시 폴더, 브라우저는 공용 캐시). 전체 ci-local·공개 검사·cross는 정규 착지 회차에서 돈다.
