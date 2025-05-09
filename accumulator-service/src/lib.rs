//! Common library for the accumulator service.
pub mod api;
pub mod builder;
pub mod pollard;
pub mod script_utils;
pub mod state_machine;
pub mod updater;
/// Expose the primary service context.
pub use state_machine::Context;
