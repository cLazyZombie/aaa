# 조건부 피드백 질문 감시 수리 검증

## 원인과 수리

질문 감시가 “필요하면 말씀해 주세요”, “지우려면 따로 말씀해 주세요”, 수정 완료 뒤
“아직 안 움직이는 경우가 있으면 말씀해 주세요”를 새 답 요청으로 분류했다.
기존 조건부 표현 목록에 없는 문구여서 일반 요청 어미에 걸린 것이다.

조건 표현과 짧은 피드백 요청이 직접 이어진 구절만 판정 입력에서 제외한다. 조건 표현만으로 문장 전체를
면제하지 않는다. 같은 문장 앞뒤의 별개 승인·선택·정보 요청과 다음 문장의 질문은 계속 검사한다.
뒤에 조건을 붙인 “말씀해 주세요, 필요하면”은 문장 시작의 일반 피드백에만 적용하며,
“승인 여부를 말씀해 주세요, 필요하면”은 실제 요청으로 남긴다.
질문 어미·A/B 선택지 판정과 q-ID 연결·접수 원장·승인 규칙은 바꾸지 않는다.

## 회귀 owner와 필요한 이유

새 `question_guard::tests::conditional_feedback_does_not_require_an_answer_or_hide_a_decision` 하나에서
위 세 오탐과 피드백 어순·어미 변형을 확인한다. 승인 요청, A/B 선택지, 조건이 섞인 실제 질문,
같은 문장 앞뒤와 다음 문장의 별개 요청, 원문 행 번호를 함께 단언한다.
이 양방향 검사는 조건 표현을 넓게 면제해 실제 결정을 놓치는 회귀를 막기 위해 필요하다.

기존 `question_guard`의 순수 판정·카드 연결·진단 시험과 `post::tests::question_guard_*`의
실제 host/DB·harness 완료 사건 시험을 함께 실행한다. 다른 coordinator의 post, 빠른 답 뒤 접수 증거,
닫힌 카드·다른 프로젝트 참조, 중복 알림·외부 푸시 경계도 기존 owner에서 확인한다.
순수 문구 판정만 바뀌므로 웹 E2E와 Windows 전용 실행 owner는 추가하지 않는다.
고정 sleep·재시도·시한 확대·기대값 완화는 없다.

모든 검사는 `aaa verify -- <명령>`으로 실행한다.

| 범위 | 명령 | 결과 |
| --- | --- | --- |
| 판정·host/DB 회귀 | `nice -n 15 cargo test --profile verify --locked -p aaa-host --lib question_guard` | 9개 통과 |
| Rust 형식 | `nice -n 15 cargo fmt --all -- --check` | 통과 |
| 관련 Rust owner lint | `nice -n 15 cargo clippy --profile verify --locked -p aaa-host --all-targets -- -D warnings` | 통과 |
| 공개 저장소 | `bash scripts/check-public.sh` | 통과 |

수정 전에는 새 시험이 첫 문구 “필요하면 말씀해 주세요”에서 실패하는 것을 재현했다.
최종 통합 SHA의 전체 gate·cross·production 확인은 승인받은 착지 창에서 별도로 실행한다.
개발 검사를 착지 gate 통과로 기록하지 않는다.

## 참고 구현과 라이선스

- [herdr v0.9.5 agent_detection.rs](https://github.com/herdrdev/herdr/blob/v0.9.5/src/pane/agent_detection.rs)의
  일반 상태와 visible blocker 신호 분리([Apache-2.0](https://github.com/herdrdev/herdr/blob/v0.9.5/LICENSE)).
- [herdr-projects v0.2.35 THREAD.md](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/skill/THREAD.md)의
  보고와 사용자 다음 행동 구분([MIT](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/LICENSE)).
- [herdr-web-ui prompt.ts 고정 판](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/server/prompt.ts)의
  질문·확인·계속하기 표지별 판정([MIT](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/LICENSE)).

일반 안내와 실제 사용자 입력 요청을 구별하는 구조를 대조했다. 위 파일의 명시적인 상태·표지 분리는 참고하되,
한국어 조건부 피드백의 판정식은 aaa의 기존 질문 감시에 맞춰 작성했다. 외부 코드·새 의존성은 추가하지 않았다.
자연어 전체의 의도를 보장하거나 운영 오탐률을 새로 측정한 결과는 아니다.
