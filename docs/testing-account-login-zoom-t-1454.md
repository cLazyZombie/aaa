# 휴대폰 로그인 입력과 완료 화면 확대(t-1454)

## 원인과 수리

로그인 창의 메일·인증 코드 입력이 `font: inherit`로 본문 크기를 받았다. 기본은 14px,
compact는 13px이며 작은 글자 설정에서는 더 작아진다. 공통 `.input`의 `--fs-input: 16px` 하한이
이 별도 입력에는 적용되지 않았다. viewport 메타에는 이미 device-width·initial-scale=1이 있었다.

[WebKit iOS 초점 확대 구현](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/API/ios/WKWebViewIOS.mm)은
확대를 허용할 때 표준 16px을 입력 글자 크기로 나눠 배율을 계산한다. 이 경로와 확인한 14px 입력은
입력 뒤 완료 화면이 확대된 채 남았다는 증상에 부합한다. 사용자 기기의 당시 배율을 직접 측정한 것은 아니다.

로그인 입력에 `font-size: max(var(--fs-input), 1em)`을 적용한다. 메일·비밀번호 입력 모두 16px 이상이며
20px 같은 큰 글자 설정은 보존한다. 가로 화면에서도 하한을 유지한다. viewport 제한·강제 줌 초기화는 추가하지 않는다.
제품 문구·요청·인증 처리·API·프로토콜은 바꾸지 않는다. 기존 plan 절·E2E118을 확장하며 새 번호는 없다.

## 검증

기존 격리 Playwright 1.64.1과 브라우저 캐시, `aaa verify`·nice 15를 사용했다. 설치·운영 로그인은 하지 않는다.

1. CSS 수리 전 새 E2E 단언은 휴대폰 Chromium의 실제 계산 크기 14px에서 실패했다. 원문은 작업 전용 target에 보존했다.
2. 같은 단언·시한으로 CSS 수리 뒤 PC/휴대폰 Chromium·WebKit 네 조합이 통과했다.
3. 휴대폰의 메일·인증 코드에서 기본 14px·compact 13px·작은 설정 11px의 입력 하한과 큰 설정 20px 보존을 확인했다.
4. 390px 세로와 844×390 가로 viewport에서 실제 입력 초점을 주고, 완료 화면의 배율 1·가로 넘침 없음·닫기 버튼의 viewport 안 위치·hit test·실제 tap 종료를 확인했다.
5. 일반 다시 로그인·조회 로그인 성공과 실패 완료 화면, 원래 코드·재조회·권한 흐름도 유지했다. 가짜 계정 별칭만 쓴다.

웹 타입·번들·유닛 685개와 E2E118 네 조합이 통과했다. 가로 화면도 compact·11px의 작은 설정에서 확인했다.
마지막 가로 화면 단언을 이 작은 설정으로 강화한 뒤 관련 E2E118 네 조합만 다시 확인했다.
Rust·API 변경이 없어 무관한 native/cross/production을 반복하지 않는다.
착지 전체 gate는 별도 승인을 받은 최종 통합 SHA에서 수행한다. 공개 검사와 diff check도 통과했다.

## 검증 한계

macOS의 mobile WebKit/Chromium emulation은 iOS UIKit의 키보드·네이티브 입력 자동 확대를 재현하지 않는다.
이 시험은 실제 CSS의 확대 유발 조건과 모바일 viewport의 완료 조작을 검증한다. 원래 iPhone Safari에서의
최종 확대 해소는 배포 뒤 확인해야 하며 이 후보에서 실제 로그인·착지·배포는 하지 않는다.

조회 인증 저장 성공과 사용량 창 복구도 별개다. 인증 성공만으로 누락된 5시간 숫자나 관측 시각을 합성하지 않는다.
