# 아이디어 그림·새 글·분할 조절 검증(t-1412)

아이디어 노트는 내용 해시 파일만 표시했지만 전용 thread는 작업 폴더의 `library/…` 주소를 넣을 수 있었다. 저장 시 자기 library의 미디어를 agent의 기존 파일 읽기로 받아 아이디어 폴더에 보존하고, 링크와 Markdown 그림의 주소를 함께 바꾼다. 기존 노트는 열 때 주소만 이행한다. 판·갱신 시각·알림은 유지하며, 원본이 없으면 원문과 읽기 실패 원인을 보인다. 16MiB 상한·미디어 signature 검사·기존 파일 보존을 유지한다.

새 글은 host `seen_rev`와 thread 판 기록으로 판정한다. 사용자 생성·직접 고침·중복 저장은 새 알림을 만들지 않는다. 실제 노트를 표시한 판만 확인하므로 다른 화면·휴대폰 대화·편집 중 도착한 판은 남는다. thread가 읽음 요청으로 사람의 배지를 지우지 못한다. 웹 새 글 숫자와 앱 안 알림, 기존 Web Push·webhook 전달 경로를 연결했다. 외부 알림을 모으는 동안 읽었거나 새 판으로 대체된 것은 보내지 않는다.

분할 경계는 pointer capture로 끌고, 키보드 ←·→·Home·End와 두 번 누르기로 조절한다. 휴대폰은 기존 노트·대화 탭을 유지한다.

## 관련 owner와 필요한 시험

| owner | 지키는 계약 |
| --- | --- |
| host `ideas::tests::thread_updates_stay_unseen_until_the_displayed_revision_is_seen` | host 읽음 보존·옛 판 확인 경합·중복 저장·사용자 저장·푸시 목적지·기기 완료 알림 선택 |
| host `ideas::tests::media_import_is_scoped_to_the_threads_library` | 자기 library 경계·상위/숨김/외부/비미디어 경로 거부·코드 예시 제외 |
| host `idea_thread::tests::only_the_ideas_own_thread_saves_from_a_session` | 자기 노트 저장 권한과 thread의 사람 읽음 변경 금지, 보기 기기의 읽음 처리 |
| 웹 E2E 120 | 실제 전용 thread 저장→미디어 복사→암호화 푸시→새로 열어도 배지 보존→기존 노트 이행·실제 그림 decode·링크 목적지→원본 제거 뒤 읽기→드래그·키보드→WebKit 휴대폰 읽음 |
| 웹 E2E 115 | 보고·질문 카드의 기존 빈 alt 그림·영상 렌더링 유지 |

개별 추가 단계는 사건·DOM·파일 읽기·복호화 완료로 끝난다. 정한 시간을 자거나 실제 사용자 알림·clipboard·소리를 쓰지 않는다.

## 개발 검증

- `aaa verify`로 관련 host·protocol crate 시험, strict Clippy, 웹 타입·번들·유닛 전체와 E2E 115·120, 공개 검사를 실행한다.
- 첫 관련 crate 시험은 통과했다. 초기 컴파일의 응답 variant·알림 초기화 누락과 strict Clippy의 시험 불필요 clone은 원인을 고쳤다. 시한·기대값·재시도 설정은 바꾸지 않았다.
- 첫 웹 검사: 유닛 675개 통과, E2E 115·120 통과. E2E 120의 개별 단계는 모두 5초 미만이었다.
- 추가한 원본 누락 E2E가 오류 배너 부재로 실패했다. `idea_read`의 이행 오류를 `idea_wake`의 일반 노트 응답이 덮은 것이 원인이었다. 깨우기·말하기 응답에도 같은 미디어 읽기를 적용했고, 깨우기 응답의 원본 경로·오류와 화면 배너를 함께 단언했다. 시한이나 기대를 완화하지 않았다.
- 추가 시험에서 SPA 이동 직후 reload하면서 이전 문서의 깨우기 응답을 기다리는 경합도 드러났다. 새 문서 진입 뒤 직접 호출한 깨우기 응답과 DOM을 함께 확인하도록 고쳤다. 최종 원본 누락 단계는 213ms에 통과했다.
- 보완한 E2E 120과 관련 host 시험은 통과했다. `aaa` 의존 경계 시험 2개, 읽기 전용 fmt, host·protocol strict Clippy도 통과했다.
- 개발 검사이며 최종 통합 SHA의 전체 gate는 착지 창에서 수행한다. 병합·push·배포는 이 작업에서 실행하지 않았다.

## 참고 구현

- [markdown-it](https://github.com/markdown-it/markdown-it) ([MIT](https://github.com/markdown-it/markdown-it/blob/master/LICENSE)): 일반 링크와 이미지의 대상 해석을 일관되게 두는 구성을 참고했다. 기존 Markdown 렌더러를 유지하고 host에서 주소를 정규화했다.
- [react-resizable-panels](https://github.com/bvaughn/react-resizable-panels) ([MIT](https://github.com/bvaughn/react-resizable-panels/blob/main/LICENSE.md)): separator의 키보드 접근과 pointer 취소 처리를 비교했다. 저장소 `SidebarResizer` 패턴을 재사용했다.

외부 코드를 복사하거나 새 제품 의존성을 추가하지 않았다.
