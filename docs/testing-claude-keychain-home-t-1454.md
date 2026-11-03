# macOS Claude 로그인 HOME 격리 결함(t-1454)

## 확인한 사실과 한계

배포 뒤 조회 로그인은 공식 CLI에서 종료 코드 1로 실패했다. 비밀 없는 분류는 첫 시도
`credential_store_failed`, 다음 시도 `unknown`이고 둘 다 HTTP 상태가 기록되지 않았다. 완료 단계는
`cli/cli_failed`이며 aaa의 인증 읽기·신원 확인·조회 금고 저장에는 도달하지 않았다.

기존 macOS 로그인 자식은 Claude 설정뿐 아니라 HOME도 임시 폴더로 바꿨다. 조사 시점의 읽기 전용 OS API 비교에서
원래 HOME은 `SecKeychainCopyDefault=0`, 잠금 해제·읽기·쓰기 가능 상태였고, 임시 HOME은
`errSecNoDefaultKeychain(-25307)`이었다. `security show-keychain-info`도 각각 0과 37이었다.
존재하지 않는 임의 시험 항목 조회는 양쪽 모두 44였다. 항목 없음과 기본 저장소 발견 실패를 구별한다.

설치된 공식 CLI 2.1.316의 저장소 구현은 Keychain을 우선 사용하고, CLAUDE_CONFIG_DIR의 해시로
별도 서비스 이름을 만든다. 저장소 읽기·쓰기·기존 인증 제거 거부를 조건부 Keychain 안내로 표시할 수 있으므로,
그 안내나 `credential_store_failed`만으로 당시 Keychain이 잠겼다거나 특정 OS 오류였다고 단정할 수 없다.
임시 HOME의 기본 Keychain 발견 결함은 재현했지만, 당시 첫 실패의 직접 원인과 두 번째 unknown의 상세 원인은 미확정이다.
가짜 Keychain을 쓴 공식 CLI의 격리 로그아웃은 양쪽 HOME에서 성공했다. 이 결과를 실제 로그인 성공으로 간주하지 않는다.

## 수리 범위

- 로컬 터미널과 웹 원격의 macOS Claude 로그인에서 OS HOME을 보존한다. 유효한 절대 HOME이 없으면 시작을 거절한다.
- CLAUDE_CONFIG_DIR·USERPROFILE·XDG 경로·cwd는 로그인 전용 임시 폴더를 유지한다. 환경 전체를 물려받지 않는다.
- 인증 읽기와 정리는 해당 임시 폴더 및 그 해시의 Keychain 항목만 대상으로 한다. 기본 Claude 인증 항목을 쓰지 않는다.
- Codex와 다른 OS의 HOME 격리, 실행 인증·조회 인증 분리, 소유자·취소·메일 확인 계약은 유지한다.
- API·프로토콜·plan 절·E2E 번호는 늘리지 않는다. 실제 로그인·운영 Keychain 변경·착지·배포는 수행하지 않는다.

## 개발 검증

관련 owner를 `aaa verify`와 nice 15로 검사한다. 전체 통합 gate·OS cross·production은 별도 착지 창에서 수행한다.

새 가짜 CLI 단언의 첫 실행은 macOS 임시 경로의 논리 경로와 물리 경로를 문자열로 비교해 6개가 실패했다.
원문은 작업 전용 target에 보존했다. 같은 폴더가 `/var`와 `/private/var`로 표현되는 것을 재현한 뒤,
cwd만 물리 경로로 대조하도록 시험을 고쳤다. HOME·설정·인증 격리 단언이나 기존 시한은 완화하지 않았다.

| 검사 | 결과 |
| --- | --- |
| host 로그인·소유자·취소·조회 신원·실패 보존 | 11개 통과 |
| host usage·소비자 | 17개 통과 |
| 실제 CLI→host 가짜 로그인 | 5개 통과 |
| host·aaa all-targets strict Clippy | 통과 |
| fmt·공개 검사 | 통과 |

가짜 CLI는 실제 PTY 자식의 HOME·cwd·Claude/Codex 설정·XDG 경계와 민감 환경 제거를 단언한다.
조회 성공 뒤 CLI 실패에도 실행 인증·메타데이터와 기존 암호화 조회 인증을 보존하며,
응답·로그·상태·프로젝트에 가짜 인증의 모든 8바이트 조각이 새지 않는 기존 검사를 유지했다.
웹 코드·API·요청 흐름은 바꾸지 않아 웹 유닛/E2E를 반복하지 않았다.
