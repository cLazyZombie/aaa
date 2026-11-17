# 질문 답 E2E의 brief 준비 경합(t-1978)

착지 전체 gate의 E2E 53은 직접 답·그림 시험에서 `bad_request: t-0004는 아직 brief를 받지 않았다`로 실패했다.
q-4170 1A로 원인 조사·수리와 착지 재개를 승인받았다. 실패한 실행을 성공으로 덮거나 그대로 재실행하지 않았다.

## 원인 근거

가짜 agent는 brief 안내를 받아 보고 파일을 쓴다. host의 보고 수집과 입력 수락·brief 전달 확정은 별도 작업이다.
`brief.rs`에도 보고가 받음 확인보다 먼저 올 수 있음을 명시한다.
`thread_message`는 brief가 `delivered`가 아니면 입력 전에 거절한다.
시험은 `report_hash`가 생긴 것만 보고 질문 보고로 교체하고 답을 보내 이 보호 조건과 경합했다.

보존한 실패 실험실의 host 로그·진단 파일·읽기 전용 SQLite에서 다음 순서를 확인했다(UTC).

| 시각 | 사건 |
| --- | --- |
| 13:39:25.619420 | 첫 보고 수집 로그 |
| 13:39:25.658234 | 바꾼 질문 보고 수집 로그 |
| 13:39:25.934470 | brief 미전달 답 API 오류의 실패 진단 파일 기록 |
| 13:39:25.960516 | brief 입력의 accepted 로그 |
| 13:39:25.961000 | DB의 brief 전달 확정 시각 |

실패 증거가 brief 확정보다 약 27ms 앞선다. 부하 자체를 원인으로 추측하지 않고, 필요한 준비 조건의 차이를 수리했다.

## 변경과 보존하는 계약

`53-thread-questions.mjs`의 첫 보고 대기 네 곳과 `question-reply.mjs`의 같은 대기에
`report_hash && brief === "delivered"`를 요구한다. 그 뒤 기존대로 보고를 바꾸고 브라우저에서 답한다.
대기 상한은 그대로이며 첫 보고·전달 상태가 관측되면 바로 끝난다.

- 줄바꿈·조합 중 전송 방지·첨부 사본·답 원문·inbox·카드·늦은 응답 순서 단언을 유지한다.
- brief를 강제로 보내거나 DB 상태를 바꾸지 않는다. 답 API의 거절을 허용하지 않는다.
- 고정 sleep·재시도·시한 연장·기대값 완화·skip은 추가하지 않았다.
- 제품 코드는 바꾸지 않았다.

기존 host의 `evidence_warning_waits_for_brief_confirmation_then_reaches_the_pane_once`는
BriefGate와 가상 시각으로 보고가 먼저 수집된 상태에서 brief 확인까지 안내 전송이 보류되는 경계를 검사한다.
이 owner와 E2E 53의 실제 host·agent·Chromium·Mac WebKit으로 수리를 확인한다.

## 검증 결과

모든 검사는 `aaa verify -- <명령>`으로 실행했다.

- 기존 BriefGate Rust owner 1건 통과(0.76초). 보고가 먼저 수집됐을 때 brief가 Pending임을 확인하는 결정적 시험이다.
- `ci-local.sh --only web,e2e,public --spec 53`: 웹 타입·번들·유닛 744건, E2E 53 전체, 공개 검사 통과.
- 실제 Mac WebKit 페이지에서 E2E 53 전체를 별도로 실행해 통과했다.
- 실제 host·agent 실험실 넷(Chromium 둘·WebKit 둘)을 동시에 실행하고 기존 CDP CPU 4배 지연을 더한 조건도 모두 통과했다.
  WebKit 자체에는 CDP 지연을 적용하지 않는다. 각 실행 25.89~27.03초였으며 실패 재시도는 없다.
- 일반·부하를 합쳐 주 페이지 Chromium·WebKit 각각 3회씩, 전체 실행 6회가 통과했다.

제품 파일·공용 harness·의존성·시한은 바꾸지 않았다.
착지 재개 때 최신 main 위에서 전체 ci-local과 Linux·Windows strict cross를 실행한다.
원래 실패와 보존 위치는 작업 보고의 `library/landing-failure.md`에 남겼다.
