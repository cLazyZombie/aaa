# provider별 모델 켜기·끄기 검증(t-1509)

설정 「모델」과 `aaa model`이 host.toml `[models.<harness>] disabled`를 고치고, 끈 모델을 profile model 칸에서만 막는지 본다.

| 시험 | 지키는 계약 |
| --- | --- |
| `aaa-config` `models::tests` | Claude 별칭이 계열 ID·`[1m]` 꼴을 막고 ID 하나는 그 ID만 막는다. Codex는 slug 그대로. 고치기는 다른 글을 보존하고 빈 표를 지운다. profile은 model을 새로 정할 때만 거절한다 |
| `aaa-agent` `model_catalog::tests` | Codex 목록 파일에서 `/model`에 보이는 모델만, 계정 칸 없이 싣는다 |
| `aaa-host` `settings_admin::models::tests` | 알린 목록·끈 모델·profile model을 한 줄씩 합치고 Claude 계열 ID는 별칭 줄에 묶는다 |
| `aaa-host` `settings_admin::tests::disabled_models_stay_listed_and_block_only_profile_model` | 끈 모델이 목록에 남고, 옛 판은 conflict, 다른 harness 거부, profile 더하기 거절·다른 칸 고치기 허용, 끌 때 쓰는 profile 경고 |
| `aaa-host` `settings_admin::tests::settings_changes_refuse_session_agents_and_reads_refuse_readers` | 모델 바꾸기는 aaa 세션 안 agent가 못 하고 읽기도 보기 전용 기기가 못 한다 |
| 웹 `test/settingsModel.test.mjs` | 1.158 이상 host만 그리고, 「모두 끄기」·「모두 켜기」와 profile 후보 규칙 |
| 웹 E2E `specs/126-model-toggles.mjs` | 실험실 agent의 Codex 목록 → 켜기·끄기·새로고침 유지 → profile 거절 → 모두 끄기·켜기(PC Chromium·휴대폰 WebKit) |

E2E는 그 spec만 `CODEX_HOME`을 실험실 폴더로 돌려 실제 사용자 Codex 폴더를 읽지 않는다.
