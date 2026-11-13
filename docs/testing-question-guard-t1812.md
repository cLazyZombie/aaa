# 질문 감시의 부정문·선택적 피드백·기존 카드 안내 수리(t-1812)

## 보존하는 계약

- `question_guard::tests::observed_negative_and_optional_feedback_do_not_hide_real_requests`: 관측한 부정문·뜻 정정·과거 오류 제보 안내를 경고하지 않는다. 같은 문장 앞뒤의 승인 요청, 다음 문단의 실제 질문·정보 요청, 선택지는 계속 경고한다. 실제 요청의 원본 행 번호도 유지한다.
- `question_guard::tests::observed_work_and_visual_choices_still_require_cards`: 실제 카드 생성으로 이어졌던 작업 선택과 다음 시안 선택 요청 두 건을 계속 경고한다. 미래 안내 전반을 면제하지 않는다.
- `question_guard::tests::card_or_chat_reply_instructions_link_only_to_the_existing_topic`: 열린 카드 링크 뒤의 “카드나 채팅으로 답해 주세요” 안내는 기존 카드로 연결한다. 답한 카드 재안내·닫힌 카드 경고를 구별하며, 모르는 번호·다른 주제·추가 질문·별도 승인 내용은 면제하지 않는다.
- `post::tests::question_guard_accepts_optional_feedback_and_existing_card_reply_methods`: 실제 Host·DB에서 이전 턴의 질문 카드를 만든 뒤 완료 사건을 처리한다. 세 오탐과 카드 답변 방법 안내에 누락·교체 경고가 없고, 별도 배포 질문에는 누락 경고가 생기는지 확인한다.

모든 사례는 공개할 수 있는 짧은 문장과 재구성한 내부 fragment 링크만 쓴다. 고정 sleep·재시도·시한 확대는 없다. 선택적 안내와 부정 구절을 원래 바이트 길이의 공백으로 바꿔 진단 위치를 보존한다. 새 카드 연결은 답변 방법만 담은 구절과 같은 주제의 앞선 q-ID에 한정한다.

## 검증

수정 전 새 회귀 시험에서 부정문·두 피드백과 기존 카드 안내의 오탐을 재현했다. 정상 경고 두 건과 기존 시험은 통과했다.

수정 후 `aaa verify -- nice -n 15 cargo test --profile verify --locked -p aaa-host --lib question_guard`에서 판정·실제 Host·DB·전달 경로 18개가 통과했다. 기존 인용 설명·닫힘·교체·답변 후 재안내·다른 프로젝트·추가 요청·중복 완료·입력 초안 보존 owner도 포함한다.

개발 검증 `aaa verify -- nice -n 15 python3 scripts/verify.py develop --base <분기 기준 SHA> --package aaa-host`가 통과했다. aaa-host 전체 시험은 906개 통과·기존 2개 제외, doctest는 0개이며, 전체 target strict Clippy(`-D warnings`)와 공개 검사도 통과했다. 형식 검사 `aaa verify -- nice -n 15 cargo fmt --all -- --check`도 통과했다. 최종 통합 SHA의 전체 gate와 배포 검사는 착지 창에서 수행한다([검증 규칙](verification.md)).
