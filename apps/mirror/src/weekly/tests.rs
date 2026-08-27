use super::*;
use crate::score::{Indicator, Signal};
use refine_core::knowledge::{Item, ItemId, ItemType, RestoreParams, Tag};
use refine_core::session::{GlobalStats, ProjectCluster};
use std::collections::{HashMap, HashSet};

fn make_weekly_record(seed: usize) -> WeeklyRecord {
    WeeklyRecord {
        week_end: Utc::now() + Duration::weeks(seed as i64),
        scores: [
            LayerSignal {
                name: "depth".into(),
                signal: "green".into(),
            },
            LayerSignal {
                name: "breadth".into(),
                signal: "yellow".into(),
            },
            LayerSignal {
                name: "collaboration".into(),
                signal: "red".into(),
            },
        ],
        suggestions: Vec::new(),
    }
}

fn make_item_at(time: DateTime<Utc>, idx: usize) -> Item {
    Item::restore(RestoreParams {
        id: ItemId::new(),
        item_type: ItemType::Observation,
        title: format!("obs-{}", idx),
        summary: format!("test observation {}", idx),
        content: String::new(),
        tags: vec![Tag::new("test").unwrap()],
        source: None,
        document_id: None,
        excerpt: None,
        created_at: time,
        updated_at: time,
    })
    .unwrap()
}

fn make_score_result(signals: [Signal; 3]) -> ScoreResult {
    let names = ["depth", "breadth", "collaboration"];
    ScoreResult {
        layers: std::array::from_fn(|i| LayerScore {
            name: names[i].to_string(),
            signal: signals[i],
            indicators: Vec::new(),
        }),
        tension: None,
        timestamp: Utc::now(),
    }
}

fn empty_cluster() -> ClusterResult {
    ClusterResult {
        projects: HashMap::new(),
        global_stats: GlobalStats {
            total_sessions: 0,
            total_decisions: 0,
            total_bugfixes: 0,
            total_summaries: 0,
            cognitive_levels: HashMap::new(),
            collaboration_modes: HashMap::new(),
            tool_frequency: HashMap::new(),
            project_ranking: Vec::new(),
        },
        data_quality: DataQualityStats::default(),
        untagged_count: 0,
    }
}

fn cluster_with_project_evidence() -> ClusterResult {
    let mut projects = HashMap::new();
    projects.insert(
        "codex-tool".to_string(),
        ProjectCluster {
            project_name: "codex-tool".to_string(),
            session_count: 1,
            doc_ids: HashSet::new(),
            summary_excerpts: Vec::new(),
            decision_titles: Vec::new(),
            bugfix_titles: Vec::new(),
            cognitive_levels: HashMap::new(),
            collaboration_modes: HashMap::new(),
            tools: Vec::new(),
            frictions: Vec::new(),
            architectures: Vec::new(),
            knowledge_gained: Vec::new(),
            patterns: Vec::new(),
            progress_items: Vec::new(),
            question_items: vec!["validate Codex session attribution".to_string()],
            code_artifacts: Vec::new(),
        },
    );

    ClusterResult {
        projects,
        global_stats: GlobalStats {
            total_sessions: 1,
            total_decisions: 0,
            total_bugfixes: 0,
            total_summaries: 1,
            cognitive_levels: HashMap::new(),
            collaboration_modes: HashMap::new(),
            tool_frequency: HashMap::new(),
            project_ranking: vec![("codex-tool".to_string(), 1)],
        },
        data_quality: DataQualityStats {
            input_observations: 1,
            linked_observations: 1,
            detached_observations: 0,
            mode_excluded_observations: 0,
            source_excluded_observations: 0,
            eligible_observations: 1,
            cohort_identity: "sha256:test-weekly".into(),
        },
        untagged_count: 0,
    }
}

#[test]
fn test_filter_by_time_range() {
    let now = Utc::now();
    let week_ago = now - Duration::days(7);
    let two_weeks_ago = now - Duration::days(14);

    let mut items = Vec::new();
    for i in 0..2 {
        items.push(make_item_at(now - Duration::days(3), i));
    }
    for i in 0..3 {
        items.push(make_item_at(now - Duration::days(10), 10 + i));
    }
    items.push(make_item_at(now - Duration::days(20), 20));

    let this_week = filter_by_time_range(&items, week_ago, now);
    assert_eq!(this_week.len(), 2);

    let last_week = filter_by_time_range(&items, two_weeks_ago, week_ago);
    assert_eq!(last_week.len(), 3);
}

#[test]
fn test_build_weekly_report_no_suggestions_text() {
    let score = make_score_result([Signal::Green, Signal::Yellow, Signal::Red]);
    let report = build_weekly_report(&score, None, &empty_cluster());
    assert!(!report.contains("建议"), "report must not contain '建议'");
    assert!(
        !report.to_lowercase().contains("suggestion"),
        "report must not contain 'suggestion'"
    );
}

#[test]
fn test_build_weekly_report_contains_three_axes() {
    let score = make_score_result([Signal::Green, Signal::Yellow, Signal::Red]);
    let report = build_weekly_report(&score, None, &empty_cluster());
    assert!(report.contains("depth") || report.contains("深度") || report.contains("Depth"));
    assert!(report.contains("breadth") || report.contains("广度") || report.contains("Breadth"));
    assert!(
        report.contains("collaboration") || report.contains("协作") || report.contains("Collab")
    );
    assert!(report.contains("Cohort:"));
    assert!(report.contains("Data quality:"));
}

#[test]
fn test_build_weekly_report_no_prior_data() {
    let score = make_score_result([Signal::Green, Signal::Yellow, Signal::Red]);
    let report = build_weekly_report(&score, None, &empty_cluster());
    assert!(
        report.contains("No prior week data") || report.contains("无上周数据"),
        "should indicate absence of prior data"
    );
}

#[test]
fn test_build_weekly_report_with_prior_data_shows_delta() {
    let this_score = make_score_result([Signal::Green, Signal::Yellow, Signal::Red]);
    let last_score = make_score_result([Signal::Yellow, Signal::Red, Signal::Green]);
    let last_quality = DataQualityStats {
        input_observations: 1,
        linked_observations: 1,
        detached_observations: 0,
        mode_excluded_observations: 0,
        source_excluded_observations: 0,
        eligible_observations: 1,
        cohort_identity: "sha256:last-week".into(),
    };
    let report = build_weekly_report(
        &this_score,
        Some((&last_score, &last_quality)),
        &empty_cluster(),
    );
    assert!(
        report.contains('↑') || report.contains('↓') || report.contains('='),
        "report with prior data must show trend arrows"
    );
}

#[test]
fn test_build_weekly_report_with_indicators() {
    let score = ScoreResult {
        layers: std::array::from_fn(|i| {
            let names = ["depth", "breadth", "collaboration"];
            LayerScore {
                name: names[i].to_string(),
                signal: Signal::Green,
                indicators: vec![Indicator {
                    name: "dreyfus".into(),
                    actual: 4.2,
                    target: ">3.5".into(),
                    signal: Signal::Green,
                }],
            }
        }),
        tension: Some("test tension".into()),
        timestamp: Utc::now(),
    };
    let report = build_weekly_report(&score, None, &empty_cluster());
    assert!(
        report.contains("4.2"),
        "indicator values must appear in report"
    );
    assert!(report.contains("tension") || report.contains("张力"));
}

#[test]
fn degraded_quality_suppresses_week_over_week_trend() {
    let this_score = make_score_result([Signal::Green, Signal::Yellow, Signal::Red]);
    let last_score = make_score_result([Signal::Yellow, Signal::Red, Signal::Green]);
    let mut cluster = empty_cluster();
    cluster.data_quality = DataQualityStats {
        input_observations: 3,
        linked_observations: 2,
        detached_observations: 1,
        mode_excluded_observations: 0,
        source_excluded_observations: 0,
        eligible_observations: 2,
        cohort_identity: "sha256:degraded".into(),
    };

    let report = build_weekly_report(
        &this_score,
        Some((&last_score, &DataQualityStats::default())),
        &cluster,
    );
    assert!(report.contains("DEGRADED"));
    assert!(report.contains("suppressed") || report.contains("不输出环比趋势"));
    assert!(!report.contains("| Trend |"));
    assert!(!report.contains("| 趋势 |"));
}

#[test]
fn test_build_weekly_report_includes_action_card_from_same_cluster() {
    let score = ScoreResult {
        layers: [
            LayerScore {
                name: "depth".into(),
                signal: Signal::Green,
                indicators: Vec::new(),
            },
            LayerScore {
                name: "breadth".into(),
                signal: Signal::Red,
                indicators: vec![
                    Indicator {
                        name: "exploration".into(),
                        actual: 5.2,
                        target: ">15%".into(),
                        signal: Signal::Red,
                    },
                    Indicator {
                        name: "deep_invest".into(),
                        actual: 19.0,
                        target: "15-30%".into(),
                        signal: Signal::Yellow,
                    },
                    Indicator {
                        name: "fragmentation".into(),
                        actual: 26.0,
                        target: "<20%".into(),
                        signal: Signal::Red,
                    },
                ],
            },
            LayerScore {
                name: "collaboration".into(),
                signal: Signal::Green,
                indicators: Vec::new(),
            },
        ],
        tension: None,
        timestamp: Utc::now(),
    };

    let report = build_weekly_report(&score, None, &cluster_with_project_evidence());

    assert!(report.contains("Weekly Action Card"));
    assert!(report.contains("codex-tool"));
    assert!(report.contains("validate Codex session attribution"));
}

#[test]
fn fragmented_other_only_cohort_keeps_weekly_report_without_action_card() {
    let score = ScoreResult {
        layers: [
            LayerScore {
                name: "depth".into(),
                signal: Signal::Green,
                indicators: Vec::new(),
            },
            LayerScore {
                name: "breadth".into(),
                signal: Signal::Red,
                indicators: vec![
                    Indicator {
                        name: "exploration".into(),
                        actual: 20.0,
                        target: ">15%".into(),
                        signal: Signal::Green,
                    },
                    Indicator {
                        name: "deep_invest".into(),
                        actual: 5.0,
                        target: "15-30%".into(),
                        signal: Signal::Red,
                    },
                    Indicator {
                        name: "fragmentation".into(),
                        actual: 40.0,
                        target: "<20%".into(),
                        signal: Signal::Red,
                    },
                ],
            },
            LayerScore {
                name: "collaboration".into(),
                signal: Signal::Green,
                indicators: Vec::new(),
            },
        ],
        tension: None,
        timestamp: Utc::now(),
    };
    let mut other = cluster_with_project_evidence();
    let mut project = other.projects.remove("codex-tool").unwrap();
    project.project_name = "other".into();
    other.projects.insert("other".into(), project);
    other.global_stats.project_ranking = vec![("other".into(), 1)];

    let report = build_weekly_report_with_portfolio(&score, None, &other, &score, &other)
        .expect("synthetic-only cohort must not abort the weekly report");

    assert!(report.contains("This Week Signals"));
    assert!(report.contains("One-off Project Share=40.0"));
    assert!(report.contains("Data quality:"));
    assert!(!report.contains("Weekly Action Card"));
}

#[test]
fn test_signal_delta_arrows() {
    assert_eq!(signal_delta(Signal::Green, Signal::Yellow), "↑");
    assert_eq!(signal_delta(Signal::Yellow, Signal::Green), "↓");
    assert_eq!(signal_delta(Signal::Green, Signal::Green), "=");
    assert_eq!(signal_delta(Signal::Red, Signal::Red), "=");
    assert_eq!(signal_delta(Signal::Green, Signal::Red), "↑");
    assert_eq!(signal_delta(Signal::Red, Signal::Green), "↓");
}

#[test]
fn test_load_last_weekly_record_reports_invalid_jsonl_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("weekly-history.jsonl");
    std::fs::write(&path, "{\"week_end\":\n").unwrap();

    let err = load_last_weekly_record_from_path(&path).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("line 1"));
    assert!(msg.contains("weekly history"));
}

#[test]
fn test_load_last_weekly_record_returns_last_record() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("weekly-history.jsonl");

    let first = WeeklyRecord {
        week_end: Utc::now() - Duration::days(7),
        scores: [
            LayerSignal {
                name: "depth".into(),
                signal: "green".into(),
            },
            LayerSignal {
                name: "breadth".into(),
                signal: "yellow".into(),
            },
            LayerSignal {
                name: "collaboration".into(),
                signal: "red".into(),
            },
        ],
        suggestions: Vec::new(),
    };
    let second = WeeklyRecord {
        week_end: Utc::now(),
        scores: [
            LayerSignal {
                name: "depth".into(),
                signal: "yellow".into(),
            },
            LayerSignal {
                name: "breadth".into(),
                signal: "yellow".into(),
            },
            LayerSignal {
                name: "collaboration".into(),
                signal: "yellow".into(),
            },
        ],
        suggestions: Vec::new(),
    };
    let first_line = serde_json::to_string(&first).unwrap();
    let second_line = serde_json::to_string(&second).unwrap();
    std::fs::write(&path, format!("{}\n{}\n", first_line, second_line)).unwrap();

    let last = load_last_weekly_record_from_path(&path).unwrap();
    assert!(last.is_some());
    // Second record has all-yellow signals; verify that record was returned
    assert_eq!(last.unwrap().scores[0].signal, "yellow");
}

#[test]
fn test_load_last_weekly_record_accepts_legacy_missing_suggestions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("weekly-history.jsonl");
    let legacy_line = format!(
            "{{\"week_end\":\"{}\",\"scores\":[{{\"name\":\"depth\",\"signal\":\"green\"}},{{\"name\":\"breadth\",\"signal\":\"yellow\"}},{{\"name\":\"collaboration\",\"signal\":\"red\"}}]}}",
            Utc::now().to_rfc3339()
        );
    std::fs::write(&path, format!("{}\n", legacy_line)).unwrap();

    let record = load_last_weekly_record_from_path(&path)
        .unwrap()
        .expect("expected record from legacy line");
    assert!(record.suggestions.is_empty());
    assert_eq!(record.scores[0].name, "depth");
}

#[test]
fn test_load_last_weekly_record_accepts_legacy_with_suggestions() {
    // Old JSONL records that have a "suggestions" field must still deserialize.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("weekly-history.jsonl");
    let legacy_line = format!(
            "{{\"week_end\":\"{}\",\"scores\":[{{\"name\":\"depth\",\"signal\":\"green\"}},{{\"name\":\"breadth\",\"signal\":\"yellow\"}},{{\"name\":\"collaboration\",\"signal\":\"red\"}}],\"suggestions\":[\"do more reviews\"]}}",
            Utc::now().to_rfc3339()
        );
    std::fs::write(&path, format!("{}\n", legacy_line)).unwrap();

    let record = load_last_weekly_record_from_path(&path)
        .unwrap()
        .expect("expected record from legacy line with suggestions");
    assert_eq!(record.scores[0].name, "depth");
    assert_eq!(record.suggestions, vec!["do more reviews"]);
}

#[test]
fn test_save_weekly_record_caps_history_at_52() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("weekly-history.jsonl");

    for i in 0..53 {
        let record = make_weekly_record(i);
        persist_weekly_record_to_path(&path, &record).unwrap();
    }

    let content = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(lines.len(), 52, "history must be capped at 52 records");

    let records: Vec<WeeklyRecord> = lines
        .into_iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    // All records use signal "green" for depth in make_weekly_record
    assert_eq!(records.first().unwrap().scores[0].signal, "green");
    assert_eq!(records.last().unwrap().scores[0].signal, "green");
}

#[test]
fn test_new_records_do_not_serialize_suggestions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("weekly-history.jsonl");
    let record = make_weekly_record(0);
    persist_weekly_record_to_path(&path, &record).unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(
        !content.contains("suggestions"),
        "new records must not serialize the suggestions field"
    );
}

#[test]
fn test_save_last_weekly_md_roundtrip() -> Result<()> {
    let dir = tempfile::tempdir().map_err(|e| anyhow::anyhow!(e))?;
    let path = dir.path().join("last-weekly.md");
    let report = "# Weekly Report\n\n- Focus on testing\n- Write more docs\n";
    save_last_weekly_md_to_path(report, &path)?;
    let content = std::fs::read_to_string(&path)?;
    assert_eq!(content, report);
    Ok(())
}

#[test]
fn test_weekly_sentinel_written_with_nonempty_content() -> Result<()> {
    let score = make_score_result([Signal::Green, Signal::Yellow, Signal::Red]);
    let report = build_weekly_report(&score, None, &empty_cluster());

    let dir = tempfile::tempdir().map_err(|e| anyhow::anyhow!(e))?;
    let path = dir.path().join("last-weekly.md");
    save_last_weekly_md_to_path(&report, &path)?;
    let content = std::fs::read_to_string(&path)?;
    assert!(
        !content.trim().is_empty(),
        "sentinel file must not be empty"
    );
    Ok(())
}
