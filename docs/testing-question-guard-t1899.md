# 질문 감시의 명시적 카드 대체 설명(t-1899)

## 원인과 수리 범위

관측한 문장 ‘앞서 열려 있던 q-3822는 이 질문이 대신합니다.’와 최신 q-3834 안내 링크를
같은 문단에 둔 회귀 시험을 먼저 실행했다. 링크의 안내 문구는 재구성했고 교체 설명은 그대로 보존했다.
수정 전에는 `missing=None`, `stale=request`로 재현됐으며 열린 q-3834와 닫힌 q-3822가 함께 참조에 남았다.

`answer_ids()`의 기존 구절 제외 규칙에 ‘q-ID는/은 이 질문이 대신합니다’를 추가했다.
평문과 Markdown 링크를 모두 처리하며 해당 참조만 제외한다. ‘대신합니다만’처럼 설명이 이어지는 말은
어미 경계로 구분해 제외하지 않는다. 대체 여부를 묻거나 옛 카드가 대체 주체인 문장은 제외하지 않는다.
카드 상태 조회·완료 사건 처리·누락 및 교체 경고 정책은 바꾸지 않았다.

## 확인하는 계약

- `question_guard::tests::observed_replacement_explanation_is_not_an_old_card_answer_request`:
  실제 문장의 최신 카드 Open·옛 카드 Closed 조합에 경고가 없어야 한다.
- `question_guard::tests::replacement_explanations_keep_answer_targets_and_separate_requests`:
  교체 설명의 평문·링크를 답할 카드 수에서 빼고, 같은 옛 번호를 실제로 다시 안내하면 교체 경고를 유지한다.
  같은 줄·다음 문단의 별개 승인 요청과 질문 수 부족은 누락 경고를 유지한다.
- `post::tests::question_guard_keeps_reported_card_links_through_replacement`:
  실제 Host·DB에 보고 개정을 저장해 카드를 교체한 뒤 완료 사건을 처리한다.
  새 카드 링크와 관측한 대체 설명에는 경고가 없고, 같은 옛 번호로 실제 판단을 요청하면 현재 카드 번호를 포함한
  교체 경고를 남긴다. 대체 설명 뒤의 독립 배포 질문에는 누락 경고를 남긴다.

기존 질문 감시·Host/DB·전달 owner도 함께 실행한다. 새 sleep·재시도·시한 연장·기대값 완화는 없다.
UI·프로토콜·Windows 전용 코드·의존성은 변경하지 않았다. 자연어 전체의 의도를 보장하는 변경은 아니다.

## 검증

수정 전 회귀 한 건이 실패했음을 확인했다. 첫 수리 후 관측 문장과 Host/DB 경로는 통과했다.
추가 경계 시험은 ‘판단해 주세요’ 단독 표현이 기존 `requests()`의 요청 어휘에 포함되지 않아 경고 경로를
거치지 않으면서 실패했다. 기존 Host/DB 사례처럼 판단 요청에 A/B 선택지를 함께 구성해 실제 카드 안내를
검사하도록 고쳤다. 교체 경고를 요구하는 단언은 유지했고 요청 탐지 어휘는 이번 범위에서 바꾸지 않았다.
모든 검증 명령은 `aaa verify -- <명령>`으로 실행했다.
개발 검사는 `nice -n 15 python3 scripts/verify.py develop --base c999ccab --package aaa-host`를 사용했다.

| 검사 | 결과 |
| --- | --- |
| aaa-host 전체 | 927개 통과·실패 0개·기존 수동 제외 2개 |
| 관련 질문 감시 판정·Host/DB·전달 | 위 전체에 포함된 23개 통과 |
| aaa-host doctest | 0개 |
| aaa-host 모든 target strict Clippy | `-D warnings` 통과 |
| Rust 형식 | `cargo fmt --all -- --check` 통과 |
| 공개 저장소 검사 | 통과 |

수동 제외는 기존 외부 서버 요청과 설치된 Codex 설정 시험이다.
개발 계획·결과는 `target/verification/develop-1794781532310389000/`에 보존한다.
개발 검사의 미커밋 상태로 `aaa verify` workspace 원장은 미수집이다.
착지 창의 최종 통합 SHA gate·OS cross와 배포 검사는 별도 단계다.

## 참고한 공개 구현

- [regex](https://github.com/rust-lang/regex): 기존 정규식의 일치 구절만 교체하는 방식을 유지했다.
  MIT 또는 Apache-2.0 라이선스를 확인했다.
- [pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark): CommonMark 구조 파싱과 비교했다.
  MIT 라이선스를 확인했다. 이번 원인은 한국어 교체 설명의 의도 구분이므로 Markdown 파서 교체를 추가하지 않았다.

외부 코드를 가져오거나 새 의존성을 추가하지 않았다.
