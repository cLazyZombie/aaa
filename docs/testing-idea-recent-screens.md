# 아이디어 최근 화면 등록 검증(t-1292)

## 원인과 변경

`appScreenTarget`의 허용 목록에 `ideas:`·`idea:…`가 없어 아이디어를 방문해도 최근 화면 후보로 등록되지 않았다.
아이디어 목록은 주소가 없는 사이드바 패널이므로 `App`의 목록 방문 기록과 재열기 처리도 자동화 목록과 같은 경로에 넣었다.
개별 노트는 기존 주소를 그대로 복원하고 목록으로 돌아올 때는 보던 본문을 유지한다.

## 회귀를 지키는 시험

- 기존 `web/test/recentScreens.test.mjs`의 앱 화면 계약에 아이디어 목록·짧은 노트 주소·프로젝트를 붙인 노트 주소를 추가했다.
  수정 전에는 이 세 주소가 결과에서 빠져 실패했고 수정 후 6개 시험이 모두 통과했다.
- 새 `web/e2e/specs/123-idea-recent-screens.mjs`는 실제 host·agent에서 만든 노트 둘을 목록 클릭으로 방문한다.
  목록·노트 둘의 정확한 방문 순서, Ctrl 유지 중 순서 고정, 정방향·역방향 순환, 확정 뒤 노트 주소와 제목,
  목록 복귀 시 본문 유지와 사이드바 펼침을 확인한다. 허용 목록만 고치고 패널 복원을 빠뜨리는 회귀를 막기 위해 필요하다.
- Chromium·WebKit 각각 1280px·390px로 나눈 네 시험은 551·1374·1047·1456ms에 통과했다.
  고정 sleep 없이 DOM과 방문 기록 반영을 기다린다.

## 실행 결과

- `cd web && npm run build`: 타입 검사·번들 빌드 통과.
- `cd web && npm test`: 655개 통과.
- `nice -n 15 cargo build --profile e2e --locked -p aaa --bin aaa`: 통과.
- 웹 E2E105·123을 worker 2개로 함께 실행: 두 파일 모두 통과(전체 28.1초).
- `bash scripts/check-public.sh`: 통과.

Playwright는 작업 폴더에 1.64.1 패키지만 설치하고 기존 공용 캐시의 브라우저를 사용했다.
standalone 조건을 주입한 headless 입력이므로 설치된 PWA의 OS 예약 키 전달을 검증한 결과는 아니다.

## 참고한 구현

- [VS Code 편집기 명령](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/browser/parts/editor/editorCommands.ts)과
  [MIT 라이선스](https://github.com/microsoft/vscode/blob/main/LICENSE.txt): 최근 활성 편집기 순서로 대상을 찾는 구조를 확인했다.
- [Kate tabswitcher](https://github.com/KDE/kate/blob/master/addons/tabswitcher/tabswitcher.cpp): LGPL-2.0-or-later.
  문서와 일반 widget을 같은 전환 모델에 등록하고 Ctrl+Tab·Ctrl+Shift+Tab으로 순회한다.
- 두 후보처럼 기존 전환 모델에 대상을 등록하는 방식을 따랐으며 외부 코드를 복사하거나 의존성을 추가하지 않았다.
