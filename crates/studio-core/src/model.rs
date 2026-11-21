//! プロジェクトのモデル。時刻はすべてミリ秒（`i64`）。
//! 重いメディア実体は持たず、素材への参照（`source`）とパラメータだけを持つ。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Video,
    Audio,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: u64,
    pub name: String,
    /// トラック上の開始位置（ミリ秒）
    pub start_ms: i64,
    /// 長さ（ミリ秒）。常に正。
    pub duration_ms: i64,
    /// 素材への参照（ファイルパスや URI）。実体はアプリ側が持つ。
    pub source: String,
    /// 音量ゲイン（1.0 = 原音）。映像クリップでは未使用でも保持する。
    pub gain: f32,
}

impl Clip {
    pub fn end_ms(&self) -> i64 {
        self.start_ms + self.duration_ms
    }
}

fn default_track_volume() -> f32 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: u64,
    pub name: String,
    pub kind: TrackKind,
    pub clips: Vec<Clip>,
    /// トラック音量（Issue #15）。旧形式の JSON では欠けているため
    /// 既定 1.0 で読み込む。
    #[serde(default = "default_track_volume")]
    pub volume: f32,
    /// パン（-1.0 = 左、0.0 = 中央、1.0 = 右。Issue #15）。
    #[serde(default)]
    pub pan: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub tracks: Vec<Track>,
    next_id: u64,
}

impl Project {
    pub fn new(name: impl Into<String>) -> Self {
        Project {
            name: name.into(),
            tracks: Vec::new(),
            next_id: 1,
        }
    }

    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn track(&self, track_id: u64) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == track_id)
    }

    pub fn track_mut(&mut self, track_id: u64) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == track_id)
    }

    pub fn clip(&self, clip_id: u64) -> Option<(&Track, &Clip)> {
        self.tracks
            .iter()
            .find_map(|t| t.clips.iter().find(|c| c.id == clip_id).map(|c| (t, c)))
    }

    /// 状態の全量ダンプ（JSON）。API と computer use テストの観測経路（ADR-0005）。
    pub fn dump_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("Project is always serializable")
    }

    /// プロジェクトを JSON ファイルに保存する（Issue #3）。
    ///
    /// 保存はアトミックに行う（Issue #11 レビュー対応）: いったん
    /// `<path>.tmp` に書いてから rename する。共有ファイルを GUI と MCP が
    /// 読むため、書き込み途中の壊れた内容が読者に見えてはならない。
    pub fn save_to_path(&self, path: &std::path::Path) -> Result<(), PersistenceError> {
        let json = serde_json::to_string_pretty(self)?;
        let tmp = std::path::PathBuf::from(format!("{}.tmp", path.display()));
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// JSON ファイルからプロジェクトを読み込む（Issue #3）。
    ///
    /// 読み込んだ結果は新しい `Project` として返る。失敗した場合は
    /// `Err` を返すだけで、呼び出し側が保持している既存の状態には
    /// 一切触れない。
    pub fn load_from_path(path: &std::path::Path) -> Result<Project, PersistenceError> {
        let text = std::fs::read_to_string(path)?;
        let project = serde_json::from_str(&text)?;
        Ok(project)
    }
}

/// プロジェクトの保存・読込で起こるエラー（Issue #3）。
#[derive(Debug)]
pub enum PersistenceError {
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PersistenceError::Io(e) => write!(f, "failed to access project file: {e}"),
            PersistenceError::Json(e) => write!(f, "failed to parse project JSON: {e}"),
        }
    }
}

impl std::error::Error for PersistenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PersistenceError::Io(e) => Some(e),
            PersistenceError::Json(e) => Some(e),
        }
    }
}

impl From<std::io::Error> for PersistenceError {
    fn from(e: std::io::Error) -> Self {
        PersistenceError::Io(e)
    }
}

impl From<serde_json::Error> for PersistenceError {
    fn from(e: serde_json::Error) -> Self {
        PersistenceError::Json(e)
    }
}
