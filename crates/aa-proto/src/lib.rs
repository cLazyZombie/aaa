//! aa의 경계 계약. Rust 타입이 정본이고 웹 TypeScript 타입은 여기서 생성한다(`web/src/gen/proto.ts`).

pub mod frame;
pub mod layout;
pub mod msg;
pub mod version;

pub use frame::{Frame, FrameError, FrameKind, MAX_FRAME, MAX_FRAME_PREAUTH, TerminalChunk, read_frame, write_frame};
pub use msg::*;
pub use version::{AGENT_LINK, FRONT_API, NegotiateError, SESSION_IPC, Version, negotiate, require_features};

/// Agent link heartbeat: 양쪽이 이 간격으로 ping하고, 이만큼 아무것도 못 받으면 끊는다.
pub const LINK_PING_EVERY: std::time::Duration = std::time::Duration::from_secs(15);
pub const LINK_DEAD_AFTER: std::time::Duration = std::time::Duration::from_secs(45);

/// 이름 없이 부르면 쓰는 workspace
pub const DEFAULT_WORKSPACE: &str = "main";

/// 웹 front가 쓰는 타입 선언을 한 파일로 만든다.
pub fn typescript_bindings() -> String {
    use ts_rs::TS;
    let cfg = ts_rs::Config::default();
    let decls = [
        Version::decl(&cfg),
        B64::decl(&cfg),
        ErrorCode::decl(&cfg),
        ErrorBody::decl(&cfg),
        <Outcome<()>>::decl(&cfg),
        SessionState::decl(&cfg),
        ScreenText::decl(&cfg),
        DeviceRole::decl(&cfg),
        Connection::decl(&cfg),
        MachineView::decl(&cfg),
        LeaseView::decl(&cfg),
        PaneView::decl(&cfg),
        SplitDir::decl(&cfg),
        LayoutNode::decl(&cfg),
        Tab::decl(&cfg),
        Workspace::decl(&cfg),
        LayoutChange::decl(&cfg),
        GridRun::decl(&cfg),
        GridLine::decl(&cfg),
        GridFrame::decl(&cfg),
        PendingEnroll::decl(&cfg),
        PublishState::decl(&cfg),
        HostStatus::decl(&cfg),
        FrontRequest::decl(&cfg),
        FrontResponse::decl(&cfg),
        FrontEvent::decl(&cfg),
        TermServerMsg::decl(&cfg),
        TermClientMsg::decl(&cfg),
    ];
    let mut out = String::from(
        "// 생성 파일: crates/aa-proto의 Rust 타입에서 만든다. 손으로 고치지 않는다.\n// 갱신: AA_UPDATE_TS=1 cargo test -p aa-proto typescript\n\n",
    );
    for d in decls {
        out.push_str("export ");
        out.push_str(&d);
        out.push_str("\n\n");
    }
    out
}

#[cfg(test)]
mod ts_tests {
    /// 생성된 TS 타입이 Rust 정본과 같은지 본다. 다르면 AA_UPDATE_TS=1로 다시 만든다.
    #[test]
    fn typescript_bindings_are_current() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/src/gen/proto.ts");
        let want = super::typescript_bindings();
        if std::env::var_os("AA_UPDATE_TS").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &want).unwrap();
        }
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(have == want, "web/src/gen/proto.ts가 오래됐다. AA_UPDATE_TS=1 cargo test -p aa-proto typescript");
    }
}
