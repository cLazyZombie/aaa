//! aaa의 경계 계약. Rust 타입이 정본이고 웹 TypeScript 타입은 여기서 생성한다(`web/src/gen/proto.ts`).

pub mod automation;
pub use automation::*;
pub mod auto_mode;
pub use auto_mode::*;
pub mod workspace_metrics;
pub use workspace_metrics::*;
pub mod task_queue;
pub use task_queue::*;
pub mod account;
pub mod quota;
pub use quota::*;
pub mod ci;
pub use account::*;
mod git_watch;
pub mod image;
pub mod install;
pub use ci::*;
pub use git_watch::*;
pub use image::*;
pub mod deploy;
pub mod diff;
pub use diff::*;
pub mod input;
pub use deploy::*;
pub use input::*;
pub mod frame;
mod short_id;
pub use short_id::{canonical_short_id, cmp_short_ids, short_id_number};
pub mod layout;
pub mod legacy_special;
pub mod msg;
pub mod orch;
pub mod passkey;
pub mod post;
pub use legacy_special::*;
pub use post::*;
pub mod settings_admin;
pub use settings_admin::*;
pub mod usage;
pub mod version;

pub use frame::{
    Frame, FrameError, FrameKind, MAX_FRAME, MAX_FRAME_PREAUTH, TerminalChunk, read_frame, take_frame, write_frame,
};
pub use msg::*;
pub use orch::*;
pub use passkey::*;
pub use usage::*;
pub use version::{
    AGENT_LINK, FRONT_API, NegotiateError, SESSION_IPC, SESSION_IPC_COLORS, SESSION_IPC_DETECT, Version, negotiate,
    require_features,
};

/// Agent link heartbeat: 양쪽이 이 간격으로 ping하고, 이만큼 아무것도 못 받으면 끊는다.
/// 붙여 넣은 그림 한 조각 상한(Agent link frame 1MiB 안에 base64로 들어간다). 웹·CLI 첨부가 같이 쓴다.
pub const ATTACH_CHUNK: usize = 512 * 1024;

pub const LINK_PING_EVERY: std::time::Duration = std::time::Duration::from_secs(15);
pub const LINK_DEAD_AFTER: std::time::Duration = std::time::Duration::from_secs(45);

/// 이름 없이 부르면 쓰는 workspace
pub const DEFAULT_WORKSPACE: &str = "main";

/// 웹 front가 쓰는 타입 선언을 한 파일로 만든다.
pub fn typescript_bindings() -> String {
    use ts_rs::TS;
    let cfg = ts_rs::Config::default();
    let decls = [
        AutoMode::decl(&cfg),
        AutoDecision::decl(&cfg),
        AutoDecisionNote::decl(&cfg),
        MetricCounts::decl(&cfg),
        MetricTiming::decl(&cfg),
        MetricRanking::decl(&cfg),
        MetricRun::decl(&cfg),
        CodeLines::decl(&cfg),
        WorkspaceCollected::decl(&cfg),
        WorkspaceMachine::decl(&cfg),
        RepoWorkspace::decl(&cfg),
        WorkspaceQuery::decl(&cfg),
        MetricPoint::decl(&cfg),
        WorkspaceDashboard::decl(&cfg),
        SecretText::decl(&cfg),
        AccountView::decl(&cfg),
        QuotaSource::decl(&cfg),
        QuotaProblem::decl(&cfg),
        QuotaFreshness::decl(&cfg),
        QuotaWindow::decl(&cfg),
        QuotaSnapshot::decl(&cfg),
        AccountQuotaWindow::decl(&cfg),
        AccountQuota::decl(&cfg),
        AccountQuotaReport::decl(&cfg),
        AccountHealth::decl(&cfg),
        AccountDefault::decl(&cfg),
        AccountPoolMember::decl(&cfg),
        AccountPoolView::decl(&cfg),
        AccountTaskSize::decl(&cfg),
        AccountSelection::decl(&cfg),
        AccountStartState::decl(&cfg),
        AccountStartView::decl(&cfg),
        AccountLoginView::decl(&cfg),
        Version::decl(&cfg),
        B64::decl(&cfg),
        ErrorCode::decl(&cfg),
        ErrorBody::decl(&cfg),
        PasskeyState::decl(&cfg),
        PasskeyCreation::decl(&cfg),
        PasskeyRegistration::decl(&cfg),
        PasskeyChallenge::decl(&cfg),
        PasskeyAssertion::decl(&cfg),
        <Outcome<()>>::decl(&cfg),
        SessionState::decl(&cfg),
        Responsibility::decl(&cfg),
        ResponsibilityState::decl(&cfg),
        ScreenText::decl(&cfg),
        ReadSource::decl(&cfg),
        ReadFormat::decl(&cfg),
        ReadView::decl(&cfg),
        ThreadPaneView::decl(&cfg),
        OutputMatch::decl(&cfg),
        PaneDirection::decl(&cfg),
        WorkspaceSummary::decl(&cfg),
        ChatRole::decl(&cfg),
        ChatMessage::decl(&cfg),
        ChatView::decl(&cfg),
        InputTraceEvent::decl(&cfg),
        InputReceiptMethod::decl(&cfg),
        InputEvent::decl(&cfg),
        InputSubmitMeta::decl(&cfg),
        ChatReceipt::decl(&cfg),
        DeviceRole::decl(&cfg),
        Connection::decl(&cfg),
        MachineView::decl(&cfg),
        MachineHealth::decl(&cfg),
        MachineInfo::decl(&cfg),
        MachineSelfcheck::decl(&cfg),
        HealthProblem::decl(&cfg),
        QueueState::decl(&cfg),
        QueueOp::decl(&cfg),
        TaskChoicePin::decl(&cfg),
        TaskReservation::decl(&cfg),
        ArchivedTask::decl(&cfg),
        TaskHistorySearch::decl(&cfg),
        TaskArchiveFilter::decl(&cfg),
        TaskArchiveSort::decl(&cfg),
        TaskHistoryRepo::decl(&cfg),
        TaskArchiveQuery::decl(&cfg),
        TaskView::decl(&cfg),
        TaskListView::decl(&cfg),
        TaskThread::decl(&cfg),
        TaskNotesSection::decl(&cfg),
        TaskNotesFile::decl(&cfg),
        TaskBoard::decl(&cfg),
        TaskSuggestion::decl(&cfg),
        TaskAnchor::decl(&cfg),
        TaskOp::decl(&cfg),
        TaskConflict::decl(&cfg),
        TaskAction::decl(&cfg),
        TaskLogEntry::decl(&cfg),
        TaskLogFilter::decl(&cfg),
        ThreadStartPreview::decl(&cfg),
        RepoChoice::decl(&cfg),
        OwnerChoice::decl(&cfg),
        ProjectContext::decl(&cfg),
        ToolCheck::decl(&cfg),
        LeaseView::decl(&cfg),
        PaneView::decl(&cfg),
        SplitDir::decl(&cfg),
        LayoutNode::decl(&cfg),
        Tab::decl(&cfg),
        Workspace::decl(&cfg),
        LayoutChange::decl(&cfg),
        PendingEnroll::decl(&cfg),
        EnrollGuide::decl(&cfg),
        EnrollAddress::decl(&cfg),
        MachineSetupView::decl(&cfg),
        MachineSetupPhase::decl(&cfg),
        SshResolved::decl(&cfg),
        HostKeyView::decl(&cfg),
        MachineDetected::decl(&cfg),
        MachineSetupSettings::decl(&cfg),
        MachineSetupReview::decl(&cfg),
        MachineSetupPassword::decl(&cfg),
        MachineSetupInstall::decl(&cfg),
        PublishState::decl(&cfg),
        DeviceView::decl(&cfg),
        HostStatus::decl(&cfg),
        MissionView::decl(&cfg),
        MissionBlockReason::decl(&cfg),
        PostType::decl(&cfg),
        PostInput::decl(&cfg),
        PostResult::decl(&cfg),
        PostSender::decl(&cfg),
        PostEnvelope::decl(&cfg),
        DiffScope::decl(&cfg),
        DiffFileRef::decl(&cfg),
        DiffQuery::decl(&cfg),
        DiffStatus::decl(&cfg),
        DiffFile::decl(&cfg),
        DiffView::decl(&cfg),
        FrontRequest::decl(&cfg),
        SharedPreferences::decl(&cfg),
        RolloutSpec::decl(&cfg),
        RolloutArtifact::decl(&cfg),
        RolloutPhase::decl(&cfg),
        RolloutStep::decl(&cfg),
        RolloutState::decl(&cfg),
        RolloutView::decl(&cfg),
        RolloutCard::decl(&cfg),
        RolloutCardStep::decl(&cfg),
        DeployCandidateView::decl(&cfg),
        RolloutTargetView::decl(&cfg),
        RolloutPlanView::decl(&cfg),
        DeployWork::decl(&cfg),
        DeployJobCard::decl(&cfg),
        FrontResponse::decl(&cfg),
        FrontEvent::decl(&cfg),
        SubmitStage::decl(&cfg),
        SteerRefusal::decl(&cfg),
        TerminalCodec::decl(&cfg),
        TerminalCompression::decl(&cfg),
        TermServerMsg::decl(&cfg),
        TermClientMsg::decl(&cfg),
        EventsClientMsg::decl(&cfg),
        Role::decl(&cfg),
        ThreadKind::decl(&cfg),
        Launch::decl(&cfg),
        ProfileView::decl(&cfg),
        LibFile::decl(&cfg),
        Skipped::decl(&cfg),
        ProjectView::decl(&cfg),
        ProjectStatus::decl(&cfg),
        ProjectAction::decl(&cfg),
        ProjectChange::decl(&cfg),
        ProjectFilesCheck::decl(&cfg),
        ProjectOpened::decl(&cfg),
        ThreadStatus::decl(&cfg),
        BriefState::decl(&cfg),
        DeployAction::decl(&cfg),
        DeployStageView::decl(&cfg),
        RuntimeIdentity::decl(&cfg),
        DeployGuardSpec::decl(&cfg),
        DeployGuardPhase::decl(&cfg),
        DeployGuardView::decl(&cfg),
        DeployGuardControl::decl(&cfg),
        DeployService::decl(&cfg),
        DeployState::decl(&cfg),
        DeployJob::decl(&cfg),
        ThreadGroup::decl(&cfg),
        ReportAttention::decl(&cfg),
        ThreadStage::decl(&cfg),
        ThreadView::decl(&cfg),
        ThreadLimitOverride::decl(&cfg),
        ThreadPark::decl(&cfg),
        UnparkOption::decl(&cfg),
        UnparkPreview::decl(&cfg),
        ThreadPr::decl(&cfg),
        SummaryMark::decl(&cfg),
        ThreadQuestions::decl(&cfg),
        ThreadQuestion::decl(&cfg),
        QuestionChoice::decl(&cfg),
        AskKind::decl(&cfg),
        AskRecord::decl(&cfg),
        UserRequestView::decl(&cfg),
        QuestionAnswerView::decl(&cfg),
        SummaryEntry::decl(&cfg),
        FollowUpLine::decl(&cfg),
        SummaryStepState::decl(&cfg),
        SummaryStep::decl(&cfg),
        ThreadSummaryView::decl(&cfg),
        FlowEvent::decl(&cfg),
        AgentActivity::decl(&cfg),
        PlanSummary::decl(&cfg),
        ContextUsage::decl(&cfg),
        ModelObservation::decl(&cfg),
        ContextCompaction::decl(&cfg),
        AgentStatus::decl(&cfg),
        QuestionState::decl(&cfg),
        QuestionView::decl(&cfg),
        QuestionMenu::decl(&cfg),
        NoticeKind::decl(&cfg),
        NoticeView::decl(&cfg),
        MenuOption::decl(&cfg),
        MenuPick::decl(&cfg),
        MessageMode::decl(&cfg),
        DeliveryState::decl(&cfg),
        MessageView::decl(&cfg),
        InboxItem::decl(&cfg),
        AutomationCompletion::decl(&cfg),
        AutomationOutcome::decl(&cfg),
        AutomationDefinition::decl(&cfg),
        AutomationTrigger::decl(&cfg),
        AutomationCatchUp::decl(&cfg),
        AutomationEventSource::decl(&cfg),
        AutomationObserve::decl(&cfg),
        AutomationCommand::decl(&cfg),
        AutomationCondition::decl(&cfg),
        AutomationPolicy::decl(&cfg),
        AutomationAction::decl(&cfg),
        AutomationNotice::decl(&cfg),
        AutomationMessage::decl(&cfg),
        AutomationThread::decl(&cfg),
        AutomationReuse::decl(&cfg),
        AutomationProfilePick::decl(&cfg),
        AutomationMachinePick::decl(&cfg),
        AutomationRunState::decl(&cfg),
        AutomationStage::decl(&cfg),
        AutomationReceipt::decl(&cfg),
        AutomationRunEvent::decl(&cfg),
        AutomationExecution::decl(&cfg),
        RoutineView::decl(&cfg),
        AutomationMemory::decl(&cfg),
        AutomationClosing::decl(&cfg),
        RoutineRun::decl(&cfg),
        AutomationQueue::decl(&cfg),
        AutomationInflight::decl(&cfg),
        RoutineSource::decl(&cfg),
        AutomationDoc::decl(&cfg),
        AutomationCheck::decl(&cfg),
        AutomationReview::decl(&cfg),
        AutomationTrashEntry::decl(&cfg),
        ProjectSettingsView::decl(&cfg),
        WorkspaceAccessView::decl(&cfg),
        WorkspaceAccessRow::decl(&cfg),
        SettingsScope::decl(&cfg),
        SafetyRowView::decl(&cfg),
        SafetyView::decl(&cfg),
        ProfileAdminRow::decl(&cfg),
        ProfileAdminView::decl(&cfg),
        ProfileChangeReq::decl(&cfg),
        ProfilePlanView::decl(&cfg),
        WorkRootView::decl(&cfg),
        WorkRootCheckView::decl(&cfg),
        HookView::decl(&cfg),
        ResolvePreview::decl(&cfg),
        LibraryFile::decl(&cfg),
        LibraryView::decl(&cfg),
        LinkView::decl(&cfg),
        LibraryReport::decl(&cfg),
        LibraryRef::decl(&cfg),
        LibraryPrunePlan::decl(&cfg),
        LibraryPruneEntry::decl(&cfg),
        LibrarySkip::decl(&cfg),
        FileKind::decl(&cfg),
        FileInfo::decl(&cfg),
        DirListing::decl(&cfg),
        CommandList::decl(&cfg),
        SlashCommand::decl(&cfg),
        CommandSource::decl(&cfg),
        CommandKind::decl(&cfg),
        DirFile::decl(&cfg),
        EvaluatorView::decl(&cfg),
        EvaluationDoc::decl(&cfg),
        EvalStage::decl(&cfg),
        Minutes::decl(&cfg),
        EvalDefect::decl(&cfg),
        Proposal::decl(&cfg),
        ProposalDecision::decl(&cfg),
        EvaluationView::decl(&cfg),
        UsageProviderId::decl(&cfg),
        UsageWindowKind::decl(&cfg),
        UsageWindow::decl(&cfg),
        UsageProblem::decl(&cfg),
        ProviderUsage::decl(&cfg),
        UsageReport::decl(&cfg),
    ];
    let mut out = String::from(
        "// 생성 파일: crates/aaa-proto의 Rust 타입에서 만든다. 손으로 고치지 않는다.\n// 갱신: AAA_UPDATE_TS=1 cargo test -p aaa-proto typescript\n\n",
    );
    for d in decls {
        out.push_str("export ");
        out.push_str(&d.lines().map(str::trim_end).collect::<Vec<_>>().join("\n"));
        out.push_str("\n\n");
    }
    out.push_str(&install::typescript());
    out
}

#[cfg(test)]
mod ts_tests {
    #[test]
    fn old_summary_response_has_no_invented_progress() {
        let old = serde_json::json!({
            "project": "demo", "thread": "t-0001", "text": "# 목표\n표시 확인", "goal": "표시 확인",
            "origin": null, "now": ["화면 확인"], "done": ["준비"], "decided": [], "changed": [], "decisions": [],
            "at_ms": 1, "by": "coordinator", "follow_ups": 0, "pending": 0, "pending_lines": [], "task": null
        });
        let v: super::ThreadSummaryView = serde_json::from_value(old).unwrap();
        assert!(v.progress.is_empty());
        assert_eq!(v.now, ["화면 확인"]);
        assert_eq!(v.done, ["준비"]);
    }

    /// 생성된 TS 타입이 Rust 정본과 같은지 본다. 다르면 AAA_UPDATE_TS=1로 다시 만든다.
    #[test]
    fn typescript_bindings_are_current() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/src/gen/proto.ts");
        let want = super::typescript_bindings();
        if std::env::var_os("AAA_UPDATE_TS").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &want).unwrap();
        }
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(have == want, "web/src/gen/proto.ts가 오래됐다. AAA_UPDATE_TS=1 cargo test -p aaa-proto typescript");
    }
}
