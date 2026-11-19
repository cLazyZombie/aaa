# 메시지 입력 단계와 늦은 수락 알림 검증

- `harness::tests::late_message_receipts_update_the_original_alert_without_writing_again`은 Host에서 실제 메시지 요청 경로와 SQLite를
  사용한다. 가상 시계로 본문 및 Enter 쓰기 뒤 기존 수락 시한을 지난다. 그 뒤 대화 기록이나 큐 영수증이 오면 같은 ID의 알림과
  메시지 상태가 바뀌는지 본다. 다른 메시지의 알림과 기존 읽음·완료 상태도 함께 확인한다.
- 같은 시험에서 다른 입력의 기록과 별도로 들어온 동일 본문을 주입한다. 후속 입력이 원래 메시지의 확인으로 연결되지 않아야 한다.
  붙여넣기와 Enter의 전체 쓰기 목록으로 재전송 여부를 확인한다.
- `an_earlier_same_body_can_arrive_in_the_transcript_late`는 줄바꿈 표기가 다른 동일 본문을 포함해, 앞선 수락·미확인 전송과
  입력하지 않은 기록을 구별하는지 본다.
- `claude_thread_states_questions_and_messages`는 격리된 host·agent·PTY에서 글을 친 뒤 확인창이 뜨면 Enter가 전송되지 않고
  CLI JSON에서 `draft_written`과 `unknown`이 함께 반환되는지 확인한다.
- 웹 알림 분류 시험은 기존 미확인 항목과 늦게 확인된 항목의 표시를 확인한다. 새 응답의 입력 단계 필드는 선택이다.
