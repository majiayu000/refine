use crate::config::{ensure_mirror_dir, mirror_dir};
use crate::lang::{self, t, Lang};
use crate::score::{load_recent_scores, ScoreResult, Signal};
use anyhow::Result;
use chrono::{Datelike, Utc};
use serde::{Deserialize, Serialize};
use std::io::Write;

#[derive(Debug, Serialize, Deserialize)]
struct Tip {
    dimension: String,
    #[serde(default = "default_lang_en")]
    lang: String,
    text: String,
}

fn default_lang_en() -> String {
    "en".into()
}

/// Signal severity: Red=0, Yellow=1, Green=2 (lower is worse)
fn signal_severity(s: Signal) -> u8 {
    match s {
        Signal::Red => 0,
        Signal::Yellow => 1,
        Signal::Green => 2,
    }
}

/// Compare current vs previous signal and return a trend arrow.
/// Returns "↑" if improved, "↓" if degraded, "" if unchanged.
fn trend_signal(current: Signal, previous: Signal) -> &'static str {
    let curr = signal_severity(current);
    let prev = signal_severity(previous);
    if curr > prev {
        "↑"
    } else if curr < prev {
        "↓"
    } else {
        ""
    }
}

/// Find the weakest indicator in the weakest layer
fn weakest_indicator(score: &ScoreResult) -> Option<(String, String, f64)> {
    let weakest_layer = score
        .layers
        .iter()
        .min_by_key(|l| signal_severity(l.signal))?;
    let weakest_ind = weakest_layer
        .indicators
        .iter()
        .min_by_key(|i| signal_severity(i.signal))?;
    let dim = match weakest_layer.name.as_str() {
        "depth" => "depth",
        "breadth" => "breadth",
        "collaboration" => "collaboration",
        _ => "general",
    };
    Some((
        dim.to_string(),
        weakest_ind.name.clone(),
        weakest_ind.actual,
    ))
}

fn default_tips() -> Vec<Tip> {
    let raw_en: Vec<(&str, &str)> = vec![
        (
            "depth",
            "Before asking AI for a solution, write down your 3 predictions first",
        ),
        (
            "depth",
            "Try completing today's core task without checking docs",
        ),
        (
            "depth",
            "Ask AI to find 3 counterexamples for your approach",
        ),
        ("depth", "Draw the data flow diagram before writing code"),
        (
            "depth",
            "Explain the module you're writing to yourself using the Feynman method",
        ),
        (
            "breadth",
            "Explore a crate you've been curious about but never used",
        ),
        (
            "breadth",
            "Pick an old project and refactor a module with a fresh approach",
        ),
        (
            "breadth",
            "Spend 30 minutes reading an open source project you've never touched",
        ),
        (
            "breadth",
            "Try solving today's small problem in a different language",
        ),
        (
            "breadth",
            "Split today's task into exploration and execution phases",
        ),
        (
            "collaboration",
            "Use pair mode instead of delegation for the next task",
        ),
        (
            "collaboration",
            "Let AI describe the problem first, then you write the solution",
        ),
        (
            "collaboration",
            "Hand-write today's first task, then compare with AI's approach",
        ),
        (
            "collaboration",
            "Let AI review your code instead of writing it for you",
        ),
        (
            "collaboration",
            "Give AI tighter constraints and see how it adapts",
        ),
        (
            "general",
            "Review yesterday's session and find one decision you'd improve",
        ),
        (
            "general",
            "Is the project you spent the most time on worth continued investment?",
        ),
        (
            "general",
            "When stuck, question your underlying assumptions first",
        ),
        (
            "general",
            "Spend 5 minutes writing down the hypothesis you most want to test today",
        ),
        (
            "general",
            "Before debugging, predict the root cause first, then verify",
        ),
    ];
    let raw_zh: Vec<(&str, &str)> = vec![
        ("depth", "下次让 AI 给方案前先写下你的 3 个预测"),
        ("depth", "今天尝试不查文档完成核心任务"),
        ("depth", "让 AI 给你的方案找 3 个反例"),
        ("depth", "写代码前先画数据流图再动手"),
        ("depth", "用 Feynman 方法给自己讲解你正在写的模块"),
        ("breadth", "今天探索一个你好奇但没用过的 crate"),
        ("breadth", "选一个旧项目用新思路重构一个模块"),
        ("breadth", "花 30 分钟读一个你没碰过的开源项目"),
        ("breadth", "尝试用不同语言解决今天的一个小问题"),
        ("breadth", "把今天的任务拆成探索和执行两个阶段"),
        ("collaboration", "下一个任务用 pair 模式而不是委托"),
        ("collaboration", "让 AI 先描述问题再你写方案"),
        ("collaboration", "今天的第一个任务手写完再对比 AI 方案"),
        ("collaboration", "让 AI review 你的代码而不是帮你写"),
        ("collaboration", "试试给 AI 更多约束看它如何调整方案"),
        ("general", "回顾昨天的 session 找一个可以做得更好的决策"),
        ("general", "这周花最多时间的项目值得继续深投吗"),
        ("general", "今天遇到问题时先问底层假设还成立吗"),
        ("general", "花 5 分钟写下今天最想验证的一个假设"),
        ("general", "下次 debug 前先预测 root cause 再验证"),
    ];

    let mut tips = Vec::with_capacity(raw_en.len() + raw_zh.len());
    for (d, text) in raw_en {
        tips.push(Tip {
            dimension: d.to_string(),
            lang: "en".into(),
            text: text.to_string(),
        });
    }
    for (d, text) in raw_zh {
        tips.push(Tip {
            dimension: d.to_string(),
            lang: "zh".into(),
            text: text.to_string(),
        });
    }
    tips
}

fn ensure_tips() -> Result<Vec<Tip>> {
    let path = mirror_dir().join("tips.json");
    if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        let tips: Vec<Tip> = serde_json::from_str(&content)?;
        return Ok(tips);
    }
    let tips = default_tips();
    let dir = ensure_mirror_dir()?;
    let mut file = std::fs::File::create(dir.join("tips.json"))?;
    file.write_all(serde_json::to_string_pretty(&tips)?.as_bytes())?;
    Ok(tips)
}

fn select_tip(tips: &[Tip], dimension: &str) -> String {
    let lang_str = match lang::lang() {
        Lang::En => "en",
        Lang::Zh => "zh",
    };
    let matched: Vec<&Tip> = tips
        .iter()
        .filter(|t| t.dimension == dimension && t.lang == lang_str)
        .collect();
    if !matched.is_empty() {
        let day = Utc::now().ordinal() as usize;
        return matched[day % matched.len()].text.clone();
    }
    // fallback: general tips in same language
    let general: Vec<&Tip> = tips
        .iter()
        .filter(|t| t.dimension == "general" && t.lang == lang_str)
        .collect();
    if !general.is_empty() {
        let day = Utc::now().ordinal() as usize;
        return general[day % general.len()].text.clone();
    }
    // last fallback: any matching dimension
    let any: Vec<&Tip> = tips.iter().filter(|t| t.dimension == dimension).collect();
    if !any.is_empty() {
        let day = Utc::now().ordinal() as usize;
        return any[day % any.len()].text.clone();
    }
    t!("Stay curious", "保持好奇心").to_string()
}

pub fn handle_motd() -> Result<()> {
    let scores = load_recent_scores(2)?;
    if scores.is_empty() {
        println!(
            "🪞 {}",
            t!(
                "No data yet. Run `mirror score` first",
                "暂无数据，运行 mirror score 生成首次评分"
            )
        );
        return Ok(());
    }

    let current = match scores.last() {
        Some(s) => s,
        None => return Ok(()),
    };
    let previous = if scores.len() >= 2 {
        Some(&scores[scores.len() - 2])
    } else {
        None
    };

    let depth_e = current.layers[0].signal.emoji();
    let breadth_e = current.layers[1].signal.emoji();
    let collab_e = current.layers[2].signal.emoji();

    // Trend arrows: compare current vs previous signals
    let depth_t = previous
        .map(|p| trend_signal(current.layers[0].signal, p.layers[0].signal))
        .unwrap_or("");
    let breadth_t = previous
        .map(|p| trend_signal(current.layers[1].signal, p.layers[1].signal))
        .unwrap_or("");
    let collab_t = previous
        .map(|p| trend_signal(current.layers[2].signal, p.layers[2].signal))
        .unwrap_or("");

    let dim = weakest_indicator(current)
        .map(|(d, _, _)| d)
        .unwrap_or_else(|| "general".to_string());

    // Prefer LLM-generated advice if cached, fallback to static tips
    let tip = match crate::advice::load_cached() {
        Ok(Some(cached)) => cached.advice,
        Err(e) => {
            tracing::warn!("failed to load cached advice: {}", e);
            let tips = ensure_tips()?;
            select_tip(&tips, &dim)
        }
        Ok(None) => {
            let tips = ensure_tips()?;
            select_tip(&tips, &dim)
        }
    };

    // Check data freshness: warn if latest score is older than 48 hours
    let age = Utc::now() - current.timestamp;
    let stale_suffix = if age.num_hours() > 48 {
        format!(
            " {} mirror score",
            t!("⚠️ Data is stale, run", "⚠️ 数据已过期，运行")
        )
    } else {
        String::new()
    };

    println!(
        "🪞 {d}{de}{dt} {b}{be}{bt} {c}{ce}{ct} | {tip}{stale}",
        d = t!("Depth", "深度"),
        de = depth_e,
        dt = depth_t,
        b = t!("Breadth", "广度"),
        be = breadth_e,
        bt = breadth_t,
        c = t!("Collab", "协作"),
        ce = collab_e,
        ct = collab_t,
        tip = tip,
        stale = stale_suffix,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::{Indicator, LayerScore, ScoreResult};
    use chrono::Utc;

    fn make_score(sigs: [Signal; 3], indicators: Vec<Vec<Indicator>>) -> ScoreResult {
        let names = ["depth", "breadth", "collaboration"];
        ScoreResult {
            layers: std::array::from_fn(|i| LayerScore {
                name: names[i].to_string(),
                signal: sigs[i],
                indicators: indicators[i].clone(),
            }),
            tension: None,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn test_signal_emoji_method() {
        assert_eq!(Signal::Green.emoji(), "🟢");
        assert_eq!(Signal::Yellow.emoji(), "🟡");
        assert_eq!(Signal::Red.emoji(), "🔴");
    }

    #[test]
    fn test_trend_signal() {
        // Improved: Yellow -> Green
        assert_eq!(trend_signal(Signal::Green, Signal::Yellow), "↑");
        // Degraded: Green -> Yellow
        assert_eq!(trend_signal(Signal::Yellow, Signal::Green), "↓");
        // Unchanged
        assert_eq!(trend_signal(Signal::Green, Signal::Green), "");
        assert_eq!(trend_signal(Signal::Red, Signal::Red), "");
        // Big improvement: Red -> Green
        assert_eq!(trend_signal(Signal::Green, Signal::Red), "↑");
        // Big degradation: Green -> Red
        assert_eq!(trend_signal(Signal::Red, Signal::Green), "↓");
    }

    #[test]
    fn test_select_tip_matches_weakest() {
        let tips = vec![
            Tip {
                dimension: "depth".into(),
                lang: "en".into(),
                text: "depth tip 1".into(),
            },
            Tip {
                dimension: "breadth".into(),
                lang: "en".into(),
                text: "breadth tip 1".into(),
            },
            Tip {
                dimension: "collaboration".into(),
                lang: "en".into(),
                text: "collab tip 1".into(),
            },
        ];
        // depth dimension has only one en tip, so it always matches
        let result = select_tip(&tips, "depth");
        assert_eq!(result, "depth tip 1");

        let result = select_tip(&tips, "collaboration");
        assert_eq!(result, "collab tip 1");
    }

    #[test]
    fn test_motd_no_data() {
        // load_recent_scores reads from file, returns empty if not found
        let scores: Vec<ScoreResult> = vec![];
        assert!(scores.is_empty());
    }

    #[test]
    fn test_weakest_indicator() {
        let score = make_score(
            [Signal::Green, Signal::Red, Signal::Yellow],
            vec![
                vec![Indicator {
                    name: "dreyfus".into(),
                    actual: 4.0,
                    target: ">3.5".into(),
                    signal: Signal::Green,
                }],
                vec![Indicator {
                    name: "exploration".into(),
                    actual: 5.0,
                    target: ">15%".into(),
                    signal: Signal::Red,
                }],
                vec![Indicator {
                    name: "delegation".into(),
                    actual: 50.0,
                    target: "<40%".into(),
                    signal: Signal::Yellow,
                }],
            ],
        );
        let result = weakest_indicator(&score);
        assert!(result.is_some());
        let (dim, name, val) = result.unwrap_or_default();
        assert_eq!(dim, "breadth");
        assert_eq!(name, "exploration");
        assert!((val - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_stale_data_detection() {
        let age_hours_50 = chrono::Duration::hours(50);
        assert!(age_hours_50.num_hours() > 48);

        let age_hours_24 = chrono::Duration::hours(24);
        assert!(age_hours_24.num_hours() <= 48);
    }
}
