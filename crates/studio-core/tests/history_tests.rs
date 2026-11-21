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
