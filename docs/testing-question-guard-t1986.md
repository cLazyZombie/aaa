# 질문 감시의 시안·카드 교체와 정정 안내(t-1986)

## 원인과 수리

11월 17일 관측한 네 구절을 수정 전 코드로 실행했다. ‘제가 정해 둔 것(다르면 말씀해 주세요)’와
‘다르게 생각하시면 말씀해 주세요’는 카드 누락으로, 최신 카드 안내 뒤 ‘앞 시안 q-4026은 이 시안이
대신합니다.’와 ‘앞 카드 q-4053은 이 카드가 대신합니다.’는 옛 카드 재안내로 각각 재현됐다.
관측 구절은 그대로 두고 최신 카드 안내·이미 적용한 기준만 시험에서 재구성했다.

`answer_ids()`의 기존 교체 설명 주체에 ‘이 시안이’·‘이 카드가’를 추가했다. 해당 옛 참조만 제외하고
최신 카드와 동일 번호의 실제 답변 요청은 남긴다. `requests()`에는 ‘다르면’·‘다르게 생각하시면’에
짧은 ‘말씀해/알려 주세요’가 직접 이어진 구절만 추가했다. 같은 문장의 별개 승인·선택 요청을 지우지 않는다.
질문 어미·A/B 판정·카드 상태 조회·완료 사건·별도 승인 화면 감지 규칙은 유지한다.

## 회귀 owner

- `question_guard::tests::observed_preview_card_replacements_and_corrections_do_not_require_answers`:
  네 관측 구절의 누락·옛 카드 오탐을 함께 확인한다. 수정 전 네 건 모두 실패했다.
- `question_guard::tests::observed_corrections_preserve_pending_decisions_and_separate_approvals`:
  정정 구절 앞뒤의 승인, 다음 문장 질문, A/B 선택지와 원본 행 번호를 확인한다. 점검에서 정상 경고로
  남긴 기획 폴더 변경·보스 싸움 중 두루마리 허용 여부도 계속 경고한다. 두 정상 사례는 원문 전체가
  없으므로 점검의 결정 내용과 독립 선택지를 재구성했다.
- `question_guard::tests::replacement_explanations_keep_answer_targets_and_separate_requests`:
  기존 사례 표에 시안·카드 평문/링크를 추가했다. 질문 수 부족·같은 옛 번호의 실제 답변 요청·별도 승인은
  계속 경고하며, 대체 주체가 옛 카드이거나 대체 여부를 묻는 문장은 참조로 남긴다.
- `post::tests::question_guard_accepts_optional_feedback_and_existing_card_reply_methods`와
  `post::tests::question_guard_keeps_reported_card_links_through_replacement`:
  실제 Host·DB에 카드를 만들고 보고 개정으로 교체한 뒤 harness 완료 사건을 전달한다.
  네 구절에는 알림이 없고 독립 승인·두 미결 결정에는 알림이 저장되는지 확인한다.

고정 sleep·재시도·시한 확대·기대값 완화는 추가하지 않았다. UI·프로토콜·Windows 전용 코드는 바꾸지 않았다.
이번 변경은 관측 표현의 회귀 수리이며 자연어 전체의 의도나 배포 후 오탐률을 보장하지 않는다.

## 검증

모든 검증은 `aaa verify -- <명령>`으로 실행했다. 수정 전 네 구절 회귀 한 건이 예상대로 실패했다.
수정 후 관련 질문 감시 25개가 통과했다. 실제 Host·DB에 카드 생성·보고 개정·완료 사건을 전달해
오탐 알림이 없고 필요한 알림이 저장되는 것까지 직접 실행 확인했다.

개발 검사는 `nice -n 15 python3 scripts/verify.py develop --base 90d63476 --package aaa-host`로 실행했다.

| 검사 | 결과 |
| --- | --- |
| 질문 감시 판정·Host/DB·전달 | 25개 통과, 1.24초 |
| aaa-host 전체 | 953개 통과·실패 0개·기존 수동 제외 2개, 23.29초 |
| aaa-host doctest | 0개 |
| Rust 형식 | `cargo fmt --all -- --check` 통과 |
| aaa-host 모든 target strict Clippy | `-D warnings` 통과 |
| 공개 저장소 검사 | 통과 |

수동 제외는 기존 실제 외부 서버 요청과 설치된 Codex 설정 계약 시험이다.
별도 승인 화면 감지의 `harness::tests::claude_permission_screen`·`codex_approval_screen` 및
`prompt::tests::the_review_and_approvals`도 위 전체 검사에서 통과했다.
개발 계획·결과는 `target/verification/develop-1794959397962113000/`에 보존했다.
미커밋 변경 상태의 개발 검사는 `aaa verify` workspace 원장이 미수집이다.
착지 창의 최종 통합 SHA gate·OS cross·배포는 별도 단계다.

## 참고한 공개 구현

- [regex replace_all](https://docs.rs/regex/latest/regex/struct.Regex.html#method.replace_all),
  [MIT 라이선스](https://github.com/rust-lang/regex/blob/master/LICENSE-MIT):
  일치 구절만 교체하는 기존 방식에 맞춰 예외를 좁혔다.
- [pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark),
  [MIT 라이선스](https://github.com/pulldown-cmark/pulldown-cmark/blob/main/LICENSE):
  Markdown 구조 파싱과 비교했다. 한국어 구절 누락이 원인이므로 파서 교체를 추가하지 않았다.

외부 코드·새 의존성은 추가하지 않았다.
