//! OpenFlow engines: processes and network services that turn audio into text and text into better text.

pub mod cloud;
pub mod downloader;
pub mod live;
pub mod meeting_engine;
pub mod notes;
pub mod ollama;
pub mod polisher;
pub mod prompts;
pub mod sidecar;
pub mod whisper_server;
