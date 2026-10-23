# Windows agent의 배포와 복구

`aaa setup`이 설치한 Windows SCM agent를 macOS host와 같은 rollout으로 갱신한다.
상태 이름과 처리 순서는 기존과 같다. 전송·전체 사전 검사 뒤 host를 먼저 교체하고,
각 agent를 차례로 교체·재시작한다. 실패한 대상만 배포 직전 SHA로 복구한다.

## 최초 반영

독립 복구 감시는 배포 전 실행 파일의 사본으로 실행된다. 처음 한 번은 이 기능이 든
Windows 실행 파일을 `aaa setup`으로 반영해야 한다. 그 뒤부터 rollout을 사용할 수 있다.
Windows agent가 `rollout.v1`, `deploy_stage.v1`, `rollout_recovery.v1`,
`windows_scm_rollout.v1`을 알려야 한다. host와 CLI는 FRONT_API 1.114 이상이어야 한다.

대상 Windows 서비스는 기본 `aaa-agent`, 별도 instance는 `aaa-<이름>-agent`다.
이름이 이미 `aaa-`로 시작하면 접두사를 중복하지 않는다. 서비스 명령의 실행 파일·instance·
설정·상태 경로가 배포 대상과 일치해야 한다. 서비스 계정에는 해당 서비스의 조회·중지·시작 권한과
설치 폴더의 쓰기 권한이 있어야 한다. rollout은 서비스 계정·암호·시작 방식·실패 정책을 변경하지 않는다.

## 플랫폼별 후보

기본 `--file`·`--sha256`은 host 플랫폼 후보다. 같은 커밋에서 빌드한 다른 플랫폼 후보를
JSON 배열에 적고 `--artifacts`로 함께 준다. 추가 후보는 OS·아키텍처 조합마다 하나다.
모든 후보의 `--version`은 동일하고 `+<커밋 앞 12자리>`로 끝나야 한다.

```json
[
  {
    "os": "windows",
    "arch": "x86_64",
    "source_machine": "mac-b",
    "from": "/opt/aaa-candidate/windows/aaa.exe",
    "sha256": "<Windows 실행 파일의 SHA-256>"
  }
]
```

```sh
aaa deploy start --file mac-b:/opt/aaa-candidate/macos/aaa \
  --sha256 '<macOS 실행 파일의 SHA-256>' --commit '<소스 커밋 40자리>' \
  --artifacts /opt/aaa-candidate/artifacts.json --machines mac-a,win-a
```

`source_machine`은 빌드 후보를 보관한 머신이다. 예를 들어 macOS 머신에 Windows exe를 둘 수 있다.
각 후보 이름은 `aaa` 또는 `aaa.exe`이고 옆에 `provenance.json`이 있어야 한다.
기존 전송의 provenance·symlink·SHA 검사를 그대로 수행하며, 실행 검사는 실제 대상 머신이 한다.
Windows 저장 경로는 JSON의 `"C:\\candidate\\aaa.exe"`와 같은 절대 경로도 받는다.

host는 시작 때 hello의 OS·아키텍처와 후보를 맞춰 대상별 단계에 고정한다. 다른 플랫폼의 후보가
빠졌으면 교체 전에 시작을 거절한다. 재연결·host 재시작 후에도 기록한 후보와 작업 ID를 사용한다.
추가 후보가 없는 기존 단일 플랫폼 입력도 계속 지원한다. 웹의 단일 후보 화면에서 다른 플랫폼을
고르면 CLI의 `--artifacts` 입력이 필요하다고 표시한다.

## 교체·서비스 수명

Windows는 실행 중인 파일을 제자리에서 덮어쓰지 않는다. 새 파일의 self-check와 SHA 검사를 끝낸 뒤
기존 exe를 옆 `.aaa-old/`로 rename하고 새 파일을 원래 이름에 둔다. 두 번째 rename이 실패하면
기존 파일 이름을 복원한다. 서비스 등록의 실행 경로는 유지한다.

SCM 중지·시작은 agent 밖에서 살아 있는 기존 독립 감시가 수행한다. agent는 재시작 작업을 먼저
Running으로 기록하고 감시를 활성화한다. 감시는 서비스 중지 완료와 새 SCM PID를 확인하며,
다시 연결한 agent의 새 instance·실행 SHA·판을 대조한다. 중지 도중 연결이 끊겨도 같은 작업을
다시 시작하지 않는다. 승인된 복구 사본은 일반 보관 정리와 별개로 유지한다.

새 서비스 시작 실패·다른 실행 SHA·기존 감시 시한 초과는 같은 감시의 복구 흐름으로 간다.
배포 전 SHA를 복원하고 서비스를 다시 시작한 뒤 실행 신원이 맞아야 `stopped_rolled_back`이다.
복구 자체가 실패하면 `recovery_failed`다. host가 잠시 내려가도 복구 실행기는 남으며,
host가 돌아오면 기존 내구 기록과 같은 rollout ID로 상태를 이어 확인한다.

```sh
aaa deploy status '<rollout ID>' --wait
```

사람 터미널 또는 허용된 coordinator에서 조회한다. 끊겼다고 `deploy start`를 다시 보내지 않는다.
이 기능은 Windows agent 서비스에 적용하며, Windows host와 Linux 서비스 감독자는 포함하지 않는다.

## 참고한 구현

- [rustup Windows updater](https://github.com/rust-lang/rustup/blob/main/src/cli/self_update/windows.rs),
  [MIT](https://github.com/rust-lang/rustup/blob/main/LICENSE-MIT): 실행 이미지와 별도 갱신 프로세스 수명.
- [self-replace Windows](https://github.com/mitsuhiko/self-replace/blob/main/src/windows.rs),
  [Apache-2.0](https://github.com/mitsuhiko/self-replace/blob/main/LICENSE): 기존 파일 rename 후 새 사본 설치.
- [Tailscale Windows updater](https://github.com/tailscale/tailscale/blob/main/clientupdate/clientupdate_windows.go),
  [BSD-3-Clause](https://github.com/tailscale/tailscale/blob/main/LICENSE): 서비스가 끝나도 갱신용 사본으로 진행.

외부 코드·의존성을 추가하지 않고 기존 updater·SCM·독립 감시를 재사용했다.
