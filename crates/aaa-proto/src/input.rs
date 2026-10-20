//! 입력 진단: 원문·화면 사본 없이 한 보내기의 사건을 잇는다.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum InputTraceEvent {
    Receipt,
    Preparing,
    CtrlEnterBlocked,
    Unlocked,
    Slow,
    Unlock,
    Cancel,
    Failed,
    Pending,
    Stuck,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum InputReceiptMethod {
    Normal,
    PastedContent,
    QueueOperation,
    CodexDelivery,
    NoTranscript,
    Connection,
    Rejected,
    Unknown,
    HeldDraft,
    Composing,
    Readonly,
    Locked,
    Uploading,
    NoRoom,
    /// 대화 정리 중이라 보내지 않았다(plan §8.304, FRONT_API 1.59)
    Compacting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct InputEvent {
    pub at_ms: u64,
    pub id: String,
    pub pane: String,
    pub thread: Option<String>,
    pub harness: Option<String>,
    pub working: bool,
    pub event: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct InputSubmitMeta {
    pub pasted: bool,
    pub attachments: u16,
    #[serde(default)]
    pub ctrl_enter: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ChatReceipt {
    pub sha256: String,
    pub method: InputReceiptMethod,
}

/// 웹 receiptText와 같은 wire 계약. 온전한 전체 봉투만 걷고 본문의 공백은 바꾸지 않는다.
pub fn receipt_text(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut value = normalized.trim().to_owned();
    loop {
        let Some(rest) = value.strip_prefix("<pasted_content id=\"") else { return value };
        let Some((id, body)) = rest.split_once("\">") else { return value };
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
            return value;
        }
        let close = format!("</pasted_content id=\"{id}\">");
        let Some(body) = body.strip_suffix(&close) else { return value };
        if body.contains(&format!("</pasted_content id=\"{id}\">"))
            || body.contains(&format!("<pasted_content id=\"{id}\">"))
        {
            return value;
        }
        value = body.trim().to_owned();
    }
}
