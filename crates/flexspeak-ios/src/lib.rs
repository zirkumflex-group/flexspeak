//! The Flexspeak voice session behind a small C interface for the SwiftUI app.
//!
//! The session itself is the desktop app's code, compiled here unchanged. Commands arrive as
//! JSON and events leave as JSON, so Swift never sees a Rust type.
#![allow(dead_code, unused_imports)]

#[path = "../../../src/alerts.rs"]
mod alerts;
#[path = "../../../src/audio.rs"]
mod audio;
#[path = "../../../src/audio_processing.rs"]
mod audio_processing;
#[path = "../../../src/client.rs"]
mod client;
#[path = "../../../src/command_flow.rs"]
mod command_flow;
#[path = "../../../src/hub.rs"]
mod hub;
#[path = "../../../src/memory.rs"]
mod memory;
#[path = "../../../src/profiler.rs"]
mod profiler;
#[path = "../../../src/streaming/mod.rs"]
mod streaming;
#[path = "../../../src/transmission.rs"]
mod transmission;
#[path = "../../../src/usage.rs"]
mod usage;

mod bridge;
