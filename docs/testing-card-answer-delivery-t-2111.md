# 한도 대기 카드 답 재전달

- Host의 `harness::tests::card_answers_wait_for_the_same_report_and_pane_and_start_input_once`는 agent 링크에 들어간 입력을 센다. 한도 화면에서는 0건, 같은 보고와 pane에서 재개하면 동일 본문 붙여넣기와 Enter 각 1건, 보고 교체·agent에만 아직 있는 새 보고·thread 종료·pane 교체·같은 ID의 새 실행은 0건이다. 붙여넣은 다음 대기 화면이 나타나면 기존 수락 확인 경로에 남는다. 재개 후 수락 미확인 및 늦은 수락은 원래 inbox 항목을 갱신한다. 입력 시작으로 기록된 답은 host 재시작 뒤 재전송 대상에서 제외된다.
- 웹 E2E 142는 가짜 Claude 화면을 실제 PTY에 출력한다. Chromium 데스크톱과 WebKit 휴대폰에서 카드로 답하고, 한도 대기 동안의 0바이트와 재개 후 같은 답 한 건, 질문 ID 링크에서 다시 연 카드와 같은 inbox 항목의 상태 변화를 검사한다.
