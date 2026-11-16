# coordinator 정기 점검 재안내 경계 검증

`guide.rs`의 「매 턴」 안내에 이미 알린 미답 질문의 정기 반복 금지,
변화가 없을 때의 한 줄 응답, 첫 안내·새 오류·질문 변경·답 접수·사용자 질문의 알림 경계를 넣었다.
운영 자동화는 변경하지 않았으며, 안내 준수를 강제하는 기능은 추가하지 않았다.

## 크기 제한 실패와 수리

첫 문구 추가 후 `coordinator_guide_stays_small`이 16,637바이트로 실패했다.
변경 전 안내는 16,362바이트여서 16 KiB 제한까지 22바이트만 남아 있었다.
역할 소개와 매 턴 절의 중복 표현을 줄이고 기존 지시를 유지했다.
수리 후 생성한 안내는 16,360바이트·168줄이며, 기존 16 KiB·200줄 제한을 그대로 통과한다.

## 관련 확인

검증 명령은 `aaa verify --`로 실행했다.

- `cargo test --profile verify --locked -p aaa-host --lib guide::tests::`: 4개 통과.
  안내 크기·머리말, 주제 목록과 연결, 질문 안내 도달성 및 필터에 포함된 enroll 안내 시험이다.
- 변경한 `guide.rs`를 읽는 임시 실행 파일에서 실제 `coordinator_guide`를 호출해
  「매 턴」 문구와 위 크기·줄 수를 확인했다. 운영 설정이나 생성 파일은 바꾸지 않았다.
- `cargo fmt --all -- --check`: 통과.
- `cargo clippy --profile verify --locked -p aaa-host --all-targets -- -D warnings`: 통과.

새 문구를 그대로 대조하는 시험은 추가하지 않았다. 모델의 실제 응답 준수를 시험한 결과는 아니다.
전체 착지 gate는 승인된 착지 창의 최종 통합 SHA에서 수행한다.
