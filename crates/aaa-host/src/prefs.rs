//! 이 host의 모든 웹 화면이 함께 쓰는 설정과 고정(herdr-web-ui `shared/preferences.ts`·`server/preferences.ts` `PreferenceStore`, plan §8.79).
//!
//! 문서 하나다. 화면은 바꾼 설정만(고정은 바뀌었을 때만) 보내고, host는 그것을 가진 것 위에 놓는다(설정마다 마지막 쓰기가 이긴다).
//! 다른 화면들은 events의 `preferences` 사건으로 문서 전체를 받는다. 변경은 저장된 뒤에만 갖고 알린다.
//! 값의 뜻은 쓰는 쪽(웹 `sanitizeShared`)이 본다. host는 공유 이름과 모양·한도만 지킨다.
//! 보기 권한 기기의 쓰기는 `FrontRequest::needs_control`이 앞에서 막는다(참고 index.ts readOnly).
use std::collections::BTreeMap;

use aaa_proto::{ErrorBody, ErrorCode, SharedPreferences};
use tokio::sync::watch;

/// 참고 `SHARED_SETTING_KEYS`와 같은 이름. aaa에 아직 없는 설정도 다른 판의 화면을 위해 그대로 든다
pub(crate) const SHARED_SETTING_KEYS: &[&str] = &[
    "theme",
    "palette",
    "language",
    "sidebarRows",
    "agentOrder",
    "agentMarks",
    "autoSendQueued",
    "showThinking",
    "quickReplies",
    "showQuickReplies",
    "showUsage",
    "usageCount",
    "usageGlance",
    "usageOrder",
    "usageHidden",
    // aaa: 웹 목록 pane 줄 순서(참고는 herdr workspace 순서라 herdr 서버에 있다, plan §8.195)
    "paneOrder",
];
/// TUI 사이드바 배치(plan §8.193): 사용자가 끌거나 접은 값만 있다. herdr는 client 기기 파일(`client-shell/*.json`
/// `sidebar_width`·`sidebar_section_split`·`sidebar_collapsed`)에 두지만 aaa front는 상태를 갖지 않아 host에 둔다(plan §8.92
/// 설정 절). 값이 null이면 사용자가 되돌린 것이다(설정값을 쓴다). 웹은 이 이름을 읽지 않는다
pub(crate) const TUI_SETTING_KEYS: &[&str] = &["tuiSidebarWidth", "tuiSidebarSplit", "tuiSidebarCollapsed"];
pub(crate) const MAX_PINS: usize = 500;
const MAX_PIN_LENGTH: usize = 300;
/// 공유 값은 몇 낱말이나 짧은 목록이다: 변경 하나와 그것이 만드는 문서 전체가 작게 남는다
pub(crate) const MAX_PREFERENCES_BYTES: usize = 64 * 1024;
/// host DB `meta`의 열쇠(참고는 상태 폴더의 preferences.json)
pub(crate) const META_KEY: &str = "preferences";

/// 변경: 바꾼 설정, 바뀌었으면 고정
pub(crate) struct Change {
    pub settings: BTreeMap<String, serde_json::Value>,
    pub pins: Option<Vec<String>>,
}

/// 참고 `sharedPreferencesBody`: 공유 이름과 한도로 자른다. 고정이 한도를 넘으면 거부
pub(crate) fn change_body(
    settings: BTreeMap<String, serde_json::Value>,
    pins: Option<Vec<String>>,
) -> Result<Change, ErrorBody> {
    if let Some(p) = &pins {
        let fits = |pin: &String| !pin.is_empty() && pin.encode_utf16().count() <= MAX_PIN_LENGTH;
        if p.len() > MAX_PINS || !p.iter().all(fits) {
            return Err(ErrorBody::new(
                ErrorCode::BadRequest,
                format!("고정은 {MAX_PINS}개까지, 하나는 1~{MAX_PIN_LENGTH}자다"),
            ));
        }
    }
    let known = |k: &str| SHARED_SETTING_KEYS.contains(&k) || TUI_SETTING_KEYS.contains(&k);
    let settings = settings.into_iter().filter(|(k, _)| known(k)).collect();
    let pins = pins.map(|p| {
        let mut seen = std::collections::HashSet::new();
        p.into_iter().filter(|x| seen.insert(x.clone())).collect()
    });
    Ok(Change { settings, pins })
}

/// 저장된 문서를 읽는다. 없으면 빈 문서, 깨졌으면 빈 문서로 시작하고 다음 쓰기가 덮는다
pub(crate) fn parse(raw: Option<&str>) -> SharedPreferences {
    let empty = SharedPreferences { revision: 0, settings: BTreeMap::new(), pins: None };
    let Some(raw) = raw else { return empty };
    match serde_json::from_str::<SharedPreferences>(raw) {
        Ok(doc) => match change_body(doc.settings, doc.pins) {
            Ok(c) => SharedPreferences { revision: doc.revision, settings: c.settings, pins: c.pins },
            Err(_) => empty,
        },
        Err(e) => {
            tracing::warn!("공유 설정을 읽지 못해 비운 채 시작한다: {e}");
            empty
        }
    }
}

/// 문서와 그 구독(events 연결마다 하나). 바꾸기는 `save`가 성공한 뒤에만 보인다
pub(crate) struct Store {
    tx: watch::Sender<SharedPreferences>,
}

impl Store {
    pub(crate) fn new(doc: SharedPreferences) -> Store {
        Store { tx: watch::channel(doc).0 }
    }

    pub(crate) fn get(&self) -> SharedPreferences {
        self.tx.borrow().clone()
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<SharedPreferences> {
        self.tx.subscribe()
    }

    /// 변경을 놓고 revision을 하나 올린다. 문서 전체가 한도를 넘거나 저장이 실패하면 아무것도 바꾸지 않는다
    pub(crate) fn set(
        &self,
        change: Change,
        save: impl FnOnce(&str) -> anyhow::Result<()>,
    ) -> Result<SharedPreferences, ErrorBody> {
        let mut out = Err(ErrorBody::new(ErrorCode::Internal, "공유 설정을 바꾸지 못했다"));
        self.tx.send_if_modified(|cur| {
            let mut settings = cur.settings.clone();
            settings.extend(change.settings);
            let next = SharedPreferences {
                revision: cur.revision + 1,
                settings,
                pins: change.pins.or_else(|| cur.pins.clone()),
            };
            let text = serde_json::to_string(&next).expect("공유 설정 직렬화");
            if text.len() > MAX_PREFERENCES_BYTES {
                out = Err(ErrorBody::new(
                    ErrorCode::TooLarge,
                    format!("공유 설정은 {}KB까지다", MAX_PREFERENCES_BYTES / 1024),
                ));
                return false;
            }
            if let Err(e) = save(&text) {
                out = Err(ErrorBody::new(ErrorCode::Internal, format!("공유 설정을 저장하지 못했다: {e}")));
                return false;
            }
            *cur = next.clone();
            out = Ok(next);
            true
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn change(v: serde_json::Value) -> Change {
        let doc: SharedPreferences =
            serde_json::from_value(json!({ "revision": 0, "settings": v["settings"], "pins": v.get("pins") })).unwrap();
        change_body(doc.settings, doc.pins).unwrap()
    }
    fn ok(_: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn doc(s: &SharedPreferences) -> serde_json::Value {
        serde_json::to_value(s).unwrap()
    }

    #[test]
    fn starts_empty_keeps_only_shared_names_counts_each_write_and_survives_a_restart() {
        // 참고 preferences.test.ts 「starts empty, keeps only the shared names, …」
        let store = Store::new(parse(None));
        // 화면이 고정을 처음 나누기 전에는 고정이 없다: 빈 목록이면 모두의 것으로 읽힌다
        assert_eq!(doc(&store.get()), json!({ "revision": 0, "settings": {} }));
        let mut heard = store.subscribe();
        let mut saved = String::new();
        let first = store.set(change(json!({ "settings": { "showThinking": false } })), ok).unwrap();
        assert_eq!(doc(&first), json!({ "revision": 1, "settings": { "showThinking": false } }));
        assert!(heard.has_changed().unwrap());
        heard.mark_unchanged();
        let second = store
            .set(
                change(json!({
                    "settings": { "theme": "light", "terminalFontSize": 22, "alertsOn": false, "nonsense": 1 },
                    "pins": ["local:w1:p1", "local:w1:p1", "pc:w2:p3"]
                })),
                ok,
            )
            .unwrap();
        assert_eq!(
            doc(&second),
            json!({ "revision": 2, "settings": { "showThinking": false, "theme": "light" }, "pins": ["local:w1:p1", "pc:w2:p3"] })
        );
        // 변경은 설정을 가진 것 위에 놓고, 고정을 말하지 않으면 고정을 둔다
        store.set(change(json!({ "settings": { "agentOrder": "activity" } })), ok).unwrap();
        assert_eq!(
            doc(&store.get()),
            json!({ "revision": 3, "settings": { "showThinking": false, "theme": "light", "agentOrder": "activity" }, "pins": ["local:w1:p1", "pc:w2:p3"] })
        );
        store
            .set(change(json!({ "settings": { "theme": "dark" }, "pins": [] })), |t| {
                saved = t.to_string();
                Ok(())
            })
            .unwrap();
        assert!(heard.has_changed().unwrap());
        let want = json!({ "revision": 4, "settings": { "showThinking": false, "theme": "dark", "agentOrder": "activity" }, "pins": [] });
        assert_eq!(serde_json::from_str::<serde_json::Value>(&saved).unwrap(), want);
        // 다시 띄우면 저장된 문서에서 시작한다
        assert_eq!(doc(&parse(Some(&saved))), want);
    }

    #[test]
    fn keeps_the_tui_sidebar_names_too() {
        let store = Store::new(parse(None));
        let doc = store
            .set(
                change(json!({ "settings": { "tuiSidebarWidth": 48, "tuiSidebarCollapsed": null, "tuiOther": 1 } })),
                ok,
            )
            .unwrap();
        assert_eq!(
            doc.settings,
            change(json!({ "settings": { "tuiSidebarWidth": 48, "tuiSidebarCollapsed": null } })).settings
        );
        // 웹이 제 설정을 바꿔도 TUI 값은 남는다
        store.set(change(json!({ "settings": { "theme": "dark" } })), ok).unwrap();
        assert_eq!(store.get().settings.get("tuiSidebarWidth"), Some(&json!(48)));
    }

    #[test]
    fn refuses_pins_over_the_limits() {
        let pins = |p: Vec<String>| change_body(BTreeMap::new(), Some(p)).err().map(|e| e.code);
        assert_eq!(pins(vec![String::new()]), Some(ErrorCode::BadRequest));
        assert_eq!(pins(vec!["x".repeat(MAX_PIN_LENGTH + 1)]), Some(ErrorCode::BadRequest));
        assert_eq!(pins((0..=MAX_PINS).map(|i| format!("pc:{i}")).collect()), Some(ErrorCode::BadRequest));
        assert_eq!(pins(vec!["x".repeat(MAX_PIN_LENGTH)]), None);
        // 설정 자리가 객체가 아니거나 고정이 글자가 아니면 몸을 읽지 못한다(serde가 거부, 웹 bad_request)
        assert!(serde_json::from_value::<SharedPreferences>(json!({ "revision": 0, "settings": [] })).is_err());
        assert!(
            serde_json::from_value::<SharedPreferences>(json!({ "revision": 0, "settings": {}, "pins": [1] })).is_err()
        );
    }

    #[test]
    fn keeps_the_whole_document_under_the_limit_not_only_each_change() {
        let store = Store::new(parse(None));
        store.set(change(json!({ "settings": { "quickReplies": ["x".repeat(40_000)] } })), ok).unwrap();
        let over = store.set(change(json!({ "settings": { "usageOrder": ["y".repeat(40_000)] } })), ok);
        assert_eq!(over.err().map(|e| e.code), Some(ErrorCode::TooLarge));
        assert_eq!(store.get().revision, 1);
        assert!(!store.get().settings.contains_key("usageOrder"));
    }

    #[test]
    fn holds_and_tells_a_change_only_once_it_is_saved() {
        let store = Store::new(parse(None));
        let heard = store.subscribe();
        let failed = store.set(change(json!({ "settings": { "theme": "light" } })), |_| anyhow::bail!("디스크 가득"));
        assert_eq!(failed.err().map(|e| e.code), Some(ErrorCode::Internal));
        assert_eq!(doc(&store.get()), json!({ "revision": 0, "settings": {} }));
        assert!(!heard.has_changed().unwrap());
    }

    #[test]
    fn starts_empty_over_a_damaged_record() {
        assert_eq!(doc(&parse(Some("not json"))), json!({ "revision": 0, "settings": {} }));
    }
}
