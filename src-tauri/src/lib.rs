//! Native desktop interface for Dial.
//!
//! The app talks to the engine only through `dial rpc` via [`dial_client`]: [`engine`] owns the
//! connection on a worker thread, [`model`] folds what it reports into state, [`tabs`] keeps one
//! engine per running session, [`view`] turns that state into the JSON snapshot the web UI
//! renders, and [`app`] wires them into Tauri.

pub mod agent_browser;
pub mod app;
pub mod attachments;
pub mod browser;
pub mod browser_mcp;
pub mod cli;
pub mod engine;
pub mod files;
pub mod launcher;
pub mod model;
mod model_library;
mod notifications;
pub mod preview;
pub mod settings;
pub mod tabs;
pub mod terminal;
pub mod time;
pub mod updates;
pub mod view;
pub mod voice;
