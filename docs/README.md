# 문서 목록

aaa 문서를 쓰는 사람에 따라 세 묶음으로 나눈다. 처음이면 [README](../README.md)의 빠른 시작부터 본다.

## 쓰기

| 문서 | 내용 |
| --- | --- |
| [사용법](usage.md) | 웹 화면·단축키·자주 쓰는 CLI |
| [자동화](automations.md) | 일정·사건 조건, 승인, 실행 기록 |
| [보고서 운영](library.md) | 결과물·보관·공유 링크·웹 보고서 sandbox |
| [머신별 workspace](workspace-access.md) | 머신마다 자동 배정할 저장소·작업 폴더 |
| [머신 배정](machine-assignment.md) | 독점 예약·비중 |
| [짧은 번호](short-ids.md) · [할 일 ID](task-ids.md) | `t-0001`·`d-0001` 같은 번호 |
| [작업별 계정 인증](accounts.md) | host에서 로그인한 계정을 작업에 배정 |
| [이미지 생성·편집](images.md) | host 계정으로 그림 만들기 |
| [GitHub 테마](github-themes.md) · [추가 앱 테마](theme-catalog.md) · [터미널 색 테마](terminal-themes.md) | 화면 색 |

## 설치·운영

| 문서 | 내용 |
| --- | --- |
| [host 설치](host-setup.md) | macOS host: 실행 파일·launcher·설정·서비스·웹 |
| [머신 설치](install.md) | 한 줄 설치·setup·수동 다운로드·갱신·제거 |
| [macOS·Linux setup](unix-setup.md) · [Windows setup](windows-setup.md) | OS별 준비·서비스·로그 |
| [Windows 배포와 복구](windows-rollout.md) | Windows agent 갱신 |
| [host DB 백업](host-db-backup.md) | 암호화 백업·복원 |

## 개발

내부 설계·운영 기록·검증 기록이다. 쓰는 데에는 필요하지 않다.

| 문서 | 내용 |
| --- | --- |
| [CONTRIBUTING](../CONTRIBUTING.md) | 소스 빌드·검사·README 그림 |
| [구조 지도](architecture.md) · [기획](plan.md) | crate 역할과 설계 결정 |
| [개발 검사와 착지 검증](verification.md) · [빌드 profile](build-profiles.md) | gate·profile |
| [시험 쓰는 법](testing-rules.md) · [검증 기록](testing.md) | 유닛·통합·웹 E2E 규칙과 기능별 검증 |
| [운영 설치 점검표](install-runbook.md) · [P0 절차서](p0-runbook.md) · [옮기기](migration.md) | 운영 이관·launcher 측정 기록 |
| [설치 계약](install-contract.md) · [Pages 게시](install-pages.md) · [Windows 한 줄 설치](windows-bootstrap.md) | 설치기·입구 설계 |
| [README 매체](readme-media.md) | README 그림 촬영 |
| [질문 카드 문법](questions.md) · [계정 Codex hook](account-codex-hooks.md) | thread가 쓰는 질문 형식·계정 실행 hook |
| [조사 근거](source-analysis.md) · [삭제된 TUI](removed-tui.md) | 참고 도구 분석·삭제 기록 |

그 밖의 설계·진단 기록(`testing-*.md` 등)은 이 폴더에 있다.
