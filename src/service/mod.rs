//! Service layer: lifecycle logic expressed as "request in, result out".
//!
//! Service functions never print, never prompt and never parse CLI types.
//! They report progress through a [`RequestContext`]'s event sink and run
//! child processes with the caller's environment snapshot, so the CLI and the
//! Daemon can share the same code.

pub mod context;
pub mod lifecycle;

pub use context::{EnvRunner, EnvSnapshot, EventSink, RequestContext, ServiceEvent};
