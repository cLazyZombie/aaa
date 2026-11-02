# 질문 카드 답 초안 보존(t-1436)

## 원인과 변경

`AsksView`의 thread 질문 카드 key가 보고 hash와 서버 일부 답 전체에 묶여 있었다.
주변 진행 글만 바꾸어 보고를 수집해도 새 q-ID·hash가 생겨 카드가 재생성되고,
선택·직접 답은 카드 내부 state뿐이라 초기화됐다. 다른 카드 왕복과 페이지 reload에도 같은 문제가 있었다.

카드 소유자와 질문 내용·선택지·검수 규칙을 identity로 쓴다. q-ID·보고 hash·일부 답은 이 identity에서 제외한다.
질문 내용이 같은 재게시에는 입력 DOM·초점·선택 범위를 유지하고, 내용이 바뀌면 새 폼으로 구분한다.
q-ID 링크로 열어 둔 카드도 같은 thread의 동일 질문에만 이어지며 새로고침 가능한 thread 주소로 바뀐다.

입력 사건에서 선택·공통 직접 답·질문별 직접 답을 localStorage에 저장한다. 카드가 없어졌다 다시 생겨도 읽는다.
서버의 새 일부 답만 해당 칸에 반영하고 다른 로컬 입력은 보존한다. 이미 받은 일부 답을 사용자가 다시 고쳤다면
같은 서버 응답이 와도 그 수정을 덮지 않는다. 전송 성공은 전송 시작 때의 사본과 여전히 같을 때만 초안을 지우며,
실패·새 판의 입력·늦은 응답에는 새 초안을 지우지 않는다. 저장소 차단·용량 초과에는 현재 페이지 메모리로 보존한다.
초안은 이 브라우저의 미전송 UI 입력이며 서버 질문·답의 상태는 host가 관리한다.

## 꼭 필요한 회귀

- `web/test/questionDraft.test.mjs`: 질문·소유자 경계, 서버 일부 답 병합, 일부 답 재편집,
  전송 성공 시 삭제와 늦은 성공의 새 초안 보호, 저장소 차단·손상·용량 초과를 검증한다.
  잘못된 질문에 답이 옮겨가거나 저장 실패가 최신 입력을 지우는 것을 막기 위해 필요하다.
- E2E `123-question-drafts`: 실제 host·agent·보고 파일 수집과 coordinator post를 이용한다.
  Chromium, Mac 크기 WebKit, iPhone 크기 WebKit에서 같은 질문의 새 q-ID·목록 갱신·새로고침·카드 왕복·
  일부 답 수신·네트워크 전송 실패·바뀐 선택지의 초기화·현재 보고로의 답 전송을 검증한다.
  실제 입력 DOM과 커서도 비교하며 고정 sleep 없이 보고 hash·q-ID DOM·입력·답 수신을 기다린다.
- 기존 E2E 53·69: 늦은 이전 답 응답·root capture 입력 경합·질문별 의견·실제 host의 답 보관·
  coordinator 질문·다른 기기와 보기 권한 계약을 유지한다.

원본에서 Chromium과 iPhone 크기 WebKit의 같은 질문 재게시 뒤 `aria-checked=false`로 실패를 재현했다.
새 spec의 단계 이름 수집 형식과 Node의 TypeScript strip-only 제한은 시험 작성 중 원인을 확인해 수정했다.
최종 시험에 이미 답한 질문을 다시 올린 단계는 host가 답한 질문을 제거해 두 번째 입력칸이 없었다.
보고와 `orch`의 답한 질문 재게시 처리를 확인하고, 다음 입력 시험은 별개 질문으로 만들었다.
성공한 초안 삭제는 실제 저장소의 삭제와 단위 시험으로 계속 확인한다. 제품의 시한·기대값은 바꾸지 않았다. 새 단계는 각각 분리해 정상 실행이 5초 안에 끝나도록 한다.
실제 iPhone·Mac Safari 실기를 실행한 결과로 확대해 해석하지 않는다.

최종 관련 검사 결과는 작업 보고의 검증 요약에 기록한다. 착지 gate는 별도 승인 뒤 최종 통합 SHA에서 수행한다.

## 참고한 공개 구현

- [React 상태 보존 규칙](https://react.dev/learn/preserving-and-resetting-state),
  [MIT 라이선스](https://github.com/facebook/react/blob/main/LICENSE):
  key 변경이 폼을 재생성하므로 실제 질문 경계에만 key를 바꾸는 접근을 따른다.
- [Zulip 초안 저장](https://github.com/zulip/zulip/blob/main/web/src/drafts.ts),
  [Apache-2.0 라이선스](https://github.com/zulip/zulip/blob/main/LICENSE):
  편집기 수명과 분리한 브라우저 초안 저장·복원 접근을 참고했다.

외부 코드를 복사하거나 의존성을 추가하지 않았다.
