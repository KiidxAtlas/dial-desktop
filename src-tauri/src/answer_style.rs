//! What answers may draw (Settings → Appearance → In answers), and the standing guidance that
//! tells the model so. The web UI owns the switches; it reports them here so every engine this
//! app starts gets the matching `--append-system-prompt`, read once per launch and cached by the
//! provider like the rest of the system prompt.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;

/// The engine flag that carries the guidance.
pub const FLAG: &str = "--append-system-prompt";

/// The Appearance switches that change what the model is asked to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnswerStyle {
    pub cards: bool,
    pub diagrams: bool,
}

impl Default for AnswerStyle {
    fn default() -> Self {
        Self { cards: true, diagrams: true }
    }
}

const INTRO: &str = "Dial Desktop draws rich answers. When a picture or something to try would \
explain better than prose (how parts connect, a process, trade-offs, numbers to play with, steps \
to follow), include one without waiting to be asked. Use at most one or two per answer, only where \
they add real value. Skip them for short factual replies, code changes and reviews, or when the \
user wants plain text.";

const DIAGRAMS: &str = "Diagrams and charts: a fenced mermaid block (flowchart, sequenceDiagram, \
stateDiagram-v2, classDiagram, erDiagram, timeline, or xychart-beta for charts). Keep them small \
with short plain labels. Lay out a chain of more than four steps top-down (flowchart TD) so it \
fits the answer column.";

const CARDS: &str = r#"Interactive cards: one fenced dial-app block of JSON (no scripts or HTML) with "title", "kind" and optional "followUps" like [{"label":"Go deeper","ask":"Explain … in more detail"}]. Kinds:
- "guide": {"steps":[{"title":"Install","text":"Markdown","checklist":["Node 20 installed"]}]}
- "calculator": {"inputs":[{"id":"users","label":"Users","type":"slider","min":0,"max":1000,"step":10,"value":100,"unit":"k"}],"outputs":[{"id":"cost","label":"Monthly cost","formula":"users * 0.4","unit":"$","decimals":0}]}. Input types: slider, number, select (with "options":[{"label":"…","value":1}]), toggle. Formulas use + - * / % ^, comparisons, a ? b : c, min max abs round floor ceil sqrt pow clamp log ln exp, input ids and earlier output ids.
- "compare": {"tabs":[{"label":"Option A","text":"Markdown"}]} with 2-8 tabs.
- "explore": {"diagram":"flowchart LR\n  A[Client] --> B[Server]","intro":"Markdown","nodes":{"A":{"text":"Markdown"},"B":{"text":"Markdown"}}}, keyed by Mermaid node id. Use a flowchart or stateDiagram-v2 (other diagram types have no clickable nodes) and give every node and subgraph an entry, so none is left unexplained. Keep it to about 12 nodes so it reads at a glance; for a bigger system show the top level and offer the parts as followUps.
- "screens": branching steps {"screens":[{"id":"start","text":"Markdown","choices":[{"label":"Next","to":"end"}]},{"id":"end","text":"Done"}]}.
Text fields are Markdown. Keep labels short."#;

impl AnswerStyle {
    /// The text appended to the system prompt; `None` when both switches are off.
    pub fn guidance(self) -> Option<String> {
        let parts: Vec<&str> = [(self.diagrams, DIAGRAMS), (self.cards, CARDS)]
            .into_iter()
            .filter_map(|(on, text)| on.then_some(text))
            .collect();
        if parts.is_empty() {
            return None;
        }
        Some(format!("{INTRO}\n\n{}", parts.join("\n\n")))
    }
}

/// Where the switches are kept for engines started before the window reports them.
pub fn file(home: Option<&Path>) -> Option<PathBuf> {
    crate::launcher::app_dir(home).map(|dir| dir.join("answer-style.json"))
}

/// The saved switches; the defaults when none were saved or the file is unreadable.
pub fn load(path: Option<&Path>) -> AnswerStyle {
    path.and_then(|p| std::fs::read(p).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, style: AnswerStyle) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_vec(&style).map_err(|e| e.to_string())?;
    let partial = path.with_extension("json.partial");
    std::fs::write(&partial, json).map_err(|e| e.to_string())?;
    std::fs::rename(&partial, path).map_err(|e| e.to_string())
}

/// Whether each engine binary takes [`FLAG`] after `rpc`; older engines refuse it and would not
/// start, so they get no guidance instead.
#[derive(Default)]
pub struct Support(Mutex<HashMap<PathBuf, bool>>);

impl Support {
    pub fn accepts(&self, program: &Path) -> bool {
        let mut known = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        *known
            .entry(program.to_path_buf())
            .or_insert_with(|| probe(program))
    }
}

/// `dial rpc --append-system-prompt x --help` exits 0 only when the flag is known.
fn probe(program: &Path) -> bool {
    let mut command = Command::new(program);
    command.args(["rpc", FLAG, "x", "--help"]);
    crate::launcher::run_with_timeout(&mut command, Duration::from_secs(10)).is_some()
}
