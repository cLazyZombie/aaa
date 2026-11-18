# Codex thread의 번개 표시 검증

t-2008과 t-2009의 pane은 둘 다 `service_tier="ultrafast"`로 시작했고 상태줄에 Ultrafast가 있었다. 웹 목록은 `model_observation.speed`에만 의존했다. 대화 기록에서 모델을 확인한 t-2008에만 그 필드가 있어 t-2009에서 번개가 빠졌다.

host는 실제 시작 인자에서 초기 속도를 정한다. agent의 OSC 제목 관측은 모델과 별개인 사건으로 전달한다. host는 명시적으로 관측한 속도를 적용하고 thread 사건을 내보낸다. 이미 열린 목록도 Standard 전환과 Ultrafast 복귀를 따라간다.

- agent 유닛은 OSC 제목의 첫 Standard, Fast·Ultrafast 변경과 중복·무관한 제목을 확인한다.
- host 유닛은 마지막 시작 옵션의 우선순위, `--` 경계, 모델 기록 없는 Standard·Fast 관측, 늦게 도착한 관측과 종료한 pane을 확인한다.
- 웹 E2E 141은 같은 profile에서 제목 출력 시점이 다른 두 pane에 번개가 뜨고, 한 pane만 Standard로 전환했다가 Ultrafast로 돌아올 때 열린 목록이 따라오는지 검사한다. 실제 host·agent·Chromium과 대화 기록을 만들지 않는 Codex 대역을 쓴다.

첫 E2E에서 agent가 Ultrafast 복귀를 기록했으나 웹 목록에서 번개가 돌아오지 않았다. pane 상태 갱신만 보내고 thread 사건을 빠뜨린 것이 원인이었다. 기존 `thread_changed` 경로로 연결했다. 시험 시한과 단언은 그대로 뒀다.

수정 뒤 E2E 141과 agent·host 유닛이 통과했다. 웹 타입·번들과 유닛 746개, 프로토콜 생성 검사, crate 의존 경계 2개, macOS·Linux·Windows strict Clippy도 통과했다.
