# ProfileDialog 입력 유실 수리(t-1746)

## 원인과 기준 비교

E2E 126의 휴대폰 WebKit에서 profile 이름을 입력한 뒤 「미리보기」가 비활성화된 채 30초 누름 대기에 실패했다. 원본에는 실패 순간의 폼 상태가 없어 `busy`와 이름 유효성 중 어느 조건인지 구별할 수 없었다.

같은 폼의 harness 변경을 native 이름 input의 capture와 React onChange 사이에 끼워 대조했다.
이름 `mp`가 capture에는 있지만 harness 변경의 렌더가 controlled input을 빈 값으로 복원했다.
뒤의 이름 onChange도 이미 비워진 DOM 값을 읽어 저장하므로 `nameOk=false`가 된다.
입력칸은 같은 DOM이고 disabled가 아니었으며, 미리보기만 disabled였다. 미리보기 요청은 0건이었다.

기준 main `ed8947d3`의 격리 worktree와 수리 전 진단 후보에서 같은 사건 배치로 모두 재현했다.
PC의 `mc`와 휴대폰 WebKit의 `mp`가 각각 빈 문자열이 됐다. 두 판의 `SettingsAdminPages.tsx`와 의존성은 같고 기존 진단 후보는 E2E 123·문서만 바꿨다. 따라서 새 진단 후보가 도입한 코드 회귀가 아니라 기존 폼의 사건 순서 의존 결함이다.

이 대조는 원본의 미리보기 비활성을 만들 수 있는 이름 유실 경로를 재현한다. 원본 실패에서 발생한 모든 native 사건 순서나 정확한 비활성 분기까지 복구했다는 뜻은 아니다. 단순 부하 수치나 반복 통과를 분류 근거로 쓰지 않았다.

## 제품 수리

`ProfileDialog`의 폼에 속한 필드만 window native input capture에서 읽는다. React/portal capture가 앞선 렌더를 commit하기 전에 받은 실제 글을 현재 초안에 합친다. 이름·harness·model·effort·설명·인자를 모두 같은 경계로 처리한다. 기존 onChange도 유지한다.

이전 렌더의 `d`를 펼쳐 초안 전체를 바꾸던 코드는 functional updater로 해당 필드만 합치게 했다. 따라서 다른 필드가 함께 바뀌어도 새 입력을 덮지 않는다. 폼 소유자와 필드 이름을 확인해 다른 창의 입력은 받지 않고, 처리 중에는 입력이 비활성인 기존 정책을 유지한다.

이미 같은 저장소의 ChatComposer·AsksView·AccountsPage·AutomationEditor가 쓰는 capture 경계를 따른다. API·서버의 profile 검증·이름 규칙은 바꾸지 않았다.

## 회귀 시험과 진단

E2E 126의 기존 PC/휴대폰 WebKit 모델 켜기·끄기와 꺼진 모델의 profile 거부 계약을 유지했다.
이름 input의 document capture 중 같은 폼의 harness 선택 사건을 한 번 넣는다. AdminDialog가 body portal에 있으므로 document에서 그 경계를 관측한다. 이름이나 React state를 직접 채우거나 잃은 입력을 다시 넣지 않는다.

- 같은 DOM에서 이름과 동시에 바꾼 harness가 모두 남아야 한다.
- 이어 입력한 model도 남고, 원래의 꺼진 모델 거부 응답까지 도달해야 한다.
- 입력한 이름·harness·model로 미리보기 요청이 정확히 한 번 나가야 한다.
- 실패하면 휴대폰 context를 닫기 전에 폼 DOM·렌더 이력·사건 배치·요청·assertion stack과 actual/expected를 보존한다.

이름 유실이 단순 누름 시한 오류로만 보이지 않고, 실제 입력 보존과 서버에 전달한 내용을 지키기 위해 필요한 회귀다. 시한·기대값·기존 계약을 완화하거나 건너뛰지 않았다. 원인 대조용으로 휴대폰 경로를 따로 고른 실행은 최종 전체 조합 검증과 구분한다.

## 검증 과정

초기 관측기에서는 PC reload 뒤 관측기가 없어지거나 body portal의 입력을 `#root`에서 관측해 사건 회수 단언이 실패했다. 설치 시점과 실제 ancestor를 바로잡은 뒤의 이름 유실 단언을 원인 재현으로 기록했다. 초기 관측기 준비 오류를 제품 재현으로 세지 않는다.

수리 전 후보·기준 main 모두 PC와 휴대폰 WebKit에서 실제 이름 유실이 기록됐다. 수리 후 웹 타입·번들·웹 유닛 전체와 E2E 126 PC/휴대폰 WebKit을 확인한다. 기존 Safari 진단 후보의 E2E 123도 최종 전체 착지 검사에 남는다.

독립 리뷰에서는 capture 대상 whitelist에 있던 harness select의 `name` 속성 누락과, 이어지는 selectOption이 harness 유실을 가릴 수 있는 시험 빈틈을 지적했다. 속성을 추가하고 명시적 selectOption 전에 harness 값을 단언하도록 반영했다. 최종 리뷰와 착지 결과는 작업 보고의 상세 기록에 남긴다.

## 참고

- [React controlled input](https://react.dev/reference/react-dom/components/input), [MIT](https://github.com/facebook/react/blob/main/LICENSE): 실제 입력을 동기적으로 state에 반영하는 계약을 따른다.
- [React state updater](https://react.dev/reference/react/useState): 앞선 렌더 사본 대신 현재 초안의 한 필드만 갱신한다.
- [Playwright actionability](https://playwright.dev/docs/actionability), [Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE): disabled인 단추를 강제로 누르거나 대기 상한을 늘려 문제를 숨기지 않는다.

외부 코드나 새 제품 의존성은 추가하지 않았다.
