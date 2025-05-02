//! Common library for the accumulator service.
pub mod api;
pub mod builder;
pub mod leaves;
pub mod pollard;
pub mod state_machine;
pub mod updater;
/// Expose the primary service context.
pub use state_machine::Context;
