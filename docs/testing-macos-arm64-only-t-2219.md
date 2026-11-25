# macOS 지원 범위 변경과 Apple Silicon 로그 회전 확인

macOS 지원 대상은 Apple Silicon(arm64)이다. GitHub의 Rust job과 웹 E2E 세 shard를 `macos-15`로 옮겼다. 릴리스는 macOS arm64, Linux arm64·x86_64, Windows x86_64 실행 파일을 게시한다. macOS launcher와 dSYM도 arm64용이 게시된다. [GitHub 호스팅 러너 표](https://docs.github.com/en/actions/how-tos/write-workflows/choose-where-workflows-run/choose-the-runner-for-a-job#standard-github-hosted-runners-for-public-repositories)

## 기준 로그 회전 시험

CI에서 실패한 `9a9b2e7c`의 코드를 Apple Silicon Mac에서 Rust 1.100.0의 verify profile로 빌드했다. `file`로 시험 실행 파일과 CLI가 모두 Mach-O arm64임을 확인했다. Cargo로 처음 한 번 실행한 뒤 같은 시험 파일을 새 프로세스로 20번 실행했다. 각 실행은 격리 경로에서 실제 host·agent를 띄우고 기존 파일 보존, 활성 로그 이름과 시작·종료 기록, 둘의 연결, 정상 종료 상태를 확인했다. 처음 1회와 독립 실행 20회가 모두 통과했다. 독립 실행은 0.64~0.67초였다.

Intel 러너의 SIGTERM은 이 환경에서 재현되지 않았다. 로그 회전 코드와 시험은 그대로 사용했다.

## 지원 범위 확인

- 새 manifest의 실행 파일 4개·launcher 1개·심볼 2개를 검증했다. 한 줄 설치는 `Darwin/x86_64`에서 자산 다운로드 전에 거절하며 Apple Silicon과 Linux의 두 CPU에서는 원래 setup 인자·터미널·종료 코드를 전달한다. 설치·릴리스·CI 스크립트 시험은 36개 통과, Windows 실기 전용 1개는 제외했다.
- SSH 머신 설정 시험은 이전에 저장된 macOS x86_64 설정과 승인 대기 계획을 거부하고, arm64의 계획·승인·등록을 이어 가는 것을 확인했다. 다른 플랫폼의 host가 macOS arm64 실행 파일과 launcher를 받아 원격에서 SHA를 확인하는 계약도 검사했다. 해당 owner 32개가 통과했다.
- SSH 회귀 시험 확대 직후 시험 스택이 고갈됐다. 기존 머신 설정 시험의 요청 future를 `Box::pin`으로 분리하는 방식을 따랐다. ARM64 기계어에서 시험 poll 프레임이 504,176바이트에서 103,168바이트로 줄었고 기본 스택, 단언, 시한으로 통과했다. [기존 수리 기록](testing-machine-setup-stack-t-1584.md)
- 웹 타입 검사·번들·유닛 727개와 CI·릴리스 workflow의 actionlint 검사를 통과했다.
- macOS에서 launcher 파일 transaction 유닛 3개, 설치 통합 36개, 의존 경계 2개, 변경 후보의 로그 회전 통합 1개가 통과했다. GUI LaunchAgent 실제 등록은 CI용으로 남겨 둔 1개다.

설치 대상 판별은 현재의 POSIX 입구에서 유지했다. Herdr의 OS·CPU별 설치 파일 선택과 ripgrep의 release 대상 행렬을 대조했고 외부 코드를 가져오지 않았다. [Herdr 설치 코드](https://github.com/herdrdev/herdr/blob/master/distribution/install.sh) · [Herdr Apache-2.0](https://github.com/herdrdev/herdr/blob/master/LICENSE) · [ripgrep release workflow](https://github.com/BurntSushi/ripgrep/blob/master/.github/workflows/release.yml) · [ripgrep MIT](https://github.com/BurntSushi/ripgrep/blob/master/LICENSE-MIT)
