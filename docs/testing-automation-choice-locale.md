# 자동화 모델 선택지 로캘 검증

자동화 편집기는 모델·생각 수준·프로필 이름 순으로 정렬된 후보를 표시하고, 첫 모델과 첫 생각 수준을 기본으로 고른다. 기준판의 `localeCompare`는 실행 환경의 기본 로캘을 물려받았다. `ko-KR`에서는 `다른 머신`이 라틴 문자 모델 앞에 와 기본 후보가 달라졌다.

세 비교에 영문 `Intl.Collator`를 공통으로 사용한다. 기존 순서인 `claude → opus → 다른 머신`을 모든 기기 로캘에서 사용한다. 기존 단위 시험은 전체 모델 순서와 생각 수준 순서를 그대로 단언한다.

## 결과

- Windows 11에서 Node 24.22.2의 기본 로캘 `ko-KR`을 확인했다. `node --test test/automationChoices.test.mjs`는 기준판에서 순서 단언 한 건이 실패했고 수정판에서 9건 모두 통과했다. 수정판의 `npm test`는 743건 통과, 3건 건너뜀, 실패·취소 0건이었다.
- macOS에서 `LANG=ko_KR.UTF-8 LC_ALL=ko_KR.UTF-8`로 `scripts/ci-local.sh --only web,public`을 실행했다. 타입 검사, 번들, 웹 유닛 746건, 공개 검사가 통과했다.
- Playwright 1.64.1의 Chromium·WebKit에서 실제 `ProfilePicker`를 시험용 데이터로 렌더링했다. 두 브라우저에서 `ko-KR`·`en-US` 각각의 기본 로캘, 모델 순서와 기본값, `opus`의 생각 수준 순서, 프로필 칩 추가를 확인했다.
