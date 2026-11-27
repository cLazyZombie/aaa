# 다른 모델 검수 작업 검증

- `m13::review::remote_review_is_pinned_owned_and_cancellable`는 서로 다른 실험실 host·agent를 TLS로 연결한다. 실제 thread pane의 CLI에서 Codex·Claude 가짜 실행기를 쓰고, 후보 판과 작업 중 파일의 격리, profile과 인증, finding JSON, 현재 판 판정, 다른 thread 접근 거부, 인증 실패·취소, 임시 폴더 정리와 완료 사건을 확인한다. 실행기는 agent와 취소 socket으로 준비·종료를 확인한다.
- `review::tests::host_restart_keeps_sent_reservation_and_resumes_only_pre_dispatch_waiting`는 host를 다시 열어 실행 표지의 불명 전환과 small 예약 보존을 확인한다. 송신 전 작업만 대기로 돌아간다.
- CLI와 host·agent의 관련 owner를 개발 지침에 따라 검증한다. 실제 제공자 계정은 사용하지 않는 시험이다.
