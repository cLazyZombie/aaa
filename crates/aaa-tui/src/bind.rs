//! prefix 명령 키와 도움말 묶음. 기본값은 herdr 0.9.5 `src/config/model.rs` `KeysConfig::default`,
//! 묶음과 순서는 `src/input/keybind_help.rs` `keybind_help_groups`를 따른다(aaa에 없는 동작은 뺐다).

use crossterm::event::{KeyCode, KeyModifiers};

use crate::keys::{self, Combo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Help,
    Detach,
    ReloadConfig,
    OpenNotificationTarget,
    WorkspacePicker,
    NewTab,
    RenameTab,
    CloseTab,
    PreviousTab,
    NextTab,
    SwitchTab(usize),
    SplitVertical,
    SplitHorizontal,
    ClosePane,
    Zoom,
    ResizeMode,
    ToggleSidebar,
    FocusLeft,
    FocusDown,
    FocusUp,
    FocusRight,
    CycleNext,
    CyclePrevious,
    SwapLeft,
    SwapDown,
    SwapUp,
    SwapRight,
}

/// prefix 뒤 키 → 동작(herdr 기본 키)
pub fn prefix_action(c: Combo) -> Option<Action> {
    use Action::*;
    if c.mods == KeyModifiers::NONE
        && let KeyCode::Char(d @ '1'..='9') = c.code
    {
        return Some(SwitchTab(d as usize - '1' as usize));
    }
    PREFIX.iter().find(|(k, _)| *k == c).map(|(_, a)| *a)
}

const PREFIX: &[(Combo, Action)] = &[
    (Combo::plain('?'), Action::Help),
    (Combo::plain('q'), Action::Detach),
    (Combo::shift('r'), Action::ReloadConfig),
    (Combo::plain('o'), Action::OpenNotificationTarget),
    (Combo::plain('w'), Action::WorkspacePicker),
    (Combo::plain('c'), Action::NewTab),
    (Combo::shift('t'), Action::RenameTab),
    (Combo::shift('x'), Action::CloseTab),
    (Combo::plain('p'), Action::PreviousTab),
    (Combo::plain('n'), Action::NextTab),
    (Combo::plain('v'), Action::SplitVertical),
    (Combo::plain('-'), Action::SplitHorizontal),
    (Combo::plain('x'), Action::ClosePane),
    (Combo::plain('z'), Action::Zoom),
    (Combo::plain('r'), Action::ResizeMode),
    (Combo::plain('b'), Action::ToggleSidebar),
    (Combo::plain('h'), Action::FocusLeft),
    (Combo::plain('j'), Action::FocusDown),
    (Combo::plain('k'), Action::FocusUp),
    (Combo::plain('l'), Action::FocusRight),
    (Combo::new(KeyCode::Tab, KeyModifiers::NONE), Action::CycleNext),
    (Combo::new(KeyCode::BackTab, KeyModifiers::NONE), Action::CyclePrevious),
    (Combo::shift('h'), Action::SwapLeft),
    (Combo::shift('j'), Action::SwapDown),
    (Combo::shift('k'), Action::SwapUp),
    (Combo::shift('l'), Action::SwapRight),
];

/// 그 동작의 prefix 뒤 키 이름(`w`, `shift+r`)
pub fn rhs(a: Action) -> String {
    PREFIX.iter().find(|(_, x)| *x == a).map(|(k, _)| keys::label(*k)).unwrap_or_else(|| "unset".into())
}

fn p(a: Action) -> String {
    format!("prefix+{}", rhs(a))
}

pub type Group = (&'static str, Vec<(String, &'static str)>);

/// 도움말 묶음(herdr `keybind_help_groups`와 같은 차례). `prefixes`는 설정한 prefix 키 이름들.
pub fn help_groups(prefixes: &str) -> Vec<Group> {
    use Action::*;
    vec![
        (
            "전체",
            vec![
                (prefixes.to_string(), "prefix 모드"),
                (p(Help), "단축키"),
                (p(Detach), "나가기(세션은 그대로)"),
                (p(ReloadConfig), "설정 다시 읽기"),
                (p(OpenNotificationTarget), "질문·알림 pane으로"),
            ],
        ),
        (
            "탐색",
            vec![
                ("esc".into(), "돌아가기"),
                ("up / down".into(), "목록"),
                ("h / j / k / l / left / right".into(), "포커스 옮기기"),
                ("tab / shift+tab".into(), "pane 차례로"),
                ("enter".into(), "고른 줄 열기"),
                ("1..9".into(), "목록 n번째 열기"),
            ],
        ),
        (
            "목록 / 탭",
            vec![
                (p(WorkspacePicker), "목록 탐색"),
                (p(NewTab), "새 탭"),
                (p(RenameTab), "탭 이름 바꾸기"),
                (p(PreviousTab), "이전 탭"),
                (p(NextTab), "다음 탭"),
                ("prefix+1..9".into(), "탭 1-9로"),
                (p(CloseTab), "탭 닫기"),
            ],
        ),
        (
            "pane",
            vec![
                (p(SplitVertical), "좌우로 나누기"),
                (p(SplitHorizontal), "위아래로 나누기"),
                (p(ClosePane), "pane 닫기"),
                (p(Zoom), "pane 크게 보기"),
                (p(ResizeMode), "크기 모드(h/j/k/l, esc 끝)"),
                (p(ToggleSidebar), "목록 펴기·접기"),
                (p(FocusLeft), "왼쪽 pane"),
                (p(FocusDown), "아래 pane"),
                (p(FocusUp), "위 pane"),
                (p(FocusRight), "오른쪽 pane"),
                (p(CycleNext), "다음 pane"),
                (p(CyclePrevious), "이전 pane"),
                // herdr 도움말에는 없지만 기본 키는 있다(`swap_pane_*`)
                (p(SwapLeft), "왼쪽 pane과 자리 바꾸기"),
                (p(SwapDown), "아래 pane과 자리 바꾸기"),
                (p(SwapUp), "위 pane과 자리 바꾸기"),
                (p(SwapRight), "오른쪽 pane과 자리 바꾸기"),
                ("ctrl+v".into(), "그림 붙여넣기(prefix 없이)"),
            ],
        ),
    ]
}

/// herdr `filter_keybind_help_groups`: 키 이름이나 설명에 검색어가 든 줄만(대소문자 무시)
pub fn filter(groups: Vec<Group>, query: &str) -> Vec<Group> {
    if query.is_empty() {
        return groups;
    }
    let q = query.to_lowercase();
    groups
        .into_iter()
        .filter_map(|(g, entries)| {
            let entries: Vec<_> = entries
                .into_iter()
                .filter(|(k, l)| k.to_lowercase().contains(&q) || l.to_lowercase().contains(&q))
                .collect();
            (!entries.is_empty()).then_some((g, entries))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn herdr_default_keys() {
        use Action::*;
        let k = |s: &str| prefix_action(keys::parse_combo(s).unwrap());
        assert_eq!(k("q"), Some(Detach));
        assert_eq!(k("v"), Some(SplitVertical));
        assert_eq!(k("minus"), Some(SplitHorizontal));
        assert_eq!(k("b"), Some(ToggleSidebar));
        assert_eq!(k("w"), Some(WorkspacePicker));
        assert_eq!(k("tab"), Some(CycleNext));
        assert_eq!(k("shift+tab"), Some(CyclePrevious));
        assert_eq!(k("shift+r"), Some(ReloadConfig));
        assert_eq!(k("3"), Some(SwitchTab(2)));
        assert_eq!(k("shift+t"), Some(RenameTab));
        assert_eq!(k("shift+x"), Some(CloseTab));
        assert_eq!(k("x"), Some(ClosePane));
        assert_eq!(k("z"), Some(Zoom));
        assert_eq!(k("r"), Some(ResizeMode));
        assert_eq!(k("shift+h"), Some(SwapLeft));
        assert_eq!(k("shift+l"), Some(SwapRight));
        // tmux식 예전 키는 없다
        for old in ["d", "t", "s", "%", "\"", "|"] {
            assert_eq!(k(old), None, "{old}");
        }
        assert_eq!(rhs(WorkspacePicker), "w");
        assert_eq!(rhs(ReloadConfig), "shift+r");
    }

    #[test]
    fn help_filter_matches_keys_and_labels() {
        let g = filter(help_groups("ctrl+b"), "나누기");
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].1.len(), 2);
        assert!(filter(help_groups("ctrl+b"), "prefix+w").iter().any(|(_, e)| e.iter().any(|(k, _)| k == "prefix+w")));
        assert!(filter(help_groups("ctrl+b"), "없는말").is_empty());
    }
}
