# K0 환경값 비노출 시험의 조각 충돌 수리(t-1504)

## 확인한 실패와 한계

`af18a5985ce44330c936a3a9626399371a5aeebf`의 GitHub Actions run `41660213196`,
Ubuntu job `126001107747`의 전체 로그와 Rust 시험 산출물을 확인했다.
`m13-64 secret_k0::env_values_stay_out_of_audit_db_logs_and_export`는
`secret_k0.rs:245`의 최종 비노출 단언에서 실패했다(0 통과·1 실패·89 filtered, 1.36초).
검출은 C의 `조각 82` 한 건이며, `host.db-wal`의 바이트 위치 580243이다.
감사 행·일반 로그·프로젝트 export에서 추가 검출한 기록은 없다. 의심 값 원문은 옮기지 않았다.

CI 산출물은 실행 로그·원장만 보존하며 실패 실험실의 DB/WAL과 입력은 포함하지 않는다.
따라서 당시 바이트의 SQL 필드와 입력 원문을 직접 대조할 수 없다.
아래는 같은 실패 SHA의 제품 코드에 결정적 시험 입력만 넣어 확인한 재현이다.
당시 CI 입력을 복구했다거나 운영 비밀값 노출이 확인됐다는 뜻은 아니다.

## 재현한 원인

K0는 A·C의 모든 8바이트 조각을 host DB/WAL 전체에서 검색한다. B는 기존 K2 범위의
`threads.data` → `launch.env`에 저장되므로 DB 전체의 B 검색만 제외한다.
공용 opaque 토큰은 원본 digest 1바이트를 세 문자로 인코딩한다. 서로 다른 96바이트 토큰도
짧은 8바이트 조각이 같을 수 있다. 전체 토큰의 무작위성은 조각 간 비중복을 보장하지 않는다.
B에 우연히 C의 조각이 들어가면 B의 허용된 DB 기록을 C의 누출로 오인한다.

공용 생성기의 고정 seed `k0-collision-39`와 `k0-collision-345`는 서로 다른 토큰이지만
앞 토큰의 `[82..90]`과 뒤 토큰의 `[67..75]`가 같다. 이 쌍을 C·B로 넣고 원래 host·agent
시험을 실행하면 C의 `조각 82` 한 건으로 실패한다(1.51초). 검색 길이·범위·단언·시한은 그대로다.

실패한 재현 실험실의 WAL을 바이트를 출력하지 않고 분석했다.

- 검출 위치 580234, 141번째 WAL frame, DB page 33.
- `sqlite_master`에서 page 33은 `threads` 테이블의 root page다.
- 해당 위치는 같은 page에 있는 B 전체 값 안의 67번째 바이트이며, 앞에는 `launch`·`K0_B` 키가 있다.
- 현재 `threads` 행은 0개다. 프로젝트 삭제 전 B가 WAL에 남아 검출됐다.
- CI 위치도 기본 4096바이트 page 기준 141번째 frame에 해당한다. CI의 필드까지 직접 확인한 것은 아니다.

같은 단언·라벨·WAL frame의 결정적 재현을 근거로 시험 입력의 조각 충돌을 수리했다.
이 증거로 운영 환경값 비노출 계약 위반이나 배포 되돌림 필요를 확인하지는 않았다.

## 관련 변경의 관계

- `3be37831`은 K0의 기존 무작위 hex 본문을 공용 96바이트 opaque 값으로 옮기고,
  그 본문의 모든 8바이트를 검색하게 했다. A·B·C 사이의 조각 독립성 검사는 없었다.
- owner ID가 없는 `cc4ef5a6`은 보고 첨부의 바이너리 원본 보존을 추가했다.
  K0 환경값 시험의 생성기·DB 제외 규칙·검색 단언은 바꾸지 않았다.
- t-1499의 `05003e99`와 병합 `af18a598`은 작업 파일 원본 해시 검증의 전송 예외와
  자동화 보고 회수를 수리했다. K0 입력·DB 감사 기록 가림은 바꾸지 않았다.
- 따라서 새 전송 예외가 C를 DB에 흘렸다고 단정할 근거는 없다. 이 수리는 시험 파일에 한정했다.

## 수리와 회귀

K0의 A·B·C만 조각이 겹치지 않는 고정 opaque 값으로 바꿨다. 공용 생성기와 다른 인증 시험의
무작위 입력은 유지했다. C의 escape 접두사도 유지한다.
제품의 가림·저장·export·원본 해시 전송 경계는 수정하지 않았다.
기존 검색어(원문·모든 8바이트 조각·base64 정렬별·URL·JSON), 검사 대상과 비노출 단언도 유지한다.

새 `k0_db_scan_distinguishes_k2_env_and_still_detects_a_pane_fragment`는 실제 SQLite WAL에서
다음을 고정 sleep 없이 확인한다.

1. 서로 다른 opaque 입력의 C 조각 82가 B 기록에서 검출되는 충돌을 재현한다.
2. 수리한 B가 실제 DB/WAL에 있는지 검사하고, A·C는 여기서 검출되지 않아야 한다.
3. A·C 각각의 8바이트만 감사 기록에 심으면 전체 원문 없이도 검출돼야 한다.

결함 심기로 K0의 고정 입력을 위 충돌 쌍으로 바꾸면 새 회귀는
`A·C는 K2의 B와 구분한다` 단언에서 실패했다(0.07초). 수리 입력을 복원한 뒤 관련 시험을 확인했다.

## 개발 검증

검증은 `aaa verify --`로 실행했다. 아래는 owner별 재현 명령이다. 실패 재현과 결함 심기는 예상된 실패로 따로 기록한다.

```sh
cargo test --profile verify --locked -p aaa --test m13 -- \
  secret_k0:: accounts::credential_canary_distinguishes_hex_metadata_and_detects_every_eight_byte_fragment
cargo test --profile verify --locked -p aaa-host --lib link::work_file_tests::
cargo test --profile verify --locked -p aaa-host --lib \
  db::audit_env_tests::open_scrubs_old_pane_run_env_values_and_vacuums -- --exact
cargo clippy --profile verify --locked -p aaa --test m13 -- -D warnings
cargo fmt --all -- --check
python3 scripts/check_fixture_secrets.py
bash scripts/check-public.sh
```

- m13 관련 owner 5개 통과(3.46초): K0 전체 4개와 모든 8바이트 조각 검출 owner 1개.
  원격 보고 원본 해시·바이너리 원본·표시 사본 가림도 기존 실제 TCP+TLS 시험에서 확인했다.
- `aaa-host::link::work_file_tests` 4개 통과(0.79초): 신형·구형 원본 hash 전송,
  표시용 가림 유지, 변조 거부, 취소된 요청의 전송 예외 해제.
- 감사 DB의 옛 env 제거·VACUUM owner 1개 통과(0.07초).
- m13 strict Clippy(`-D warnings`), fmt, 공용 인증 fixture 검사와 공개 저장소 검사 통과.

검증은 macOS에서 수행했다. Ubuntu에서 수리 후 실행한 결과나 GitHub CI 회복은 아직 없다.
관련 owner 통과를 실패한 GitHub run의 회복으로 기록하지 않는다.
병합·push는 하지 않았으며, 착지 창에서 최종 통합 SHA의 전체 gate와 OS cross를 수행해야 한다.
