//! post는 자료다. 지시 종류나 보낸 이 자기 신고 칸은 없다.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PostType {
    Notice,
    Data,
    Request,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PostInput {
    pub post_id: String,
    pub to: String,
    pub post_type: PostType,
    pub topic: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub needs_user: bool,
    #[serde(default)]
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PostResult {
    Accepted { id: String },
    Refused { reason: String, existing: Option<String> },
    Queued { post_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PostSender {
    pub thread: Option<String>,
    pub role: crate::Role,
    pub mission: Option<String>,
    pub machine: String,
    pub session: Option<String>,
    pub verified: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PostEnvelope {
    pub id: String,
    pub project: String,
    pub from: PostSender,
    #[serde(flatten)]
    pub input: PostInput,
    pub caused_by: Vec<String>,
    pub body_sha256: String,
    #[ts(type = "number")]
    pub created_ms: u64,
    pub state: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    /// 기획 §7.3·§7.4의 독립 JSON 계약. 중첩 input이나 instruction 종류를 도입하면 실패한다.
    #[test]
    fn post_json_matches_flat_wire_and_envelope_contract() {
        let wire = json!({"ev":"post","session":"s1","post_id":"00000000000000000000000000000001","to":"coordinator","post_type":"request","topic":"ci-red","title":"보고","body":"본문","needs_user":true});
        let event: crate::AgentEvent = serde_json::from_value(wire.clone()).expect("기획의 최상위 post 필드를 읽는다");
        let crate::AgentEvent::Post { input, .. } = event else { panic!("post가 아니다") };
        let envelope = PostEnvelope {
            id: "p-00000001".into(),
            project: "demo".into(),
            from: PostSender {
                thread: Some("t-0001".into()),
                role: crate::Role::Worker,
                mission: None,
                machine: "mac-a".into(),
                session: Some("s1".into()),
                verified: "peer+token".into(),
            },
            input: input.clone(),
            caused_by: vec![],
            body_sha256: "fixture-sha".into(),
            created_ms: 1,
            state: "delivered".into(),
        };
        let value = serde_json::to_value(envelope).unwrap();
        assert_eq!(value["post_type"], "request");
        assert_eq!(value["body"], "본문");
        assert!(value.get("input").is_none());
        let output = serde_json::to_value(crate::AgentEvent::Post { session: "s1".into(), input }).unwrap();
        assert_eq!(output["post_id"], wire["post_id"]);
        assert_eq!(output["post_type"], "request");
        assert!(output.get("input").is_none());
        let mut instruction = wire;
        instruction["post_type"] = json!("instruction");
        assert!(serde_json::from_value::<crate::AgentEvent>(instruction).is_err(), "post에는 지시 종류가 없다");
    }
}
