//! Issue #70: MIDI 読み込み（SMF → ノート列）のテスト。
//! 検証用 SMF はバイト列を組み立てて作る（外部ファイルに依存しない）。

use studio_core::midi_import::{import_midi, parse_midi, MidiImportError};
use studio_core::{History, Project};

fn vlq(mut n: u32) -> Vec<u8> {
    let mut buf = vec![(n & 0x7f) as u8];
    n >>= 7;
    while n > 0 {
        buf.push(((n & 0x7f) | 0x80) as u8);
        n >>= 7;
    }
    buf.reverse();
    buf
}

fn note_on(delta: u32, ch: u8, key: u8, vel: u8) -> Vec<u8> {
    let mut v = vlq(delta);
    v.extend([0x90 | ch, key, vel]);
    v
}

fn note_off(delta: u32, ch: u8, key: u8) -> Vec<u8> {
    let mut v = vlq(delta);
    v.extend([0x80 | ch, key, 64]);
    v
}

fn tempo(delta: u32, micros: u32) -> Vec<u8> {
    let mut v = vlq(delta);
    v.extend([
        0xff,
        0x51,
        0x03,
        (micros >> 16) as u8,
        (micros >> 8) as u8,
        micros as u8,
    ]);
    v
}

fn track_name(delta: u32, name: &str) -> Vec<u8> {
    let mut v = vlq(delta);
    v.extend([0xff, 0x03, name.len() as u8]);
    v.extend(name.bytes());
    v
}

fn end_of_track(delta: u32) -> Vec<u8> {
    let mut v = vlq(delta);
    v.extend([0xff, 0x2f, 0x00]);
    v
}

fn smf(format: u16, division: u16, tracks: Vec<Vec<u8>>) -> Vec<u8> {
    let mut out = b"MThd".to_vec();
    out.extend(6u32.to_be_bytes());
    out.extend(format.to_be_bytes());
    out.extend((tracks.len() as u16).to_be_bytes());
    out.extend(division.to_be_bytes());
    for track in tracks {
        out.extend(b"MTrk");
        out.extend((track.len() as u32).to_be_bytes());
        out.extend(track);
    }
    out
}

fn track(events: Vec<Vec<u8>>) -> Vec<u8> {
    events.into_iter().flatten().collect()
}

#[test]
fn format0_notes_become_a_track_and_clip() {
    // Issue #70: TPQN 480、テンポ 500000us（120 BPM）なら
    // 480 tick = 500ms。ノートオン／オフがノート列になる。
    let bytes = smf(
        0,
        480,
        vec![track(vec![
            tempo(0, 500_000),
            note_on(0, 0, 60, 100),
            note_off(480, 0, 60),
            note_on(0, 0, 64, 90),
            note_off(240, 0, 64),
            end_of_track(0),
        ])],
    );
    let import = parse_midi(&bytes).unwrap();
    assert_eq!(import.bpm, Some(120.0));
    assert_eq!(import.tracks.len(), 1);
    let t = &import.tracks[0];
    assert_eq!(t.clip_start_ms, 0);
    assert_eq!(t.clip_duration_ms, 750);
    assert_eq!(
        t.notes,
        vec![
            studio_core::Note {
                pitch: 60,
                start_ms: 0,
                duration_ms: 500,
                velocity: 100,
            },
            studio_core::Note {
                pitch: 64,
                start_ms: 500,
                duration_ms: 250,
                velocity: 90,
            },
        ]
    );
}

#[test]
fn format1_tempo_reflects_to_bpm_and_noteless_tracks_are_skipped() {
    // Issue #70: format 1 のコンダクタートラック（テンポのみ）は
    // DAW トラックにせず、テンポは bpm に反映する。150 BPM では
    // 480 tick = 400ms。トラック名はメタイベントから取る。
    let bytes = smf(
        1,
        480,
        vec![
            track(vec![tempo(0, 400_000), end_of_track(0)]),
            track(vec![
                track_name(0, "Piano"),
                note_on(0, 0, 72, 127),
                note_off(480, 0, 72),
                end_of_track(0),
            ]),
        ],
    );
    let import = parse_midi(&bytes).unwrap();
    assert_eq!(import.bpm, Some(150.0));
    assert_eq!(import.tracks.len(), 1);
    let t = &import.tracks[0];
    assert_eq!(t.name, "Piano");
    assert_eq!(
        t.notes,
        vec![studio_core::Note {
            pitch: 72,
            start_ms: 0,
            duration_ms: 400,
            velocity: 127,
        }]
    );
}

#[test]
fn channels_merge_within_a_track_including_drums() {
    // Issue #70: 同一 SMF トラック内のチャンネルは統合する
    // （ドラム ch.10 = チャンネル 9 も同じ扱い）。
    let bytes = smf(
        0,
        480,
        vec![track(vec![
            tempo(0, 500_000),
            note_on(0, 0, 60, 100),
            note_on(0, 9, 38, 80),
            note_off(240, 0, 60),
            note_off(0, 9, 38),
            end_of_track(0),
        ])],
    );
    let import = parse_midi(&bytes).unwrap();
    assert_eq!(import.tracks.len(), 1);
    let pitches: Vec<u8> = import.tracks[0].notes.iter().map(|n| n.pitch).collect();
    assert_eq!(pitches, vec![38, 60], "start と pitch でソートされる");
}

#[test]
fn tempo_map_handles_a_mid_file_tempo_change() {
    // Issue #70: 途中のテンポ変更はテンポマップで変換に使う。
    // 前半 120 BPM（480 tick = 500ms）、tick 480 から 240 BPM
    // （480 tick = 250ms）。bpm には先頭のテンポが入る。
    let bytes = smf(
        0,
        480,
        vec![track(vec![
            tempo(0, 500_000),
            note_on(0, 0, 60, 100),
            note_off(480, 0, 60),
            tempo(0, 250_000),
            note_on(0, 0, 62, 100),
            note_off(480, 0, 62),
            end_of_track(0),
        ])],
    );
    let import = parse_midi(&bytes).unwrap();
    assert_eq!(import.bpm, Some(120.0));
    let t = &import.tracks[0];
    assert_eq!(
        t.notes,
        vec![
            studio_core::Note {
                pitch: 60,
                start_ms: 0,
                duration_ms: 500,
                velocity: 100,
            },
            studio_core::Note {
                pitch: 62,
                start_ms: 500,
                duration_ms: 250,
                velocity: 100,
            },
        ]
    );
}

#[test]
fn velocity_zero_note_on_closes_the_note_and_unmatched_off_is_ignored() {
    // Issue #70: ベロシティ 0 のノートオンはノートオフ扱い。
    // 対応するノートオンの無いノートオフは無視する。
    let bytes = smf(
        0,
        480,
        vec![track(vec![
            tempo(0, 500_000),
            note_off(0, 0, 50),
            note_on(0, 0, 60, 100),
            note_on(480, 0, 60, 0),
            end_of_track(0),
        ])],
    );
    let import = parse_midi(&bytes).unwrap();
    assert_eq!(
        import.tracks[0].notes,
        vec![studio_core::Note {
            pitch: 60,
            start_ms: 0,
            duration_ms: 500,
            velocity: 100,
        }]
    );
}

#[test]
fn overlapping_same_key_notes_pair_in_order() {
    // Issue #70: 同じ音高の重なりは先入れ先出しで対応付ける。
    let bytes = smf(
        0,
        480,
        vec![track(vec![
            tempo(0, 500_000),
            note_on(0, 0, 60, 100),
            note_on(240, 0, 60, 90),
            note_off(240, 0, 60),
            note_off(240, 0, 60),
            end_of_track(0),
        ])],
    );
    let import = parse_midi(&bytes).unwrap();
    assert_eq!(
        import.tracks[0].notes,
        vec![
            studio_core::Note {
                pitch: 60,
                start_ms: 0,
                duration_ms: 500,
                velocity: 100,
            },
            studio_core::Note {
                pitch: 60,
                start_ms: 250,
                duration_ms: 500,
                velocity: 90,
            },
        ]
    );
}

#[test]
fn note_on_without_off_closes_at_the_track_end() {
    // Issue #70: ノートオフの無いノートはトラック終端で閉じる。
    let bytes = smf(
        0,
        480,
        vec![track(vec![
            tempo(0, 500_000),
            note_on(0, 0, 60, 100),
            end_of_track(480),
        ])],
    );
    let import = parse_midi(&bytes).unwrap();
    assert_eq!(
        import.tracks[0].notes,
        vec![studio_core::Note {
            pitch: 60,
            start_ms: 0,
            duration_ms: 500,
            velocity: 100,
        }]
    );
}

#[test]
fn smpte_division_is_an_error() {
    // Issue #70 スコープ: SMPTE division は精密対応せずエラー。
    // division 0xE728 = SMPTE 25fps / 40 tick。
    let bytes = smf(0, 0xE728, vec![track(vec![end_of_track(0)])]);
    assert_eq!(parse_midi(&bytes), Err(MidiImportError::SmpteNotSupported));
}

#[test]
fn garbage_and_noteless_files_are_errors() {
    // Issue #70: 壊れた／非 SMF はエラー。ノートが1つも無い
    // ファイルもエラー（何も取り込めないことを黙って成功にしない）。
    assert!(matches!(
        parse_midi(b"not a midi file at all"),
        Err(MidiImportError::InvalidFile(_))
    ));
    assert!(matches!(
        parse_midi(b""),
        Err(MidiImportError::InvalidFile(_))
    ));
    let bytes = smf(
        0,
        480,
        vec![track(vec![tempo(0, 500_000), end_of_track(0)])],
    );
    assert_eq!(parse_midi(&bytes), Err(MidiImportError::NoNotes));
    // 長さ 0 になるノート（オンとオフが同時刻）は捨てられ、
    // それだけなら NoNotes になる。
    let bytes = smf(
        0,
        480,
        vec![track(vec![
            note_on(0, 0, 60, 100),
            note_off(0, 0, 60),
            end_of_track(0),
        ])],
    );
    assert_eq!(parse_midi(&bytes), Err(MidiImportError::NoNotes));
}

#[test]
fn import_midi_applies_as_one_undo_step_and_failures_keep_state() {
    // Issue #70: 取り込みはコマンド列として適用され、Undo 1回で
    // 取り込み前に戻る。失敗時は状態も履歴も変わらない。
    let bytes = smf(
        1,
        480,
        vec![
            track(vec![tempo(0, 400_000), end_of_track(0)]),
            track(vec![
                track_name(0, "Piano"),
                note_on(0, 0, 72, 127),
                note_off(480, 0, 72),
                end_of_track(0),
            ]),
        ],
    );
    let mut history = History::new(Project::new("t"));
    let before = history.project().dump_json();
    // 失敗系: 状態も履歴も変わらない
    assert!(import_midi(&mut history, b"garbage").is_err());
    assert_eq!(history.project().dump_json(), before);
    assert!(!history.can_undo());
    // 成功系
    let summary = import_midi(&mut history, &bytes).unwrap();
    assert_eq!(summary.tracks_added, 1);
    assert_eq!(summary.notes_added, 1);
    assert_eq!(summary.bpm, Some(150.0));
    let project = history.project();
    assert_eq!(project.bpm, 150.0);
    assert_eq!(project.tracks.len(), 1);
    assert_eq!(project.tracks[0].name, "Piano");
    let clip = &project.tracks[0].clips[0];
    assert_eq!(clip.notes.len(), 1);
    assert_eq!(clip.notes[0].pitch, 72);
    // Undo 1回で取り込み前に戻る
    assert!(history.undo());
    assert_eq!(history.project().dump_json(), before);
    assert!(history.redo());
    assert_eq!(history.project().tracks.len(), 1);
}
