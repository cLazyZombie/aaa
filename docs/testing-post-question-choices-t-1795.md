# 선택지 없는 사용자 요청 수신 거절(t-1795)

## 원인과 수리

`post::store_post`는 사용자 요청의 역할·크기·자료를 검사했지만 선택지를 요구하지 않았다. `link_thread_question`도 명시적 접미사가 있으면 확인 없이 반환했다. 이 때문에 열린 thread 카드가 없는 평문도 `asks::post_question_ids`의 구판 호환 경로에서 독립 q-ID가 됐다.

새 `post user --request --needs-user`는 다음 두 경로로만 받는다.

- 같은 프로젝트의 현재 미답변 thread 질문에 연결한다. 제목의 `aaa:question/<thread 또는 현재 q-ID>`를 확인하고, 접미사가 없으면 제목·본문의 t-ID·q-ID로 열린 thread 하나만 찾는다. q-ID는 현재 보고 판과 질문 위치까지 확인한다.
- 접미사 없이 본문에 `## Questions`를 쓴다. 기존 proto 파서의 경고를 거절 사유로 쓰고 모든 질문에 선택지 두 개 이상을 요구한다. 새 본문과 기존 카드 접미사를 함께 보내면 거절한다.

거절에는 형식 예시와 `aaa guide questions`, 새 post ID로 다시 보내는 방법이 있다. 거절 원장은 재전송 응답을 보존하지만 새 질문 번호·카드·푸시는 만들지 않는다. 연결 검사에서 읽은 질문 번호를 저장까지 보존하므로 그사이 보고가 닫혀도 독립 평문 카드로 바뀌지 않는다.

인증·GUI 조작·터미널 승인도 기존 coordinator 안내에서 사용자 요청으로 쓰인다. 별도 옵션을 추가하지 않고 「A 완료했다 / B 보류한다」로 답을 받는다. 일반 완료·보고서 알림은 기존 notice 경로를 유지한다. coordinator AGENTS 생성 원본과 상세 questions guide, 사용 안내를 함께 수정했다.

## 회귀와 실행 확인

관련 owner는 `aaa-host`다. 모든 검증은 `aaa verify -- <명령>`으로 실행한다.

- `post::tests::standalone_user_requests_require_answerable_questions_before_creating_cards`: 평문·선택지 없음·하나·중복·여러 질문 중 누락을 거절한다. 재전송도 같은 거절이며 원장 외 카드·질문 번호를 만들지 않는다. 두 선택지가 있는 사람 전용 행동 요청과 재전송은 받는다.
- `post::tests::plain_user_requests_link_only_one_current_thread_question`: t-ID·현재 q-ID 자동 연결과 명시적 접미사를 받는다. 옛 판·다른 프로젝트·부분 문자열·여러 대상·잘못된 접미사·답한 카드·끝난 thread를 거절한다. 기존 카드 접미사와 새 질문을 섞어 기존 질문을 가리는 조합도 거절한다.
- `post::tests::linked_request_keeps_its_question_ownership_when_the_report_closes_during_receipt`: barrier로 연결 검사 완료와 저장 사이에 보고가 질문을 비우는 사건을 넣는다. 기존 thread 소유권과 번호를 유지하며 독립 post 질문 번호를 만들지 않는지 확인한다.
- `asks`·`question_ids`: 구판에서 이미 받은 평문·카드 없는 접미사 기록은 시험에서 원장을 직접 준비한다. 과거 답이 독립 요청을 닫지 않는 계약과 기록 보존을 유지한다. 새 요청 fixture는 두 선택지로 바꿨다. 질문 본문 보존과 푸시 이동 대상의 기대도 새 fixture의 실제 원문·q-ID로 검사한다.
- 웹 E2E 69: 실제 CLI→host에서 평문·없는 접미사·선택지 하나의 거절 메시지와 카드 수 보존을 검사한다. thread·현재 q-ID 접미사가 별개 카드를 만들지 않는지 확인한다. 두 선택지 본문의 카드·답·다른 기기 동기화도 확인한다.
- 웹 E2E 129와 일괄 처리 공용 fixture는 새 요청을 두 선택지로 준비한다. 지연 응답과 기록 사본 보존의 기존 단언은 유지한다.

시간을 자는 대신 기존 CLI 완료·host 사건·DOM 조건과 barrier를 사용한다. 운영 host의 post·질문 기록은 읽거나 수정하지 않는다.

## 독립 리뷰

독립 리뷰에서 구조화 본문의 접미사 검사 누락과 연결 검사 뒤 보고 교체 경합을 찾았다. 접미사 조합 거절과 검증한 번호 보존으로 고쳤으며 두 경계의 회귀를 추가했다.

## 참고한 공개 구현

- [clap ArgGroup](https://docs.rs/clap/latest/clap/struct.ArgGroup.html) ([MIT](https://github.com/clap-rs/clap/blob/main/LICENSE-MIT)): 서로 다른 유효 입력 경로를 구분하는 검사를 비교했다. 열린 카드와 보고 판은 host만 확인할 수 있으므로 CLI 인자 검사로 옮기지 않았다.
- [Ajv 배열 길이 검증](https://ajv.js.org/json-schema.html#minitems-maxitems) ([MIT](https://github.com/ajv-validator/ajv/blob/master/LICENSE)): 선택지의 최소 개수 검사를 비교했다. Markdown을 중복 파싱하지 않고 기존 `aaa-proto::parse_report_questions`의 결과·경고를 재사용한다.

외부 코드나 새 의존성을 추가하지 않았다.

## 관련 owner 결과

고정한 최종 소스에서 `aaa verify -- nice -n 15 python3 scripts/verify.py develop --base HEAD --package aaa-host`로 902개 통과·실패 없음·기존 제외 2개를 확인했다. 같은 실행의 strict Clippy와 공개 검사도 통과했다. 선택지 검사를 제거한 변이는 새 평문 거절 회귀에서 종료 101로 실패했고 원문을 복원했다. 정규 전체 gate의 후보 SHA·결과·실행 확인은 t-1795 보고의 상세 기록에 남긴다. 착지 창에서는 최종 통합 SHA의 gate를 별도로 수행한다.
