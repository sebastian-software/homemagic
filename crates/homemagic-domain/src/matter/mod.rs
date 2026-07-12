//! SDK-neutral Matter controller domain contracts.
//!
//! The module models stable protocol identity, bounded descriptors, projected
//! state, durable operations, normalized controller events, and redacted errors.
//! It contains no controller SDK or transport dependency.

mod descriptor;
mod error;
mod event;
mod operation;
mod state;

pub use descriptor::*;
pub use error::*;
pub use event::*;
pub use operation::*;
pub use state::*;
