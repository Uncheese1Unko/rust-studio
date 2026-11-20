//! studio-core: 動画編集ソフトと DAW の共通コア。
//!
//! モデル（Project / Track / Clip）とコマンド層を持つ。状態の変更は
//! [`command::apply`] 経由でのみ行う（ADR-0004）。GUI・メディア・
//! ネットワークには依存しない。

pub mod command;
pub mod model;

pub use command::{apply, Command, CommandError};
pub use model::{Clip, Project, Track, TrackKind};
