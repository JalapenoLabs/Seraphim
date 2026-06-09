//! Driving Claude Code: building turns, executing them in the workspace, and
//! parsing the resulting stream-json into normalized events.

pub mod events;
pub mod exec;

pub use events::AgentEventKind;
pub use exec::{run_turn, TurnArgs};
