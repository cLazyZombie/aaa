# 휴대폰 인증 창 회전과 설정 기록 경합(t-1483)

## 실패와 원인

기준 `8bd56130`의 [실패 job](https://github.com/cLazyZombie/aaa/actions/runs/41572046517/job/125706972315)에서
E2E 118 「휴대폰 Chromium 메뉴·설정 원격 인증」이 로그인 완료 뒤
`sample-claude 사용량 조회 로그인` 단추를 찾다가 30초 시한을 넘겼다.
메뉴 열기·계정 탭 선택·Claude 인증 시작·코드 전송은 이미 끝났고, `account_usage_login`은 전송되지 않았다.
실패 본문에는 설정 탭 목록만 있었다. 단추가 가려지거나 비활성화된 문제가 아니라 계정 본문 자체가 사라진 상태다.
CI artifact에는 해당 시험의 trace·화면이 없었다. 로컬 계측으로 같은 locator 시간 초과·설정 목록 화면을 재현하고 trace를 수집했다.

인증 입력 검사에서 휴대폰 너비 390 → 가로 844 → 390으로 바뀐다.
설정은 너비 768 이하에서 목록·계정 쪽을 두 칸으로, 그보다 넓으면 한 칸으로 기록한다.
인증 창은 너비 640 이하에서 별도 sheet 칸을 갖는다.
가로 전환 때 설정이 깊이를 줄이는 `history.go(-1)`과 sheet가 자기 칸을 거두는 `history.back()`을 함께 요청했다.
브라우저가 두 요청을 차례로 처리하면 계정 쪽 아래의 목록까지 내려간다.
로그인 창을 닫은 뒤에도 목록이 남아 다음 인증 단추를 찾지 못한다.

원래 E2E는 로컬에서 통과했고 CPU 부하 주입만으로도 통과했다. 이를 수리 근거로 삼지 않았다.
기록 이동 전달을 잠시 보류해 두 요청이 모두 발생한 뒤 실제 popstate를 순서대로 도착시키자 CI와 같은 실패가 났다.
새 회귀 시험도 수리 전 제품에서 「가로 회전은 계정 쪽을 바꾸지 않는다」 단언으로 실패했다.

최근 변경의 책임 범위도 확인했다. t-1454의 `05d614f9`가 입력 글자 하한 검사와 가로/세로 전환을 추가해
기존 기록 관리자 사이의 경합을 드러냈다. t-1477의 `a91c6925`는 메일 저장·수정과 fixture를 확장했지만,
이번 실패에서는 메일 수정·조회 인증 요청 전에 계정 본문이 사라졌다. 메일 데이터 처리나 인증 backend는 수리 범위에 넣지 않았다.

## 수리와 필요한 회귀 시험

- 설정 위의 sheet가 소유한 칸에서는 설정 기록 조정을 미룬다.
- sheet 닫기의 내부 복귀는 사용자가 설정 쪽을 고른 사건으로 처리하지 않는다.
- 그 복귀가 실제 도착한 뒤 설정이 보던 쪽을 유지하면서 필요한 깊이를 조정한다.
- `settingsHistory`가 `sheetHistory`의 사건 표시를 직접 사용하므로 sheet의 popstate 처리가 먼저 등록된다.

E2E 118에 Chromium·WebKit 각각 독립적인 회전 경합 단계를 추가했다.
브라우저의 실제 기록과 popstate는 그대로 쓰고 전달 시점만 보류한다.
sheet의 뒤로 요청과 그리기 사건이 오면 즉시 해제하므로 고정 sleep은 없다.
계정 탭 선택·세로 복귀·로그인 완료·닫기 뒤 계정 본문과 다음 조회 인증 요청 한 번을 검사한다.
화면 회전이나 닫기만으로는 인증 요청이 늘지 않아야 한다.
이 시험은 클릭 시간 초과 전의 원인인 설정 쪽 유실을 직접 잡기 때문에 필요하다.
기존 입력 글자 하한·가로 입력·닫기 hit-test·인증/메일 단언·시한은 유지했다.

## 확인 기록

`aaa verify -- nice -n 15 python3 scripts/verify.py develop --base 8bd56130 --spec 118,39`로 확인했다.
자기 작업 폴더의 Playwright 1.64.1을 `NODE_PATH`로 선택하고, 공용 캐시의 기존 Chromium·WebKit을 사용했다.

- 웹 TypeScript·번들·웹 유닛 685개 통과.
- E2E 118: 새 회전 경합 2개와 기존 원격 인증·메일 등록/수정 8조합 모두 통과.
- 새 회전 경합은 Chromium 2.892초, WebKit 2.895초. 기존 E2E 118의 개별 단계도 모두 5초 이하였다.
- E2E 39: 기존 휴대폰 메뉴·쓸기·뒤로/앞으로·설정 준비·설정 내부 복귀의 모든 단계 통과.
- 공개 저장소 검사·`git diff --check` 통과.

개발 로그는 `target/t-1483-development/`에 남겼다. 로컬 실패 계측의 원본 trace·화면·전체 로그는
`target/t-1483-evidence/`에 보존하고, 보고 library에는 검토한 요약과 화면만 둔다.
개발 검사는 미커밋 변경 상태라 workspace 원장에 수집되지 않았지만 각 단계의 성공 기록과 종료 코드 0은 보존했다.
실패한 명령을 통과할 때까지 반복하지 않았다. 원래 재현이 어려운 경합은 사건 순서로 고정하고,
수리 전 제품에서 새 시험이 실패하는 것을 먼저 확인한 뒤 수리한 제품의 관련 owner를 실행했다.
전체 착지 gate는 별도 착지 창의 최종 통합 SHA에서 수행한다.

## 참고한 공개 구현

- [Playwright actionability](https://playwright.dev/docs/actionability), [trace viewer](https://playwright.dev/docs/trace-viewer)
  ([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE)): 클릭 가림·안정성·활성 상태와 실제 DOM·요청을 분리해 조사했다.
- [React Router useNavigationType](https://reactrouter.com/api/hooks/useNavigationType)
  ([MIT](https://github.com/remix-run/react-router/blob/main/LICENSE.md)): 기록 쓰기와 실제 POP 도착을 구분하는 관점을 참고했다.
- [Radix Dialog](https://www.radix-ui.com/primitives/docs/components/dialog)
  ([MIT](https://github.com/radix-ui/primitives/blob/main/LICENSE)): 겹친 창의 focus 복귀와 본문 상태를 구분해 확인했다.

새 라이브러리나 외부 코드는 추가하지 않았다. 이번 원인은 focus가 아니라 두 기록 관리자의 중복 이동이었다.
