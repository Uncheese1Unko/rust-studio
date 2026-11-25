//! プロジェクトのモデル。時刻はすべてミリ秒（`i64`）。
//! 重いメディア実体は持たず、素材への参照（`source`）とパラメータだけを持つ。

use std::collections::BTreeMap;

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

/// プラグインの形式（Issue #72、ADR-0017）。当面は CLAP のみ。
/// 形式の追加はこの列挙の拡張で受ける。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginFormat {
    Clap,
}

/// 組み込みシンセの波形（Issue #89、ADR-0022）。外部素材を持たず、
/// ノートの音高から波形を手続き的に生成して鳴らす楽器
/// （[`Instrument::Synth`]）の波形を選ぶ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SynthWaveform {
    Sine,
    Triangle,
    Square,
    Sawtooth,
}

impl SynthWaveform {
    /// JSON 表記と同じ短い名前（表示用）。
    pub fn as_str(&self) -> &'static str {
        match self {
            SynthWaveform::Sine => "sine",
            SynthWaveform::Triangle => "triangle",
            SynthWaveform::Square => "square",
            SynthWaveform::Sawtooth => "sawtooth",
        }
    }
}

/// 音色（楽器）（Issue #68、ADR-0016）。トラック単位で参照する。
/// サンプラーと、外部プラグイン（Issue #72、ADR-0017 の
/// ヘッドレス CLAP ホスティング＋プリセット JSON 注入）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Instrument {
    /// `source` の素材を1つ鳴らすサンプラー。`root_note` は素材の
    /// 原音の音高（MIDI ノート番号）。再生時のピッチシフト量は
    /// `note.pitch - root_note`（半音）で導出する。
    Sampler { source: String, root_note: u8 },
    /// 外部プラグインを別プロセスのヘルパー経由で鳴らす楽器
    /// （Issue #72、ADR-0017）。プラグイン本体は同梱せず、
    /// 利用者が入手したものへの参照だけを持つ。
    Plugin {
        /// プラグイン形式（当面 CLAP のみ）。
        format: PluginFormat,
        /// プラグイン（.clap バンドル／ファイル）へのパス。
        plugin_path: String,
        /// プリセットファイル（Vital なら .vital）へのパス。
        /// None はプラグインの既定状態で鳴らす。
        preset_path: Option<String>,
    },
    /// 組み込みシンセ（Issue #89、ADR-0022）。外部素材を持たず、
    /// ノートの音高から波形を手続き的に生成して鳴らす。MIDI
    /// 取り込み時の既定楽器として割り当てられ、利用者は後から
    /// サンプラーやプラグインへ差し替えられる。
    Synth {
        /// 波形の種類。
        waveform: SynthWaveform,
    },
}

impl Instrument {
    /// MIDI 取り込み時の既定楽器（Issue #89、ADR-0022）。
    /// 取り込んだトラックが追加操作なしで鳴るための組み込み
    /// シンセ。波形は、倍音が少なく音高が分かりやすい三角波。
    pub fn default_synth() -> Self {
        Instrument::Synth {
            waveform: SynthWaveform::Triangle,
        }
    }
}

/// スクリプトエフェクトのパラメータ値（Issue #81）。JSON では
/// 素のスカラーとして読み書きする（untagged）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EffectParam {
    Number(f64),
    Bool(bool),
    Text(String),
}

/// クリップに適用する Lua スクリプトエフェクト（Issue #81）。
/// スクリプト本体は埋め込まず参照（名前または相対パス）で持つ。
/// 実行と描画はアプリ側（video-editor）が担い、コアは参照と
/// パラメータだけを持つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptEffect {
    /// スクリプト参照（名前または相対パス）。
    pub script: String,
    /// パラメータ（名前 → 値）。旧形式では空。
    #[serde(default)]
    pub params: BTreeMap<String, EffectParam>,
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
    /// Lua スクリプトエフェクト（Issue #81）。None = エフェクト
    /// なし。フェード（#34）・トランジション（#49）と同じく
    /// クリップの属性として持つ。旧形式の JSON では None。
    #[serde(default)]
    pub script_effect: Option<ScriptEffect>,
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
    /// 割り当てられた音色（Project.tones への参照、Issue #73、
    /// ADR-0018）。Some の間は実効楽器は音色側が優先される。
    /// 旧形式の JSON では None。
    #[serde(default)]
    pub tone_id: Option<u64>,
}

/// 名前付きの音色（Issue #73、ADR-0018）。プロジェクト内に
/// コレクションとして持ち、トラックは `Track.tone_id` で参照する。
/// 中身は楽器そのもの（サンプラー／プラグイン）で、音色の切り替えは
/// 参照の差し替えとして表れる。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tone {
    pub id: u64,
    pub name: String,
    pub instrument: Instrument,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub tracks: Vec<Track>,
    /// 音色のコレクション（Issue #73、ADR-0018）。旧形式の
    /// JSON では空。
    #[serde(default)]
    pub tones: Vec<Tone>,
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
            tones: Vec::new(),
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

    pub fn tone(&self, tone_id: u64) -> Option<&Tone> {
        self.tones.iter().find(|t| t.id == tone_id)
    }

    pub fn tone_mut(&mut self, tone_id: u64) -> Option<&mut Tone> {
        self.tones.iter_mut().find(|t| t.id == tone_id)
    }

    /// トラックの実効楽器（Issue #73、ADR-0018）。音色が割り当て
    /// られていれば音色の楽器、無ければ（参照先が消えている場合も）
    /// トラック直接の楽器を返す。再生・事前レンダリングはこちらを
    /// 使うこと。
    pub fn effective_instrument(&self, track: &Track) -> Option<Instrument> {
        if let Some(tone_id) = track.tone_id {
            if let Some(tone) = self.tone(tone_id) {
                return Some(tone.instrument.clone());
            }
        }
        track.instrument.clone()
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
