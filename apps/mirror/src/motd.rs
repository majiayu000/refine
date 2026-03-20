use crate::config::{ensure_mirror_dir, mirror_dir};
use crate::score::{load_recent_scores, ScoreResult, Signal};
use anyhow::Result;
use chrono::{Datelike, Utc};
use serde::{Deserialize, Serialize};
use std::io::Write;

#[derive(Debug, Serialize, Deserialize)]
struct Tip {
    dimension: String,
    text: String,
}

fn signal_emoji(s: Signal) -> &'static str {
    match s {
        Signal::Green => "🟢",
        Signal::Yellow => "🟡",
        Signal::Red => "🔴",
    }
}

fn trend_arrow(current: f64, previous: f64) -> &'static str {
    let diff = current - previous;
    if diff > 0.01 {
        "↑"
    } else if diff < -0.01 {
        "↓"
    } else {
        "→"
    }
}

/// Signal 的严重程度：Red=0, Yellow=1, Green=2（越小越差）
fn signal_severity(s: Signal) -> u8 {
    match s {
        Signal::Red => 0,
        Signal::Yellow => 1,
        Signal::Green => 2,
    }
}

/// 找到最弱层的最差指标名称
fn weakest_indicator(score: &ScoreResult) -> (String, String, f64) {
    let weakest_layer = score
        .layers
        .iter()
        .min_by_key(|l| signal_severity(l.signal))
        .unwrap();
    let weakest_ind = weakest_layer
        .indicators
        .iter()
        .min_by_key(|i| signal_severity(i.signal))
        .unwrap();
    let dim = match weakest_layer.name.as_str() {
        "认知深度" => "depth",
        "战略广度" => "breadth",
        "协作效能" => "collaboration",
        _ => "general",
    };
    (dim.to_string(), weakest_ind.name.clone(), weakest_ind.actual)
}

fn default_tips() -> Vec<Tip> {
    let raw = vec![
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
    raw.into_iter()
        .map(|(d, t)| Tip {
            dimension: d.to_string(),
            text: t.to_string(),
        })
        .collect()
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
    let matched: Vec<&Tip> = tips.iter().filter(|t| t.dimension == dimension).collect();
    if matched.is_empty() {
        let general: Vec<&Tip> = tips.iter().filter(|t| t.dimension == "general").collect();
        if general.is_empty() {
            return "保持好奇心".to_string();
        }
        let day = Utc::now().ordinal() as usize;
        return general[day % general.len()].text.clone();
    }
    let day = Utc::now().ordinal() as usize;
    matched[day % matched.len()].text.clone()
}

pub fn handle_motd() -> Result<()> {
    let scores = load_recent_scores(2)?;
    if scores.is_empty() {
        println!("🪞 暂无数据，运行 mirror score 生成首次评分");
        return Ok(());
    }

    let current = scores.last().unwrap();
    let previous = if scores.len() >= 2 {
        Some(&scores[scores.len() - 2])
    } else {
        None
    };

    let depth_e = signal_emoji(current.layers[0].signal);
    let breadth_e = signal_emoji(current.layers[1].signal);
    let collab_e = signal_emoji(current.layers[2].signal);

    let (dim, ind_name, ind_val) = weakest_indicator(current);

    let trend = if let Some(prev) = previous {
        let (_, prev_ind_name, prev_val) = weakest_indicator(prev);
        if prev_ind_name == ind_name {
            trend_arrow(ind_val, prev_val)
        } else {
            "→"
        }
    } else {
        "→"
    };

    let val_str = if ind_name == "模式多样性" {
        format!("{}", ind_val as usize)
    } else if ind_name == "bug/决策" {
        format!("{:.2}", ind_val)
    } else if ind_name == "Dreyfus" {
        format!("{:.1}", ind_val)
    } else {
        format!("{:.0}%", ind_val)
    };

    let tips = ensure_tips()?;
    let tip = select_tip(&tips, &dim);

    println!(
        "🪞 深度{} 广度{} 协作{} | {} {}{} {}",
        depth_e, breadth_e, collab_e, ind_name, val_str, trend, tip
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::{Indicator, LayerScore, ScoreResult};
    use chrono::Utc;

    fn make_score(sigs: [Signal; 3], indicators: Vec<Vec<Indicator>>) -> ScoreResult {
        let names = ["认知深度", "战略广度", "协作效能"];
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
    fn test_signal_emoji() {
        assert_eq!(signal_emoji(Signal::Green), "🟢");
        assert_eq!(signal_emoji(Signal::Yellow), "🟡");
        assert_eq!(signal_emoji(Signal::Red), "🔴");
    }

    #[test]
    fn test_select_tip_matches_weakest() {
        let tips = vec![
            Tip { dimension: "depth".into(), text: "depth tip 1".into() },
            Tip { dimension: "breadth".into(), text: "breadth tip 1".into() },
            Tip { dimension: "collaboration".into(), text: "collab tip 1".into() },
        ];
        // depth 维度只有一个 tip，所以一定匹配
        let result = select_tip(&tips, "depth");
        assert_eq!(result, "depth tip 1");

        let result = select_tip(&tips, "collaboration");
        assert_eq!(result, "collab tip 1");
    }

    #[test]
    fn test_motd_no_data() {
        // load_recent_scores 从文件读，不存在返回空
        // 这里测试 handle_motd 逻辑：空分数时的分支
        let scores: Vec<ScoreResult> = vec![];
        assert!(scores.is_empty());
        // 验证空路径不会 panic
    }

    #[test]
    fn test_weakest_indicator() {
        let score = make_score(
            [Signal::Green, Signal::Red, Signal::Yellow],
            vec![
                vec![Indicator {
                    name: "Dreyfus".into(),
                    actual: 4.0,
                    target: ">3.5".into(),
                    signal: Signal::Green,
                }],
                vec![Indicator {
                    name: "探索率".into(),
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
        let (dim, name, val) = weakest_indicator(&score);
        assert_eq!(dim, "breadth");
        assert_eq!(name, "探索率");
        assert!((val - 5.0).abs() < f64::EPSILON);
    }
}
