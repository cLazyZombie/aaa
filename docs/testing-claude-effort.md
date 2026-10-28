# Claude thread 생각 수준 표시 검증(t-1264)

## 공식 경로 확인

- [hooks 문서](https://code.claude.com/docs/en/hooks) 공통 입력: `effort.level`, 도구 문맥 사건(PreToolUse·PostToolUse·Stop·SubagentStop)에만.
- [statusline 문서](https://code.claude.com/docs/en/statusline): `effort.level`, `/effort` 변경 반영, 미지원 모델이면 없음.
- [model-config 문서](https://code.claude.com/docs/en/model-config): 수준 결정 순서(환경 변수·`--effort`·`/effort` → 설정 → 모델 기본)와 `/effort status`.
- 실측(Claude Code 2.1.312): 격리 설정 hook으로 받은 SessionStart·UserPromptSubmit 입력에 `effort`가 없었다(문서와 같음). 계정 홈 격리로 로그인이 없어 PostToolUse까지는 가지 못했다. aaa thread pane의 Bash 환경에는 `CLAUDE_EFFORT=high`(실행 인자 `--effort high`)가 있었다.

## 자동 검증

- `nice -n 15 cargo test -p aaa-agent --lib model_observation --locked`: `model_observation_takes_claude_effort_from_documented_hook_input`가 PostToolUse·상태줄 꼴 입력의 수준 기억, 같은 값의 시각 유지, 하위 agent·다른 harness 세션 제외, 모델 관측에 붙이기(늦은 시각), 미지원 모델의 칸 없음으로 비우기, 프로세스 시작에서 비우기를 본다. 기존 Codex·Claude 모델 관측 시험과 함께 3개 통과.
- `bash scripts/ci-local.sh`: web·fmt·build·clippy·test·public 통과(test 107초). e2e는 전역 Playwright 1.58.2라 판 검사에서 멈췄고, 작업 폴더에만 둔 Playwright 1.64.1(브라우저는 공용 캐시 chromium-1248·webkit-2370)로 `--only web,build,e2e`를 다시 돌려 통과(237초).
- `bash scripts/ci-local.sh --only linux-clippy`: 통과.

## 남은 실기 확인

배포 뒤 Claude thread의 요약 카드에 「최근 확인 모델: opus 5.5 high」처럼 보이는지, `/effort` 변경 뒤 다음 도구 호출(또는 상태줄 갱신)에서 바뀌는지 운영에서 본다.
