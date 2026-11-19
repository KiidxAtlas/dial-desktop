//! Hands-free voice mode, like a phone call with the agent.
//!
//! The engine records, recognizes and speaks; this only sequences its live voice requests:
//! `live_start`, then `live_listen` (one utterance, run as a turn) and, once the reply has been
//! read aloud (`live_speech` false), `live_listen` again. Nothing here listens on its own after
//! a failure or a stop: the user taps to talk again.

use crate::engine::Command;
use dial_client::RequestId;
use serde::Serialize;
use serde_json::Value;

#[cfg(test)]
mod tests;

/// Asked of the agent while voice mode is on, unless the user set `voice.live.instructions`.
pub const SPOKEN_STYLE: &str = "You are talking with the user by voice: your final answer is read \
aloud. Answer in short, plain spoken sentences without Markdown, tables, code blocks or long \
lists, unless the user asks for them. Keep working with tools as usual.";

/// Where the conversation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    #[default]
    Off,
    /// `live_start` sent.
    Starting,
    /// The microphone is recording one utterance.
    Listening,
    /// The agent is working on what it heard.
    Thinking,
    /// The reply is being read aloud.
    Speaking,
    /// Waiting for the user to tap to talk (after a stop, silence or a failure).
    Paused,
}

#[derive(Debug, Default)]
pub struct Voice {
    pub phase: Phase,
    pub muted: bool,
    /// What the microphone last heard.
    pub heard: Option<String>,
    /// Why voice mode paused, when it was not the user's choice.
    pub problem: Option<String>,
    /// The microphone while listening, 0 to 100 on a perceptual scale.
    pub level: u8,
    /// The engine is recording an utterance (it detected speech that has not ended).
    pub hearing: bool,
    /// Something is being read aloud: a voice reply or one asked for with "Read aloud".
    pub reading: bool,
    /// The `live_listen` request in flight; kept after a stop so its late reply is recognized.
    listen: Option<RequestId>,
    /// That request's speech was recognized, so it now runs as a turn the conversation shows.
    transcribed: bool,
    /// The reply to the current utterance finished being read aloud.
    spoke: bool,
}

impl Voice {
    pub fn active(&self) -> bool {
        self.phase != Phase::Off
    }

    /// Start voice mode, or talk again while paused. `instructions` go to `live_start`.
    pub fn start(&mut self, instructions: Option<String>) -> Option<Command> {
        match self.phase {
            Phase::Off => {
                *self = Self {
                    listen: self.listen,
                    phase: Phase::Starting,
                    ..Self::default()
                };
                Some(Command::LiveStart { instructions })
            }
            Phase::Paused if self.listen.is_none() => {
                self.problem = None;
                Some(Command::LiveListen)
            }
            _ => None,
        }
    }

    /// The main control: interrupt a reply being read to talk, or stop recording.
    pub fn tap(&mut self, turn_running: bool) -> Option<Command> {
        match self.phase {
            Phase::Speaking if !turn_running && self.listen.is_none() => Some(Command::LiveListen),
            Phase::Listening if self.listen.is_some() && !self.transcribed => Some(Command::Abort),
            Phase::Paused => self.start(None),
            _ => None,
        }
    }

    pub fn mute(&mut self, muted: bool) -> Option<Command> {
        self.active().then(|| {
            self.muted = muted;
            Command::LiveMute(muted)
        })
    }

    /// End voice mode; a recording still running is cancelled. A turn already running continues.
    pub fn stop(&mut self) -> Vec<Command> {
        if !self.active() {
            return Vec::new();
        }
        let mut commands = Vec::new();
        if self.listen.is_some() && !self.transcribed {
            commands.push(Command::Abort);
        }
        commands.push(Command::LiveStop);
        *self = Self {
            listen: self.listen,
            transcribed: self.transcribed,
            ..Self::default()
        };
        commands
    }

    /// The connection ended, and the engine's live session with it.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// `live_level`: the microphone while recording. `rms` near 0.25 is loud speech.
    pub fn heard_level(&mut self, rms: f64, speech: bool) {
        if self.phase == Phase::Listening {
            self.level = ((rms.max(0.0) / 0.25).sqrt().min(1.0) * 100.0).round() as u8;
            self.hearing = speech;
        }
    }

    /// Stop the turn started from what was heard, because it was misheard; voice waits for a tap.
    pub fn cancel(&mut self, turn_running: bool) -> Option<Command> {
        (self.phase == Phase::Thinking && self.transcribed && turn_running).then(|| {
            self.phase = Phase::Paused;
            self.problem = None;
            Command::Abort
        })
    }

    /// A command went out as request `id`.
    pub fn requested(&mut self, command: &Command, id: RequestId) {
        if *command == Command::LiveListen {
            self.listen = Some(id);
            self.transcribed = false;
            self.spoke = false;
            if self.active() {
                self.phase = Phase::Listening;
                self.problem = None;
                self.level = 0;
                self.hearing = false;
            }
        }
    }

    /// `live_start` answered.
    pub fn started(&mut self, result: Result<&Value, String>) -> Vec<Command> {
        if self.phase != Phase::Starting {
            return Vec::new();
        }
        match result {
            Ok(session) => {
                self.muted = session["muted"].as_bool().unwrap_or_default();
                vec![Command::LiveListen]
            }
            Err(error) => {
                self.phase = Phase::Off;
                self.problem = Some(error);
                Vec::new()
            }
        }
    }

    /// `live_transcript`: the listen request whose speech is now a turn, to show as a prompt, and
    /// whether it repeats what was heard last. Noise recognized as words tends to come back as the
    /// same phrase every time, so a repeat stops the loop instead of answering it again.
    pub fn transcript(&mut self, text: &str) -> Option<(RequestId, bool)> {
        let id = self.listen.filter(|_| !self.transcribed)?;
        self.transcribed = true;
        self.level = 0;
        self.hearing = false;
        let mut repeated = false;
        if self.active() {
            repeated = self
                .heard
                .as_deref()
                .is_some_and(|last| same_words(last, text));
            self.heard = Some(text.to_owned());
            self.phase = Phase::Thinking;
            if repeated {
                self.phase = Phase::Paused;
                self.problem =
                    Some("Heard the same thing twice, so voice paused. Tap to talk".into());
            }
        }
        Some((id, repeated))
    }

    /// Whether `id` is a listen request that ended before any speech was recognized: its reply
    /// is a recording problem for this panel, not a turn for the conversation.
    pub fn owns_failed_listen(&self, id: RequestId, failed: bool) -> bool {
        failed && self.listen == Some(id) && !self.transcribed
    }

    /// `live_listen` answered: the turn it ran finished, or recording failed.
    pub fn listened(
        &mut self,
        id: RequestId,
        result: Result<&Value, (Option<&str>, String)>,
    ) -> Vec<Command> {
        if self.listen != Some(id) {
            return Vec::new();
        }
        self.listen = None;
        let transcribed = std::mem::take(&mut self.transcribed);
        if !self.active() {
            return Vec::new();
        }
        match result {
            Err((code, error)) => {
                self.phase = Phase::Paused;
                let stopped = code == Some("aborted") || error.contains("cancelled");
                // A stop the user asked for is not a problem; a reason already given stays.
                if !stopped && !transcribed {
                    self.problem = Some(hint(&error));
                }
                Vec::new()
            }
            Ok(turn) if turn["interrupted"] == true => {
                self.phase = Phase::Paused;
                Vec::new()
            }
            // Engines before `live_transcript` cannot be followed hands-free.
            Ok(_) if !transcribed => {
                self.phase = Phase::Paused;
                self.problem = Some("Update the Dial engine to talk hands-free".into());
                Vec::new()
            }
            Ok(_) if self.muted || self.spoke => vec![Command::LiveListen],
            Ok(_) => {
                self.phase = Phase::Speaking;
                Vec::new()
            }
        }
    }

    /// `live_speech`: a reply started or stopped being read aloud.
    pub fn speech(&mut self, speaking: bool, turn_running: bool) -> Vec<Command> {
        self.reading = speaking;
        // While listening, a read-out ending is the previous reply being cut off.
        if !matches!(self.phase, Phase::Thinking | Phase::Speaking) {
            return Vec::new();
        }
        if speaking {
            self.phase = Phase::Speaking;
            return Vec::new();
        }
        self.spoke = true;
        if turn_running || self.listen.is_some() {
            // The turn's answer settles it; `listened` then listens again.
            return Vec::new();
        }
        vec![Command::LiveListen]
    }
}

/// A recording failure as the next thing to try.
fn hint(error: &str) -> String {
    let lower = error.to_lowercase();
    if lower.contains("voice.stt.enabled") {
        "Turn on Dictation in Settings → Voice to talk to Dial".into()
    } else if lower.contains("no speech")
        || lower.contains("too short")
        || lower.contains("no text")
    {
        "Didn't catch that. Tap to talk again".into()
    } else if lower.contains("no microphone audio") || lower.contains("permission") {
        "No microphone audio. Check the input device and Dial's microphone permission".into()
    } else {
        error.to_owned()
    }
}

/// The same words, ignoring case and punctuation.
fn same_words(a: &str, b: &str) -> bool {
    let words = |s: &str| -> Vec<String> {
        s.split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let a = words(a);
    !a.is_empty() && a == words(b)
}
