# README 매체 재생성

README는 실제 host·agent 둘을 띄운 격리 실험실을 촬영한다. agent의 대화와 작업 결과만 가짜이며 웹 UI·질문 판정·등록·보고 수집·auto mode 전환·자동화 조회는 실제 구현을 사용한다. 운영 앱을 촬영하거나 화면의 텍스트를 사후 교체하지 않는다.

## 실행

웹과 Rust 실행 파일을 먼저 빌드하고 Playwright 1.64.1의 Chromium, ffmpeg를 준비한다. 자세한 설치는 [README 개발](../README.md#개발)을 따른다.

```sh
NODE_PATH=$(npm root -g) node web/e2e/showcase.mjs --readme docs/media
```

세 번째 경로를 주면 그 폴더에 H.264 MP4도 만든다. README에는 작은 GIF만 넣는다. `AAA_BIN`으로 자기 작업 트리에서 만든 실행 파일을 지정할 수 있다.

| 파일 | 장면 |
| --- | --- |
| `desktop.png` | 작업 목록과 웹 터미널 |
| `question.png` | 가짜 agent의 확인창을 읽은 질문 카드 |
| `report.png` | thread의 보고와 다음 행동 |
| `auto-mode.png` | coordinator 대화와 auto mode 입력창 |
| `automation.png` | 꺼진 견본 일정 규칙의 조건·대상·승인·기록 |
| `workspaces.png` | 머신 정보의 작업 폴더 자동 배정 |
| `phone.png` | 390px 휴대폰 작업 목록 |
| `flow.gif` | 입력 → thread 작업 → 보고의 네 장면 |

## 격리와 확인

- 임시 폴더에 host·agent 설정, 가짜 HOME·사용량·셸 환경을 만든다. 부모의 `AAA_*`·`CODEX_*`·`CLAUDE_*`·`XDG_*` 문맥은 전달하지 않는다. 머신 이름은 `mac-a`·`mac-b`, 프로젝트는 `shop`이다.
- 외부 agent를 실행하지 않는다. 가짜 Claude의 판 조회도 즉시 견본 값으로 응답한다. 실제 모델 계정·사용량을 읽지 않는다.
- 페이지·보고·출력 사건이 완료된 뒤 촬영한다. GIF의 각 장면 길이는 ffmpeg concat의 duration으로 주며 촬영 준비를 고정 sleep으로 기다리지 않는다. 가짜 작업의 완료는 FIFO로 제어한다.
- 성공하면 자기 자식과 실험실 session-host PID만 종료하고 임시 폴더를 지운다. 실패하면 진단용 로그와 실패 화면을 남긴다. 이 원본은 실행 파일 경로와 임시 인증 자료가 들어 있으므로 커밋하거나 보고 library에 복사하지 않는다.
- 매체는 파일마다 1 MiB 아래여야 한다. `scripts/check-public.sh`와 README의 상대 링크 검사 외에 이미지·GIF 모든 장면을 열어 글자를 확인한다. OCR은 보조 수단이며 원본 이미지 확인을 대신하지 않는다.

## 구성 참고

아래 고정판의 README와 LICENSE를 확인하고 구성만 참고했다. 문구·그림·코드를 복사하지 않았다.

| 원본 | 라이선스 | 참고한 구성 |
| --- | --- | --- |
| [herdr v0.9.5](https://github.com/herdrdev/herdr/tree/v0.9.5) | [Apache-2.0](https://github.com/herdrdev/herdr/blob/v0.9.5/LICENSE) | 첫 화면의 짧은 소개·기능·설치·상세 문서 순서 |
| [herdr-web-ui aa624cb](https://github.com/devswha/herdr-web-ui/tree/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741) | [MIT](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/LICENSE) | 기능별 매체와 캡션, 휴대폰 흐름 |
| [herdr-projects v0.2.35](https://github.com/eliasstravik/herdr-projects/tree/v0.2.35) | [MIT](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/LICENSE) | coordinator에 맡기기 → 진행 → 질문·보고의 설명 순서 |
