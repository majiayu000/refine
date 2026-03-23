use crate::lang::{lang, Lang};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IndicatorFormat {
    Percent0,
    Fixed1,
    Fixed2,
    Integer,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct IndicatorSpec {
    pub key: &'static str,
    pub format: IndicatorFormat,
    pub higher_is_better: bool,
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

const INDICATOR_SPECS: [IndicatorSpec; 11] = [
    IndicatorSpec {
        key: "dreyfus",
        format: IndicatorFormat::Fixed1,
        higher_is_better: true,
        aliases: &["Dreyfus"],
        display_en: "Dreyfus",
        display_zh: "Dreyfus",
    },
    IndicatorSpec {
        key: "decision_quality",
        format: IndicatorFormat::Percent0,
        higher_is_better: true,
        aliases: &["Decision Quality", "决策质量"],
        display_en: "Decision Quality",
        display_zh: "决策质量",
    },
    IndicatorSpec {
        key: "depth_output",
        format: IndicatorFormat::Percent0,
        higher_is_better: true,
        aliases: &["Depth Output", "深度产出比"],
        display_en: "Depth Output",
        display_zh: "深度产出比",
    },
    IndicatorSpec {
        key: "knowledge_rate",
        format: IndicatorFormat::Fixed1,
        higher_is_better: true,
        aliases: &["Knowledge", "知识获取"],
        display_en: "Knowledge",
        display_zh: "知识获取",
    },
    IndicatorSpec {
        key: "exploration",
        format: IndicatorFormat::Percent0,
        higher_is_better: true,
        aliases: &["Exploration", "探索率", "探索占比"],
        display_en: "Exploration",
        display_zh: "探索率",
    },
    IndicatorSpec {
        key: "deep_invest",
        format: IndicatorFormat::Percent0,
        higher_is_better: true,
        aliases: &["Deep Invest", "深耕率", "深挖率", "深挖占比"],
        display_en: "Deep Invest",
        display_zh: "深耕率",
    },
    IndicatorSpec {
        key: "fragmentation",
        format: IndicatorFormat::Percent0,
        higher_is_better: false,
        aliases: &["Fragmentation", "碎片化"],
        display_en: "Fragmentation",
        display_zh: "碎片化",
    },
    IndicatorSpec {
        key: "delegation",
        format: IndicatorFormat::Percent0,
        higher_is_better: false,
        aliases: &["Delegation", "delegation", "委派率", "委派比"],
        display_en: "delegation",
        display_zh: "委派率",
    },
    IndicatorSpec {
        key: "mode_diversity",
        format: IndicatorFormat::Integer,
        higher_is_better: true,
        aliases: &["Mode Diversity", "模式多样性"],
        display_en: "Mode Diversity",
        display_zh: "模式多样性",
    },
    IndicatorSpec {
        key: "bug_decision",
        format: IndicatorFormat::Fixed2,
        higher_is_better: false,
        aliases: &["Bug/Decision", "bug/decision", "bug/决策"],
        display_en: "bug/decision",
        display_zh: "bug/决策",
    },
    IndicatorSpec {
        key: "friction_density",
        format: IndicatorFormat::Fixed1,
        higher_is_better: false,
        aliases: &["Friction", "摩擦密度"],
        display_en: "Friction",
        display_zh: "摩擦密度",
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

pub(super) fn indicator_higher_is_better(name: &str) -> Option<bool> {
    indicator_spec(name).map(|spec| spec.higher_is_better)
}
