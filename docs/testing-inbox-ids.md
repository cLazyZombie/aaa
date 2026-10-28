# inbox 짧은 ID 검증(t-1228)

plan §8.533, FRONT_API 1.145. 실제 저장 ID·JSON·투영 파일명과 모든 API의 `id`는 유지하고 표시 전용 `short_id`에 프로젝트별 `i-NNNN`을 싣는다. CLI 일반 출력·웹·알림은 short_id를 보이고 연결·요청은 저장 id를 쓴다. 새 E2E 번호·고정 sleep·재시도·시한 확대는 없다.

- 새 DB keeper `inbox_aliases_backfill_without_rewriting_and_survive_prune_restart_and_rename`은 구판 DB에서 원장 없이 시작해 기존 완료/열린 항목을 시각순으로 채운다. JSON 바이트 불변, 앞 0 별칭, 다른 프로젝트 차단, 완료 항목 prune 뒤 번호 비재사용, 재시작·프로젝트 rename·내보내기/삭제와 9999→10000을 확인한다. 필요성: 표시를 줄이는 변경 때문에 과거 항목·번호가 다른 대상으로 바뀌면 안 된다.
- 새 DB keeper `inbox_aliases_commit_with_every_insert_and_reject_legacy_collisions`는 동시에 저장한4개 항목의 고유 번호, 종료 알림 번호, 별칭 저장 실패 때 batch 전체 rollback과 다음 번호 비점유, 실제 옛 ID/별칭 충돌 거부를 본다. 필요성: post·종료 알림·이관 등 서로 다른 INSERT 경로도 같은 transaction에서 번호를 예약해야 한다.
- 새 Front keeper `aliases_keep_front_seen_done_and_request_protection_on_the_stored_item`은 실제 host에서 list·peek/읽음·Reader 종료 거부, 없는 별칭이 섞인 묶음의 무변경, 미답변 요청 보호, 긴 ID/짧은 ID 혼합 종료, 원래 파일명으로 done 이동과 webhook 사본 ID를 확인한다. 목록 순서는 저장 ID 기준의 기존 순서를 그대로 대조한다. 필요성: 별칭이 읽음·권한·사용자 요청 보호를 우회하거나 투영 경로에 들어가지 않아야 한다.
- 기존 nudge unit과 m13 `inbox_and_nudge`·`nudge_only_marks_the_set_without_a_coordinator_or_when_off`는 대상별 사건 묶음에 i-ID가 붙고, 초안 보호·처음 보는 묶음 한 번·peek/읽음 계약이 유지됨을 본다. m13은 일반/JSON list와 context의 같은 번호, alias/긴 ID 각각의 종료, 실제 DB 원장과 기존 파일명을 함께 확인한다. 기존 `routines`의 파일 확인도 표시 ID를 원장에서 저장 ID로 푼 뒤 같은 투영 파일과 본문을 대조한다.
- 기존 m13 `notify_webhook`은 실제 HTTP webhook에 짧은 i-ID가 실리고 서로 다른 알림이 다른 번호임을 확인한다. coordinator 사용자 post만 발송·thread 보고 미발송·실패 재전송 금지·HTTP500 원인 기록의 기존 단언을 유지한다.
- 기존 E114의 inbox4단계(Chromium/WebKit ×1280/390px)는 실제 host에500개를 넣고 화면의 고유 i-ID500개와 종료 요청의 저장 ID500개가 원장에서 정확히 같은 항목에 대응함을 확인한다. 확인창을 연 뒤 온 항목을 남기는 기존 계약을 유지한다. WebKit390에서는 남은 한 항목의 실제 목록 응답 ID만 구형 host처럼 긴 ID로 전달하고 새 웹이 그 ID를 그대로 보내 실제 host에서 종료하는지 확인한다. 서버 동작은 mock하지 않는다. 필요성: 새/옛 host가 준 두 ID 형식 모두 실제 웹 조작에서 돌아가야 한다.

첫 새 Front 시험은 번호 배정 순서를 목록 순서로 가정해 실패했다. 같은 시각 항목은 기존 대상/저장 ID 정렬을 따른다. 별칭 전환 뒤 다시 정렬하던 초기 코드도 없애고 기존 정렬·예산·읽음 처리를 마친 사본만 줄인다. 원래 순서와 저장 대상으로 정확히 대조하도록 고친 뒤 관련 host55개가1.31초에 통과했다. 원래 실패는 thread 로그에 보존했다.

부분 검증 결과:

- `cargo test --profile verify --locked -p aaa-proto --lib`:50개 통과, TypeScript 생성 일치.
- `cargo test --profile verify --locked -p aaa-host --lib -- inbox:: db::inbox asks:: followups:: question_ids:: notify::`:55개 통과·기존 ignored1개,1.31초.
- `cargo test --profile e2e --locked -p aaa --test m13 -- inbox_and_nudge nudge_only_marks notify_webhook`:3개 통과,8.41초(기존 owner 전체 시간).
- `cargo test --profile verify --locked -p aaa-host --lib`:745개 통과·기존 ignored2개,14.79초.
- `cargo clippy --profile verify --locked -p aaa-host -p aaa-proto -p aaa --all-targets -- -D warnings`:통과.
- `cargo test --profile e2e --locked -p aaa --test m13 routines -- --exact`:기존 owner1개 통과,3.39초.
- `cargo test --profile verify --locked -p aaa --test deps`:2개 통과.
- `python3 -m unittest scripts/test_short_ids.py`:2개 통과.
- fmt·변경 diff 공백·공개 검사와 변경한 m13의 strict clippy가 통과했다. 공개 검사의 첫 실패는 검증 문서의 실제 머신명 때문이었고 일반 표현으로 고쳤다.
- `npm --prefix web run build`, `npm --prefix web test`:타입·번들 및649개 통과.
- E114 inbox4단계:모두 통과,각0.76~1.32초. 옛 ID 왕복을 포함한 마지막 회차 전체6.7초(실험실 준비·정리 포함).

Playwright1.64.1 패키지만 작업 폴더에 두고 내부 공용 Chromium1248·WebKit2370 캐시를 사용했다. 브라우저 설치·사용자 설정 변경은 없다. 실행 binary는 웹 시험 전에 별도 사본으로 고정했다. 전체 ci-local·cross와 main 병합/push는 coordinator의 별도 정규 착지 차례에서 수행한다.

참고는 [Taskwarrior의 ID/UUID 분리](https://taskwarrior.org/docs/ids/)([MIT](https://github.com/GothenburgBitFactory/taskwarrior/blob/develop/LICENSE))와 [GitHub CLI의 번호/URL lookup](https://github.com/cli/cli/blob/trunk/pkg/cmd/issue/shared/lookup.go)([MIT](https://github.com/cli/cli/blob/trunk/LICENSE))다. 저장 ID와 표시 선택자를 나누고 프로젝트/저장소 범위를 유지하는 원칙을 참고했다. aaa는 번호를 재사용하지 않는다. 외부 코드·의존성을 가져오지 않았다.


## 첫 정규 착지 실패와 제품 계약 수리

- 기준 main63a88780에 no-ff 병합한8ba1e77의 최초 전체 ci-local에서 E53과 m13 `big_inbox_lists_within_frame`가 실패했다. E53은 질문 목록의 저장 ID와 inbox 응답의 표시 별칭을 직접 대조했다. 원본 실험실 DB에서 연결 post3개는 실패 전에 모두 done=1, 별칭 원장 i-0003~0005에 대응했다. m13도 저장 ID4001을 찾지 못했다. 두 실패는 같은 항목의 `id`를 다른 API 형식으로 바꾼 제품 회귀이며 부하·시한 문제로 덮지 않는다.
- 사용자 지시대로 저장 ID를 연결 키로 유지하고 표시만 줄인다. `InboxItem.id`를 원래 저장 키로 복원하고 선택적 `short_id`와 Rust `display_id()`를 추가한다. CLI 일반 list/context·nudge·웹은 표시 값을 쓰고 JSON/API·질문 목록·웹 종료 요청·webhook id·투영 파일은 동일한 저장 키를 쓴다. 별칭이 없는 구형 JSON은 그대로 읽고 표시 fallback을 쓴다. inbox done의 별칭 입력은 원장에서 저장 ID로 해석한다.
- 별칭은 응답 사본에만 채우며 JSON 예산 계산 전 채워 실제 응답 크기에 포함한다. 모든 항목 생성 경로의 short_id는 None(직렬화 생략)이라 기존 저장 JSON 형식을 유지한다. schema·원장·번호와 파일명은 바꾸지 않는다.
- 기존 E53과 `big_inbox_lists_within_frame`의 ID·done·원문·개수·페이지·시한 단언은 유지한다. 새 Front keeper에 API id 불변, 사용자 요청 목록과 같은 id, 구형 JSON fallback을 추가한다. E114는 별칭 표시/저장 ID 요청의 정확한 원장 대응과 short_id 없는 구형 응답 왕복을 검사한다. 실패 시험을 우회하거나 기대값만 바꾸지 않는다.
- 최초 회차 원문·시작/실패 uptime·실험실은 thread library에 보존한다. 수리 뒤 전체 정규 착지 회차를 새 로그 경로에서 처음부터 실행한다. 검증 결과는 thread 보고와 library에 남기며 testing.md에는 기존 링크 한 줄만 둔다.
