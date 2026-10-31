# 접속 기기의 계정 재로그인 검증

2026-10-31, 개발 후보의 관련 owner 검사다. 최종 통합 SHA의 착지 gate와 운영 계정 승인을 대신하지 않는다.

## 바뀐 계약과 결과

| 검사 | 결과 | 지키는 계약 |
| --- | --- | --- |
| host `account_` owner | 41개 통과 | 계정 신원·권한·이름 이전·사용량·기존 인증 유지 |
| 새 `account_login::remote::tests` | 5개 통과, 합계 0.83초 | 공식 수동 링크만 허용, 요청 기기 결속, 1회 코드 제출, 만료 차단, 자식 회수·임시 홈 삭제, 신원 미확인 교체 확인, Codex loginId 매칭·오류 비노출 |
| `aaa-proto` | 50개 통과 | 요청·응답 직렬화와 생성 TypeScript 일치 |
| `m13 accounts::` | 17개 통과 | 기존 로그인·취소·메일 불일치·계정 실행·중앙 갱신·기록 가림 |
| `aaa --test deps` | 2개 통과 | 웹→proto, host/agent/session 의존 경계 |
| strict Clippy | 통과 | aaa-host·aaa-proto·aaa의 모든 target |
| 웹 타입·번들·유닛 | 670개 통과 | 상태 필터와 기존 웹 계약 |
| 새 E2E 118 | 4개 조합 통과 | Chromium·WebKit, 1280px·390px에서 메뉴/설정 진입·복원·취소·코드 제출·완료 후 표시 제거 |
| fmt·diff·공개 검사 | 통과 | 형식·공개 저장소 규칙 |

새 E2E의 각 조합은 1.01~2.67초다. 고정 sleep·재시도·상한 연장은 추가하지 않았다.
화면은 제공자 응답 fixture를 사용하고, host 시험은 격리된 가짜 CLI 자식을 실제 pipe/PTY로 실행한다.
계정 권한 시험은 다른 조작 기기·보기 전용 기기의 조회·상태·코드 제출·취소를 거부하고
다른 기기의 교체 확인도 거부하는지 검사한다. 새 시험은 이 인증 경계를 지키는 데 필요하다.

```sh
cargo test --profile verify --locked -p aaa-host --lib account_
cargo test --profile verify --locked -p aaa-host --lib account_login::remote::tests
cargo test --profile verify --locked -p aaa-proto
cargo test --profile verify --locked -p aaa --test m13 accounts::
cargo test --profile verify --locked -p aaa --test deps
cargo clippy --profile verify --locked -p aaa-host -p aaa-proto -p aaa --all-targets -- -D warnings
bash scripts/ci-local.sh --only web,e2e,public --spec 118
```

실행은 기존 kache와 worktree 자체 target, `nice -n 15`를 썼다. Cargo 작업 수는 제한하지 않았다.
Playwright 1.64.1 패키지만 작업 폴더에 두고 기존 공용 Chromium·WebKit을 사용했다.
웹 빌드 뒤 관련 E2E는 현재 자산을 내장한 실행 파일 사본으로 검사했다.

## 발견과 수리

- 실제 Claude 2.1.314의 주소는 `claude.com/cai/oauth/authorize`이며 OSC 8 링크를 포함한다.
  완전한 링크 대상의 경계를 읽고 공식 host·경로·manual callback·PKCE·중복 query를 검사하도록 고쳤다.
  마지막 문자가 다중 바이트 공백이어도 바이트 경계를 자르지 않도록 보완하고 같은 회귀 시험에 포함했다.
- 새 응답으로 빠진 CLI match arm을 채웠다. 새 정리 코드의 Clippy 지적은 `inspect_err`로 고쳤다.
- 새 시험의 코드 fixture를 공통 token 생성기로 바꾸고, E2E 단계를 기존 수집기의 정적 이름 규칙에 맞췄다.
- 공개 검사가 URL userinfo 시험값을 이메일로 판정하여 예시 도메인을 `example.com`으로 바꿨다.
- 닫힌 휴대폰 메뉴는 접근성 역할 검색에서 제외되므로 메뉴를 열기 전 선택자만 숨긴 항목을 포함하도록 했다.
- WebKit browser type과 실행된 browser를 혼동한 시험을 고쳤다. 이후 WebKit service worker가
  Playwright route를 우회하여 실제 빈 계정 목록을 읽는 것을 네트워크 진단으로 확인했다.
  제공자 fixture를 사용하는 이 시험 context에서만 service worker를 막았다. 제품 worker는 바꾸지 않았다.

## 실제 제공자와 미검증 범위

설치된 Codex 0.165.0의 기본 app-server schema에서 기기 코드 요청·응답 계약을 확인했다.
Claude 2.1.314 `setup-token`을 빈 HOME/설정·브라우저 억제 helper로 시작하여 수동 URL과 코드 입력 prompt가
실제로 나오는 것을 확인했다. 승인·코드 제출 없이 해당 자식 PID를 종료하고 임시 자료를 지웠다.
실제 사용자 계정 로그인·제공자 승인 후 저장까지의 실계정 시험은 하지 않았다.

Claude 원격 경로는 POSIX host만 제공한다. Windows host의 Claude 원격 로그인은 명시적으로 거부한다.
Windows 실행·최종 통합 gate·배포는 이번 개발 검사의 범위가 아니다. 기존 CLI 로그인 경로는 유지한다.

착지 통합에서 main의 선행 예약 1.150과 E2E 116·117을 보존하고, 이 기능을 FRONT_API 1.151·E2E 118로 옮겼다.
agent wire 변경은 없어 AGENT_LINK 1.11을 유지한다. 개발 검사는 위 후보에서 수행했으며 최종 SHA의 착지 결과는 별도 보고에 보존한다.
