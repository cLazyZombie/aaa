# 기획 트리 스크롤 복원 경합(t-2088)

t-2082의 착지 검증에서 E2E 135의 Chromium 390px 기획 트리가 위치 700을 기다리다 20초에 실패했다. 당시 로그에는 어느 복원 단계였는지와 실제 위치가 남지 않았다. 기준판을 브라우저 worker 4개와 Chromium 4배 속도 제한으로 돌린 선택 실험은 8회 통과했다. 속도 제한 없이 착지와 같은 46개 spec을 브라우저 worker 4개로 실행한 대조에서도 57개 작업이 통과했다. 두 실험은 재확인 0회다.

## 재현과 수리

E2E 135는 이전 위치 700을 둔 Chromium 390px에서 긴 캐시 트리를 먼저 열고, 그 위 저장소 하나는 캐시가 비어 있는 응답 순서를 만든다. 나머지 캐시 데이터와 보류한 개별 저장소 응답은 실제 host 결과다. 위치 700을 그린 뒤 응답을 풀면 위에 문서 행이 추가된다. 기존 코드는 Chromium이 행 높이 30px만큼 스크롤을 보정하고 앱이 그 결과를 저장해 실제 위치와 저장값이 모두 730이 됐다.

기획 트리의 스크롤 영역에 `overflow-anchor: none`을 적용해 저장된 좌표를 우선한다. 사이드바와 문서 안 트리 칸이 같은 규칙을 쓴다. 회귀 단계는 행 반영과 브라우저 프레임을 기다려 위치와 저장값이 모두 700인지 검사한다. 기존 복원 대기에 단계 이름을 붙였고, 실패하면 실제·저장 위치와 트리 높이, 서랍·로딩 상태를 남긴다. 기존 기대값과 20초 상한을 유지한다.

자연 발생 조건은 재현되지 않아 최초 착지 실패의 정확한 단계와 원인은 미확정이다. 지연 응답으로 복원 좌표를 덮는 결함은 별도로 재현해 검증했다.

## 개발 검증

수정 전 실행 파일에 새 회귀 단계만 실행하면 실제 위치와 저장값이 모두 730으로 실패했다. 수정본은 같은 단계에서 두 값 모두 700으로 통과했다.

`aaa verify -- nice -n 15 python3 scripts/verify.py develop --base ddec957f --spec 135,136,137,138,139,140`로 타입 검사·번들·웹 유닛 746개·관련 E2E 6개·공개 검사를 통과했다. E2E 135의 기존 Chromium·WebKit 1280px·390px 조합과 새 회귀 단계가 포함됐다.

수정본은 Chromium 4배 속도 제한과 worker 4개로 기존 휴대폰 복원·새 지연 응답·저장 접근 거부 단계를 묶어 8회 모두 통과했다. 재확인은 0회다.

## 참고

- 위쪽 콘텐츠 변화 후 브라우저의 위치 보정과 이를 스크롤 영역에서 끄는 규칙을 확인했다. [CSS Scroll Anchoring](https://www.w3.org/TR/css-scroll-anchoring/)
- Chromium의 보정 구현과 WPT의 스크롤 앵커링 해제 검사를 참고했다. 두 프로젝트의 BSD 3-Clause 라이선스를 확인했다. [Chromium 구현](https://github.com/chromium/chromium/blob/main/third_party/blink/renderer/core/layout/scroll_anchor.cc) · [Chromium 라이선스](https://github.com/chromium/chromium/blob/main/LICENSE) · [WPT 검사](https://github.com/web-platform-tests/wpt/blob/master/css/css-scroll-anchoring/opt-out.html) · [WPT 라이선스](https://github.com/web-platform-tests/wpt/blob/master/LICENSE.md)
