//! プロジェクトのモデル。時刻はすべてミリ秒（`i64`）。
//! 重いメディア実体は持たず、素材への参照（`source`）とパラメータだけを持つ。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Video,
    Audio,
}

/// トランジションの種類（Issue #49、ADR-0014）。後続クリップの
/// 属性 `transition_in_kind` として持つ。当面はクロスディゾルブ
/// のみで、種類の追加はこの列挙の拡張で受ける。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    #[default]
    None,
    CrossDissolve,
}

/// ユーザーが付けられる色のパレット識別子（Issue #58、ADR-0013）。
/// 色の指定はテーマに合う固定パレットから選ぶ方式とし、自由色は
/// 採らない。再生中専用色のホットピンク（ACCENT）はパレットに
/// 含めない。具体的な RGB はアプリ側のテーマが決める（DAW は
/// `apps/daw/src/theme.rs`）。未設定（None）は「なし＝既定の種別色」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorTag {
    Pink,
    Coral,
    Peach,
    Lemon,
    Mint,
    Teal,
    Sky,
    Lavender,
}

/// ノート（Issue #68、ADR-0016）。メロディーを構成する1音。
/// クリップに従属する値データで、独立した ID は持たない
/// （コマンドからはクリップ内のインデックスで指す）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    /// 音高（MIDI ノート番号 0–127、絶対音高）。
    pub pitch: u8,
    /// クリップ相対の開始位置（ミリ秒、0 以上）。
    pub start_ms: i64,
    /// 長さ（ミリ秒、正）。ノートは常にクリップの範囲内に収まる。
    pub duration_ms: i64,
    /// ベロシティ（0–127）。再生時は 0.0–1.0 のゲインに線形変換する。
    pub velocity: u8,
}

impl Note {
    pub fn end_ms(&self) -> i64 {
        self.start_ms + self.duration_ms
    }
}

/// 音色（楽器）（Issue #68、ADR-0016）。トラック単位で参照する。
/// 当面はサンプラーのみ。外部プラグインは後続（Issue #71 の方式
/// 決定後、#72）で Plugin バリアントとして足す拡張点。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Instrument {
    /// `source` の素材を1つ鳴らすサンプラー。`root_note` は素材の
    /// 原音の音高（MIDI ノート番号）。再生時のピッチシフト量は
    /// `note.pitch - root_note`（半音）で導出する。
    Sampler { source: String, root_note: u8 },
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
    /// 素材内の開始位置（ミリ秒、Issue #32）。0 = 素材の先頭から。
    /// 分割した右クリップでは分割点まで進んでいる。旧形式では 0。
    #[serde(default)]
    pub offset_ms: i64,
    /// 音量ゲイン（1.0 = 原音）。映像クリップでは未使用でも保持する。
    pub gain: f32,
    /// 人物切り抜きの適用有無（Issue #22）。旧形式の JSON では false。
    #[serde(default)]
    pub cutout: bool,
    /// ピッチ変更量（半音、Issue #18）。0.0 = 原音。旧形式では 0.0。
    #[serde(default)]
    pub pitch_semitones: f32,
    /// ユーザーが付けた色（Issue #58、ADR-0013）。None は未設定
    /// （既定の種別色）。旧形式の JSON では None。
    #[serde(default)]
    pub color: Option<ColorTag>,
    /// フェードインの長さ（ミリ秒、Issue #34）。0 = フェードなし。
    /// 旧形式の JSON では 0。
    #[serde(default)]
    pub fade_in_ms: i64,
    /// フェードアウトの長さ（ミリ秒、Issue #34）。0 = フェードなし。
    /// 旧形式の JSON では 0。
    #[serde(default)]
    pub fade_out_ms: i64,
    /// タイトル文字列（Issue #50）。空文字 = タイトルなし。
    /// プレビューでは再生区間の画面下部中央に重ねて表示する。
    /// 旧形式の JSON では空文字。
    #[serde(default)]
    pub title_text: String,
    /// 先頭境界のトランジション種類（Issue #49、ADR-0014）。
    /// 後続クリップ側の属性として持ち、直接隣接する前のクリップ
    /// との境界にだけプレビューで適用される。旧形式では None。
    #[serde(default)]
    pub transition_in_kind: TransitionKind,
    /// トランジションの長さ（ミリ秒、Issue #49、ADR-0014）。
    /// kind が None のときは常に 0。旧形式の JSON では 0。
    #[serde(default)]
    pub transition_in_ms: i64,
    /// ノート列（Issue #68、ADR-0016）。空でないクリップは
    /// ノートクリップとして扱い、再生では `source` の波形を
    /// 鳴らさない（ノートクリップの `source` は空文字で運用）。
    /// 旧形式の JSON では空。
    #[serde(default)]
    pub notes: Vec<Note>,
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
    /// ユーザーが付けた色（Issue #58、ADR-0013）。None は未設定
    /// （既定の種別色）。旧形式の JSON では None。
    #[serde(default)]
    pub color: Option<ColorTag>,
    /// ミュート（消音。Issue #39）。旧形式の JSON では false。
    #[serde(default)]
    pub muted: bool,
    /// ソロ（このトラックだけ鳴らす。Issue #39）。複数可。
    /// 旧形式の JSON では false。
    #[serde(default)]
    pub solo: bool,
    /// 音色（楽器）（Issue #68、ADR-0016）。None は楽器なし
    /// （ノートクリップは鳴らない）。旧形式の JSON では None。
    #[serde(default)]
    pub instrument: Option<Instrument>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub tracks: Vec<Track>,
    /// テンポ（BPM、Issue #35）。旧形式の JSON では既定 120。
    #[serde(default = "default_bpm")]
    pub bpm: f32,
    next_id: u64,
}

fn default_bpm() -> f32 {
    120.0
}

impl Project {
    pub fn new(name: impl Into<String>) -> Self {
        Project {
            name: name.into(),
            tracks: Vec::new(),
            bpm: 120.0,
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
