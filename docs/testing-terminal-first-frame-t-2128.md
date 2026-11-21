# 첫 터미널 화면과 agent 색상 준비(t-2128)

pane WebSocket이 열리면 host 화면 사본을 보낸다. agent의 색상 응답이 아직 없으면 입력과 크기 변경을 보류 상태로 알리고, 전송 경합으로 일찍 온 요청에는 거절을 답한다. 화면·출력·ACK·이전 기록·기기 폐기도 그 사이에 처리된다. 색상 응답이나 기존 시한이 오면 현재 pane과 기기의 권한으로 입력을 연다.

- `aaa-host::colors` 시험은 같은 agent의 대기 중인 요청 공유, 성공한 색 재사용, 실패 뒤 새 요청, 다른 색의 전송 순서를 검사한다. 색상 요청 상태는 agent 연결마다 새로 생긴다.
- E2E97의 두 단계는 Chromium과 WebKit에서 실제 host·agent·PTY를 쓴다. 실험실 agent를 정지하고 대상 WebSocket의 open과 xterm의 실제 글을 확인한다. 이때 입력 준비 표시, 앞서 들어온 크기와 제출의 응답, 기존 PTY 크기를 확인한다. agent를 다시 진행시키고 첫 키가 일으킨 OSC 10/11 응답이 웹이 지정한 두 색인지 검사한다. 고정 시간 대신 화면·WebSocket·PTY 사건을 기다린다.
- 인증한 조작 기기에서 앱 주소에 `?trace=attach`를 붙이면 브라우저 Console과 host 로그에 이 화면의 단계가 찍힌다. 두 기록을 `front` 번호로 짝짓는다. 화면 내용과 키 입력은 수집하지 않는다.

이미 열린 화면 자료와 조작 준비를 따로 전달하는 방식을 참고했다. [herdr](https://github.com/herdrdev/herdr/blob/v0.9.5/src/server/headless.rs#L1737) · [herdr-web-ui](https://github.com/devswha/ovis/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/server/index.ts#L2963)
