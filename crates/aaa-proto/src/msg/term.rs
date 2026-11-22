//! 웹 터미널·상태 사건 WebSocket 메시지.

use super::*;

/// 웹 터미널 WebSocket에서 서버가 보내는 텍스트 메시지(출력은 binary message)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum TermServerMsg {
    /// 화면을 비우고 replay를 쓴다. `stream`은 이 snapshot부터의 출력 구독 번호다: 브라우저는 replay와 뒤따르는
    /// binary 출력을 xterm이 다 쓴 뒤 그 끝(replay 바이트 + 출력 바이트)을 `Ack`로 알린다(herdr-web-ui `pty-ack`).
    Snapshot {
        replay: B64,
        #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "terminal_compression")]
        #[ts(optional)]
        compression: Option<TerminalCompression>,
        cols: u16,
        rows: u16,
        readonly: bool,
        stream: u32,
        /// replay에 싣지 않은 앞 기록이 host에 더 있다: 위로 굴려 끝에 닿으면 `History`로 받아 온다(plan §8.67)
        history_more: bool,
    },
    /// `History`의 답: anchor 바로 앞 기록 `lines`줄(replay 기록과 같은 모양, 줄마다 `ESC[m CR LF`), 그 앞에 더 있는지.
    /// `found`가 false면 host 기록에서 anchor를 찾지 못했다(크기가 바뀌어 줄이 달라졌거나 그만큼 지나 기록에서 빠짐).
    History {
        id: u32,
        data: B64,
        #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "terminal_compression")]
        #[ts(optional)]
        compression: Option<TerminalCompression>,
        lines: u32,
        more: bool,
        found: bool,
    },
    /// 입력 가능 여부가 바뀌었다
    Readonly {
        readonly: bool,
        reason: Option<String>,
    },
    State {
        // box: PaneView가 커서 다른 variant까지 크게 만든다(clippy large_enum_variant). wire 모양은 같다
        pane: Box<PaneView>,
    },
    /// `state=grid.v1`로 붙은 웹에는 터미널에 필요한 크기와 조작권만 보낸다.
    Grid {
        cols: u16,
        rows: u16,
        lease: Option<LeaseView>,
    },
    /// 이 WebSocket의 front ID(처음 한 번). pane.lease.front와 같으면 이 화면이 조작 중이다.
    Front {
        id: String,
        /// 대기 메시지 「지금 보내기」(`SteerQueued`)를 안다(1.45). 옛 host는 이 칸이 없어 웹이 단추를 숨긴다
        #[serde(default)]
        steer_queued: bool,
    },
    /// 붙여 넣은 그림을 대상 머신에 저장했다. `inserted`면 harness 입력창에 경로를 넣었다(Enter는 보내지 않는다).
    Attached {
        path: String,
        inserted: bool,
        note: Option<String>,
    },
    /// `Submit`의 답(herdr-web-ui `submit-result`). `ok`면 글과 Enter가 pane에 들어갔다. 아니면 `message`가 이유다.
    Submitted {
        id: u32,
        ok: bool,
        message: Option<String>,
    },
    /// `SubmitCancel`의 답(plan §8.247): 그 submit이 어디서 멈췄나. 멈췄으면 뒤에 오는 `Submitted`는 실패다
    SubmitCancelled {
        id: u32,
        stage: SubmitStage,
    },
    /// `SteerQueued`의 답(plan §8.271). `sent`면 steering 키가 pane에 들어갔다(글이 실제로 읽혔는지는 대화 기록이 말한다).
    /// 아니면 `reason`이 까닭이고, `Failed`의 `message`는 host 오류 글이다
    SteerQueued {
        id: u32,
        sent: bool,
        reason: Option<SteerRefusal>,
        message: Option<String>,
    },
    Error(ErrorBody),
}

/// 대기 메시지 「지금 보내기」를 하지 않은 까닭(`TermServerMsg::SteerQueued`, plan §8.271)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SteerRefusal {
    /// agent가 작업 중이 아니다(대기 글은 곧 읽힌다)
    NotWorking,
    /// agent가 질문·확인창에 답을 기다린다
    Question,
    /// harness 입력창에 보내지 않은 글이 있다(steering 키가 그 글까지 보낸다)
    Draft,
    /// 화면에 대기 글이 없다(이미 읽혔을 수 있다)
    NoQueue,
    /// 입력 상자·작업 표시를 확인하지 못했다
    UnknownScreen,
    /// 대기 글을 지금 보내는 키가 없는 프로그램이다
    Unsupported,
    /// 보내다 실패했다(권한·pane 끝남·연결)
    Failed,
    /// agent가 대화를 정리하는 중이다(plan §8.309, 1.61): harness가 정리가 끝날 때까지 대기 글을 읽지 않는다
    Compacting,
}

/// 멈춘 submit이 어디까지 갔나(`SubmitCancelled`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SubmitStage {
    /// pane에 아무것도 쓰지 않았고 쓰지 않는다
    NotSent,
    /// 글은 pane에 갔거나 가는 중이고 Enter는 보내지 않는다(harness 입력창에 글이 남을 수 있다)
    NoEnter,
    /// Enter까지 보냈거나 이미 끝난 submit이다: 멈추지 않는다
    TooLate,
}

/// 웹 터미널 WebSocket에서 브라우저가 보내는 메시지
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum TermClientMsg {
    Input {
        data: String,
    },
    /// front에서 입력줄을 추측하지 않고 host의 공통 Ctrl+Enter 판정을 쓴다.
    CtrlEnter {
        #[serde(default)]
        trace: Option<String>,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    /// 이 화면이 지금 쓰는 화면이 된다(크기를 이 화면으로). 입력·크기만으로도 같아지므로 보통은 필요 없다.
    Take,
    /// 이 화면을 쓰지 않는다(탭이 1초 넘게 가려짐): 다른 화면이 크기를 되찾게 놓는다
    Release,
    /// 입력줄 한 줄(herdr-web-ui `submit`): host가 `data`를 쓰고 조금 뒤 Enter를 쓴 다음 `Submitted`로 답한다.
    /// 브라우저는 그 답을 받고서야 보낸 앞부분을 입력줄에서 지운다(그 사이 친 글은 남는다).
    Submit {
        id: u32,
        data: String,
        #[serde(default)]
        trace: Option<String>,
        #[serde(default)]
        meta: crate::InputSubmitMeta,
        /// 입력창의 Ctrl+Enter(사용자 10-19 15:3x, plan §8.239 후속): 글 뒤 Enter 대신 host가 steering 키를 고른다(작업 중인 Claude는
        /// sendNow, 그 밖은 Enter). 옛 host는 이 칸을 버려 보통 보내기가 된다
        #[serde(default)]
        steer: bool,
    },
    /// 같은 WebSocket에서 받은 submit의 UI 사건. 원문·임의의 실패 문자열은 받지 않는다.
    InputTrace {
        id: String,
        event: crate::InputTraceEvent,
        method: Option<crate::InputReceiptMethod>,
    },
    /// 대기 메시지 「지금 보내기」(plan §8.271): harness 대기열에 이미 있는 글을 지금 턴에 넣는 키만 보낸다(글은 다시 보내지
    /// 않는다). host가 입력창이 비고 대기 글이 화면에 있을 때만 키를 고르고(Claude sendNow, Codex Esc) `SteerQueued`로 답한다
    SteerQueued {
        id: u32,
        #[serde(default)]
        trace: Option<String>,
    },
    /// 진행 중인 `Submit`을 멈춰 달라(plan §8.247): 전달 전에 막혔을 때 입력창이 빠져나가는 길이다.
    /// host는 submit을 기다리는 동안에도 이것은 바로 읽고 `SubmitCancelled`로 답한다(다른 frame은 submit 뒤 차례대로).
    SubmitCancel {
        id: u32,
    },
    /// 받은 기록보다 앞 덩어리를 달라(위로 굴려 끝에 가까워졌다). `anchor`는 받은 기록 맨 위 몇 줄의 글,
    /// `depth`는 그 첫 줄부터 맨 아래 줄까지의 줄 수(같은 글이 여러 곳이면 가까운 곳을 고른다)
    History {
        id: u32,
        anchor: Vec<String>,
        depth: u32,
    },
    /// xterm이 이 구독(`Snapshot.stream`)의 출력을 `offset` 바이트까지 다 썼다(herdr-web-ui `pty-ack`).
    /// host는 이것으로 이 화면이 따라오는지 보고, 따라오지 못하면 4008로 닫는다(plan §8.62).
    Ack {
        stream: u32,
        #[ts(type = "number")]
        offset: u64,
    },
    /// 붙여 넣은 그림 한 조각(최대 512KiB). `offset` 0이 새 첨부의 시작이고, 마지막 조각에서 `sha256`(파일 전체)을 확인한다.
    Attach {
        #[ts(type = "number")]
        offset: u64,
        #[ts(type = "number")]
        total: u64,
        sha256: String,
        data: B64,
    },
    /// 이 화면의 터미널 기본 색이 바뀌었다(테마를 바꿈). 처음 색은 pane WebSocket 주소의 `fg`·`bg`로 보낸다(plan §8.187).
    /// `rrggbb`이고 앞의 `#`은 있어도 된다
    Colors {
        fg: String,
        bg: String,
    },
}

/// 상태 사건 WebSocket(`/api/v1/events`)에서 브라우저가 보내는 메시지
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum EventsClientMsg {
    /// 이 화면을 지금 쓰고 있다(보이고 focus가 있다, herdr-web-ui `presence`). 연결이 끊기면 쓰지 않는 것이다.
    /// 나머지는 판정 근거 기록에만 쓴다(10-18 실기, 옛 웹은 보내지 않는다).
    Presence {
        active: bool,
        /// 무엇이 알리게 했나(focus·blur·visibilitychange·pagehide·pageshow·freeze·resume·open·heartbeat)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        why: Option<String>,
        /// 그때 `document.visibilityState === "visible"`
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        visible: Option<bool>,
        /// 그때 `document.hasFocus()`
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        focus: Option<bool>,
        /// 이 웹 빌드(주 묶음 파일 이름). 배포 뒤에도 옛 페이지가 도는지 본다
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        build: Option<String>,
    },
    /// 사건 하나를 앱 안 알림으로 어떻게 했나(판정 근거 기록, host는 흐름 사건으로 남긴다). `target`은 알림 자리
    /// (`thread=…`·`pane=…`), `kind`는 `blocked`·`done`, `outcome`은 웹이 정한 한 줄(띄움·보는 중·가려짐·꺼짐 등)
    Alert { project: Option<String>, target: String, kind: String, outcome: String },
    /// 사람이 이 기기에서 방금 눌렀거나 쳤거나 화면을 켰다(보이고 focus가 됐다·앱을 열었다). host는 마지막으로 이것을 보낸
    /// 기기(또는 pane에 입력한 기기)를 「쓰는 기기」로 두고, 그 기기가 지금 쓰는 화면일 때만 Web Push를 건너뛴다
    /// (사용자 결정 10-18, plan §8.105. 참고는 쓰는 화면이 하나라도 있으면 건너뛴다)
    Touch,
    /// 연결이 살아 있는지 묻는다. host는 같은 `id`의 `pong`으로 답한다(herdr-web-ui ws.ts `probe`·server `ping`).
    /// 휴대폰이 잠들었다 깨거나 망이 바뀌면 열린 것처럼 보이는 죽은 연결이 몇 분씩 남는다.
    Ping { id: u32 },
    /// 이 화면이 그 pane의 대화를 보고 있다(herdr-web-ui `conversation-watch`). 켜 둔 동안 host는 `chat_watching`과
    /// `chat_changed`를 보낸다. 연결이 끊기면 끈 것이다
    ChatWatch { pane: String, enabled: bool },
}
