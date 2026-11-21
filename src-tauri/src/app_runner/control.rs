//! Who may act on a run, and the single-use token that lets the agent act on one frame.
//!
//! The agent may send input only while it holds control, the human has not taken over, and the
//! token it names is the one from its latest frame, for the frame number that frame reported. A
//! token is spent by the input it authorizes. Taking over, resuming, a newer agent frame, a change
//! of screen size, or [`TOKEN_TTL`] passing invalidates it.
//!
//! A redraw does not: a live screen (a blinking cursor, a clock) records new frames every few
//! hundred milliseconds, faster than any model can answer, so voiding the token on each one made
//! a text console impossible to drive. Coordinates stay meaningful across redraws of the same size;
//! a resize is what moves them. The UI's input needs only the frame it saw, and always takes
//! control for the user.

use std::time::{Duration, Instant};

use super::{AppRunError, Controller};

/// How long an agent token stays valid after its frame. Long enough for a slow model turn, short
/// enough that the agent does not act on a screen it last saw minutes ago.
pub const TOKEN_TTL: Duration = Duration::from_secs(120);

/// One run's control state. Lives inside the run's entry in the table, under the table lock.
#[derive(Debug)]
pub struct Ledger {
    controller: Controller,
    human_took_over: bool,
    seq: u64,
    token: Option<Issued>,
    /// Counts the user's interruptions (input in the guest, access off). An agent batch that sees
    /// it change stops before its next step.
    user_epoch: u64,
}

/// The agent's current token: its value, the frame it was issued on, and when.
#[derive(Debug, Clone, Copy)]
struct Issued {
    value: u128,
    seq: u64,
    at: Instant,
}

impl Default for Ledger {
    fn default() -> Self {
        Ledger {
            controller: Controller::User,
            human_took_over: false,
            seq: 0,
            token: None,
            user_epoch: 0,
        }
    }
}

impl Ledger {
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// A run the agent launched: the agent holds control from the first frame, as after a resume.
    pub fn agent_launch() -> Self {
        let mut ledger = Self::default();
        ledger.resume();
        ledger
    }

    /// Whether the human took over and has not handed control back. Only `resume` clears it.
    pub fn taken_over(&self) -> bool {
        self.human_took_over
    }

    pub fn controller(&self) -> Controller {
        self.controller
    }

    /// Agent access off: control goes to the user, any agent token is void, and only turning
    /// access on again ([`Self::resume`]) gives the agent a view or input.
    pub fn take_over(&mut self) {
        self.controller = Controller::User;
        self.human_took_over = true;
        self.token = None;
        self.user_epoch += 1;
    }

    /// The user typed or clicked in the guest. Access is unchanged (the switch decides it), but the
    /// screen the agent last saw may no longer be what it meant to act on, so its token is void.
    pub fn user_acted(&mut self) {
        self.token = None;
        self.user_epoch += 1;
    }

    /// Changes whenever the user acts or turns access off; see [`Self::agent_may_continue`].
    pub fn user_epoch(&self) -> u64 {
        self.user_epoch
    }

    /// Whether an agent batch started at `epoch` may send its next step: the agent still has
    /// access and the user has not acted since.
    pub fn agent_may_continue(&self, epoch: u64) -> bool {
        self.controller == Controller::Agent && !self.human_took_over && self.user_epoch == epoch
    }

    /// The human hands control to the agent.
    pub fn resume(&mut self) {
        self.controller = Controller::Agent;
        self.human_took_over = false;
        self.token = None;
    }

    /// A new frame was recorded: its seq replaces the old one. The agent token survives a redraw
    /// of the same size (see the module note); [`Self::screen_resized`] voids it.
    pub fn advance_frame(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    /// The guest changed its screen size: coordinates chosen on the old frame no longer land
    /// where the agent meant, so its token is void.
    pub fn screen_resized(&mut self) {
        self.token = None;
    }

    /// The UI's input may name any frame already shown: a live screen (a blinking cursor, a clock)
    /// advances between a keypress and its arrival, and refusing then drops the user's typing. A
    /// frame from the future is still refused.
    pub fn check_ui_frame(&self, frame_seq: u64) -> Result<(), AppRunError> {
        if frame_seq <= self.seq {
            Ok(())
        } else {
            Err(stale_frame())
        }
    }

    /// Give the agent `token` for the current frame, if it holds control. Returns whether it did.
    /// A screenshot calls this; without control the agent gets no token. It replaces any earlier
    /// token, so only the newest frame the agent saw can authorize input.
    pub fn issue_token(&mut self, token: u128) -> Option<u128> {
        if self.controller == Controller::Agent && !self.human_took_over {
            self.token = Some(Issued {
                value: token,
                seq: self.seq,
                at: Instant::now(),
            });
            Some(token)
        } else {
            self.token = None;
            None
        }
    }

    /// Spend the token for an agent input chosen on frame `seq`.
    pub fn spend_agent_token(&mut self, seq: u64, token: u128) -> Result<(), AppRunError> {
        match self.controller {
            Controller::User if self.human_took_over => {
                return Err(AppRunError::new(
                    "taken_over",
                    "Agent access is off; only the user can turn it on again in the App sidebar.",
                ));
            }
            Controller::User => {
                return Err(AppRunError::new(
                    "not_controller",
                    "The agent does not have control of the app.",
                ));
            }
            Controller::Agent => {}
        }
        let Some(issued) = self.token.filter(|issued| issued.value == token) else {
            return Err(AppRunError::new(
                "stale_token",
                "That frame's token is no longer valid (used, replaced by a newer frame, or the screen was resized); take a new frame before acting.",
            ));
        };
        if seq != issued.seq {
            return Err(stale_frame());
        }
        if issued.at.elapsed() > TOKEN_TTL {
            self.token = None;
            return Err(AppRunError::new(
                "stale_token",
                "That frame is more than two minutes old; take a new frame before acting.",
            ));
        }
        self.token = None;
        Ok(())
    }

    /// Tests: pretend the token was issued `age` ago.
    #[cfg(test)]
    fn age_token(&mut self, age: Duration) {
        if let Some(issued) = self.token.as_mut() {
            issued.at = Instant::now() - age;
        }
    }
}

fn stale_frame() -> AppRunError {
    AppRunError::new(
        "stale_frame",
        "The screen changed; wait for the next frame and try again.",
    )
}

#[cfg(test)]
mod tests;
