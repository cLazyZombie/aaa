# 계정 메일 등록·수정 검증(t-1477)

## 보존하는 계약

- 새 계정 로그인·가져오기에서 메일을 한 번 받아 host DB에 저장한다. 공식 CLI가 메일을 남기지 않는 Claude 등록도 포함한다.
- 저장한 메일을 조회 로그인·재로그인에 사용하며 웹 설정과 세션 밖 CLI에서 수정할 수 있다.
- 등록 메일이 없던 계정은 조회 로그인에서 실제 신원을 확인한 뒤 메일을 기억한다. 실패·취소는 기억하지 않는다.
- 메일 수정은 인증 금고의 세대·인증 날짜를 바꾸지 않는다. DB를 새로 열어도 보존된다.
- 조작 기기와 세션 밖 CLI만 수정한다. 보기 전용·세션 안 agent는 거부하고 메일도 가린다. 감사에는 메일을 쓰지 않는다.
- 진행 중 로그인·교체 확인과 메일 수정은 섞지 않는다. 알려진 다른 신원은 기존 교체 확인, 미확인 기존 실행 신원도 명시적 확인을 유지한다.
- 메일 저장 실패는 실패 응답으로 남긴다. 공식 사용량 조회의 신원 일치 검사·실행 인증과의 분리는 유지한다.

## 필요한 시험과 owner

| 시험 | 필요한 이유 |
| --- | --- |
| `account_meta::tests::registered_email_is_editable_durable_and_private_without_changing_credentials` | 등록·수정·DB 재열기·권한·검증 오류·실제 DB 쓰기 실패·감사 가림과 인증 불변을 실제 host 요청으로 확인 |
| `account_meta::tests::automatic_refresh_preserves_the_edited_email` | 자동 인증 갱신이 사용자가 고친 메일을 원래 값으로 되돌리지 않도록 보호 |
| `account_login::remote::tests::process::registration_email_survives_missing_cli_identity_and_is_used_on_relogin` | 실제 가짜 CLI 자식에서 메일 없는 setup-token 등록, 저장 메일 재사용·수정 충돌·취소를 확인 |
| `account_login::remote::tests::process::unknown_execution_identity_remembers_verified_email_for_next_login` | 기존 미등록 메일은 다른 신원일 때 저장하지 않고 일치 성공 때만 다음 로그인에 재사용 |
| m13 `host_usage_login_separates_credentials_without_changing_execution` | CLI 조회 로그인 뒤 메일 재입력 없이 다시 시작하고 CLI 수정 뒤에도 실행 인증·시각을 보존 |
| 웹 E2E 118 | PC·휴대폰에서 새 등록 메일 전달·수정값 보존·수정값으로 조회 로그인·재입력 생략을 확인 |
| 웹 E2E 124 | 메일 기능을 더한 계정 화면에서도 기존 사용량 창·경과 표시를 보존 |

기존 m13의 목록 동일성 단언은 새로 확인해 보관한 메일만 더해 비교한다. 실행 인증 판·인증 날짜·상태 단언은 유지한다.
기존 미등록 메일 owner도 성공 뒤 메일을 기억하는 새 계약을 확인하도록 바꾸고 신원 거부·실행 인증 세대 보존은 유지했다.
고정 sleep·재시도·시한 연장·시험 삭제는 추가하지 않았다.

## 확인 기록

개발 owner 검사는 다음과 같이 통과했다. 전체 착지 gate·양 OS cross·Windows production은 착지 창에서 별도로 수행한다.

- 계정 관련 host 51개 통과. 이후 저장 실패·자동 갱신 보존 단언을 보강한 계정 메타데이터 8개도 통과했다.
- 사용량 관련 host 17개 통과.
- CLI m13의 host 로그인·재로그인·조회 로그인 3개, 의존 경계 2개, TypeScript 생성 계약 2개 통과.
- 관련 crate(aaa-host·aaa-proto·aaa)의 all-targets strict Clippy 통과.
- 웹 타입·번들·웹 유닛 685개, 공개 검사 통과.
- 웹 E2E 118(원격 인증·메일 등록/수정 각 4조합)·124(사용량 4조합) 통과.

초기 컴파일에서 기대 메일 대입을 다른 메서드에도 삽입한 위치 오류를 고쳤다.
웹 fixture 검사는 메일 예시를 인증값 직접 문자열로 오인해 멈췄으며, 예시 메일을 변수로 분리했다.
새 휴대폰 E2E는 새로고침 뒤 복원된 계정 상세에서 없는 탭을 다시 찾다가 실패했다. 복원된 계정 화면 준비를 기다리도록 고쳤다.
추가한 로그인 사슬이 휴대폰에서 5초를 넘어 메일 계약을 독립 fixture로 분리했다. 기존 인증·복원·실패 단언은 유지했다.
최종 E2E 118의 개별 단계는 1.19~4.10초였고 메일 등록·수정 단계는 1.19~2.81초였다.
실패한 명령을 그대로 재시도하거나 시한·기대값을 완화하지 않았다.

## 참고한 공개 구현

- [Git의 한 번 등록하는 user.email 설정](https://git-scm.com/book/en/v2/Getting-Started-First-Time-Git-Setup): 저장한 신원을 계속 쓰고 필요할 때 수정하는 흐름을 참고했다. [라이선스 GPL v2](https://github.com/git/git/blob/master/COPYING).
- [npm의 웹·CLI 프로필 수정](https://docs.npmjs.com/managing-your-profile-settings/): 웹 설정과 CLI 양쪽에서 메일을 고치는 흐름을 참고했다. [npm CLI 라이선스 Artistic 2.0](https://github.com/npm/cli/blob/latest/LICENSE).

외부 코드를 복사하거나 런타임 의존성을 추가하지 않았다. aaa는 기존 계정 메타데이터와 권한 검사를 확장한다.
