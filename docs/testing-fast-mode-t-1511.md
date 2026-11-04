# thread fast mode 표시 검증(t-1511)

## 근거 확인

- Claude Code [statusline 문서](https://code.claude.com/docs/en/statusline): `fast_mode`는 「Whether fast mode is enabled for the session」, 예시 `"fast_mode": false`. [hooks 문서](https://code.claude.com/docs/en/hooks) 공통 입력에는 fast 칸이 없다.
- Claude Code 2.1.318 실행 파일: 상태줄 입력을 만드는 함수가 `fast_mode`를 `effort` 앞에 싣고, 상태줄 재실행 조건 목록에 `fastMode`가 있다.
- Codex 0.166.0: `TurnContextItem` 18칸에 service tier가 없고 최근 rollout도 같았다. 제목 항목 `fast-mode`를 pty로 띄운 TUI에서 읽었다(글은 보내지 않음): 기본 `Ready | Standard | cx`, `-c service_tier="fast"`·`"priority"`는 `Ready | Fast | cx`. `Ultrafast`는 표시 문자열로만 확인했고 켜는 설정은 찾지 못했다.

## 자동 검증

- `cargo test -p aaa-agent --lib model_observation`: 새 `model_observation_takes_speed_from_claude_statusline_and_codex_title`가 상태줄 켜짐·꺼짐, 같은 값의 시각 유지, hook·하위 agent·다른 harness 세션 입력 무시, 프로세스 시작에서 비우기, Codex 제목 `Fast`·`Standard`·`Ultrafast`와 말하지 않는 제목의 앞 값 유지를 본다. `Fast` 대응과 관측에 붙이기를 일부러 틀리게 하면 실패하는 것을 확인했다. 기존 모델 관측 시험 3개와 함께 통과.
- `node --test web/test/summaryCard.test.mjs`: 새 시험이 fast·ultrafast만 보이고 표준·모르는 값·칸 없음(옛 agent)을 숨기는지, 모델 줄 글이 그대로인지 본다.
- Codex 실행·재개 argv 시험(`codex_spawn_spec_stays_in_the_pane`·`codex_launch_and_resume_preserve_user_notify`)의 제목 설정을 새 값으로 바꿨다.

## 실제 경로 확인(일회)

`web/e2e/showcase.mjs` 실험실(가짜 host·agent 둘)에 실행 파일 이름이 `codex`인 가짜 harness를 thread로 띄웠다. 이 가짜는 SessionStart hook으로 rollout 위치를 알리고, `turn_context` 한 줄을 쓰고, OSC 제목을 `Ready | shop | Fast`로 둔다. 결과는 다음과 같다.

- `aaa thread show`의 `agent.model_observation`이 `{"model":"gpt-6.1-sol","effort":"medium","speed":"fast"}`가 되었다.
- 웹 PC·휴대폰(어두움·밝음)에서 그 thread 줄에만 번개 표지가 보였고, hover 카드 모델 줄이 「최근 확인 모델: sol medium · fast」였다.
- 제목을 `Standard`로 바꾸면 관측의 speed와 표지가 사라졌고, 다시 `Fast`로 돌리면 돌아왔다.

시험 파일로 남기지 않은 까닭: 경로의 host 부분은 관측을 통째로 옮기기만 하고, 읽기와 표시 규칙은 위 단위 시험이 지킨다.

## 남은 실기 확인

배포 뒤 Codex thread에서 /fast를 켜고 끌 때와, 계정 Claude thread에서 /fast를 켜고 끌 때 표지가 따라 바뀌는지 운영에서 본다.
