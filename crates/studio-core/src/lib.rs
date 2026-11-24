//! studio-core: 動画編集ソフトと DAW の共通コア。
//!
//! モデル（Project / Track / Clip）とコマンド層を持つ。状態の変更は
//! [`command::apply`] 経由でのみ行う（ADR-0004）。GUI・メディア・
//! ネットワークには依存しない。

pub mod command;
pub mod history;
pub mod midi_import;
pub mod model;

pub use command::{apply, Command, CommandError};
pub use history::History;
pub use midi_import::{
    import_midi, parse_midi, ImportSummary, ImportedTrack, MidiImport, MidiImportError,
};
pub use model::{
    Clip, ColorTag, EffectParam, Instrument, Note, PersistenceError, PluginFormat, Project,
    ScriptEffect, Tone, Track, TrackKind, TransitionKind,
};
