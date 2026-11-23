//! Core for `gd`, the Geometry Dash version manager.
//!
//! No GUI framework anywhere in here. Every module takes a path or a callback
//! and does the work, so the terminal front end is the only consumer and there is
//! nothing to keep in sync with a second one.

pub mod config;
pub mod depot;
pub mod depot_session;
pub mod geode;
pub mod instance;
pub mod manifests;
pub mod mh;
pub mod mh_install;
pub mod paths;
pub mod pull;
pub mod steam;
pub mod tui;
pub mod util;
