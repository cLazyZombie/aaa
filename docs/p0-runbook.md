# P0 절차서: 불변 launcher 등록 한 번(결정 14)

기획서 [§10.6](plan.md#106-p0-실험)의 P0를 사람이 실제로 돌리는 순서다. 결정 14에 따라 범위를 줄였다.

- 재부팅 실험은 하지 않는다. 시험 Mac도 쓰지 않는다.
- 개발자 도구에는 **한 번 만들고 다시 빌드하지 않는 작은 launcher**(`aa-launcher.app`)를 등록한다. LaunchAgent나 LaunchServices가 launcher를 띄우고, launcher가 그때의 `aa`를 자식으로 띄운다. `aa`는 자유롭게 다시 빌드·교체한다.
- 자체 서명 인증서(결정 9)는 보류한다. launcher 방식이 실측에서 실패할 때만 다시 꺼낸다(아래 「보류」).
- P0 세부 실측보다 M1-2~M1-4 완성이 먼저다.

문서의 머신 이름은 일반 이름이다. mac-a는 화면 접속이 되는 Mac, mac-b는 coordinator Mac, mac-c는 구현 Mac이다. 결정 10에 따라 화면이 필요한 일은 mac-a에서 하고, 명령만이면 ssh로 한다.

- 도구: `p0/launcher/`(launcher 소스·설치·측정), `aa doctor probe`
- 결과 위치: 실행한 Mac의 `~/aa-p0/results/`
- 판정은 도구가 한다. 첫 실행 추가 지연 median ≤10ms, p95 ≤20ms, 100ms 초과 0, 전체 디스크 접근 `ok`, 책임 프로세스 `aa-launcher`

## launcher가 하는 일

`p0/launcher/aa-launcher.c`(C, 의존 없음)

- `aa-launcher <aa 실행 파일 절대 경로> [인자...]`를 받아 자식을 `posix_spawn`으로 띄우고 끝날 때까지 기다린다. exec는 하지 않는다.
- SIGTERM·SIGINT·SIGHUP·SIGQUIT·SIGUSR1·SIGUSR2는 자식에게 전달한다. 자식의 종료 코드를 그대로 돌려준다(신호로 끝나면 128+신호).
- 일부러 기능을 더하지 않는다. launcher가 바뀌면 등록을 다시 해야 한다.

`p0/launcher/install.sh`

- launcher를 빌드해 최소 `.app`(`dev.aa.launcher`, adhoc 서명, `LSUIElement`)으로 감싼다. 그리고 `~/Applications/aa-launcher.app`에 quarantine 없이 둔다.
- cdhash를 `~/Applications/aa-launcher.app.txt`에 적는다. **이미 있으면 바꾸지 않는다**(`--replace`를 줄 때만 바꾼다).
- 같은 소스로 다시 만들면 cdhash가 같다(mac-c 실측). 시스템 설정은 바꾸지 않는다.

## mac-c에서 이미 확인한 것(등록 없이, 음성 대조)

등록하지 않은 launcher를 LaunchServices로 띄워 자식 `aa`가 쟀다(5표본).

| 책임 프로세스 | 첫 실행 추가 지연 median | warm median | FDA |
|---|---:|---:|---|
| `aa-launcher` | 220.294ms | 5.369ms | denied |

- 자식이 launcher에 귀속된다(책임 프로세스가 `aa-launcher`).
- 미등록이면 느리고 FDA가 없다. 등록 뒤에 이 값이 기준 안으로 들어오면 합격이다.

## 사용자 작업: launcher 등록 한 번 (mac-a 화면에서)

```bash
# mac-a 터미널(화면이 있는 세션). 저장소는 thread가 ~/aa-dev/aa에 준비한다.
bash ~/aa-dev/aa/p0/launcher/install.sh
```

1. 위 명령이 `~/Applications/aa-launcher.app`을 만든다. 키체인·시스템 설정은 바꾸지 않는다.
2. **시스템 설정 > 개인정보 보호 및 보안 > 개발자 도구**에서 「+」를 누르고, `~/Applications/aa-launcher.app`(홈 폴더 > 응용 프로그램)을 넣은 뒤 켠다. 변경할 때 비밀번호(또는 Touch ID) 창이 뜬다.
3. **시스템 설정 > 개인정보 보호 및 보안 > 전체 디스크 접근 권한**에서 같은 앱을 넣고 켠다.
4. 끝나면 thread에 알린다.

목록이 `.app`을 받지 않으면 `~/Applications/aa-launcher.app/Contents/MacOS/aa-launcher`(실행 파일)를 넣어 본다. 그것도 안 되면 무슨 창이었는지 알려 준다.

## thread가 하는 확인(ssh, 재부팅·재등록 없음)

```bash
# 1) 지금 aa로 측정
bash ~/aa-dev/aa/p0/launcher/measure.sh before --aa ~/aa-dev/bin/aa
# 2) aa를 다시 빌드(AA_BUILD_SALT)해 같은 경로에 rename으로 교체 → cdhash가 바뀐다
# 3) 재등록 없이 다시 측정
bash ~/aa-dev/aa/p0/launcher/measure.sh after --aa ~/aa-dev/bin/aa
```

- `measure.sh`는 launcher의 cdhash가 설치 때와 같은지 먼저 확인한다. 다르면 측정하지 않는다.
- launcher를 `open -n -W`(LaunchServices)로 띄우고, 자식 `aa doctor probe --samples 30 --fda-check`가 잰다. 결과는 `~/aa-p0/results/launcher-<단계>.json`과 요약 표다.
- LaunchAgent → launcher → `aa` 기동(E2)은 설치 작업(M1-4)에서 같은 launcher로 확인한다. 이 단계에서는 LaunchAgent를 등록하지 않는다.

## 보류(결정 14): 서명 identity 실험

launcher 방식(L1)이 실패할 때만 다시 꺼낸다. 도구는 저장소에 그대로 둔다.

- `p0/cert/make-selfsigned.sh`, `p0/cert/import-identity.sh`: 자체 서명 코드 서명 인증서(결정 9)를 만들고 키체인에 넣는다
- `p0/e1.sh`, `p0/build-app.sh`, `p0/install-app.sh`, `p0/measure.sh`, `p0/upgrade-sim.sh`: 서명한 `aa.app` 세 개(자체 서명·Apple Development·adhoc)의 E1·E2 측정

## 다음 실험

| 실험 | 필요 조건 | 상태 |
|---|---|---|
| L1 launcher 등록 유지 | 사용자의 launcher 등록 한 번 | 등록 대기 |
| E2 기동 방식·처리량 | LaunchAgent → launcher → `aa` | 설치 작업(M1-4)과 함께 |
| E3 책임 프로세스 수명 | `aa agent`가 LaunchAgent로 돌아야 한다 | M1-4와 함께 |
| E4 기동 경합 | host·agent·웹 | M1-4와 함께 |
| ~~E5 재부팅~~ | — | 하지 않는다(결정 14) |
| E6 하위 daemon | `aa doctor lineage`로 codex app-server·kache 계보를 본다 | thread가 진행 가능 |
| E7 업그레이드 부수 효과 | L1 교체 뒤 quarantine·Gatekeeper·재질문, hook 신뢰는 M1-3 | L1과 함께 |
