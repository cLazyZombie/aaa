# brief 확정 전 질문 안내 소진 수리(t-1851)

## 첫 실패와 원인

통합 후보 `f09e20f53e9cf5ab43a5ad6f4eed4eb56f365249`의 정규 착지 검증에서 E2E 53의
「근거가 Questions 위에만」 단계가 pane의 `[aaa 질문 근거 확인]`을 기다리다 timeout으로 실패했다.
원문은 `target/verification/landing-1794617721972663000/0/e2e.log`에 보존했다.
실험실 일반 파일 333개와 로그를 별도로 바이트 대조해 `target/t-1851-first-landing-failure/`에 보존했다.

보존한 DB에는 같은 보고 hash의 `question_warning_sent`가 이미 기록돼 있었고, coordinator 경고에는
`pane 알림 전달 실패: ... 아직 brief를 받지 않았다`가 남아 있었다. 보고 수집 시각은 brief Enter 확인
뒤지만 최종 전달 확정보다 앞이었다. `thread_message`는 brief가 Delivered가 아니면 메시지 저장 전에
거부한다. 반면 `warn_question_format`은 이 조건을 확인하지 않고 먼저 보낸 표시를 확정했다.
다음 점검은 같은 hash의 보낸 표시를 보고 돌아와 pane 안내가 영구히 누락됐다.

이 함수는 시작 main과 실패한 통합 후보에서 동일했다. 원인을 부하나 t-1849 변경으로 단정하지 않는다.
빠른 agent의 보고가 host의 brief 확정보다 먼저 도착할 수 있는 제품 경합이다.

## 수리

brief 확정 전에는 경고 전달과 보낸 표시를 보류한다. 같은 보고를 다시 점검하는 기존
`collect_if_changed`가 brief 확정 뒤 안내를 전달한다. 최신 기록에 보낸 표시를 확정하는 잠금 안에서도
brief 상태를 다시 확인해 재기동과의 경합을 피한다. 기존 hash별 한 번 전달 방식은 유지한다.
보고·질문 카드·근거 경고의 수집은 그대로이며, 별도 안내의 전송 시점만 준비 뒤로 옮긴다.
새 큐·프로토콜·의존성·시간 설정은 추가하지 않았다.

## 결정적 재현

`evidence_warning_waits_for_brief_confirmation_then_reaches_the_pane_once`는 가짜 Agent link에서
첫 Enter를 처리한 응답만 사건으로 보류하고, 그 사이 실제 보고 수집을 끝낸다. 이후 응답을 놓고
같은 보고를 두 번 점검한다. 고정 sleep·시한 증가·기대값 완화는 없다.

- 기존 코드 RED: 0.47초에 「brief 확정 전에는 안내를 보낸 것으로 기록하지 않는다」 단언 실패.
- 수리 뒤 GREEN: 0.48초에 통과.
- 확정 전 표시 없음, 확정 뒤 실제 Input의 안내 태그 한 번, coordinator 경고 한 번,
  pane 전달 실패 없음, 본문 재수집 없음까지 함께 확인한다.
- 기존 brief 확인 순서 시험이 같은 Agent link gate를 사용한다. gate는 최초 Enter만 붙잡고 이후
  안내 입력은 그대로 통과시켜 실제 메시지 경로를 확인한다.

검증은 모두 `aaa verify -- <명령>`으로 실행한다. RED·GREEN 원문은 각각
`target/t-1851-e53-red.log`, `target/t-1851-e53-green.log`에 있다.
관련 검증은 `aaa verify -- nice -n 15 env NODE_PATH=<작업 전용 Playwright 1.64.1 경로>
python3 scripts/verify.py develop --base f09e20f53e9cf5ab43a5ad6f4eed4eb56f365249 --package aaa-host --spec 53`으로 실행했다.

| 검사 | 결과 |
| --- | --- |
| aaa-host 전체·doctest | 920개 통과·2개 기존 무시, doctest 0개 |
| aaa-host all-targets strict Clippy | 통과 |
| 웹 타입·번들·웹 유닛 | 720개 통과 |
| E2E 53 전체 | Chromium·WebKit의 기존 단계 모두 통과(13.9초) |
| fmt·공개 검사 | 통과 |

원래 E2E 53의 시한과 단언은 바꾸지 않았다. 첫 실패를 같은 조건으로 반복해 통과 판정한 것이 아니라,
보존 DB에서 원인을 확인하고 사건 제어 RED를 만든 뒤 제품을 고쳐 GREEN과 실제 E2E를 확인했다.
상세 개발 원문은 `target/verification/develop-1794618656642911000/`에 보존한다.
착지 검증·cross Clippy·push는 새 착지 창에서 별도로 수행한다.

## 참고한 공개 구현

- [RabbitMQ의 전달·처리 확인 구분](https://www.rabbitmq.com/docs/confirms),
  [MPL-2.0 라이선스](https://github.com/rabbitmq/rabbitmq-server/blob/main/LICENSE-MPL-RabbitMQ).
- [NATS JetStream의 명시적 처리 확인](https://docs.nats.io/learn/jetstream/pull-consumers),
  [Apache-2.0 라이선스](https://github.com/nats-io/nats-server/blob/main/LICENSE).

두 구현에서 전송 준비·처리 기록을 구분하는 관점을 비교했다. aaa에서는 외부 큐를 도입하지 않고
기존 점검과 hash별 원장을 사용한다. 외부 코드는 가져오지 않았다.
