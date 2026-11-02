# 개발

aaa를 소스로 빌드하고 검사하는 법이다. 개발 규칙은 [AGENTS.md](AGENTS.md), 문서 목록은 [docs/README.md](docs/README.md#개발)에 있다.

## 빌드

Rust toolchain·Node.js·npm이 필요하다. 웹을 먼저 빌드하면 Rust 실행 파일에 포함된다.

```sh
git clone https://github.com/cLazyZombie/aaa
cd aaa
(cd web && npm ci --no-audit --no-fund && npm run build)
cargo build --locked -p aaa --bin aaa
./target/debug/aaa --help
```

배포용은 `cargo build --locked --release -p aaa --bin aaa`로 빌드한다. macOS launcher 준비는 [P0 절차서](docs/p0-runbook.md)를 따른다.
Linux·Windows 실행 파일은 해당 환경에서 빌드한다. 빌드 profile은 [빌드 profile](docs/build-profiles.md)에 있다.

## 검사

CI와 같은 단계는 `scripts/ci-local.sh`에 있다.

```sh
bash scripts/ci-local.sh                     # web fmt build test e2e public
bash scripts/ci-local.sh --only clippy        # 네이티브 strict Clippy
bash scripts/ci-local.sh --only web,e2e,public
```

웹 E2E는 Unix에서 Playwright 1.64.1의 Chromium·WebKit을 쓴다. `npm install -g playwright@1.64.1`과
`npx playwright@1.64.1 install chromium webkit`으로 준비하고, 별도 설치는 `NODE_PATH`로 가리킨다.
실패한 단계의 원인은 `target/ci-local/` 로그로 확인하며 재시도로 덮지 않는다. 시험 작성 규칙은 [시험 쓰는 법](docs/testing-rules.md)에 있다.

공개 저장소다. push 전에 `bash scripts/check-public.sh`를 돌린다. 실제 머신 이름·계정·주소·홈 경로는 커밋하지 않고 예시(`mac-a`, `example.ts.net`)를 쓴다.

## README 그림

README 그림은 가짜 머신과 견본 프로젝트를 띄운 실험실을 GitHub Dark 테마로 찍는다. 앞의 웹·Rust 빌드와 ffmpeg가 먼저 필요하다.

```sh
cargo build --locked --profile e2e -p aaa --bin aaa
NODE_PATH=$(npm root -g) node web/e2e/showcase.mjs --readme docs/media
```

장면 목록과 격리 규칙은 [README 매체 안내](docs/readme-media.md)에 있다. 그림 속 글자도 확인하고, 매체는 파일마다 1 MiB 아래로 둔다.

## 라이선스 고지

웹 화면에 함께 넣은 npm 패키지(긴 소스 보기의 `@pierre/diffs`·`@pierre/theme`(Apache-2.0)과 Shiki(MIT)·`diff`(BSD-3-Clause), React·xterm 등)의
고지는 웹 빌드가 모아 `web/dist/licenses/THIRD_PARTY_NOTICES.txt`에 둔다(`web/scripts/license-notices.ts`). host는 그것을 웹 주소
`/licenses/THIRD_PARTY_NOTICES.txt`로 보낸다.

웹 터미널의 [Meslotendard](https://github.com/cLazyZombie/Meslotendard) 0.3.1 글꼴(Regular·Bold, WOFF2)은 SIL Open Font License 1.1이다.
구성 원본 Meslo LG(Apache 2.0)·Nerd Fonts·Pretendard(OFL 1.1)의 고지는 [web/public/fonts/meslotendard-0.3.1/](web/public/fonts/meslotendard-0.3.1/)에 있고,
host는 같은 고지를 웹 주소 `/fonts/meslotendard-0.3.1/`로도 보낸다.
