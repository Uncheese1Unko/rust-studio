//! Issue #29: コマンド履歴（Undo/Redo 基盤）のテスト。
//! 履歴はスナップショット方式で、失敗したコマンドは積まれない。

use studio_core::{Command, History, Project, TrackKind};

fn add_track(name: &str) -> Command {
    Command::AddTrack {
        name: name.into(),
        kind: TrackKind::Audio,
    }
}

#[test]
fn undo_restores_previous_project_state() {
    // Issue #29（指定テスト）: 適用→undo でダンプが適用前に一致する。
    let mut history = History::new(Project::new("t"));
    let initial = history.project().dump_json();
    history.apply(&add_track("A1")).unwrap();
    assert_ne!(history.project().dump_json(), initial);
    assert!(history.undo());
    assert_eq!(history.project().dump_json(), initial);
    // 履歴が空になったら undo は何も起きない（false）
    assert!(!history.undo());
    assert_eq!(history.project().dump_json(), initial);
}

#[test]
fn redo_reapplies_undone_state() {
    let mut history = History::new(Project::new("t"));
    history.apply(&add_track("A1")).unwrap();
    let applied = history.project().dump_json();
    assert!(history.undo());
    assert!(history.redo());
    assert_eq!(history.project().dump_json(), applied);
    assert!(!history.redo());
}

#[test]
fn new_command_clears_redo_stack() {
    let mut history = History::new(Project::new("t"));
    history.apply(&add_track("A1")).unwrap();
    history.apply(&add_track("A2")).unwrap();
    assert!(history.undo());
    // undo の後に新しいコマンドを適用したら redo は消える
    history.apply(&add_track("AX")).unwrap();
    assert!(!history.redo());
    let names: Vec<&str> = history
        .project()
        .tracks
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(names, ["A1", "AX"]);
}

#[test]
fn failed_command_is_not_recorded() {
    let mut history = History::new(Project::new("t"));
    let initial = history.project().dump_json();
    history.apply(&add_track("A1")).unwrap();
    // 空白名のリネームは検証で弾かれる（履歴に積まれない）
    let track_id = history.project().tracks[0].id;
    let result = history.apply(&Command::RenameTrack {
        track_id,
        name: " ".into(),
    });
    assert!(result.is_err());
    // undo 1回で初期状態に戻る（失敗分が積まれていない）
    assert!(history.undo());
    assert_eq!(history.project().dump_json(), initial);
    assert!(!history.undo());
}

#[test]
fn undo_redo_on_empty_history_are_noops() {
    let mut history = History::new(Project::new("t"));
    let initial = history.project().dump_json();
    assert!(!history.can_undo());
    assert!(!history.can_redo());
    assert!(!history.undo());
    assert!(!history.redo());
    assert_eq!(history.project().dump_json(), initial);
}

#[test]
fn history_is_capped_at_100_entries() {
    let mut history = History::new(Project::new("t"));
    for i in 1..=105 {
        history.apply(&add_track(&format!("A{i}"))).unwrap();
    }
    // 戻れるのは上限の100件まで。残るトラックは最初の5件。
    let mut undos = 0;
    while history.undo() {
        undos += 1;
    }
    assert_eq!(undos, 100);
    assert_eq!(history.project().tracks.len(), 5);
}

#[test]
fn transient_applies_with_recorded_snapshot_undo_as_one_step() {
    // Issue #31: ドラッグ中の逐次適用は、開始時スナップショットを
    // 1件だけ積むことで「ジェスチャ全体が1回の undo」になる。
    let mut history = History::new(Project::new("t"));
    let track_id = history.apply(&add_track("A1")).unwrap().unwrap();
    let clip_id = history
        .apply(&Command::AddClip {
            track_id,
            name: "c".into(),
            start_ms: 0,
            duration_ms: 1_000,
            source: "demo/c.wav".into(),
        })
        .unwrap()
        .unwrap();
    let before = history.project().dump_json();
    let snapshot = history.project().clone();
    history
        .apply_transient(&Command::MoveClip {
            clip_id,
            new_start_ms: 500,
        })
        .unwrap();
    history.record_undo_snapshot(snapshot);
    history
        .apply_transient(&Command::MoveClip {
            clip_id,
            new_start_ms: 900,
        })
        .unwrap();
    // undo 1回でジェスチャ開始前に戻る（途中の 500 には戻らない）
    assert!(history.undo());
    assert_eq!(history.project().dump_json(), before);
}

#[test]
fn transient_apply_alone_does_not_record_history() {
    // Issue #31: transient 適用だけでは履歴に積まれない。
    let mut history = History::new(Project::new("t"));
    history.apply_transient(&add_track("A1")).unwrap();
    assert!(!history.can_undo());
    assert_eq!(history.project().tracks.len(), 1);
}

#[test]
fn record_undo_snapshot_skips_state_equal_to_current() {
    // Issue #31 レビュー対応: 現在と同じスナップショットは積まれず、
    // redo スタックも消えない（無変化ジェスチャの幽霊 undo 防止）。
    let mut history = History::new(Project::new("t"));
    history.apply(&add_track("A1")).unwrap();
    assert!(history.undo());
    assert!(history.can_redo());
    let same = history.project().clone();
    history.record_undo_snapshot(same);
    assert!(!history.can_undo());
    assert!(history.can_redo());
}

#[test]
fn note_commands_round_trip_through_undo_and_redo() {
    // レビュー指摘（Issue #69）: MoveNote / RemoveNote を History
    // 経由で適用し、Undo で戻り、Redo で再適用されることを固定する
    // （ピアノロールの受け入れ条件4の基盤）。
    let mut history = History::new(Project::new("t"));
    let track = history.apply(&add_track("Keys")).unwrap().unwrap();
    let clip = history
        .apply(&Command::AddClip {
            track_id: track,
            name: "melody".into(),
            start_ms: 0,
            duration_ms: 4_000,
            source: String::new(),
        })
        .unwrap()
        .unwrap();
    history
        .apply(&Command::AddNote {
            clip_id: clip,
            pitch: 60,
            start_ms: 0,
            duration_ms: 500,
            velocity: 100,
        })
        .unwrap();
    let with_note = history.project().dump_json();
    history
        .apply(&Command::MoveNote {
            clip_id: clip,
            note_index: 0,
            new_pitch: 65,
            new_start_ms: 300,
        })
        .unwrap();
    let moved = history.project().dump_json();
    assert_ne!(moved, with_note);
    assert!(history.undo());
    assert_eq!(history.project().dump_json(), with_note, "undo move");
    assert!(history.redo());
    assert_eq!(history.project().dump_json(), moved, "redo move");
    history
        .apply(&Command::RemoveNote {
            clip_id: clip,
            note_index: 0,
        })
        .unwrap();
    assert!(history.project().clip(clip).unwrap().1.notes.is_empty());
    assert!(history.undo());
    assert_eq!(history.project().dump_json(), moved, "undo remove");
}
