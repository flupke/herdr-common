//! The vision MCP core: an agent's eyes and hands on an app running in the app sandbox, a
//! private herdr-app on a headless display that never reaches the user's.
//!
//! An app's MCP server starts a [`Vision`] session on an [`app_sandbox::AppSandbox`] it set up,
//! puts it in [`VisionTools::session`], and routes the vision tools with its own
//! ([`VisionTools::router`]): screen, screenshot, key, type, click, drag, settle and `wait_for`.
//! An app whose terminal program links `vision-signal` gets each action's reaction from its
//! frames; any other app, from its window's pixels.

mod frames;
mod screen;
mod session;
mod tools;

pub use app_sandbox::Restart;
pub use session::{Input, Reaction, Screen, Vision};
pub use tools::{INSTRUCTIONS, VisionTools};
