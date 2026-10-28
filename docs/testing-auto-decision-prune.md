## 자동 결정 보존 기간 정리(t-1262, plan §8.539)

- host 유닛 `auto_mode::tests::old_decisions_expire_like_answered_records_but_never_reopen_an_auto_resend`는 실제 Host·SQLite로 질문
  세 개를 만들고 두 개를 자동 답한 뒤, 원장에 14일 넘은 답함·실패(열린 질문)·전달 모름(없는 질문 q-9999) 결정과 post 번호의
  `forgotten` 항목을 넣는다. ticker와 같은 순서(`ask_prune_pass` → `auto_prune_pass`)로 돌린 뒤 최근 결정만 남는지, 오래된 결정의 질문
  사본과 자동 답만의 「답한 것」 기록이 빠지는지, `asks` 사건이 나는지, `forgotten`에 열린 질문의 실패 기록만 남고 그 질문의 자동 답이
  계속 거부되는지 본다. 사람이 그 질문에 답한 뒤 다음 정리에서 `forgotten`이 비는지도 본다. 필요한 이유: 보존 기간 삭제가 「자동
  재전송하지 않는다」(§8.479)를 깨지 않으면서 `forgotten`도 끝없이 쌓이지 않아야 한다.
- thread 질문은 `auto_exception_count`와 같은 `threads_of` → `thread_view`의 질문 ID를 쓴다(시험은 사용자 요청 질문으로 본다).
- 시험이 보호 규칙을 실제로 보는지: `forgotten` 정리 조건을 늘 남기게 바꾸면 이 시험이 `["auto-q-1002", "auto-q-9999"]` ≠
  `["auto-q-1002"]`로 실패했다(되돌림).
- 단독 약 0.7초(auto_mode 8개 1.1초). 고정 sleep·재시도·시한 확대는 없다.
- 부분 검증: `cargo test -p aaa-host --lib auto_mode`, `cargo test -p aaa-host --lib asks`, `cargo clippy -p aaa-host --all-targets
  -- -D warnings`. 전체 ci-local·공개 검사·cross는 정규 착지 회차에서 돈다.
