//! Speaker attribution for weekly insights.
//!
//! Labels are derived only from the stored Remem message roles and event times
//! that each facet entry cites (see `session_projection_evidence`). They do not
//! interpret what a message says: they tell the report who spoke and in what
//! order, so an Agent's execution error is not read as the user's habit, and an
//! entry without references stays unknown.

use super::MessageRole;
use crate::knowledge::{Item, RestoreParams};
use chrono::{DateTime, Local, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

/// Shared by route and final prompts. The bracketed labels are written by
/// refine from message roles, never by a model. They record who spoke and in
/// what order, not what was said, so the order alone never settles whether a
/// requirement had been stated before a correction.
pub const ATTRIBUTION_RULES: &str = "归因规则（条目前的 [主体: …] 由 refine 按原始消息角色与时间标注，只说明谁说话、先后顺序，不说明内容）:
- 决策、问题等判断类条目：「仅一条用户原话」「多条用户原话…」「用户先提出→Agent 跟进」可作为用户主动判断的证据。
- 「Agent 先提出或执行→用户随后回应」只能写成 Agent 提出、用户接受或否定（以原话为准）；看不出接受还是否定时写“用户态度不明”。
- 「仅 Agent 消息」「主体不明」不得用于评价用户的认知、判断或习惯，只能写成 Agent 执行情况或证据缺口。
- 阻力、返工条目：标签顺序不决定归因，要对照条目内容与引用的原话。较早的用户消息已提出纠正里的同一要求 → “用户已说明但 Agent 违背”，属 Agent 执行问题；内容明确显示该要求在 Agent 执行之后、纠正时才第一次提出 → “用户事前未说明”；看不出较早消息是否已提出 → “主体不明”。阻力条目不得默认写成用户没说清；只有归为“用户事前未说明”的阻力才可作为用户习惯的证据。";

/// Speaker order of the messages one facet entry cites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeakerPattern {
    /// No validated message reference.
    Unknown,
    /// Only Assistant messages: whether the user accepted is not recorded.
    AgentOnly,
    /// Exactly one user message.
    UserOnly,
    /// Several user messages and no Agent message: an earlier one may already
    /// state what a later one asks again.
    UserRepeated,
    /// A user message comes first; the Agent follows.
    UserFirst,
    /// User first, then the Agent, then the user again (typically a correction).
    UserFirstThenCorrected,
    /// The Agent comes first; a user message follows.
    AgentFirst,
}

impl SpeakerPattern {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "主体不明",
            Self::AgentOnly => "仅 Agent 消息，用户是否接受不明",
            Self::UserOnly => "仅一条用户原话",
            Self::UserRepeated => "多条用户原话，较早一条是否已提出同一要求见内容",
            Self::UserFirst => "用户先提出→Agent 跟进",
            Self::UserFirstThenCorrected => "用户先提出→Agent 执行→用户再纠正",
            Self::AgentFirst => "Agent 先提出或执行→用户随后回应（接受/否定见原话）",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CitedMessage {
    id: i64,
    role: Speaker,
    event_time: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Speaker {
    User,
    Agent,
}

/// Speaker order of cited messages, sorted by event time then message id.
/// System messages are ignored: they are neither the user nor the Agent.
fn speaker_pattern(messages: &[CitedMessage]) -> SpeakerPattern {
    let Some(first) = messages.first() else {
        return SpeakerPattern::Unknown;
    };
    let first_agent = messages.iter().position(|m| m.role == Speaker::Agent);
    match (first.role, first_agent) {
        (Speaker::User, None) if messages.len() == 1 => SpeakerPattern::UserOnly,
        (Speaker::User, None) => SpeakerPattern::UserRepeated,
        (Speaker::Agent, _) if messages.iter().all(|m| m.role == Speaker::Agent) => {
            SpeakerPattern::AgentOnly
        }
        (Speaker::Agent, _) => SpeakerPattern::AgentFirst,
        (Speaker::User, Some(agent)) => {
            if messages[agent..].iter().any(|m| m.role == Speaker::User) {
                SpeakerPattern::UserFirstThenCorrected
            } else {
                SpeakerPattern::UserFirst
            }
        }
    }
}

const MAX_TIMES_PER_SPEAKER: usize = 3;

fn render_times(messages: &[CitedMessage], speaker: Speaker) -> Option<String> {
    let times: Vec<String> = messages
        .iter()
        .filter(|m| m.role == speaker)
        .map(|m| {
            m.event_time
                .with_timezone(&Local)
                .format("%m-%d %H:%M")
                .to_string()
        })
        .collect();
    if times.is_empty() {
        return None;
    }
    let mut shown = times[..times.len().min(MAX_TIMES_PER_SPEAKER)].join(", ");
    if times.len() > MAX_TIMES_PER_SPEAKER {
        shown.push_str(&format!(" 等 {} 条", times.len()));
    }
    Some(shown)
}

fn render_label(messages: &[CitedMessage]) -> String {
    let pattern = speaker_pattern(messages);
    if pattern == SpeakerPattern::Unknown {
        return format!("[主体: {}]", pattern.label());
    }
    let mut label = format!("[主体: {}", pattern.label());
    if let Some(times) = render_times(messages, Speaker::User) {
        label.push_str(&format!("｜用户 {times}"));
    }
    if let Some(times) = render_times(messages, Speaker::Agent) {
        label.push_str(&format!("｜Agent {times}"));
    }
    label.push(']');
    label
}

#[derive(Deserialize)]
struct StoredEvidence {
    #[serde(default)]
    source_messages: Vec<StoredMessage>,
    #[serde(default)]
    facets: Vec<StoredFacet>,
    #[serde(default)]
    observations: Vec<StoredObservation>,
}

#[derive(Deserialize)]
struct StoredMessage {
    id: i64,
    role: MessageRole,
    event_time: DateTime<Utc>,
}

#[derive(Deserialize)]
struct StoredFacet {
    field: String,
    index: usize,
    #[serde(default)]
    message_ids: Vec<i64>,
}

#[derive(Deserialize)]
struct StoredObservation {
    item_id: String,
    #[serde(default)]
    field: Option<String>,
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    facets: Option<String>,
}

/// Which facet entries a stored observation carries.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ObservationFacets {
    /// The session summary item, which lists every facet field in its content.
    All,
    /// One decision or bug-fix item.
    Entry(String, usize),
}

/// Speaker labels for one session projection.
#[derive(Debug, Clone, Default)]
pub struct SessionAttribution {
    labels: HashMap<(String, usize), String>,
    observations: HashMap<String, ObservationFacets>,
}

impl SessionAttribution {
    /// Parse stored projection evidence. `None` when the session has no
    /// readable evidence: its entries are then all unknown.
    pub fn from_projection_evidence(evidence: &Value) -> Option<Self> {
        if evidence.is_null() {
            return None;
        }
        let stored: StoredEvidence = serde_json::from_value(evidence.clone()).ok()?;
        let messages: HashMap<i64, CitedMessage> = stored
            .source_messages
            .iter()
            .filter_map(|message| {
                let role = match message.role {
                    MessageRole::User => Speaker::User,
                    MessageRole::Assistant => Speaker::Agent,
                    MessageRole::System => return None,
                };
                Some((
                    message.id,
                    CitedMessage {
                        id: message.id,
                        role,
                        event_time: message.event_time,
                    },
                ))
            })
            .collect();
        let mut labels = HashMap::new();
        for facet in &stored.facets {
            let mut cited: Vec<CitedMessage> = facet
                .message_ids
                .iter()
                .filter_map(|id| messages.get(id).copied())
                .collect();
            cited.sort_by_key(|message| (message.event_time, message.id));
            labels.insert((facet.field.clone(), facet.index), render_label(&cited));
        }
        let observations = stored
            .observations
            .into_iter()
            .filter_map(|observation| {
                let facets = match (observation.facets.as_deref(), observation.field) {
                    (Some("all"), _) => ObservationFacets::All,
                    (_, Some(field)) => ObservationFacets::Entry(field, observation.index?),
                    _ => return None,
                };
                Some((observation.item_id, facets))
            })
            .collect();
        Some(Self {
            labels,
            observations,
        })
    }

    fn label(&self, field: &str, index: usize) -> String {
        self.labels
            .get(&(field.to_string(), index))
            .cloned()
            .unwrap_or_else(unknown_label)
    }
}

fn unknown_label() -> String {
    format!("[主体: {}]", SpeakerPattern::Unknown.label())
}

/// Summary-content sections whose entries are labelled, with their facet field.
const LABELLED_SECTIONS: &[(&str, &str)] = &[
    ("决策", "decisions"),
    ("Bug 修复", "bugs_fixed"),
    ("阻力", "friction"),
    ("问题", "questions"),
];

/// Return copies of `items` whose decision, bug-fix, friction and question
/// entries start with a speaker label. Items of a session without evidence, or
/// without a stored mapping (for example a human override), are labelled
/// unknown. Timestamps and ids are preserved.
pub fn annotate_observations(
    items: &[Item],
    attributions: &HashMap<String, SessionAttribution>,
) -> Vec<Item> {
    let empty = SessionAttribution::default();
    items
        .iter()
        .map(|item| {
            let Some(document_id) = item.document_id() else {
                return item.clone();
            };
            let attribution = attributions.get(document_id.as_str()).unwrap_or(&empty);
            let mapping = attribution.observations.get(item.id().as_str());
            let is_entry = item
                .tags()
                .iter()
                .any(|tag| matches!(tag.as_str(), "decision" | "bugfix"));
            if is_entry {
                let label = match mapping {
                    Some(ObservationFacets::Entry(field, index)) => {
                        attribution.label(field, *index)
                    }
                    _ => unknown_label(),
                };
                return with_text(item, &format!("{label} {}", item.title()), item.content());
            }
            let source = match mapping {
                Some(ObservationFacets::All) => attribution,
                _ => &empty,
            };
            with_text(
                item,
                item.title(),
                &label_summary_content(item.content(), source),
            )
        })
        .collect()
}

fn label_summary_content(content: &str, attribution: &SessionAttribution) -> String {
    let mut field: Option<&str> = None;
    let mut index = 0usize;
    let mut lines = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.ends_with(':') && !trimmed.starts_with('-') {
            let name = trimmed.trim_end_matches(':');
            field = LABELLED_SECTIONS
                .iter()
                .find(|(section, _)| *section == name)
                .map(|(_, field)| *field);
            index = 0;
            lines.push(line.to_string());
            continue;
        }
        match (field, trimmed.strip_prefix("- ")) {
            (Some(field), Some(entry)) => {
                lines.push(format!("- {} {entry}", attribution.label(field, index)));
                index += 1;
            }
            _ => {
                if !trimmed.is_empty() {
                    field = None;
                }
                lines.push(line.to_string());
            }
        }
    }
    lines.join("\n")
}

fn with_text(item: &Item, title: &str, content: &str) -> Item {
    Item::restore(RestoreParams {
        id: item.id().clone(),
        item_type: item.item_type(),
        title: title.to_string(),
        summary: item.summary().to_string(),
        content: content.to_string(),
        tags: item.tags().to_vec(),
        source: item.source().cloned(),
        document_id: item.document_id().cloned(),
        excerpt: item.excerpt().map(str::to_string),
        created_at: item.created_at(),
        updated_at: item.updated_at(),
    })
    .unwrap_or_else(|_| item.clone())
}

#[cfg(test)]
#[path = "attribution_tests.rs"]
mod tests;
