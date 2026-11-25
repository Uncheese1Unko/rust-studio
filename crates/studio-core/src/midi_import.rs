//! MIDI 読み込み（Issue #70）: Standard MIDI File をノート列へ
//! 変換し、コマンド列としてプロジェクトへ取り込む。
//!
//! 変換は純粋関数 [`parse_midi`]（SMF バイト列 → [`MidiImport`]）と、
//! 履歴への適用 [`import_midi`] に分ける。状態変更はコマンド層
//! 経由のみ（ADR-0004）。適用は複製の上でコマンド列を組み立てて
//! から [`History::commit_replacement`] で一度に差し替えるため、
//! 部分適用の状態が残らず、Undo 1回で取り込み前に戻れる。
//!
//! 割り当て方針（イシュー記載）: SMF のトラックごとに DAW の
//! トラックを1つ作り、同一トラック内のチャンネルは統合する
//! （ドラム ch.10 も同じ扱い）。ノートの無いトラックは作らない。
//! テンポはテンポマップとして tick→ms 変換に使い、プロジェクト
//! の bpm には先頭のテンポを入れる（SetBpm の範囲外なら入れない。
//! 変換自体はテンポマップで正確に行う）。SMPTE division は
//! エラーとして扱う（スコープ外）。
//!
//! 取り込んだトラックには既定の楽器（組み込みシンセ、
//! [`Instrument::default_synth`]）を `SetTrackInstrument` で
//! 割り当てる（Issue #89、ADR-0022）。楽器のないノートクリップは
//! 鳴らないため、割り当てが無いと「読み込んでも無音」になる。
//! 割り当ては取り込みのコマンド列の一部なので、Undo 1回で
//! 割り当てごと取り込み前に戻る。

use std::collections::{HashMap, VecDeque};
use std::fmt;

use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};

use crate::command::{apply, Command, CommandError};
use crate::history::History;
use crate::model::{Instrument, Note, TrackKind};

/// 変換結果: 取り込むトラック列と、プロジェクトへ反映する bpm。
#[derive(Debug, Clone, PartialEq)]
pub struct MidiImport {
    /// 先頭テンポから計算した bpm。テンポ指定が無い、または
    /// SetBpm の範囲（20–300）外なら None（bpm は変更しない）。
    pub bpm: Option<f32>,
    pub tracks: Vec<ImportedTrack>,
}

/// 取り込むトラック1つ分。クリップはノート列の範囲に置かれ、
/// ノートはクリップ相対で持つ（モデルの形そのまま）。
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedTrack {
    pub name: String,
    pub clip_start_ms: i64,
    pub clip_duration_ms: i64,
    pub notes: Vec<Note>,
}

/// 取り込みの結果サマリ（入口が表示に使う）。
#[derive(Debug, Clone, PartialEq)]
pub struct ImportSummary {
    pub tracks_added: usize,
    pub notes_added: usize,
    pub bpm: Option<f32>,
}

/// MIDI 読み込みのエラー。
#[derive(Debug, Clone, PartialEq)]
pub enum MidiImportError {
    /// SMF として読めなかった（壊れた／非 SMF ファイル）。
    InvalidFile(String),
    /// SMPTE division は未対応（スコープ外）。
    SmpteNotSupported,
    /// format 2（逐次ソング）は未対応（スコープは format 0/1）。
    SequentialFormatNotSupported,
    /// ノートが1つも無い（何も取り込めない）。
    NoNotes,
    /// コマンド適用の失敗（変換結果は検証済みのため通常は起きない）。
    Command(CommandError),
}

impl fmt::Display for MidiImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MidiImportError::InvalidFile(msg) => write!(f, "invalid MIDI file: {msg}"),
            MidiImportError::SmpteNotSupported => {
                write!(f, "SMPTE time division is not supported")
            }
            MidiImportError::SequentialFormatNotSupported => {
                write!(f, "SMF format 2 (sequential songs) is not supported")
            }
            MidiImportError::NoNotes => write!(f, "no notes found in the MIDI file"),
            MidiImportError::Command(e) => write!(f, "command failed during import: {e}"),
        }
    }
}

impl std::error::Error for MidiImportError {}

/// テンポマップ: (tick, マイクロ秒／四分音符) の点列から
/// tick → ms へ変換する。点が無い区間は標準テンポ
/// （500000us = 120 BPM）で数える。
struct TempoMap {
    ticks_per_beat: u32,
    points: Vec<(u64, u32)>,
}

impl TempoMap {
    fn ms_at(&self, tick: u64) -> i64 {
        let mut ms = 0.0f64;
        let mut cur_tick = 0u64;
        let mut cur_us = 500_000u32;
        for &(point_tick, point_us) in &self.points {
            if point_tick >= tick {
                break;
            }
            if point_tick > cur_tick {
                ms += (point_tick - cur_tick) as f64 * cur_us as f64
                    / (self.ticks_per_beat as f64 * 1000.0);
                cur_tick = point_tick;
            }
            cur_us = point_us;
        }
        ms += (tick - cur_tick) as f64 * cur_us as f64 / (self.ticks_per_beat as f64 * 1000.0);
        ms.round() as i64
    }
}

/// SMF バイト列をノート列へ変換する純粋関数。
pub fn parse_midi(bytes: &[u8]) -> Result<MidiImport, MidiImportError> {
    let smf = Smf::parse(bytes).map_err(|e| MidiImportError::InvalidFile(e.to_string()))?;
    // スコープは format 0/1。format 2（逐次ソング）を並行トラック
    // として黙って取り込まないよう明示的にエラーにする。
    if matches!(smf.header.format, midly::Format::Sequential) {
        return Err(MidiImportError::SequentialFormatNotSupported);
    }
    let ticks_per_beat = match smf.header.timing {
        Timing::Metrical(t) => t.as_int() as u32,
        Timing::Timecode(..) => return Err(MidiImportError::SmpteNotSupported),
    };
    if ticks_per_beat == 0 {
        return Err(MidiImportError::InvalidFile("time division is zero".into()));
    }

    // テンポマップは全トラックから集める（format 1 では通常
    // コンダクタートラックにある）。同 tick の複数点は下で正規化する。
    let mut points: Vec<(u64, u32)> = Vec::new();
    for track in &smf.tracks {
        let mut tick = 0u64;
        for event in track {
            tick += event.delta.as_int() as u64;
            if let TrackEventKind::Meta(MetaMessage::Tempo(us)) = event.kind {
                points.push((tick, us.as_int()));
            }
        }
    }
    points.sort_by_key(|(tick, _)| *tick);
    // 同じ tick に複数の点がある場合は最後の点が実効テンポになる。
    // bpm 反映と変換が食い違わないよう、ここで正規化して単一の
    // 点列から両方を導出する（#70 レビュー対応）。
    let mut normalized: Vec<(u64, u32)> = Vec::new();
    for point in points {
        match normalized.last_mut() {
            Some(last) if last.0 == point.0 => last.1 = point.1,
            _ => normalized.push(point),
        }
    }
    let tempo_map = TempoMap {
        ticks_per_beat,
        points: normalized,
    };

    // プロジェクトの bpm には先頭のテンポ（正規化済み点列の先頭
    // ＝最も早い tick の実効テンポ）を入れる。
    let bpm = tempo_map.points.first().and_then(|(_, us)| {
        let bpm = 60_000_000.0 / *us as f32;
        (20.0..=300.0).contains(&bpm).then_some(bpm)
    });

    let mut tracks = Vec::new();
    for (index, track) in smf.tracks.iter().enumerate() {
        if let Some(imported) = convert_track(track, index, &tempo_map) {
            tracks.push(imported);
        }
    }
    if tracks.is_empty() {
        return Err(MidiImportError::NoNotes);
    }
    Ok(MidiImport { bpm, tracks })
}

/// SMF トラック1つを変換する。ノートが無ければ None。
fn convert_track(
    track: &[midly::TrackEvent<'_>],
    index: usize,
    tempo_map: &TempoMap,
) -> Option<ImportedTrack> {
    let mut tick = 0u64;
    let mut name: Option<String> = None;
    // (チャンネル, 音高) → 開始 tick とベロシティのキュー。
    // 同じ音の重なりは先入れ先出しで対応付ける。
    let mut open: HashMap<(u8, u8), VecDeque<(u64, u8)>> = HashMap::new();
    let mut done: Vec<(u64, u64, u8, u8)> = Vec::new();
    for event in track {
        tick += event.delta.as_int() as u64;
        match &event.kind {
            TrackEventKind::Midi { channel, message } => {
                let channel = channel.as_int();
                match message {
                    MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                        open.entry((channel, key.as_int()))
                            .or_default()
                            .push_back((tick, vel.as_int()));
                    }
                    MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
                        if let Some(queue) = open.get_mut(&(channel, key.as_int())) {
                            if let Some((start, vel)) = queue.pop_front() {
                                done.push((start, tick, key.as_int(), vel));
                            }
                        }
                    }
                    _ => {}
                }
            }
            TrackEventKind::Meta(MetaMessage::TrackName(bytes)) => {
                let text = String::from_utf8_lossy(bytes).trim().to_string();
                if name.is_none() && !text.is_empty() {
                    name = Some(text);
                }
            }
            _ => {}
        }
    }
    // ノートオフの無いノートはトラック終端で閉じる。列挙順が
    // HashMap の反復順に依存しないよう、後段で決定的に並べ直す。
    for ((_, key), queue) in &open {
        for (start, vel) in queue {
            done.push((*start, tick, *key, *vel));
        }
    }
    done.sort_by_key(|(start, _, key, vel)| (*start, *key, *vel));

    let mut notes: Vec<Note> = done
        .iter()
        .filter_map(|(start, end, key, vel)| {
            let start_ms = tempo_map.ms_at(*start);
            let end_ms = tempo_map.ms_at(*end);
            // 変換後に長さ 0 以下になるノートは捨てる（モデルの
            // 不変条件: 長さは正）。
            (end_ms > start_ms).then_some(Note {
                pitch: *key,
                start_ms,
                duration_ms: end_ms - start_ms,
                velocity: *vel,
            })
        })
        .collect();
    if notes.is_empty() {
        return None;
    }
    notes.sort_by_key(|n| (n.start_ms, n.pitch, n.velocity));
    let clip_start = notes.iter().map(|n| n.start_ms).min().unwrap_or(0);
    let clip_end = notes
        .iter()
        .map(|n| n.start_ms + n.duration_ms)
        .max()
        .unwrap_or(0);
    for note in &mut notes {
        note.start_ms -= clip_start;
    }
    Some(ImportedTrack {
        name: name.unwrap_or_else(|| format!("MIDI {}", index + 1)),
        clip_start_ms: clip_start,
        clip_duration_ms: clip_end - clip_start,
        notes,
    })
}

/// 変換したコマンド列をプロジェクトへ適用する共通処理。
/// `apply_fn` はコマンド適用の実体（組み立て先のプロジェクトへの
/// 適用）。作成された ID を後続コマンドが使う。
fn run<F>(apply_fn: &mut F, import: &MidiImport) -> Result<ImportSummary, CommandError>
where
    F: FnMut(&Command) -> Result<Option<u64>, CommandError>,
{
    if let Some(bpm) = import.bpm {
        apply_fn(&Command::SetBpm { bpm })?;
    }
    let mut notes_added = 0usize;
    for track in &import.tracks {
        let track_id = apply_fn(&Command::AddTrack {
            name: track.name.clone(),
            kind: TrackKind::Audio,
        })?
        .ok_or(CommandError::InvalidRange("add_track returned no id"))?;
        // 取り込んだトラックは追加操作なしで鳴るよう、既定の
        // 楽器（組み込みシンセ）を割り当てる（Issue #89）。
        apply_fn(&Command::SetTrackInstrument {
            track_id,
            instrument: Some(Instrument::default_synth()),
        })?;
        let clip_id = apply_fn(&Command::AddClip {
            track_id,
            name: track.name.clone(),
            start_ms: track.clip_start_ms,
            duration_ms: track.clip_duration_ms,
            source: String::new(),
        })?
        .ok_or(CommandError::InvalidRange("add_clip returned no id"))?;
        for note in &track.notes {
            apply_fn(&Command::AddNote {
                clip_id,
                pitch: note.pitch,
                start_ms: note.start_ms,
                duration_ms: note.duration_ms,
                velocity: note.velocity,
            })?;
            notes_added += 1;
        }
    }
    Ok(ImportSummary {
        tracks_added: import.tracks.len(),
        notes_added,
        bpm: import.bpm,
    })
}

/// SMF バイト列を取り込む。失敗時はプロジェクトも履歴も
/// 変わらない。成功時は Undo 1回で取り込み前に戻れる。
pub fn import_midi(history: &mut History, bytes: &[u8]) -> Result<ImportSummary, MidiImportError> {
    let import = parse_midi(bytes)?;
    // 複製の上でコマンド列を最後まで適用してから一度に差し替える
    // ため、失敗しても実プロジェクトに部分適用が残らない。
    let mut built = history.project().clone();
    let summary =
        run(&mut |cmd| apply(&mut built, cmd), &import).map_err(MidiImportError::Command)?;
    history.commit_replacement(built);
    Ok(summary)
}
