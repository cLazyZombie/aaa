# 웹 입력 사건 보기

host 머신에서 최근 보내기와 Ctrl+Enter 판정을 시간순으로 본다. 사용자 글 전문과 앞부분은 자동 기록하지 않는다.

```sh
aaa debug input
aaa debug input --pane coordinator --since 10m
aaa debug input --thread projects/t-0408 --since 2h
aaa --json debug input --thread t-0408 --since 1d
```

`--since`는 `m`·`h`·`d`가 붙은 기간 또는 Unix milliseconds다(기본 1시간). `--pane`은 pane 이름·ID,
`--thread`는 thread ID 또는 `프로젝트/ID`다. 두 필터를 함께 주면 둘 다 맞는 사건만 낸다. 최신 1,000개를
오래된 것부터 출력한다. 조회는 host의 로컬 CLI만 허용한다. 웹 Controller·Reader에는 장부를 공개하지 않는다.

같은 보내기는 `front ID:보내기 UUID`로 묶인다. 입력줄의 옛 클라이언트처럼 UUID가 없으면 그 연결의 submit 번호를 쓴다.
각 행에는 host가 받은 시각(UTC), pane·thread·harness·작업 중 여부, 사건과 짧은 상세가 있다.

| 사건 | 뜻 |
|---|---|
| `ui / Preparing` | 브라우저가 수신 확인용 hash를 준비하기 시작함 |
| `received` | host가 submit을 받음. 글자·UTF-8 byte 수, SHA-256, 여러 줄, bracketed paste, 파일 언급·이미지 경로, clipboard paste 여부와 첨부 개수를 기록 |
| `written_text` / `written_submit` | agent가 글 / 마지막 제출 키를 pane에 썼다고 확인함 |
| `delivered` / `failed` | 전달 성공 / 실패 code. 전달 성공과 대화 기록의 수신 확인은 별개임 |
| `ui / Receipt` | 새 사람 글을 찾음. `Normal`, `PastedContent`, `QueueOperation`(enqueue) 등으로 방법을 구별 |
| `ui / Unlocked` | 보내기 잠금 해제 |
| `ui / Pending` | 작업 중 Codex: 아직 기록이 없어 전달을 확인한 뒤 대기 말로 남김 |
| `ui / Stuck`, `Slow` | 전달 답 3초 지연 / 전달 뒤 기록 확인 10초 지연. Codex 대기 말은 작업이 끝난 뒤 10초부터 판단 |
| `ui / Unlock`, `Cancel` | 사용자가 잠금 풀기 / 멈춤을 누름 |
| `cancel` | host가 답한 단계: `NotSent`, `NoEnter`, `TooLate` |
| `ui / Failed` | 브라우저가 본 연결 단절·거절·확인 불가. 임의의 오류 본문은 저장하지 않음 |
| `ctrl_enter` | 고른 키와 `working`, `question`, `synced`, 커서 위치, 현재 입력 상자·작업 표시의 근거 |
| `ui / CtrlEnterBlocked` | 브라우저에서 붙잡은 초안·IME·읽기 전용 상태로 막음 |

`Receipt` 시각은 브라우저가 기록에서 확인한 사건을 host가 받은 시각이다. 네트워크가 끊기거나 탭이 닫히면 그 이후
UI 사건이 없을 수 있으므로, `delivered`만으로 `Receipt`나 `Unlocked`를 추정하지 않는다. 로그가 전송 재시도를 일으키지는 않는다.

예를 들어 같은 ID에 `received → written_text → written_submit → delivered → Receipt(QueueOperation) → Unlocked`가 있으면
작업 중 Claude가 대기열에 넣은 글이다. `delivered → Slow`만 있으면 기록 확인을 못한 것이다.
Ctrl+Enter의 `keys=[] working=true working_line=false input_live=true`는 요청은 왔지만 작업 화면 근거를 못 찾은 경우다.
수리 뒤에는 `working_source=footer` 또는 `spinner`와 `keys=["ctrl+x", "ctrl+s"]`를 볼 수 있다.

자동 입력 장부는 host DB 안에서 오래된 것부터 지운다. **30일·10,000행·본문 20MiB** 중 먼저 닿은 한도를 따른다.
20MiB는 기존 수동 입력 진단 파일의 `20개 × 1MiB` 예산과 같다. DB의 다른 자료·WAL·이미 할당한 재사용 페이지는
이 본문 예산에 포함하지 않는다. 한 상세는 1,024자 이내이며, 연결당 최근 128개 ID와 ID당 최대 16개 UI 사건만 받는다.
stdout의 구조화된 `입력 사건` 줄은 종전 host 감독자의 로그 설정을 따른다.

수신 비교는 대화 보기와 같은 `unwrapPasted`로 글 전체를 감싼 같은 ID의 봉투만 걷는다. 줄 끝은 LF로 맞추고 본문 안 공백은
보존한다. 표시용 4,000자 상한과 별개로 새 agent는 자르기 전 사람 글의 SHA-256을 보내므로 긴 글도 온전하게 비교한다.
옛 agent의 짧은 글은 기존 텍스트 비교로 계속 확인한다. 이미지의 마지막 `@경로` 뒤 자동완성을 닫는 공백은 바깥 공백으로 처리한다.

WebCrypto를 제공하지 않는 연결은 진단 ID를 `getRandomValues`로 만들고 수신 확인은 기존 짧은 글 비교로 돌아간다. 큰 글의 전체 fingerprint 비교는 WebCrypto가 있는 연결에서 사용한다.
