# 휴대폰 메뉴 왼쪽 끝 누름(t-1333)

## 원인과 수정

`watchDrawerSwipe`가 화면 왼쪽 끝 24px 안의 `touchstart`를 즉시 뒤로 쓸기로 취급해 열린 메뉴를 닫았다.
활동 막대의 단추 왼쪽 절반이나 로고를 가볍게 눌러도 클릭 전에 메뉴가 사라지는 원인이었다.

시작점은 기억하고, 기존 방향 판정 여유 10px 이상 오른쪽으로 움직이며 가로 이동이 세로보다 클 때만
뒤로 미리보기에 대비해 메뉴를 닫는다. 단순 누름·작은 흔들림·세로 스크롤은 유지한다.
가장자리 몸짓에서 `preventDefault`·전파 중단을 하지 않으며, 실제 기록 이동·취소·페이지 복원의 정리는 유지한다.
선택한 패널 단추를 다시 눌러 닫는 기존 동작도 유지한다.

## 필요한 회귀 검사

- `web/test/edgeSwipe.test.mjs`: 열린 메뉴의 x=0·4·24·25 누름과 작은 흔들림, 왼쪽 끝 세로 스크롤을 추가했다.
  사용자의 누름이 쓸기로 오인되는 계약을 직접 지킨다. 수정 전 새 두 시험은 모두 `calls: [false]`로 실패했다.
  수정 뒤 같은 파일의 34건이 통과했다. 기존 가장자리 오른쪽 쓸기의 닫힘과 움직임 전달도 유지한다.
- E2E 39: Chromium·WebKit iPhone 문맥에서 실제 `touchscreen.tap`으로 x=4·24의 활동 막대 빈 공간을 누르고 메뉴 유지를 확인한다.
  이어 다른 패널 단추의 x=4를 눌러 패널 선택과 메뉴 유지를 함께 확인한다. 실제 브라우저 hit testing과 클릭까지
  확인해야 하므로 리스너 유닛 검사와 별도로 필요하다. 기존 E2E 39의 뒤로/앞으로·취소·복원 검사도 함께 실행한다.
- 첫 E2E의 로고 좌표는 로고 위를 덮는 화면 머리의 닫기 단추를 눌렀다. 터치·클릭 target 기록으로 원인을 확인하고,
  실제 hit target이 활동 막대인 빈 공간 좌표로 바로잡았다. 중복 class 선택자는 이름 있는 단추 선택자로 좁혔다.
  내장 자산 최신성 검사는 문서 변경 뒤 번들 재생성으로 충족했다.
- 고정 sleep·재시도·시한 연장·기대값 완화는 추가하지 않았다. iOS 시스템 뒤로 미리보기 자체는 headless의 관측 범위 밖이다.

## 개발 검사 결과

웹 타입·번들 및 유닛 664건, 관련 E2E 39 전체, 공개 저장소 검사를 통과했다.
추가한 Chromium·WebKit 터치 단계는 3.1초였고 새 유닛 두 건은 각각 1ms 미만이었다.
웹 이외 코드 변경이 없으므로 관련 웹 owner를 검사했으며, 전체 착지 gate는 착지 창에서 수행한다.

## 참고 후보와 라이선스

- [herdr-web-ui edgeSwipe](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/src/lib/edgeSwipe.ts)
  ([MIT](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/LICENSE)):
  이미 옮긴 리스너의 시작점 저장과 이동 뒤 방향 판정을 따른다.
- [Radix Toast](https://github.com/radix-ui/primitives/blob/main/packages/react/toast/src/toast.tsx)
  ([MIT](https://github.com/radix-ui/primitives/blob/main/LICENSE)):
  터치 시작은 위치만 저장하고 10px 움직임과 방향을 확인한 뒤 쓸기를 시작하며, 다른 방향이면 후보를 버리는 구조를 대조했다.

외부 코드 복사나 의존성 추가는 없다. 개발 검사 결과와 상세 로그는 t-1333 보고의 library에 남긴다.
착지 창의 최종 통합 SHA gate와 병합·배포는 이번 개발 검사에 포함하지 않는다.
