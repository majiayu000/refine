use crate::lang::{lang, Lang};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IndicatorFormat {
    Percent0,
    Fixed1,
    Fixed2,
    Integer,
}

/// 指标的"更好"方向。
///
/// `Band` 表示区间型指标（如 deep_invest 15-30% 为佳）：越高不等于越好，
/// 因此任何"相对滑动均值"的比较对它都没有意义，必须显式表达为无趋势，
/// 而不是当成 HigherBetter 撒谎。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Direction {
    HigherBetter,
    LowerBetter,
    Band,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct IndicatorSpec {
    pub key: &'static str,
    pub format: IndicatorFormat,
    pub direction: Direction,
    aliases: &'static [&'static str],
    display_en: &'static str,
    display_zh: &'static str,
}

impl IndicatorSpec {
    fn display_name(self) -> &'static str {
        match lang() {
            Lang::En => self.display_en,
            Lang::Zh => self.display_zh,
        }
    }

    fn matches(self, raw: &str) -> bool {
        self.key == raw || self.aliases.contains(&raw)
    }
}

const INDICATOR_SPECS: [IndicatorSpec; 8] = [
    IndicatorSpec {
        key: "dreyfus",
        format: IndicatorFormat::Fixed1,
        direction: Direction::HigherBetter,
        aliases: &["Dreyfus"],
        display_en: "Dreyfus",
        display_zh: "Dreyfus",
    },
    IndicatorSpec {
        key: "decision_quality",
        format: IndicatorFormat::Percent0,
        direction: Direction::HigherBetter,
        aliases: &["Decision Quality", "决策质量"],
        display_en: "Decision Quality",
        display_zh: "决策质量",
    },
    IndicatorSpec {
        key: "exploration",
        format: IndicatorFormat::Percent0,
        direction: Direction::HigherBetter,
        aliases: &["Exploration", "探索率", "探索占比"],
        display_en: "Exploration",
        display_zh: "探索率",
    },
    IndicatorSpec {
        key: "deep_invest",
        format: IndicatorFormat::Percent0,
        direction: Direction::Band,
        aliases: &["Deep Invest", "深耕率", "深挖率", "深挖占比"],
        display_en: "Deep Invest",
        display_zh: "深耕率",
    },
    IndicatorSpec {
        key: "fragmentation",
        format: IndicatorFormat::Percent0,
        direction: Direction::LowerBetter,
        aliases: &["Fragmentation", "碎片化"],
        display_en: "Fragmentation",
        display_zh: "碎片化",
    },
    IndicatorSpec {
        key: "delegation",
        format: IndicatorFormat::Percent0,
        direction: Direction::LowerBetter,
        aliases: &["Delegation", "delegation", "委派率", "委派比"],
        display_en: "delegation",
        display_zh: "委派率",
    },
    IndicatorSpec {
        key: "mode_diversity",
        format: IndicatorFormat::Integer,
        direction: Direction::HigherBetter,
        aliases: &["Mode Diversity", "模式多样性"],
        display_en: "Mode Diversity",
        display_zh: "模式多样性",
    },
    IndicatorSpec {
        key: "bug_decision",
        format: IndicatorFormat::Fixed2,
        direction: Direction::LowerBetter,
        aliases: &["Bug/Decision", "bug/decision", "bug/决策"],
        display_en: "bug/decision",
        display_zh: "bug/决策",
    },
];

pub(super) fn indicator_specs() -> &'static [IndicatorSpec] {
    &INDICATOR_SPECS
}

fn find_indicator_spec(raw: &str) -> Option<&'static IndicatorSpec> {
    indicator_specs().iter().find(|spec| spec.matches(raw))
}

pub(super) fn canonical_indicator_key(raw: &str) -> &str {
    find_indicator_spec(raw).map_or(raw, |spec| spec.key)
}

pub(super) fn indicator_spec(key: &str) -> Option<&'static IndicatorSpec> {
    indicator_specs()
        .iter()
        .find(|spec| spec.key == canonical_indicator_key(key))
}

pub(super) fn indicator_display_name(key: &str) -> &'static str {
    indicator_spec(key)
        .map(|spec| spec.display_name())
        .unwrap_or("unknown")
}

pub(super) fn format_indicator_value(name: &str, actual: f64) -> String {
    match indicator_spec(name)
        .map(|spec| spec.format)
        .unwrap_or(IndicatorFormat::Percent0)
    {
        IndicatorFormat::Percent0 => format!("{:.0}%", actual),
        IndicatorFormat::Fixed1 => format!("{:.1}", actual),
        IndicatorFormat::Fixed2 => format!("{:.2}", actual),
        IndicatorFormat::Integer => format!("{}", actual as usize),
    }
}

pub(super) fn indicator_direction(name: &str) -> Option<Direction> {
    indicator_spec(name).map(|spec| spec.direction)
}
