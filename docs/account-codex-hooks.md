# 계정 Codex의 aaa hook

계정으로 실행하는 Codex는 `account-runs/<project>/<thread>/codex-home`만 쓴다.
세션 시작 전에 session host가 이 홈의 `hooks.json`에 `SessionStart`·`UserPromptSubmit`·
`PostToolUse`를 쓰고 `config.toml`의 `[features] hooks = true`를 켠다.
사용자의 `~/.codex`나 다른 계정 실행 홈은 읽거나 변경하지 않는다.

전역 설치와 계정 실행의 명령·사건 생성은 `aaa-common::codex_hooks`를 공유한다.
Unix는 셸 인용, Windows는 명시적 cmd guard 안에서 기존 PowerShell UTF-16LE
EncodedCommand를 쓴다. Codex 0.164.0이 선택한 turn shell이 PowerShell이어도
cmd의 `if defined` 문법이 바뀌지 않는다.
세션 환경·token·peer 검증, 중첩 agent 제외, 진행 알림 간격은 기존 agent 경로가 맡는다.
`SessionStart`의 startup·resume·clear·compact·fork를 받으며, 기억을 쓰는 자동화의
재독 안내는 hook 파일에 기억을 복사하지 않고 저장된 실행 문맥에서 읽는다.

Codex 0.164.0에서는 기능 설정만 켜면 첫 실행이 hook 검토 화면에서 멈춘다.
aaa가 생성한 세 handler에 대해서만 정규 경로·내용 해시를 `[hooks.state]`에 기록한다.
명령이나 matcher가 달라지면 해시가 달라진다. 임의 사용자 hook을 신뢰하거나
`--dangerously-bypass-hook-trust`를 추가하지 않는다. 실행 파일 경로가 바뀐 재시작은
현재 명령·해시로 다시 쓰고 기존 대화 기록은 보존한다.
hook 쓰기 실패는 원인 사슬을 보존해 시작 실패로 보고하며 전달한 인증 파일을 지운다.

## 참고 후보와 선택

- [herdr 0.9.5의 실행 홈 선택](https://github.com/herdrdev/herdr/blob/v0.9.5/src/integration/env.rs)
  ([Apache-2.0](https://github.com/herdrdev/herdr/blob/v0.9.5/LICENSE)):
  harness별 홈 분리를 대조했다. aaa의 기존 계정 홈 수명을 유지한다.
- [herdr-projects 0.2.35 progress](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/src/progress.rs)
  ([MIT](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/LICENSE)):
  세 사건·compact 안내 원칙을 대조했다. 기존 aaa 진행 응답을 재사용한다.
- [Codex 공식 hook 문서](https://learn.chatgpt.com/docs/hooks),
  [0.164.0 신뢰 정규화](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/hooks/src/engine/discovery.rs),
  [정렬 JSON 해시](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/config/src/fingerprint.rs)
  ([Apache-2.0](https://github.com/openai/codex/blob/rust-v0.164.0/LICENSE)):
  생성한 handler의 신뢰 식별 형식을 맞췄다. 새 의존성·외부 소스 사본을 추가하지 않았다.

검증과 재현 명령은 [별도 검증 기록](testing-account-codex-hooks.md)에 둔다.
