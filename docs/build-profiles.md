# 배포와 내부 빌드 profile

2026-10-25 사용자 결정에 따라 이름·역할을 세 저장소와 PT에서 맞춘다. 실제 설정은 루트 Cargo.toml이 정본이다.

| profile | 역할 | 최적화 | 줄 정보 | LTO / CGU / incremental |
| --- | --- | --- | --- | --- |
| release | 사용자·기기에 전달할 binary와 배포 성능 검증 | 최종 실행·크기 우선 | line-tables-only | thin / 1 / false |
| bench | Cargo 기본 성능 시험(release 상속) | release 상속 | line-tables-only | thin / 1 / false |
| dev | 직접 개발·check·clippy | 0 | full(기존 Cargo 기본 유지) | false / Cargo 자동 / true |
| test | 기본 unit 및 자세한 실패 재현 | 0 + 필요한 package 예외 | line-tables-only | off / 16 / false |
| verify | 내부 CLI·일상 unit 검증·실패 스택 재현 | AAA만 0, 그 외 1 + 필요한 package 예외 | line-tables-only | off / 16 / false |
| e2e | headless 자동화의 빌드+실행 합계 최소화 | 0 | 0 | off / 16 / false |
| dev-fast | 기존 명령 호환 별칭 | verify 상속 | verify 상속 | verify 상속 |
| debugging | 명시적인 변수·타입 디버거 조사 | dev 상속 | full | dev 상속 |

빌드 스크립트·proc macro의 Cargo 기본 build-override는 유지한다. e2e도 debug assertion·overflow check·panic unwind를 유지한다. debug=0에서도 panic 메시지의 위치는 남지만 스택 각 단계의 파일·줄은 보장하지 않는다. 필요하면 같은 feature·시나리오를 verify/test로 재현한다. 일상 unit·내부 자동화에 타입·변수 정보를 추가하지 않는다. 직접 개발의 dev는 기존 full을 유지한다.

반복 자동화는 기존 kache를 쓰고 incremental=false/CGU16으로 작업 사본 사이의 재사용을 우선한다. 직접 개발은 incremental=true와 Cargo 기본 CGU(증분 workspace 256·비증분 의존성 16)로 같은 target의 편집 후 재컴파일을 우선한다. e2e의 opt-level은 headless 실행을 지나치게 늦추지 않는 1을 기본으로 두며 PNG 처리와 기존 simulation·JSON 경로의 package 예외만 유지한다. 프로필 숫자를 모든 조합으로 탐색했다는 뜻은 아니다. compiler·시나리오가 바뀌면 빌드와 실행을 합쳐 다시 비교한다.

배포 release는 line tables·unwind·symbols를 보존한다. 실제 배포 진입점이 있는 저장소에서는 전달 binary와 matching dSYM/PDB를 자동으로 보관한다. 내부 e2e·도구 설치에는 배포 기호 보관을 실행하지 않는다.

[Cargo profile 공식 문서](https://doc.rust-lang.org/cargo/reference/profiles.html)는 상속·line tables·LTO off·CGU·incremental의 근거다. 외부 구현 코드를 복사하지 않았다.

## 실제 소비 경로

| 실제 경로 | 선택 |
| --- | --- |
| scripts/ci-local.sh build·웹 E2E / web/e2e 기본 binary | build는 e2e 실행 파일 + verify unit 사전 컴파일; 웹 E2E는 target/e2e/aaa, test·clippy는 verify |
| ci-local unit(rust_tests.py)·doctest·clippy | verify |
| .github/workflows/ci.yml | ci-local에 위임: 웹 E2E/build는 e2e, unit/clippy는 verify |
| Windows ci-local 선검사 | release check만 실행(LLVM·LTO·실행 파일 생성 없음); release 전용 cfg 누락 검출 유지 |
| Windows SYSTEM SSH owner | build의 target/e2e/aaa.exe를 자원 생성 전에 확인해 절대 경로로 전달; cargo test는 verify |
| macOS 실제 LaunchAgent·root SSH owner | cargo test는 verify; 시험용 aaa는 해당 profile의 Cargo 산출물 |
| scripts/deploy-build.sh / .github/workflows/release.yml | release; 기존 실제 배포 설정·기호 보관 유지 |
| scripts/windows-production.py | 최종 SHA+salt의 독립 release 한 번, 같은 exe/PDB·native 영수증 보존·전달 전후 검증 |
| p0/build-app.sh 서명·업그레이드 모의 | e2e; 실제 배포 성능을 주장하지 않는 내부 후보 |

AAA release만 기존 opt-level="s"를 유지한다. 이 프로그램은 배포·업데이트 시 실행 파일 bytes를 전송하므로 검증한 크기 최적화를 보존한다. 다른 저장소는 release opt-level=3이다. 배포용의 역할·thin LTO·CGU1·줄 정보는 같다. SHA/base64/JSON 처리의 기존 package 최적화는 유지한다.

AAA verify는 opt-level=0이고 e2e는 0이다. 대표 unit 빌드 비용과 IO 중심 전체 자동화의 빌드+실행 비교로 골랐다. SHA/base64/JSON package 예외와 verify/test의 최소 줄 정보는 유지한다.

추가 실측은 사용자 지시로 중단했다. AAA opt0의 전체 E2E 전후 시간은 미측정이며 표준 ci-local의 기능 검증과 구분한다.
