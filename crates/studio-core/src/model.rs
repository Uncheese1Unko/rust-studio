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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: u64,
    pub name: String,
    pub kind: TrackKind,
    pub clips: Vec<Clip>,
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
        self.tracks.iter().find_map(|t| {
            t.clips
                .iter()
                .find(|c| c.id == clip_id)
                .map(|c| (t, c))
        })
    }

    /// 状態の全量ダンプ（JSON）。API と computer use テストの観測経路（ADR-0005）。
    pub fn dump_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("Project is always serializable")
    }
}
