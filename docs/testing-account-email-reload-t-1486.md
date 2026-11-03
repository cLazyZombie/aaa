# 계정 메일 시험의 닫기·새로고침 경합(t-1486)

## CI에서 확인한 범위

main `3d018dd7`의 [실패한 웹 E2E](https://github.com/cLazyZombie/aaa/actions/runs/41576224737/job/125720231254)는
E2E 118 「휴대폰 Chromium 메일 등록·수정」의 유일한 `page.reload()`에서 30초 뒤 실패했다.
이 위치 앞에는 메일 수정·저장 표시, 조회 인증 실패 표시·닫기, 새 계정 등록과 로그인 완료 표시·닫기가 모두 있다.
따라서 시험은 이 화면 표지들을 통과한 뒤 새로고침에서 실패했다. 실제 제공자·메일 저장 backend 대신
이 시험의 독립 제공자 fixture를 사용한다.

원본 artifact에는 E2E 로그·실행 시간 기록과 다른 시험의 진단만 있다. 해당 페이지의 trace·스크린샷·HTTP 기록은 없다.
실패한 페이지도 `finally`에서 먼저 닫혀 공통 진단에 나온 1280px 화면은 별도 준비 페이지다.
로그에는 `waiting for navigation until "load"`만 있고 새 문서로 이동했다는 줄이 없다.
이 자료만으로 원본 CI의 문서 요청·응답이나 미완료 리소스, 정확한 취소 순서는 확정할 수 없다.
부하 수치만으로 host 지연이나 환경 문제로 판정하지 않았다.

## 발견한 경합과 수리

휴대폰의 인증 창은 `sheetHistory`가 브라우저 기록 한 칸을 빌린다. 창을 닫는 React commit에서 DOM이 사라진 뒤,
정리 effect가 예약한 콜백이 `history.back()`을 부르고 실제 복귀는 나중의 `popstate`로 도착한다.
기존 `closeCompleted`는 DOM 제거까지만 기다려, 아직 뒤로 이동이 남아 있는데 다음 인증이나 새로고침을 시작할 수 있었다.

기존 helper를 실제 메일 흐름에서 실행하면서 마지막 닫기의 native Back 전달만 보류했다.
인증 창 DOM은 없고 sheet 기록은 남은 상태인데 helper가 완료되는 것을 확인했으며,
「기록 복귀 전에는 닫기가 끝나지 않는다」 단언은 수리 전에 실패했다.
같은 보류 조건의 수리한 helper는 완료되지 않고 기다렸고, native Back을 전달해 실제 `popstate`가 온 뒤 완료됐다.
이후 기존 새로고침·등록 메일 보존 단언도 통과했다. 두 경우 모두 고정 sleep이나 가짜 `popstate`를 쓰지 않았다.

별도 사건 순서 실험에서는 기존 helper가 반환한 뒤 새로고침의 `beforeunload` 다음에 남은 Back을 전달했다.
문서 요청을 실제 복귀 사건까지 보류하자 새로고침 GET이 `net::ERR_ABORTED`로 취소됐고,
응답·DOMContentLoaded·load 없이 원래 계정 화면이 남았다. trace·화면·요청 사건을 작업 폴더 `target/t-1486-ci/`에 보존했다.
이는 닫기 복귀가 새로고침을 취소할 수 있다는 재현이다. 원본 CI의 정확한 30초 시간 초과를 재현한 결과로 쓰지 않는다.

`closeCompleted`가 DOM 제거 뒤 sheet 기록 해제를 확인하고, 필요하면 실제 `popstate`를 기다리도록 고쳤다.
기존 사건 기반 `domIs`를 사용하며 desktop처럼 sheet 기록이 없으면 즉시 끝난다.
새로고침의 기본 `load` 조건·30초 시한과 모든 기존 단언은 유지한다. 재시도·sleep·시한 연장은 추가하지 않았다.
메일 수정값도 새로고침 뒤 다시 확인해 기존 등록 메일 보존 검사와 함께 지킨다.

앞선 회전 수리는 두 기록 관리자가 동시에 뒤로 이동해 설정 쪽을 잃는 제품 결함이었다.
이번 수리는 마지막 닫기의 비동기 완료를 시험이 잘못 판단한 경합이다. 메일 처리·host·인증 backend와 제품 기록 코드는 바꾸지 않았다.
CI의 별도 원인이 배제됐다는 뜻은 아니며, 같은 서명이 다시 나오면 해당 페이지의 navigation trace가 필요하다.

## 관련 검증

다음 검사는 모두 `aaa verify`로 실행했다. Playwright 1.64.1 패키지는 작업 폴더에만 두고 기존 공용 브라우저 캐시를 사용했다.

- 변경 전 기준: 웹 타입·번들·웹 유닛 685개와 E2E 118 모든 조합 통과. 이 한 번의 통과로 간헐 실패를 배제하지 않았다.
- 강제 기록 보류: 수리 전 helper의 잘못된 완료 단언 실패, 수리 후 실제 복귀·새로고침·메일 보존 통과.
- 기존 helper와 새로고침 중 Back 경합: 실제 문서 요청 취소를 확인. 원본 CI의 timeout과 같은 결과로 합치지 않았다.
- 개발 owner: `nice -n 15 python3 scripts/verify.py develop --base 3d018dd7 --spec 118,39` 통과.
- 웹 타입·번들·웹 유닛 685개, E2E 118의 Chromium·WebKit/PC·휴대폰/회전·인증·메일 전체, E2E 39 전체 통과.
- 공개 저장소 검사와 `git diff --check` 통과.

개발 로그는 `target/t-1486-development/`에 있다. 미커밋 상태의 검사라 workspace 원장 자동 수집은 생략됐지만
각 단계 종료 코드와 통과 결과는 보존했다. 새로 추가한 독립 시험은 없고 기존 메일 E2E의 완료 조건과 보존 단언을 강화했다.
관련 검사를 통과한 뒤 전체 gate를 반복하지 않는다. 최종 통합 SHA의 gate는 별도 착지 창에서 수행한다.

## 근거

- [History.back](https://developer.mozilla.org/en-US/docs/Web/API/History/back): 비동기 이동의 완료는 `popstate`로 확인한다.
- [Playwright page.reload](https://playwright.dev/docs/api/class-page#page-reload): 기본 완료 조건은 `load`다.
- 사용한 Playwright 1.64.1의 `Page.reload`·`Frame.waitForNavigation` 구현도 확인했다. 새 문서 navigation을 요구하며,
  로그의 `navigated to`는 그 navigation을 관측한 뒤 남긴다. 같은 문서의 기록 복귀 자체는 새 문서 완료가 아니다.
