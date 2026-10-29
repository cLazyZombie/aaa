# Ctrl+Tab 아이디어 목록 제외 검증(t-1304)

## 원인과 변경

아이디어 목록은 왼쪽 사이드바만 바꾸지만 `ideas:`로 별도 등록되고, 오른쪽 노트는 `idea:…`로 등록되어 두 후보가 함께 나왔다.
`appScreenTarget`에서 목록을 제외한다. 저장된 옛 `ideas:` 기록도 후보가 되지 않는다.
`App`의 현재 화면 지정에서도 아이디어 패널을 빼서 목록을 열었을 때 오른쪽 본문을 현재 화면으로 유지한다.
목록 재열기 분기도 제거했다. 노트의 짧은 주소와 프로젝트를 붙인 주소는 그대로 지원한다.

## 다른 목록 점검

| 화면 | 실제 이동·등록 | 판정 |
| --- | --- | --- |
| 아이디어 | 왼쪽 목록과 오른쪽 노트를 각각 등록 | 목록 제외·노트 유지 |
| 자동화 | 목록은 본문을 유지하는 `automations:`, 규칙 편집은 `automation:…` | 같은 구조 발견. 요청 범위에 따라 점검만 하고 기존 동작 유지 |
| 답할 것 | 넓은 화면은 마지막 본문 카드로 이동, 좁은 화면은 패널을 `asks:`로 등록. 개별 카드는 `ask:…`·`question:…` | 좁은 화면에서 같은 패널 후보 구조 발견. 기존 동작 유지 |
| 할 일 | 본문 보드 `tasks:`와 직접 연 상세 `task:…` | 독립 본문 화면이며 왼쪽 패널만의 중복 등록은 없음 |
| 모든 thread | 본문 목록 `threads:`와 선택한 thread·pane | 왼쪽 작업 목록은 별도 후보로 등록하지 않음 |
| 보고서 | 본문 목록 `reports:`와 개별 보고서 `reports:…` | 독립 본문 화면 |
| 작업 공간 | 본문 `workspace:`·`workspace:…`로 이동 | 독립 본문 화면 |
| 머신·검색 | 본문 주소는 있지만 최근 화면 허용 목록에 없음 | 같은 패널 후보 중복 없음 |

## 회귀 시험과 필요성

- 유닛 `아이디어 목록은 옛 기록에서도 제외하고 오른쪽 노트만 순환한다`:
  기존 저장 기록에 목록이 남아 있어도 후보에서 빼고 정방향·역방향 순환 대상을 지킨다.
  수정 전 `appScreenTarget("ideas:")`가 객체를 반환해 실패했고 수정 후 통과했다.
- 기존 E2E124의 목록 복원 계약을 사용자의 새 요구인 목록 제외 계약으로 바꿨다.
  실제 host 노트 둘을 목록에서 방문하고 목록을 다시 펼친 뒤, 현재 노트를 건너뛰는 첫 전환,
  노트 둘·할 일만 나오는 정확한 후보 목록, Ctrl 유지 중 방문 순서 고정과 정·역방향 확정을 확인한다.
  옛 `ideas:` 기록도 초기 상태에 넣어 업그레이드 뒤 후보에 남는 회귀를 막는다.
- 기존 E2E105는 자동화·답할 것·다른 본문 화면의 전환 계약을 보존하는 확인이다.
- 고정 sleep·재시도·시한 연장·기대값 완화는 추가하지 않는다.

## 실행 결과

- `node --test web/test/recentScreens.test.mjs`: 7개 통과.
- `cd web && npm test`: 657개 통과(약 4.2초).
- `cd web && npm run build`: 타입 검사·번들 통과.
- `nice -n 15 cargo build --profile e2e --locked -p aaa --bin aaa`: 통과(41.3초).
- E2E105: 전체 통과(35.4초). 대화 focus 네 단계는 각각 1.6~2.8초다.
- 첫 E2E124: Chromium 1280은 830ms 통과. Chromium 390은 목록 재열기에서 실패했다.
  노트 선택 뒤 서랍이 닫힌 상태에서 `inList(activity-ideas)`가 서랍을 먼저 열고 이미 선택된
  아이디어 단추를 다시 눌러 닫았다. `pickPanel`의 동일 패널 분기를 확인했다.
  좁은 화면에서는 본문의 서랍 열기 단추만 누르도록 시험 동작을 고쳤다. 단순 재실행이나 시한 변경이 아니다.
- 수정한 E2E124: Chromium 1280/390과 WebKit 1280/390 모두 통과.
  각각 718·1489·1054·1725ms, 준비·정리 포함 전체 7.5초다.
- `git diff --check`와 `bash scripts/check-public.sh`: 통과.
- 전체 ci-local·cross·병합·push는 착지 창에서 수행한다.

Playwright는 작업 폴더의 격리 패키지 1.64.1과 기존 공용 캐시의 Chromium·WebKit을 쓴다.
standalone 조건을 주입한 headless 시험이며 설치된 PWA의 OS 키 전달을 실기로 확인한 것은 아니다.

## 참고한 구현

- [VS Code 편집기 명령](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/browser/parts/editor/editorCommands.ts)
  ([MIT](https://github.com/microsoft/vscode/blob/main/LICENSE.txt)): 최근 활성 편집기를 대상으로 하는 기존 구조를 확인했다.
- [Kate tabswitcher](https://github.com/KDE/kate/blob/master/addons/tabswitcher/tabswitcher.cpp)
  (LGPL-2.0-or-later, 소스 SPDX): 문서·widget을 명시적으로 등록하는 구조를 비교했다.
- 기존 전환 모델에서 대상 등록을 조정한다. 외부 코드·제품 의존성은 추가하지 않았다.
