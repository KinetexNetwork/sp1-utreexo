//! Common library for the accumulator service.
pub mod builder;
pub mod updater;
pub mod pollard;
pub mod state_machine;
pub mod api;
/// Expose the primary service context.
pub use state_machine::Context;