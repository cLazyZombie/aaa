# 원격 머신 추가의 디스크 부족 진단 검증(t-1288)

## 변경

- 기본 여유 기준 20GB(1024³ bytes)는 유지했다. 부족하면 필요량·남은 용량·검사한 작업 root를 표시한다.
- `InstallError.disk_space`는 선택적 관측값이다. 기존 JSON의 필드는 유지하고 옛 결과도 읽는다.
- SSH 계획 입구는 현지 검사 실패를 충돌로 감싸지 않고 원래 결과·종료 코드를 돌려준다.
- host는 디스크 부족 결과의 상태·종료 코드·오류 코드·용량·절대 경로를 검사하고 문구를 다시 만든다.
  디스크 부족 결과를 표시할 때 다른 원격 message·details·stderr는 공개하지 않는다. 웹은 host가 정한 오류를 그대로 그린다.
- 소수 둘째 자리에서 내림한다. 20GB 바로 아래는 19.99GB로 표시한다.

## 참고 구현

- [rustup 설치기의 사전 검사 맥락](https://github.com/rust-lang/rustup/blob/master/src/cli/self_update.rs)
  과 [MIT 라이선스](https://github.com/rust-lang/rustup/blob/master/LICENSE-MIT)를 확인했다.
- [uv의 CLI 오류·종료 처리](https://github.com/astral-sh/uv/blob/main/crates/uv/src/lib.rs)
  와 [MIT 라이선스](https://github.com/astral-sh/uv/blob/main/LICENSE-MIT)를 확인했다.
- 종료 코드만으로 오류 원인을 대체하지 않고 진단 맥락을 보존하는 원칙을 따랐다. 코드는 복사하지 않았다.

## 회귀 시험이 필요한 이유

| 시험 | 지키는 계약 |
| --- | --- |
| install `insufficient_work_space_reports_observed_capacity_and_path_without_writes` | 실제 저장소 조회 후 일반 CLI·JSON·NDJSON의 원인·용량·경로, 거부 시 파일 변경 없음 |
| install `wizard_reports_disk_capacity_and_path_before_asking_to_change_choices` | 실제 PTY에서 선택 변경 질문 전에 디스크 원인을 표시 |
| binary `verified_probe_preserves_disk_shortage_result_and_exit_without_writes` | OS SSH 출처 증명만 대체하고 실제 SHA·입력·현지 검사·직렬화를 지나 원인과 exit 2 보존 |
| host `failed_probe_reports_only_validated_disk_space_observations` | 숫자·경로 표시, 20GB 직전 내림, 임의 원문 비노출, 오래된·틀린 결과 처리 |
| host `a_remote_disk_shortage_keeps_capacity_and_path_and_cleans_up` | 실험실 SSH의 실패 결과가 공개 작업 상태에 남고 원격 임시 폴더·계획은 정리됨 |
| 웹 E72 「디스크 부족: 필요량·남은 용량·검사 경로 표시」 | 실제 화면에 host 응답의 필요 20GB·남은 7.50GB·작업 경로 표시 |

실제 디스크를 채우거나 운영 머신에 설치하지 않는다. 현지 부족 재현은 시험의 명시 기준을 `u32::MAX`로 잡는다.
기본 제품 기준은 바꾸지 않는다. 기다림은 PTY 물음·SSH 단계·DOM 상태 사건으로 끝낸다.

## 검증 기록

- 최초 일반 CLI 시험은 TTY 없는 실행에서 `--non-interactive`를 빠뜨려 설치의 기존 입력 검증에 거부됐다.
  명시 비대화형 실행으로 시험 입력을 바로잡았다. 제품 기대값·시한·재시도 정책은 바꾸지 않았다.
- CLI 세 출력 0.45초, PTY 0.19초, 검증 SSH 입구 0.34초에 통과했다.
- host 시험을 작성할 때 `InstallProposal`에 Debug가 없는데 `unwrap_err`를 써 컴파일이 거부됐다.
  기존 시험과 같이 `err().unwrap()`으로 오류 값을 읽도록 수정했다.
- 최초 host 검증의 SSH 전송 실패 fixture는 기존 `why`가 임의 stderr 마지막 줄을 표시하는 계약을
  잘못 가정했다. 전송 실패는 실제 연결 거부 fixture로 구분하고, 디스크 결과의 임의 원문 비노출
  단언은 그대로 유지했다. 기존 SSH 전송 오류 처리는 바꾸지 않았다.
- host의 결과 해석 2개(기존 비노출 회귀 포함) 0.00초, 실제 SSH 실패·정리 1.30초에 통과했다.
- 웹 빌드와 웹 유닛 655개가 통과했다.
- E72 전체 13.3초에 통과했다. 새 디스크 표시 단계는 358ms이며 저장한 화면도 확인했다.
- 전체 `ci-local.sh --only web,fmt,build,clippy,test,e2e,linux-clippy,public`가 703초에 모두 통과했다.
  web 8초, fmt 2초, build 70초, strict clippy 128초, Rust/Python test 397초,
  웹 E2E 580초, Linux 교차 clippy 40초, 공개 검사 2초였다.
- Rust 검증 33개 작업이 모두 exit 0이었다. 전체 병렬 실행에서도 새 CLI·PTY·SSH 입구·host 회귀가 통과했다.
- 웹 E2E는 122개 파일·134개 작업이 모두 통과했다. 새 디스크 표시 단계는 전체 실행에서 351ms였다.
- 병렬 구간은 10코어에서 1분 load 약 188까지 올라갔다. 실패나 재시도 없이 원래 시한으로 마쳤다.
- 검증 메트릭의 별도 출처 보관은 미커밋 작업 및 실행 중 검증 문서 갱신 때문에 출처 미확인으로 거부됐다.
  이는 시험 실패가 아니며 단계별 원본 로그·결과는 작업 보고의 library에 보존했다.
- 브라우저는 저장소 요구 Playwright 1.64.1과 기존 공용 Chromium·WebKit 캐시를 썼다.
  전역 npm·브라우저 설치·시스템 설정은 바꾸지 않았다.

운영 머신 설치·배포·병합·push는 하지 않았다. 착지 창에서 통합 검증과 병합을 이어 한다.

## Windows canonical 작업 경로의 후속 수리(t-1288, d-0523)

착지 전 추가 검증에서 이번 변경의 제품 결함을 발견했다. 원격 설치기는
`work_root::check` → `std::fs::canonicalize`로 해석한 경로를 결과에 담는다.
Windows의 확장 길이 접두사가 붙은 드라이브 경로를 host의 일반 입력 경로 검사기가
거부하면서 필요량·남은 용량·경로가 다시 빠졌다.

- 보존한 일회 검증을 정식 `remote_disk_shortage_preserves_capacity_for_windows_canonical_work_roots`
  회귀로 먼저 추가했다. 수리 전 같은 「디스크 여유 부족(work_space_insufficient, exit 2)」만 반환해
  필요 20GB·남은 7.00GB 단언이 0.00초에 실패했다.
- 수리는 원격 오류의 관측 경로만 검사할 때 확장 길이 접두사를 벗겨 기존 Windows 드라이브 경로
  규칙을 적용한다. 표시와 결과 필드의 경로 원문은 보존한다. 파일을 다시 열거나 경로를 변환하지 않는다.
- POSIX·일반 드라이브 경로의 규칙, 기본 20GB, 종료 코드와 설정 입력 정책은 유지한다.
  길이·제어 문자 검사는 기존 두 경로 검사기에 맡겨 canonical 접두사가 길이 상한을 줄이지 않게 한다.
- 새 회귀는 일반·canonical 드라이브, 한글 경로와 4096바이트 드라이브 문자열 경계에서
  필요량·남은 용량·경로가 모두 보존되는지 확인한다. 필요한 이유는 실제 원격 결과 형식을
  일반 설정 입력의 경로 형식과 혼동한 제품 결함을 다시 만들지 않아야 하기 때문이다.
- 기존 오류 경계 시험에 UNC·기기 namespace·GLOBALROOT·상위 탈출·상대 드라이브·ADS·제어 문자
  거부를 더했다. 설정 시험은 canonical 형식을 새 입력으로 허용하지 않는 기존 정책을 확인한다.
- [Rust canonicalize 문서](https://doc.rust-lang.org/std/fs/fn.canonicalize.html)와
  [dunce simplified 문서](https://docs.rs/dunce/1.0.5/dunce/fn.simplified.html)의 경로 표현 차이를 확인했다.
  dunce는 비Windows 호스트에서는 변환하지 않으므로 다른 OS 원격 결과를 읽는 이 경계에는 넣지 않았다.
  외부 코드나 의존성은 추가하지 않고 기존 경로 검사기를 재사용했다.

수리 후 macOS의 새 회귀 1개·오류 경계 2개·설정 정책 1개가 모두 0.00초에 통과했다.
실제 Windows x86_64에서도 새 회귀·오류 경계·설정 정책 3개를 필터로 각각 한 번 실행해
모두 exit 0, 각 0.00초에 통과했다. 전달한 시험 파일의 SHA-256을 대조했고,
서비스·예약 작업·Run 키·사용자 PATH·영구 방화벽 규칙의 전후 fingerprint가 모두 일치했다.
자기 시험 폴더를 정리했으며 설치·서비스 기동·배포를 하지 않았다.

준비 중 첫 Cargo 메타데이터 조회는 target 인자가 없어 호스트 파일을 가리켰다.
파일을 전달하기 전에 이 차이를 확인하고 Windows target을 명시해 PE 시험 파일을 빌드했다.
처음 SSH 명령은 cmd의 명령줄 길이 제한으로 시험 실행 전에 거부됐다.
검토한 스크립트 파일을 전달하고 짧은 호출로 실행하게 전달 방식을 고쳤다.
실제 시험을 재시도하거나 시한·기대값을 바꾼 것은 아니다. 두 준비 기록도 보고 library에 보존했다.

macOS host strict clippy(36.86초)와 Windows host/all-targets strict clippy(21.41초),
fmt·diff 검사가 통과했다. 전체 통합 검증·production·push·설치기 전달은 다음 착지 창에서
그때의 최신 원격 main 위에 새로 통합해 수행한다. 미push 통합본 `0f1879a2`는 사용하지 않는다.
