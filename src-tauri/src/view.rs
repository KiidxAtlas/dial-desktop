//! The snapshot the web UI renders: everything on screen, as plain serializable data.
//!
//! The UI never interprets wire data itself. Display decisions that need engine knowledge (tool
//! states, labels, relative paths, session timestamps) are made here, where they are tested.

use crate::engine::QuestionRef;
use crate::model::{Connection, Model, ModelOption};
use crate::tabs::Activity;
use crate::time;
use dial_client::{Entry, ToolEntry};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[cfg(test)]
mod tests;

/// The project the engine works in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    /// Absolute path.
    pub root: String,
    /// Path as shown, with the home directory as `~`.
    pub display: String,
    /// Last path component.
    pub name: String,
}

impl Workspace {
    /// Describe `root`; `home` abbreviates it when it lies inside the home directory.
    pub fn new(root: &str, home: Option<&str>) -> Self {
        let display = match home {
            Some(home) if !home.is_empty() && root.starts_with(home) => {
                format!("~{}", &root[home.len()..])
            }
            _ => root.to_owned(),
        };
        let name = root
            .trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .filter(|n| !n.is_empty())
            .unwrap_or(root)
            .to_owned();
        Self {
            root: root.to_owned(),
            display,
            name,
        }
    }

    /// `path` relative to the workspace when it lies inside it.
    pub fn relative<'a>(&self, path: &'a str) -> &'a str {
        path.strip_prefix(self.root.as_str())
            .and_then(|rest| rest.strip_prefix('/').or(rest.strip_prefix('\\')))
            .filter(|rest| !self.root.is_empty() && !rest.is_empty())
            .unwrap_or(path)
    }
}

/// Connection state as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConnectionView {
    Starting,
    Live {
        model: String,
        engine_version: Option<String>,
    },
    Preview,
    Failed {
        error: String,
        /// What the UI offers to fix it: install the engine, update it, or reconnect.
        problem: crate::updates::EngineProblem,
    },
    Ended,
}

impl From<&Connection> for ConnectionView {
    fn from(connection: &Connection) -> Self {
        match connection {
            Connection::Starting => Self::Starting,
            Connection::Live {
                model,
                engine_version,
            } => Self::Live {
                model: model.clone(),
                engine_version: engine_version.clone(),
            },
            Connection::Preview => Self::Preview,
            Connection::Failed(error) => Self::Failed {
                error: error.clone(),
                problem: crate::updates::classify_failure(error),
            },
            Connection::Ended => Self::Ended,
        }
    }
}

/// One sidebar row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub id: String,
    pub title: String,
    /// "3h ago · 12 messages".
    pub meta: String,
    pub pinned: bool,
    /// The session on screen.
    pub selected: bool,
    /// A turn runs in it, on screen or in the background.
    pub running: bool,
    /// An approval or a question in it waits for the user.
    pub waiting: bool,
}

/// One conversation row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum EntryView {
    User {
        text: String,
        images: usize,
    },
    Assistant {
        text: String,
        reasoning: String,
        streaming: bool,
        abandoned: bool,
    },
    Tool(Box<ToolView>),
    Notice {
        message: String,
        lane: Option<String>,
        notice_kind: Option<String>,
    },
    Error {
        message: String,
    },
}

/// How a tool call settled. An unknown outcome is never shown as success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolState {
    Running,
    Succeeded,
    Failed,
    Unknown,
}

impl ToolState {
    pub fn of(tool: &ToolEntry) -> Self {
        match tool.result.as_ref().map(|r| r.succeeded()) {
            None => Self::Running,
            Some(Some(true)) => Self::Succeeded,
            Some(Some(false)) => Self::Failed,
            Some(None) => Self::Unknown,
        }
    }
}

/// A captioned block of text in an expanded tool row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub caption: &'static str,
    pub text: String,
}

/// One tool call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolView {
    /// Stable key for UI state such as expansion.
    pub key: String,
    pub label: String,
    pub target: String,
    /// Card tone: `tools`, `shell`, `edit`, `connect` or `workers`.
    pub tone: Option<String>,
    pub state: ToolState,
    /// Live progress while running.
    pub progress: Option<String>,
    /// The card preview, else pretty-printed arguments.
    pub input: Option<Block>,
    /// The result text, else live output.
    pub output: Option<Block>,
    /// Changed files, relative to the workspace.
    pub changed_paths: Vec<String>,
    /// Absolute paths of the images an `image_gen` call wrote, shown inline in the row.
    pub images: Vec<String>,
    /// Warnings about the outcome ("status: timeout", "side effects unknown").
    pub facts: Vec<String>,
}

impl ToolView {
    pub fn new(index: usize, tool: &ToolEntry, workspace: &Workspace) -> Self {
        let card = tool.card.as_ref();
        let label = card
            .map(|c| c.label.as_str())
            .filter(|l| !l.is_empty())
            .or(Some(tool.name.as_str()).filter(|n| !n.is_empty()))
            .unwrap_or("tool")
            .to_owned();
        let target = card
            .map(|c| c.target.as_str())
            .filter(|t| !t.is_empty())
            .unwrap_or(&tool.summary)
            .to_owned();
        // An edit's hunks say what changed; its patch arguments and line-numbered receipt do not.
        let diff = tool.result.as_ref().and_then(|r| change_preview(&r.text));
        let input = match card.and_then(|c| c.preview.as_deref()) {
            _ if diff.is_some() => None,
            Some(preview) => Some(Block {
                caption: "preview",
                text: preview.to_owned(),
            }),
            None if !tool.arguments.trim().is_empty() => Some(Block {
                caption: "arguments",
                text: serde_json::from_str::<Value>(&tool.arguments)
                    .ok()
                    .and_then(|v| serde_json::to_string_pretty(&v).ok())
                    .unwrap_or_else(|| tool.arguments.clone()),
            }),
            None => None,
        };
        let output = match &tool.result {
            _ if diff.is_some() => diff.map(|text| Block {
                caption: "diff",
                text,
            }),
            Some(result) if !result.text.trim().is_empty() => Some(Block {
                caption: "result",
                text: result.text.clone(),
            }),
            _ if !tool.output.trim().is_empty() => Some(Block {
                caption: "output",
                text: tool.output.clone(),
            }),
            _ => None,
        };
        let mut facts = Vec::new();
        let mut changed_paths = Vec::new();
        if let Some(result) = &tool.result {
            changed_paths = result
                .changed_paths
                .iter()
                .map(|p| workspace.relative(p).to_owned())
                .collect();
            if let Some(status) = result.status.as_deref().filter(|s| *s != "ok") {
                facts.push(format!("status: {status}"));
            }
            if result.effects.as_deref() == Some("unknown") {
                facts.push("side effects unknown".to_owned());
            }
        }
        Self {
            key: if tool.call_id.is_empty() {
                format!("tool:{index}")
            } else {
                format!("tool:{}", tool.call_id)
            },
            label,
            target,
            tone: card.and_then(|c| c.tone.clone()),
            state: ToolState::of(tool),
            progress: tool.progress.clone().filter(|_| tool.result.is_none()),
            input,
            output,
            images: generated_images(tool, workspace),
            changed_paths,
            facts,
        }
    }
}

/// The unified hunks the engine appends to an `edit` or `write` result after its
/// `change preview (+A -R lines)` header, without the header or any notes that follow. A result
/// covering several files keeps its full text, which names each file.
fn change_preview(text: &str) -> Option<String> {
    const HEADER: &str = "\nchange preview (";
    let start = text.find(HEADER)? + 1;
    if text[start..].contains(HEADER) {
        return None;
    }
    let hunks: Vec<&str> = text[start..]
        .lines()
        .skip(1)
        .take_while(|line| {
            line.starts_with("@@")
                || line.starts_with([' ', '+', '-'])
                || (line.starts_with("[diff preview truncated")
                    || line.starts_with("[final newline"))
        })
        .collect();
    hunks
        .first()
        .is_some_and(|line| line.starts_with("@@"))
        .then(|| hunks.join("\n"))
}

/// The tool that generates images; its written files are previewed in the conversation.
const IMAGE_GEN: &str = "image_gen";

/// Images a settled `image_gen` call wrote inside the workspace, in order. Live results name them
/// in `changed_paths`; history may not, so the requested `output` argument is the fallback, as in
/// the engine's own preview. A known failure shows none.
fn generated_images(tool: &ToolEntry, workspace: &Workspace) -> Vec<String> {
    let Some(result) = tool.result.as_ref().filter(|_| tool.name == IMAGE_GEN) else {
        return Vec::new();
    };
    if result.succeeded() == Some(false) || workspace.root.is_empty() {
        return Vec::new();
    }
    let root = std::path::Path::new(&workspace.root);
    let inside = |path: &str| -> Option<String> {
        let path = std::path::Path::new(path);
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.join(path)
        };
        let contained = absolute.starts_with(root)
            && absolute
                .components()
                .all(|c| !matches!(c, std::path::Component::ParentDir));
        (contained && crate::attachments::image_mime(&absolute).is_some())
            .then(|| absolute.to_string_lossy().into_owned())
    };
    let written: Vec<String> = result
        .changed_paths
        .iter()
        .filter_map(|p| inside(p))
        .collect();
    if !written.is_empty() {
        return written;
    }
    serde_json::from_str::<Value>(&tool.arguments)
        .ok()
        .and_then(|args| args["output"].as_str().and_then(inside))
        .into_iter()
        .collect()
}

/// A pending approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalView {
    pub approval_id: String,
    pub tool: String,
    pub summary: String,
}

/// The open `ask` request: one or more questions answered together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionView {
    /// Identity the answer refers to; empty when the request cannot be answered from here.
    pub id: String,
    pub items: Vec<QuestionItemView>,
}

/// One question of an `ask` request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionItemView {
    pub id: String,
    pub header: Option<String>,
    pub text: String,
    pub options: Vec<QuestionOptionView>,
    /// Several options may be chosen.
    pub multi: bool,
    /// Index of the option the agent suggests.
    pub recommended: Option<usize>,
}

/// One choice offered for a question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionOptionView {
    pub label: String,
    pub description: Option<String>,
    pub preview: Option<String>,
}

/// One plan item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    pub text: String,
    /// `completed`, `in_progress`, `pending`, `abandoned`, `cancelled` or `blocked`.
    pub status: String,
}

/// One changed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeView {
    /// Absolute path.
    pub path: String,
    /// Directory relative to the workspace, with a trailing separator, or empty.
    pub dir: String,
    pub name: String,
}

/// Tool calls by state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsView {
    pub running: usize,
    pub succeeded: usize,
    pub failed: usize,
}

/// The model in force and its reasoning controls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelView {
    /// Empty while the engine has only named the model (before `state` answered).
    pub provider: String,
    pub model_id: String,
    pub label: String,
    /// The session's level; `None` runs the model's default.
    pub thinking: Option<String>,
    pub default_thinking: Option<String>,
    pub levels: Vec<String>,
    pub thinking_settable: bool,
    /// Whether the model takes image attachments; `None` while unknown.
    pub accepts_images: Option<bool>,
}

impl ModelView {
    /// The model `model` reports, labelled from the model list when it is loaded.
    pub fn of(model: &Model) -> Option<Self> {
        let option = |provider: &str, id: &str| {
            model
                .models
                .iter()
                .find(|m| m.provider == provider && m.model_id == id)
        };
        let label = |provider: &str, id: &str| {
            option(provider, id).map_or_else(|| id.to_owned(), |m| m.label.clone())
        };
        // The model list knows every model; `ready` only the one the engine started with.
        let started_with = match model.connection() {
            Connection::Live { model: id, .. } => Some(id.as_str()),
            _ => None,
        };
        let accepts_images = |provider: &str, id: &str| {
            option(provider, id)
                .map(|m| m.vision)
                .or_else(|| model.ready_image_input.filter(|_| started_with == Some(id)))
        };
        if let Some(current) = &model.current_model {
            return Some(Self {
                label: label(&current.provider, &current.model_id),
                provider: current.provider.clone(),
                model_id: current.model_id.clone(),
                thinking: current.thinking.clone(),
                default_thinking: current.default_thinking.clone(),
                levels: current.levels.clone(),
                thinking_settable: current.thinking_settable,
                accepts_images: accepts_images(&current.provider, &current.model_id),
            });
        }
        match model.connection() {
            Connection::Live { model: id, .. } => Some(Self {
                provider: String::new(),
                model_id: id.clone(),
                label: id.clone(),
                thinking: None,
                default_thinking: None,
                levels: Vec::new(),
                thinking_settable: false,
                accepts_images: model.ready_image_input,
            }),
            _ => None,
        }
    }
}

/// One model the user can switch to (the `models` event).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOptionView {
    pub provider: String,
    pub model_id: String,
    pub label: String,
    pub context_window: Option<u64>,
    pub reasoning: bool,
    pub vision: bool,
    pub configured: bool,
    pub local: bool,
}

impl From<&ModelOption> for ModelOptionView {
    fn from(m: &ModelOption) -> Self {
        Self {
            provider: m.provider.clone(),
            model_id: m.model_id.clone(),
            label: m.label.clone(),
            context_window: m.context_window,
            reasoning: m.reasoning,
            vision: m.vision,
            configured: m.configured,
            local: m.local,
        }
    }
}

/// Everything the UI shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// `macos`, `windows` or `linux`; the UI leaves room for the macOS window buttons.
    pub platform: &'static str,
    /// `None` until the user picks a folder; the UI then shows the welcome screen.
    pub workspace: Option<Workspace>,
    pub connection: ConnectionView,
    pub title: String,
    pub model: Option<ModelView>,
    /// Approval mode in force: `readonly`, `ask`, `write` or `auto`.
    pub approval: Option<String>,
    pub session_id: Option<String>,
    pub turn_running: bool,
    pub can_send: bool,
    pub sessions: Vec<SessionView>,
    pub entries: Vec<EntryView>,
    pub approvals: Vec<ApprovalView>,
    /// The open question, if the agent is waiting for an answer.
    pub question: Option<QuestionView>,
    pub plan: Vec<TaskView>,
    pub changes: Vec<ChangeView>,
    pub stats: StatsView,
    /// How full the model's context is, once the engine reported it.
    pub context: Option<ContextView>,
    pub voice: VoiceView,
}

/// Hands-free voice mode as the panel shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceView {
    pub phase: crate::voice::Phase,
    pub muted: bool,
    pub heard: Option<String>,
    pub problem: Option<String>,
    /// The microphone while listening, 0 to 100.
    pub level: u8,
    /// An utterance is being recorded.
    pub hearing: bool,
    /// Something is being read aloud.
    pub reading: bool,
}

/// The conversation's size against the model's context window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextView {
    pub tokens: u64,
    /// `None` when the model list does not say.
    pub window: Option<u64>,
    pub estimated: bool,
}

impl Snapshot {
    /// The view of `model` at time `now` (Unix seconds, for relative session times).
    pub fn new(model: &Model, workspace: Option<&Workspace>, now: i64) -> Self {
        let no_workspace = Workspace::new("", None);
        let shown = workspace.unwrap_or(&no_workspace);
        let conversation = &model.conversation;
        let session_id = conversation.session_id().map(str::to_owned);
        let mut seen = HashSet::new();
        let sessions = model
            .sessions
            .iter()
            .filter(|s| seen.insert(s.id.as_str()))
            .map(|s| {
                let mut meta = time::label(&s.updated, now);
                if s.messages > 0 {
                    meta = if meta.is_empty() {
                        format!("{} messages", s.messages)
                    } else {
                        format!("{meta} · {} messages", s.messages)
                    };
                }
                SessionView {
                    id: s.id.clone(),
                    title: s.title.clone(),
                    meta,
                    pinned: s.pinned,
                    selected: session_id.as_deref() == Some(s.id.as_str()),
                    running: false,
                    waiting: false,
                }
            })
            .collect();
        let entries = conversation
            .entries()
            .iter()
            .enumerate()
            .map(|(index, entry)| match entry {
                Entry::User(user) => EntryView::User {
                    text: user.text.clone(),
                    images: user.images,
                },
                Entry::Assistant(answer) => EntryView::Assistant {
                    text: answer.text.clone(),
                    reasoning: answer.reasoning.trim().to_owned(),
                    streaming: answer.streaming,
                    abandoned: answer.abandoned,
                },
                Entry::Tool(tool) => EntryView::Tool(Box::new(ToolView::new(index, tool, shown))),
                Entry::Notice(notice) => EntryView::Notice {
                    message: notice.message.clone(),
                    lane: notice.lane.clone(),
                    notice_kind: notice.kind.clone(),
                },
                Entry::Error(message) => EntryView::Error {
                    message: message.clone(),
                },
            })
            .collect();
        let approvals = conversation
            .pending_approvals()
            .iter()
            .map(|p| ApprovalView {
                approval_id: p.request.approval_id.clone(),
                tool: p.request.tool.clone(),
                summary: p.request.summary.clone(),
            })
            .collect();
        let plan = conversation
            .todo()
            .iter()
            .map(|task| TaskView {
                text: task["content"]
                    .as_str()
                    .or_else(|| task["title"].as_str())
                    .unwrap_or_default()
                    .to_owned(),
                status: task["status"].as_str().unwrap_or("pending").to_owned(),
            })
            .collect();
        let changes = model
            .changed_paths()
            .into_iter()
            .map(|path| {
                let shown = shown.relative(&path);
                let (dir, name) = match shown.rfind(['/', '\\']) {
                    Some(i) => (&shown[..=i], &shown[i + 1..]),
                    None => ("", shown),
                };
                ChangeView {
                    dir: dir.to_owned(),
                    name: name.to_owned(),
                    path: path.clone(),
                }
            })
            .collect();
        let stats = model.tool_stats();
        Self {
            platform: std::env::consts::OS,
            workspace: workspace.cloned(),
            connection: model.connection().into(),
            model: ModelView::of(model),
            approval: model.approval.clone(),
            title: model.title(),
            session_id,
            turn_running: conversation.turn_running(),
            can_send: model.can_send(),
            sessions,
            entries,
            approvals,
            question: conversation.question().map(question_view),
            plan,
            changes,
            stats: StatsView {
                running: stats.running,
                succeeded: stats.succeeded,
                failed: stats.failed,
            },
            context: model.context.map(|usage| ContextView {
                tokens: usage.tokens,
                window: model.context_window(),
                estimated: usage.estimated,
            }),
            voice: VoiceView {
                phase: model.voice.phase,
                muted: model.voice.muted,
                heard: model.voice.heard.clone(),
                problem: model.voice.problem.clone(),
                level: model.voice.level,
                hearing: model.voice.hearing,
                reading: model.voice.reading,
            },
        }
    }

    /// Mark sessions that run in some tab. A running session the list does not have yet (its
    /// first turn is still going) is shown on top, so the user can always get back to it.
    pub fn show_activity(&mut self, activity: &HashMap<String, Activity>) {
        for session in &mut self.sessions {
            if let Some(a) = activity.get(&session.id) {
                session.running = a.running;
                session.waiting = a.waiting;
            }
        }
        let mut missing: Vec<(&String, &Activity)> = activity
            .iter()
            .filter(|(id, a)| {
                (a.running || a.waiting) && !self.sessions.iter().any(|s| &s.id == *id)
            })
            .collect();
        missing.sort_by(|a, b| a.0.cmp(b.0));
        for (id, a) in missing.into_iter().rev() {
            self.sessions.insert(
                0,
                SessionView {
                    id: id.clone(),
                    title: a.title.clone(),
                    meta: if a.waiting {
                        "Waiting for you"
                    } else {
                        "Running"
                    }
                    .into(),
                    pinned: false,
                    selected: self.session_id.as_deref() == Some(id.as_str()),
                    running: a.running,
                    waiting: a.waiting,
                },
            );
        }
    }
}

/// The UI's view of a `question` request. A request without its identity or questions still
/// shows its text, with an empty `id` so it is not offered for answering.
fn question_view(request: &Value) -> QuestionView {
    let text = |value: &Value| {
        value
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let items: Vec<QuestionItemView> = request["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|q| {
            let options = q["options"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|o| {
                    Some(QuestionOptionView {
                        label: text(&o["label"])?,
                        description: text(&o["description"]),
                        preview: text(&o["preview"]),
                    })
                })
                .collect::<Vec<_>>();
            Some(QuestionItemView {
                id: text(&q["id"])?,
                header: text(&q["header"]),
                text: text(&q["question"])?,
                multi: q["multi"].as_bool().unwrap_or_default(),
                recommended: q["recommended"]
                    .as_u64()
                    .map(|i| i as usize)
                    .filter(|&i| i < options.len()),
                options,
            })
        })
        .collect();
    let id = request["question_id"].as_str().unwrap_or_default();
    if !items.is_empty() && QuestionRef::of(request).is_some() {
        return QuestionView {
            id: id.to_owned(),
            items,
        };
    }
    QuestionView {
        id: String::new(),
        items: vec![QuestionItemView {
            id: String::new(),
            header: None,
            text: text(&request["question"])
                .or_else(|| items.first().map(|item| item.text.clone()))
                .unwrap_or_else(|| "Dial is asking a question.".to_owned()),
            options: Vec::new(),
            multi: false,
            recommended: None,
        }],
    }
}
