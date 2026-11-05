# 보고 판정 경계 수리 검증(t-1559)

## 범위

빈 보고 발행 보류와 할 일 확인 카드의 직접 처리·근거 표시·보고 존재 판정을 함께 바꾼다.
운영 DB·기존 빈 r-ID·다른 저장소는 수정하지 않는다. 착지 gate·push·배포는 승인 뒤 별도다.

## 재현과 회귀

- 수리 전 agent의 `empty_reports_are_deferred_until_nonempty_content_is_written`는
  첫 빈 파일의 `report_hash(...).is_none()` 단언에서 실패했다.
- 수리 전 host의 `empty_report_snapshots_preserve_state_until_one_nonempty_revision_arrives`는
  빈 보고의 hash·copy·변경 시각이 갱신되어 기존 기록 보존 단언에 실패했다.
- agent 수집 시험은 첫 빈 파일→질문 본문→빈 중간 상태→새 본문을 쓴다. 실제 vnode 시험은 같은 흐름에
  빈 파일·정상 본문의 원자적 교체도 더하고 매 단계의 사건 도착을 기다린다. 빈 hash는 없고 수집 오류는 Conflict다.
- host 수집 시험은 legacy/v2 전송, 첫 판/기존 질문 판, 일반/마지막 회수와 빈 snapshot/Conflict를 조합한다.
  빈 상태의 전체 thread 기록·본문·manifest·판 목록·사건 수를 대조하고 다음 정상 변경은 한 판·한 report_ready만 만든다.
- 카드 owner는 직접 API를 통해 완료 보관·재배정 예약·연결 해제, 답 기록의 근거 보존, Reader 거부,
  중복 응답·오래된 블록/보고·다시 열린 thread·하위/미처리 후속 보존을 검사한다.
- E2E 128은 표지 대신 다른 저장소 착지 줄로 시작하는 보고를 실제 수집·resolve한 뒤
  Chromium/WebKit 1280·390에서 세 단추를 누르고 완료 보관·예약·연결·답 기록을 대조한다.

## 개발 검증

모든 검증 명령은 `aaa verify -- <명령>`으로 실행했다.

| 검사 | 결과 |
| --- | --- |
| `cargo test --profile verify --locked -p aaa-agent -p aaa-host -p aaa-proto` | agent 204·host 853·proto 54 통과, 기존 host 2개 ignored 유지 |
| 같은 세 package의 `cargo clippy --profile verify --locked --all-targets -- -D warnings` | 통과 |
| `ci-local --only web,e2e,public --spec 128`의 web | 타입·번들·웹 유닛 702개 통과 |
| 같은 실행의 public | 통과 |
| 격리 Playwright 1.64.1의 `ci-local --only e2e --spec 128` | 24.7초, 4개 화면 조합 × 3개 동작 통과 |

첫 개발 묶음의 E2E는 전역 Playwright 1.59.1이 선택되어 버전 확인에서 멈췄다(브라우저 시험 시작 전).
작업 폴더의 Playwright 1.64.1을 `NODE_PATH`로 지정해 미실행 브라우저 owner를 완료했다.
전역 설치·브라우저·시스템 설정은 바꾸지 않았다. 시험 재시도·시한 연장·기대값 변경은 없다.
웹 화면 네 장은 직접 검토했으며 390px에서도 세 처리 단추·근거가 보이고 가로 넘침이 없다.

`cargo test --profile verify --locked -p aaa --test deps`의 2개가 통과했다.
`cargo test --profile verify --locked -p aaa --test m13 main_ancestor_archives_landing_without_asking_but_keeps_unmerged_and_missing_reports`
도 통과했다. 실제 Git·host·agent를 통해 push된 원격 main 포함은 완료 보관하고,
로컬에만 병합한 경우·실제 보고가 없는 경우는 보류하는 기존 계약을 유지한다.
착지 창에서 최종 통합 SHA의 전체 gate를 수행하며 이번 개발 검사로 대신하지 않는다.

## 자동 완료 판단

원격 main에 실제 포함된 커밋은 그 저장소의 착지가 끝났다는 근거다. 기존 thread 저장소에서의 검증을 유지한다.
할 일에 적힌 저장소와 실제 착지 저장소가 다르면, 그 반영만으로 할 일의 전체 완료 범위를 확정할 수 없다.
따라서 다른 저장소의 착지 문구·SHA를 임의로 찾아 자동 완료하는 동작은 넣지 않았다.
카드는 해당 줄을 보여 주며 사용자의 완료 선택을 「사용자 완료 확인」으로 완료 보관한다.

## 참고 후보

- [notify의 편집기 저장 동작](https://docs.rs/notify/latest/notify/#editor-behaviour):
  truncate와 파일 교체를 모두 고려한다. [CC0-1.0](https://docs.rs/crate/notify/latest/source/LICENSE-CC0).
- [Watchexec의 파일 사건·debounce 문서](https://github.com/watchexec/watchexec/blob/main/doc/watchexec.1.md):
  파일 쓰기 중 여러 사건이 올 수 있음을 비교했다. [Apache-2.0](https://github.com/watchexec/watchexec/blob/main/LICENSE).

기존 vnode 감시를 유지하고 읽은 본문과 host 저장 경계에서 판정한다. 외부 코드·새 런타임 의존성은 추가하지 않았다.
