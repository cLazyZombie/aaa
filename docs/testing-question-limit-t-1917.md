# 보고 질문 9개 상한 안내와 진단

`QUESTION_MAX=9`와 기존 질문·선택지 파싱은 유지한다. 질문이 10개 이상이면
`보고당 질문 최대 9개`를 따로 알리고, 9개 이하로 나눠 남은 질문을 기록해 다음 보고에서 묻게 안내한다.
질문 수 초과 자체를 형식 오류로 표시하지 않으며, 실제 형식 오류도 함께 있으면 두 진단을 모두 남긴다.
잘린 질문의 선택지도 검사하므로 마지막 질문의 문법 오류를 놓치지 않는다.
thread brief와 `aaa guide questions`에 같은 상한·나눠 묻기 안내를 넣었다.

## 관련 owner

- `aaa-proto`: `report_questions_distinguish_question_limit_from_format_errors`에서
  `### N.`과 기존 번호 목록 각각의 9개 정상·10개 초과, 앞의 9개 카드 보존,
  선언 수 불일치·선택지 누락·잘린 마지막 질문의 파싱 누락과 상한 초과 동시 진단을 확인한다.
  선택지 10개는 기존 9개 상한과 형식 진단을 유지하며 질문 수 초과로 표시하지 않는지도 본다.
- `aaa-host`: 기존 brief 조립·guide 안내 도달성 시험과 crate 전체 시험을 실행한다.
- 두 crate strict Clippy와 공개 저장소 검사를 개발 진입점으로 실행한다.
- 착지 gate는 착지 창의 최종 통합 SHA에서 수행한다.

## 결과

`aaa verify -- nice -n 15 python3 scripts/verify.py develop --base <개발-시작-SHA> --package aaa-proto --package aaa-host` 통과.

- `aaa-host`: 942개 통과, 기존 제외 2개, 실패 0개. brief 조립·questions guide 도달성 시험 포함.
- `aaa-proto`: 60개 통과, 실패 0개. 새 경계 시험 포함.
- 두 crate doctest·strict Clippy와 공개 저장소 검사 통과.
- 상한을 임시로 10으로 바꾸면 새 경계 시험이 카드 개수 `10 != 9`로 실패했다.
  원본 바이트를 복원한 뒤 같은 시험도 통과했다.

시험의 시한·기대값·기존 제외 목록은 바꾸지 않았다.
