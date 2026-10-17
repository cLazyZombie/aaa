//! 탭 줄의 탭 이름표 자리. herdr 0.9.5 `src/client/shell/tabs.rs` `render_tab_bar`(마우스 단추를 뺀 모양)·
//! `max_tab_scroll`·`centered_tab_scroll`을 옮겼다: 이름표 폭은 이름+4(최소 8), 사이 한 칸,
//! 넘치면 지금 탭이 가운데 오게 밀고 잘린 쪽에 `…`를 둔다.

use crate::unicode_width;

/// 이름표 최소 폭(herdr `MIN_TAB_WIDTH`)
pub const MIN_TAB_WIDTH: u16 = 8;

/// 그릴 이름표 하나: 탭 차례, 자리, 폭(마지막 것은 잘려 좁을 수 있다)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chip {
    pub index: usize,
    pub x: u16,
    pub w: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Strip {
    pub chips: Vec<Chip>,
    /// 앞쪽 탭이 가려졌다(왼쪽 끝에 `…`)
    pub more_left: bool,
    /// 뒤쪽 탭이 가려졌다(오른쪽 끝에 `…`)
    pub more_right: bool,
}

pub fn desired_width(label: &str) -> u16 {
    (unicode_width(label) as u16).saturating_add(4).max(MIN_TAB_WIDTH)
}

/// `labels`를 `x`부터 `width` 칸에 놓는다. `focused` 탭이 보이게 민다.
pub fn layout(labels: &[String], focused: usize, x: u16, width: u16) -> Strip {
    let widths: Vec<u16> = labels.iter().map(|l| desired_width(l)).collect();
    let total =
        widths.iter().fold(0u16, |a, w| a.saturating_add(*w)).saturating_add(labels.len().saturating_sub(1) as u16);
    let overflow = total > width;
    let scroll = if overflow {
        centered_tab_scroll(focused.min(labels.len().saturating_sub(1)), &widths, width)
            .min(max_tab_scroll(&widths, width))
    } else {
        0
    };
    let right = x + width;
    let mut at = x;
    let mut chips = Vec::new();
    for (index, desired) in widths.iter().enumerate().skip(scroll) {
        let w = (*desired).min(right.saturating_sub(at));
        if w == 0 {
            break;
        }
        chips.push(Chip { index, x: at, w });
        at = at.saturating_add(w + 1);
        if w < *desired {
            break;
        }
    }
    let more_left = chips.first().is_some_and(|c| c.index > 0);
    let more_right = chips.last().is_some_and(|c| c.index + 1 < labels.len());
    Strip { chips, more_left, more_right }
}

/// herdr `centered_tab_scroll`: 지금 탭의 가운데가 칸 가운데에 가장 가까워지는 시작 탭
fn centered_tab_scroll(focused: usize, widths: &[u16], available: u16) -> usize {
    let mut best = focused;
    let mut best_distance = u16::MAX;
    for start in 0..=focused {
        let before = widths
            .iter()
            .skip(start)
            .take(focused.saturating_sub(start))
            .fold(0u16, |w, tab| w.saturating_add(tab + 1));
        if before >= available {
            continue;
        }
        let focused_width = widths[focused].min(available.saturating_sub(before));
        let center = before.saturating_mul(2).saturating_add(focused_width);
        let distance = center.abs_diff(available);
        if distance <= best_distance {
            best_distance = distance;
            best = start;
        }
    }
    best
}

/// herdr `max_tab_scroll`: 마지막 탭까지 다 보이는 가장 긴 꼬리의 시작
fn max_tab_scroll(widths: &[u16], available: u16) -> usize {
    let Some((&last, preceding)) = widths.split_last() else { return 0 };
    let mut start = preceding.len();
    let mut used = u32::from(last);
    for w in preceding.iter().rev() {
        let required = used + 1 + u32::from(*w);
        if required > u32::from(available) {
            break;
        }
        used = required;
        start -= 1;
    }
    start
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(n: usize) -> Vec<String> {
        (1..=n).map(|i| i.to_string()).collect()
    }

    #[test]
    fn chips_are_label_plus_four_at_least_eight() {
        let s = layout(&["1".into(), "빌드 서버".into()], 0, 10, 80);
        assert_eq!(s.chips, vec![Chip { index: 0, x: 10, w: 8 }, Chip { index: 1, x: 19, w: 13 }]);
        assert!(!s.more_left && !s.more_right);
    }

    #[test]
    fn overflow_centers_the_focused_tab() {
        // 탭 10개(폭 8)는 89칸이 필요하다. 40칸이면 지금 탭(7번째)이 가운데 오게 밀고 양쪽에 …
        let s = layout(&labels(10), 6, 0, 40);
        assert!(s.chips.iter().any(|c| c.index == 6));
        let c = s.chips.iter().find(|c| c.index == 6).unwrap();
        assert!((c.x * 2 + c.w).abs_diff(40) <= 9, "{s:?}");
        assert!(s.more_left && s.more_right);
        // 첫 탭이면 밀지 않는다
        let s = layout(&labels(10), 0, 0, 40);
        assert_eq!(s.chips[0], Chip { index: 0, x: 0, w: 8 });
        assert!(!s.more_left && s.more_right);
        // 마지막 탭이면 꼬리가 다 보이게(herdr max_tab_scroll)
        let s = layout(&labels(10), 9, 0, 40);
        assert_eq!(s.chips.last().unwrap().index, 9);
        assert_eq!(s.chips.last().unwrap().w, 8);
        assert!(s.more_left && !s.more_right);
        assert_eq!(max_tab_scroll(&[8, 13], 21), 1);
        assert_eq!(max_tab_scroll(&[8, 13], 22), 0);
    }
}
