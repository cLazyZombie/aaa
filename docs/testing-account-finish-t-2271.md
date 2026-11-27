# 계정 격리 thread의 finish 검증

## 검사 범위

- m13의 가짜 Claude·Codex는 실제 host → agent → session host 실행을 거친다. 자식이 격리 홈에서 스킬과 부속 파일을 읽고,
  모든 파일에 쓰기 권한이 없는지 확인한다. Claude 리뷰어와 Codex 스킬 안의 `agents/openai.yaml`을 각각 검사한다.
  나머지 스킬·리뷰어·설정은 원본 홈에만 둔다.
- 원본을 고친 뒤 같은 thread를 `--resume`으로 띄우면 바뀐 파일이 보이고, 지워진 부속 파일은 사라진다.
  원본을 치운 뒤에는 누락 경고를 기록하고 사본 없이 시작한다. 원본 파일의 권한과 바이트는 직접 대조한다.
- 세션 유닛 테스트는 읽기 전용 사본 교체, 중단된 준비 폴더 정리, 원본 삭제, 기존 사용자 항목 보존을 검사한다.
  Unix는 원본 파일 링크와 격리 홈의 상위 폴더 링크도 검사한다.
- macOS에서 실제 Claude Code 2.1.332의 `/finish` 자동 완성과 `@` 후보의 `finish-reviewer`, Codex 0.170.0의 `$finish`
  후보를 확인했다. aaa 세션 호스트를 가짜 인증으로 띄우고 agent를 붙였다. 임시 설정 홈을 사용하고 외부 네트워크를 차단했다.
- Windows 11 x64에서 해시를 대조한 세션 시험 exe로 finish 유닛 테스트 2개를 실행했다. 격리 시험 폴더를 지우고
  서비스·예약 작업·Run 키·사용자 PATH·방화벽 목록이 시험 전 기준과 같은지 확인했다.
- `cargo fmt --all --check`와 macOS·Linux의 `aaa-session`·`aaa`, Windows의 `aaa-session` strict Clippy를 통과했다.

```sh
aaa verify -- nice -n 15 cargo test --profile verify --locked -p aaa-session
aaa verify -- nice -n 15 cargo test --profile verify --locked -p aaa --test m13 accounts::
aaa verify -- nice -n 15 cargo test --profile verify --locked -p aaa --test deps
```

## 참고한 공개 구현

- GNU `install`은 복사한 파일에 대상 권한을 별도로 지정한다. 원본 권한과 대상 권한을 분리하는 방식을 참고했다.
  [구현](https://github.com/coreutils/coreutils/blob/master/src/install.c) · [GPL-3.0-or-later](https://github.com/coreutils/coreutils/blob/master/COPYING)
- chezmoi는 관리하는 원본 항목과 설치한 대상 상태를 구분하고, 새 사용자 전용 파일에 대상 권한을 적용한다. 해당 경로만 관리하는 범위를 참고했다.
  [파일 속성](https://www.chezmoi.io/reference/source-state-attributes/) · [MIT](https://github.com/twpayne/chezmoi/blob/master/LICENSE)

추가 런타임 의존성 없이 `std::fs`와 기존 사용자 전용 파일 도구를 사용했다.
