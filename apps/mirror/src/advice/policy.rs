use crate::lang::t;
use crate::score::{Indicator, ScoreResult, Signal};
use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PortfolioMode {
    PromoteHoldStop,
    Explore,
    Deepen,
}

impl PortfolioMode {
    pub(crate) fn response_key(self) -> &'static str {
        match self {
            Self::PromoteHoldStop => "promote_hold_stop",
            Self::Explore => "explore",
            Self::Deepen => "deepen",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PortfolioPolicy {
    pub mode: PortfolioMode,
    pub long_exploration: Indicator,
    pub long_fragmentation: Indicator,
    pub recent_exploration: Indicator,
    pub recent_fragmentation: Indicator,
}

fn breadth_indicator(score: &ScoreResult, name: &str, window: &str) -> Result<Indicator> {
    score
        .layers
        .iter()
        .find(|layer| layer.name == "breadth")
        .and_then(|layer| {
            layer
                .indicators
                .iter()
                .find(|indicator| indicator.name == name)
        })
        .cloned()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "portfolio advice requires '{}' in the {} breadth metrics",
                name,
                window
            )
        })
}

pub(crate) fn portfolio_policy(
    long_term: &ScoreResult,
    recent: &ScoreResult,
) -> Result<PortfolioPolicy> {
    let long_exploration = breadth_indicator(long_term, "exploration", "rolling-90-day")?;
    let long_fragmentation = breadth_indicator(long_term, "fragmentation", "rolling-90-day")?;
    let recent_exploration = breadth_indicator(recent, "exploration", "rolling-7-day")?;
    let recent_fragmentation = breadth_indicator(recent, "fragmentation", "rolling-7-day")?;

    let fragmentation_non_green =
        long_fragmentation.signal != Signal::Green || recent_fragmentation.signal != Signal::Green;
    let both_exploration_low =
        long_exploration.signal != Signal::Green && recent_exploration.signal != Signal::Green;
    let mode = if fragmentation_non_green {
        PortfolioMode::PromoteHoldStop
    } else if both_exploration_low {
        PortfolioMode::Explore
    } else {
        PortfolioMode::Deepen
    };

    Ok(PortfolioPolicy {
        mode,
        long_exploration,
        long_fragmentation,
        recent_exploration,
        recent_fragmentation,
    })
}

pub(crate) fn deterministic_advice(policy: &PortfolioPolicy) -> String {
    match policy.mode {
        PortfolioMode::PromoteHoldStop => t!(
            format!(
                "Promote / Hold / Stop: promote the strongest evidenced project, hold at most one bounded validation, and stop the weakest one-off thread unless it produces a named result this week. One-off share is {:.1}% over 90 days and {:.1}% over 7 days; keep the active portfolio closed to additions.",
                policy.long_fragmentation.actual, policy.recent_fragmentation.actual
            ),
            format!(
                "晋升 / 保留 / 退出：晋升证据最强的项目，最多保留一项有边界的验证；最弱的一次性线程若本周没有产出具名结果就退出。90 天与 7 天的一次性项目占比分别为 {:.1}% 和 {:.1}%，本周项目组合不增加任何条目。",
                policy.long_fragmentation.actual, policy.recent_fragmentation.actual
            )
        ),
        PortfolioMode::Explore => t!(
            format!(
                "Run one bounded exploration inside an existing project and record a keep/stop decision. Exploration is {:.1}% over 90 days and {:.1}% over 7 days while fragmentation remains green in both windows.",
                policy.long_exploration.actual, policy.recent_exploration.actual
            ),
            format!(
                "在现有项目内做一次有边界的探索，并记录保留或退出决定。90 天与 7 天探索率分别为 {:.1}% 和 {:.1}%，且两个窗口的碎片化均为绿灯。",
                policy.long_exploration.actual, policy.recent_exploration.actual
            )
        ),
        PortfolioMode::Deepen => t!(
            format!(
                "Hold the current portfolio and deepen the strongest active project with one named validation. Exploration is {:.1}% over 90 days and {:.1}% over 7 days, so expansion is not the priority.",
                policy.long_exploration.actual, policy.recent_exploration.actual
            ),
            format!(
                "保持当前项目组合，在证据最强的活跃项目中完成一项具名验证。90 天与 7 天探索率分别为 {:.1}% 和 {:.1}%，扩张不是当前优先级。",
                policy.long_exploration.actual, policy.recent_exploration.actual
            )
        ),
    }
}

pub(crate) fn deterministic_short(mode: PortfolioMode) -> String {
    match mode {
        PortfolioMode::PromoteHoldStop => t!("Promote hold stop", "晋升保留退出").to_string(),
        PortfolioMode::Explore => t!("Bound one exploration", "限定一次探索").to_string(),
        PortfolioMode::Deepen => t!("Deepen current portfolio", "深挖当前组合").to_string(),
    }
}

#[cfg(test)]
pub(crate) fn breadth_score(
    exploration: f64,
    exploration_signal: Signal,
    fragmentation: f64,
    fragmentation_signal: Signal,
) -> ScoreResult {
    let mut score = ScoreResult::default();
    score.layers[1].indicators = vec![
        Indicator {
            name: "exploration".into(),
            actual: exploration,
            target: String::new(),
            signal: exploration_signal,
        },
        Indicator {
            name: "fragmentation".into(),
            actual: fragmentation,
            target: String::new(),
            signal: fragmentation_signal,
        },
    ];
    score
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_prioritizes_fragmentation_over_exploration() {
        let green = breadth_score(20.0, Signal::Green, 5.0, Signal::Green);
        let low = breadth_score(10.0, Signal::Red, 5.0, Signal::Green);
        let fragmented = breadth_score(30.0, Signal::Green, 35.0, Signal::Red);

        assert_eq!(
            portfolio_policy(&green, &fragmented).unwrap().mode,
            PortfolioMode::PromoteHoldStop
        );
        assert_eq!(
            portfolio_policy(&fragmented, &green).unwrap().mode,
            PortfolioMode::PromoteHoldStop
        );
        assert_eq!(
            portfolio_policy(&low, &low).unwrap().mode,
            PortfolioMode::Explore
        );
        assert_eq!(
            portfolio_policy(&low, &green).unwrap().mode,
            PortfolioMode::Deepen
        );
    }

    #[test]
    fn regression_14_4_29_2_and_high_one_off_never_expands() {
        let long_term = breadth_score(14.4, Signal::Yellow, 35.0, Signal::Red);
        let recent = breadth_score(29.2, Signal::Green, 48.0, Signal::Red);
        let policy = portfolio_policy(&long_term, &recent).unwrap();
        let rendered = deterministic_advice(&policy);

        assert_eq!(policy.mode, PortfolioMode::PromoteHoldStop);
        assert!(rendered.contains("Promote / Hold / Stop"));
        assert!(!rendered.to_lowercase().contains("new project"));
        assert!(!rendered.to_lowercase().contains("new direction"));
    }

    #[test]
    fn missing_required_window_metric_fails_clearly() {
        let missing = ScoreResult::default();
        let valid = breadth_score(20.0, Signal::Green, 5.0, Signal::Green);
        let error = portfolio_policy(&missing, &valid).unwrap_err();
        assert!(error.to_string().contains("rolling-90-day"));
        assert!(error.to_string().contains("exploration"));
    }
}
