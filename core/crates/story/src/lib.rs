//! Story Mode and the Dojo lessons, run from the game's own event flows.
//!
//! - [`flow`]: a BFEVFL interpreter whose actors are implemented by a host (callbacks).
//! - [`data`]: the Story tables, hub map and flows the importer puts into the asset cache from
//!   the owner's dump (nothing of the game is in this repository).
//! - [`state`]: the Story save state (what `quest.dat` holds), as JSON.
//! - [`story`]: the Story rules behind the flows' queries and actions, the screens' request /
//!   answer loop, and the rules of playing a job (lives, TOO BAD, Luigi).
//! - [`lesson`]: running a lesson flow for the Dojo screen.
//!
//! Specs: research/specs/story-mode.md and coursebot.md §B (private).

pub mod data;
pub mod flow;
pub mod lesson;
pub mod state;
pub mod story;

pub use data::StoryData;
pub use state::StoryState;
pub use story::{JobOutcome, JobPlay, Request, Story, TooBadChoice};
