# 터미널 snapshot·history 전송 압축

승인된 t-0868 설계의 2단계다. 네트워크의 큰 기록만 줄이며 화면·기록 보관량과 원시 출력 offset은 유지한다.

| 구간 | 적용 |
| --- | --- |
| 웹 pane WebSocket | Worker에서 gzip 지원을 확인하고 `compression=gzip.v1`을 제안한 연결의 snapshot/history에 gzip 1 |
| 원격 agent link | Hello·Welcome의 `terminal_payload.zstd.v1` 교집합이 있는 TLS 연결의 snapshot/history에 zstd 1 |
| 로컬 host↔agent, agent↔session-host IPC | 원시 형식. IPC 수신에서 압축 metadata를 거부 |
| 키·ACK·상태·live·ring catch-up | 기존 형식 |

AGENT_LINK 1.11, FRONT_API 1.113, SESSION_IPC 1.3의 선택 기능이다. minor 판정으로 압축을 켜지 않으며 N−1 수용 범위도 유지한다. 모르는 query를 버리는 구 host, query가 없는 구 웹, 기능을 알리지 않는 구 agent/host에는 원시 bytes를 보낸다. 새 agent는 구 session-host의 원시 응답도 그대로 받아 원격 writer에서만 압축한다.

`Snapshot.replay`, `HistoryChunk.data`, 웹 `snapshot.replay`/`history.data`는 계속 base64다. 압축한 경우에만 같은 객체에 `compression:{codec:"gzip"|"zstd",raw_len:정수}`를 넣는다. 원본 길이는 base64 이전 replay/data 바이트 수다. 필드 부재가 identity이며 명시 null·잘못된 codec/길이는 오류다.

원시 본문 4,096 B 이상만 시도한다. metadata를 포함한 최종 JSON이 원시 JSON보다 12.5% 이상이면서 256 B 이상 작을 때만 사용한다. 원시 JSON 자체가 종류 byte 포함 기존 1 MiB link frame에 들어야 한다. 압축으로 원래 보낼 수 없던 메시지를 보내지 않는다. 압축/원시 본문 모두 768 KiB 이하이며 브라우저 텍스트 메시지도 UTF-8 1 MiB 이하로 검사한다. 화면 크기·색이 극단적으로 커 기존 원시 frame에 들어가지 않는 replay는 압축으로 우회하지 않는다.

zstd는 dictionary·skippable/multiple frame·남은 데이터·잘림을 거부하고 window 1 MiB와 정확한 출력 공간을 적용한다. gzip은 독립 stream과 CRC를 검증한다. 웹 Worker는 압축 입력을 최대 1 KiB씩 공급하고 raw_len 크기의 출력 공간에만 복사한다. 초과·오류 때 reader/writer를 취소하고 연결을 4002로 끝낸다. 자동 identity 재연결은 하지 않는다. JS 출력 제한만으로 native 내부 할당의 절대 상한을 증명하지는 않는다. 악성 표본의 실제 메모리·취소 검증을 함께 기록한다.

Rust codec은 상태 잠금 밖의 blocking 작업에서 실행한다. process당 작업 2개, 연결당 writer 1개, 대기 본문의 원시/압축 바이트 1 MiB 한도를 둔다. 취소된 blocking future의 permit은 실제 native 작업이 끝난 뒤 반환한다. 웹도 연결별 큐 하나로 snapshot 해제와 binary/history 순서를 유지하며 대기 raw_len과 실제 wire 크기를 함께 센다. 원시 길이를 작게 위조해 큰 문자열을 쌓을 수 없다.

브라우저의 첫 Worker가 확인한 지원 여부는 같은 브라우저의 재연결에 재사용한다. `online`·`pageshow`에서 기존처럼 즉시 WS를 열고 새 Worker의 준비 사건 뒤에만 해제를 시작한다. disconnect·pane 교체 때 Worker와 대기열을 폐기한다. 키 송신은 수신 큐를 기다리지 않는다.

snapshot/출력 ACK는 해제한 원시 바이트 기준이며 xterm.write callback에서만 보낸다. history는 출력 credit에 넣지 않는다. stream·id·lines·more·found·anchor·depth, OutputWindow의 HIGH/LOW/hard/stall/join/pause 규칙은 유지하고 wire 예산만 실제 전송 JSON 길이로 센다. host는 agent replay를 mirror에 반영한 뒤 웹 replay를 새로 만들므로 원격 zstd 해제와 웹 gzip 압축이 각각 한 번 필요하다.

계측은 `AAA_LOG=aaa_common::terminal_payload=debug`의 원시/원시 JSON/최종 wire 길이, codec 시간, `compressed`/`small`/`no_gain`/`identity_peer`, 거부 사건이다. 본문·주소·인증값을 추가로 기록하지 않는다. 시험 방법은 [testing.md](testing.md)의 t-0876 절을 따른다.

참고한 구현은 [VS Code의 비동기 flush 순서](https://github.com/microsoft/vscode/blob/56a079479f1c59e64840be6e092820d890324f96/src/vs/base/parts/ipc/node/ipc.net.ts)(MIT), [zstd-rs의 제한된 출력 API](https://github.com/gyscos/zstd-rs/blob/v0.13.3/src/bulk/decompressor.rs)(MIT·native BSD-3-Clause), [Compression Streams 표준](https://compression.spec.whatwg.org/)이다. 구현 코드는 복사하지 않았다. 새 zstd 의존성의 저작권·허가는 기존 flate2의 MIT 허가와 함께 실행 파일에 포함한 `/licenses/terminal-codecs.txt`에서 제공한다.
