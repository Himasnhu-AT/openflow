//! OpenFlow core: everything that does not touch the OS or the network.

pub mod apple_date;
pub mod catalog;
pub mod config;
mod config_io;
pub mod default_tones;
pub mod echo;
pub mod field_context;
pub mod meeting;
pub mod model;
pub mod paths;
pub mod pipeline;
pub mod segmenter;
pub mod speakers;
pub mod store;
pub mod swift_json;
pub mod text_rules;
pub mod traits;
pub mod wav;
pub mod wer;
pub mod whisper_filter;

pub use config::{Config, ToneConfig};
pub use field_context::FieldContext;
pub use model::*;

/// Sample rate of every audio buffer in the app.
pub const SAMPLE_RATE: usize = 16_000;
