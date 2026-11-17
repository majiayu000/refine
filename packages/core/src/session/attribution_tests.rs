use super::*;
use crate::knowledge::DocumentId;
use crate::session::{
    facets_to_items, parse_facet_response, session_projection_evidence, SourceMessageReference,
};
use chrono::TimeZone;
use serde_json::json;

fn at(day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
    Local
        .with_ymd_and_hms(2026, 10, day, hour, minute, 0)
        .single()
        .unwrap()
        .with_timezone(&Utc)
}

fn message(id: i64, role: MessageRole, time: DateTime<Utc>) -> SourceMessageReference {
    SourceMessageReference {
        id,
        role,
        event_time: time,
    }
}

/// The 2026-10-16 portrait-video session from issue #264: the user asked for
/// a vertical cut at 14:18 and 14:25, the Agent delivered landscape, and the
/// user corrected it at 19:44.
fn vertical_screen_sources() -> Vec<SourceMessageReference> {
    vec![
        message(10, MessageRole::User, at(16, 14, 18)),
        message(11, MessageRole::User, at(16, 14, 25)),
        message(12, MessageRole::Assistant, at(16, 16, 2)),
        message(13, MessageRole::User, at(16, 19, 44)),
    ]
}

/// Build items and stored evidence through the same path ingest uses.
fn project(
    facets: Value,
    sources: &[SourceMessageReference],
) -> (Vec<Item>, HashMap<String, SessionAttribution>) {
    let facets = parse_facet_response(&facets.to_string()).unwrap();
    let document_id = DocumentId::from("doc-1");
    let items = facets_to_items(&facets, &document_id, Some("mv"));
    let evidence = session_projection_evidence(&facets, sources, &items).unwrap();
    let attribution = SessionAttribution::from_projection_evidence(&evidence).unwrap();
    (items, HashMap::from([("doc-1".to_string(), attribution)]))
}

fn friction_line(items: &[Item]) -> String {
    items[0]
        .content()
        .lines()
        .skip_while(|line| *line != "阻力:")
        .nth(1)
        .unwrap()
        .to_string()
}

#[test]
fn vertical_screen_case_keeps_request_before_deviation_order() {
    let (items, attributions) = project(
        json!({
            "session_summary": "MV 竖屏交付",
            "cognitive_level": "unknown",
            "collaboration_mode": "delegation",
            "friction": ["竖屏需求交付成了横屏，返工重排"],
            // Model output order is not trusted; refine sorts by event time.
            "evidence": [{"field": "friction", "index": 0, "message_ids": [13, 12, 11, 10]}],
        }),
        &vertical_screen_sources(),
    );
    let annotated = annotate_observations(&items, &attributions);
    let line = friction_line(&annotated);
    assert!(line.contains("用户先提出→Agent 执行→用户再纠正"), "{line}");
    assert!(
        line.contains("用户 10-16 14:18, 10-16 14:25, 10-16 19:44"),
        "{line}"
    );
    assert!(line.contains("Agent 10-16 16:02"), "{line}");
    assert!(line.ends_with("竖屏需求交付成了横屏，返工重排"));
    assert!(!line.contains("主体不明"));
}

#[test]
fn vertical_screen_case_with_only_user_quotes_stays_in_time_order() {
    let (items, attributions) = project(
        json!({
            "session_summary": "MV 竖屏交付",
            "cognitive_level": "unknown",
            "collaboration_mode": "delegation",
            "friction": ["竖屏需求交付成了横屏"],
            "evidence": [{"field": "friction", "index": 0, "message_ids": [13, 10, 11]}],
        }),
        &vertical_screen_sources(),
    );
    let line = friction_line(&annotate_observations(&items, &attributions));
    // Two earlier user messages precede the correction: the rules then call it
    // a stated requirement the Agent violated, not an unstated one.
    assert!(
        line.contains("[主体: 仅用户原话｜用户 10-16 14:18, 10-16 14:25, 10-16 19:44]"),
        "{line}"
    );
    assert!(ATTRIBUTION_RULES.contains("较早一条已提出同一要求"));
    assert!(ATTRIBUTION_RULES.contains("不得写成用户没说清"));
}

#[test]
fn requirement_first_seen_after_agent_execution_is_agent_first() {
    let sources = vec![
        message(20, MessageRole::Assistant, at(17, 10, 0)),
        message(21, MessageRole::User, at(17, 10, 5)),
    ];
    let (items, attributions) = project(
        json!({
            "session_summary": "补充导出格式",
            "cognitive_level": "unknown",
            "collaboration_mode": "delegation",
            "friction": ["交付后才补充需要 MP4"],
            "evidence": [{"field": "friction", "index": 0, "message_ids": [21, 20]}],
        }),
        &sources,
    );
    let line = friction_line(&annotate_observations(&items, &attributions));
    assert!(line.contains("Agent 先提出或执行→用户随后回应"), "{line}");
    assert!(ATTRIBUTION_RULES.contains("只有要求首次出现在 Agent 执行之后，才可写“用户事前未说明”"));
}

#[test]
fn agent_only_and_missing_references_never_become_user_evidence() {
    let sources = vec![
        message(30, MessageRole::Assistant, at(18, 9, 0)),
        message(31, MessageRole::User, at(18, 9, 1)),
    ];
    let (items, attributions) = project(
        json!({
            "session_summary": "测试管道",
            "cognitive_level": "unknown",
            "collaboration_mode": "delegation",
            "decisions": ["测试全部通过即可发布"],
            "friction": ["测试管道假成功", "等待 CI"],
            "evidence": [
                {"field": "decisions", "index": 0, "message_ids": [30]},
                {"field": "friction", "index": 0, "message_ids": [30]}
            ],
        }),
        &sources,
    );
    let annotated = annotate_observations(&items, &attributions);
    let decision = annotated
        .iter()
        .find(|item| item.tags().iter().any(|tag| tag.as_str() == "decision"))
        .unwrap();
    assert!(decision
        .title()
        .starts_with("[主体: 仅 Agent 消息，用户是否接受不明｜Agent 10-18 09:00] 测试全部通过"));
    let content = annotated[0].content();
    assert!(content
        .contains("- [主体: 仅 Agent 消息，用户是否接受不明｜Agent 10-18 09:00] 测试管道假成功"));
    assert!(content.contains("- [主体: 主体不明] 等待 CI"));
    assert!(ATTRIBUTION_RULES.contains("「仅 Agent 消息」「主体不明」不得用于评价用户"));
}

#[test]
fn sessions_without_evidence_or_mapping_are_unknown_and_keep_timestamps() {
    let facets = parse_facet_response(
        &json!({
            "session_summary": "旧会话",
            "cognitive_level": "unknown",
            "collaboration_mode": "unknown",
            "decisions": ["保留缓存"],
            "friction": ["编译慢"],
        })
        .to_string(),
    )
    .unwrap();
    let items = facets_to_items(&facets, &DocumentId::from("legacy"), Some("refine"));
    assert!(SessionAttribution::from_projection_evidence(&Value::Null).is_none());
    let annotated = annotate_observations(&items, &HashMap::new());
    assert_eq!(annotated[1].title(), "[主体: 主体不明] 保留缓存");
    assert!(annotated[0].content().contains("- [主体: 主体不明] 编译慢"));
    assert!(annotated[0].content().starts_with("认知水平: unknown"));
    for (before, after) in items.iter().zip(&annotated) {
        assert_eq!(before.id(), after.id());
        assert_eq!(before.updated_at(), after.updated_at());
        assert_eq!(before.tags(), after.tags());
    }
}

#[test]
fn user_first_decision_is_user_judgment() {
    let sources = vec![
        message(40, MessageRole::User, at(19, 8, 0)),
        message(41, MessageRole::Assistant, at(19, 8, 3)),
        message(42, MessageRole::System, at(19, 7, 0)),
    ];
    let (items, attributions) = project(
        json!({
            "session_summary": "选择存储",
            "cognitive_level": "unknown",
            "collaboration_mode": "pair_programming",
            "decisions": ["用 SQLite 而不是 Postgres"],
            "evidence": [{"field": "decisions", "index": 0, "message_ids": [42, 41, 40]}],
        }),
        &sources,
    );
    let annotated = annotate_observations(&items, &attributions);
    assert!(annotated[1]
        .title()
        .starts_with("[主体: 用户先提出→Agent 跟进｜用户 10-19 08:00｜Agent 10-19 08:03]"));
    assert!(annotated[0].content().contains(
        "- [主体: 用户先提出→Agent 跟进｜用户 10-19 08:00｜Agent 10-19 08:03] 用 SQLite"
    ));
}
