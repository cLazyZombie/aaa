# 한 줄 설치 입구와 Pages 게시

배포 주소는 `https://cLazyZombie.github.io/aaa/install.sh`와 `https://cLazyZombie.github.io/aaa/install.ps1`이다.
저장소에 설치 스크립트 정본과 생성·게시 workflow를 준비한다. **Pages 활성화와 최초 공개는 사용자 승인 뒤에 한다.**
사용자가 승인한 자동 재게시 정책에 따라 정식 release workflow가 성공하면 설치 입구를 다시 게시한다. 병합만으로 게시하지 않는다.

## 설치 흐름

- macOS·Linux: `scripts/bootstrap/install-unix.sh`를 POSIX sh로 실행한다. OS·CPU 선택 → 같은 tag의
  `SHA256SUMS`와 실행 파일 검증 → macOS launcher package 검증 → `aaa setup` 순서다.
  `curl | sh`의 입력을 `/dev/tty`로 돌려 setup이 같은 터미널에서 질문한다. 별도 PATH·서비스 설정을 하지 않는다.
  다운로드는 비공개 임시 폴더에만 두고 setup 종료 뒤 지운다. 기존 설치 위치의 쓰기는 setup이 맡는다.
- Windows: 기존 `scripts/bootstrap/install-windows.ps1`과 `release.render_windows_entry`를 그대로 쓴다.
  Pages 생성 시 공개 `SHA256SUMS`와 manifest가 같은지 확인하고 그 SHA를 입구에 넣는다.
  실행 시 기존 bootstrap이 manifest·고정 SHA·실제 파일 SHA·PE x86_64·`--version`을 확인한 뒤 setup을 연다.
  Windows 다운로드·배치·인자 처리 구현을 복제하지 않는다. 기존 다른 실행 파일은 별도 executor로 보존한다.
- 설치·등록·서비스는 두 입구 모두 `aaa setup`에 위임한다. 다운로드 성공만으로 설치 완료를 표시하지 않는다.
  취소·실패 종료 코드는 전달한다. setup이 처리하는 사용자 암호나 host 주소는 입구에 저장하지 않는다.

## 생성과 검증

```sh
python3 scripts/build_install_site.py --output target/install-site
python3 -m unittest scripts.test_install_site scripts.test_release
```

생성 도구는 공개 최신 stable tag를 한 번 조회하고 그 tag의 manifest·SHA256SUMS만 읽는다.
세 OS의 setup이 있는 v0.1.6 이상, 공식 자산 URL, 다섯 실행 파일과 두 launcher의 SHA 일치를 요구한다.
잘못된 판·외부 URL·누락·중복·SHA 불일치면 출력 전에 실패한다.
출력은 `install.sh`, `install.ps1`, `release.json`, `.nojekyll` 네 파일이다. `release.json`은 게시판의 판·자산 SHA 기록이다.
`--metadata-dir <폴더>`로 로컬 manifest·SHA256SUMS를 주면 네트워크 없이 같은 결과를 검사할 수 있다.

입구는 **Pages를 생성한 시점의 공개판에 고정**된다. `release` workflow의 성공한 tag push 뒤 `workflow_run: completed`로 자동 재게시한다.
공식 저장소의 실행만 받고, `vX.Y.Z` tag·공개 release·`draft=false`·`prerelease=false`·현재 latest 일치를 모두 확인한다.
실패·취소·사전판·초안·과거판 재실행은 배포하지 않는다. 수동 `workflow_dispatch`는 main에서 계속 쓸 수 있다.
생성 직전 latest가 자동 게시 대상 tag와 달라지면 실패하며 판을 바꾸어 통과시키지 않는다.
현재 main을 몰래 빌드하거나 release·tag를 만드는 단계는 없다. SHA는 전송·자산 불일치 검증이며 별도 서명 검증은 아니다.

## 승인 뒤 게시 절차

1. 변경을 main에 병합한다. 저장소 관리자가 Settings → Pages → Source를 **GitHub Actions**로 선택한다.
   이 설정 변경과 공개에는 사용자 승인이 필요하다. workflow에서 Pages를 자동 활성화하지 않는다.
2. Actions의 **설치 스크립트 Pages 게시**를 main에서 수동 실행한다.
   게시 대상 확인 → Ubuntu 계약 시험 → 공개 최신 release에 고정한 파일 생성 → Pages artifact → `github-pages` 배포 순서다.
   Pages 쓰기·OIDC 권한은 deploy job만 갖는다.
3. 배포 완료 뒤 workflow가 두 설치 URL을 GET하여 실제 게시 artifact의 `install.sh`·`install.ps1`과 바이트를 대조한다.
   판·고정 자산 SHA·본문이 모두 같아야 성공한다. 404·이전 내용·다운로드 오류는 URL·기대 판 또는 HTTP 오류를 남기고 실패한다.
   고정 대기·재시도로 배포 실패를 숨기지 않는다.
   macOS·Linux의 `curl … | sh`, Windows의 `irm … | iex`를 설치할 사용자 터미널에서 확인한다.
4. README의 「게시 준비 중」 안내를 제거한다. 이후 정식판은 자동 재게시하며 수동 실행은 복구·재게시용으로 남긴다.

## 자동 연결과 배포 branch 제한

`GITHUB_TOKEN`으로 만든 release 사건은 다른 workflow를 깨우지 않으므로 `release: published`를 사용하지 않는다.
`workflow_call`을 release 끝에 넣으면 호출자의 tag ref로 배포하여 `github-pages`의 main 전용 branch 정책과 충돌한다.
`workflow_run`은 기본 branch 문맥에서 실행하므로 main만 허용하는 기존 환경 정책을 유지할 수 있다.
워크플로 파일은 main에 있어야 하며 checkout도 기본 main을 쓴다. release 실행의 코드·artifact를 받아 실행하지 않는다.
2026-10-24 공개 API 조회에서 환경은 custom branch 정책 `main`(type `branch`) 하나만 허용함을 확인했다. 환경 설정은 변경하지 않았다.
근거는 [GitHub workflow_run 문서](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_run)와
[재사용 workflow 문맥](https://docs.github.com/en/actions/reference/workflows-and-actions/reusing-workflow-configurations#github-context)이다.

Pages 설정·workflow 실행·release 게시를 이 준비 작업의 자동 검증으로 대신하지 않는다.
설정 방법은 [GitHub Pages의 사용자 workflow 문서](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages)를 따른다.

## 참고한 공개 구현

- [Herdr 설치 안내](https://herdr.dev/docs/install/)와 [distribution/install.sh](https://github.com/herdrdev/herdr/blob/master/distribution/install.sh)
  ([Apache-2.0](https://github.com/herdrdev/herdr/blob/master/LICENSE)): OS·CPU 선택, 검증 뒤 실행, 한 줄 명령을 먼저 보여 주는 문서 순서를 참고했다.
- [herdr-web-ui의 현 이름 Ovis](https://github.com/devswha/ovis)의 [install.sh](https://github.com/devswha/ovis/blob/main/install.sh)·
  [Pages workflow](https://github.com/devswha/ovis/blob/main/.github/workflows/pages.yml)
  ([MIT](https://github.com/devswha/ovis/blob/main/LICENSE)): 정본 설치 입구를 Pages artifact로 내보내는 구성과 파이프 입력에서 터미널을 여는 경계를 참고했다.
- [herdr-projects 설치 경로](https://github.com/eliasstravik/herdr-projects/blob/main/crates/flow/src/install.rs)
  ([MIT](https://github.com/eliasstravik/herdr-projects/blob/main/LICENSE)): 플랫폼별 release 파일 선택·체크섬 확인·실행 판 확인을 대조했다.
  과거 `scripts/install.sh`는 현재 main에 없으며 현 설치 구현을 읽었다.
- 자동 재게시에서는 Ovis의 위 Pages workflow(MIT)의 `workflow_run` 성공·동일 저장소 검사와
  [GitHub 정적 Pages 예제](https://github.com/actions/starter-workflows/blob/main/pages/static.yml)
  ([MIT](https://github.com/actions/starter-workflows/blob/main/LICENSE))의 환경·artifact·직렬 배포 구성을 대조했다.
  Ovis의 성공 후 연결 방식을 따르되 aaa는 태그 release의 정식판·latest 여부를 추가 확인한다.

외부 코드를 복사하거나 의존성을 추가하지 않았다. 설치 자체는 aaa의 기존 setup과 Windows bootstrap을 사용한다.
