use std::path::PathBuf;

use studio_core::{apply, Command, Project, TrackKind};

fn sample_project() -> Project {
    let mut p = Project::new("round trip");
    let track_id = apply(
        &mut p,
        &Command::AddTrack {
            name: "A1".into(),
            kind: TrackKind::Audio,
        },
    )
    .unwrap()
    .unwrap();
    apply(
        &mut p,
        &Command::AddClip {
            track_id,
            name: "vocal".into(),
            start_ms: 1_000,
            duration_ms: 2_500,
            source: "media/vocal.wav".into(),
        },
    )
    .unwrap();
    p
}

fn temp_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "studio-core-persistence-{}-{}.json",
        std::process::id(),
        tag
    ))
}

#[test]
fn save_and_load_round_trip_restores_identical_state() {
    let project = sample_project();
    let path = temp_path("round-trip");

    project.save_to_path(&path).unwrap();
    let loaded = Project::load_from_path(&path).unwrap();

    assert_eq!(loaded, project);
    assert_eq!(loaded.dump_json(), project.dump_json());

    std::fs::remove_file(&path).ok();
}

#[test]
fn load_of_corrupt_json_errors() {
    let path = temp_path("corrupt");
    std::fs::write(&path, "{ this is not json").unwrap();

    let result = Project::load_from_path(&path);

    assert!(result.is_err());
    std::fs::remove_file(&path).ok();
}

#[test]
fn failed_load_leaves_existing_state_unchanged() {
    // 読み込みは新しい Project を返す方式なので、失敗しても既存の状態は
    // 影響を受けないことを、既存プロジェクト側から確認する。
    let existing = sample_project();
    let before = existing.dump_json();

    let path = temp_path("corrupt-unchanged");
    std::fs::write(&path, "[[[broken").unwrap();
    let result = Project::load_from_path(&path);
    assert!(result.is_err());

    assert_eq!(existing.dump_json(), before);
    std::fs::remove_file(&path).ok();
}

#[test]
fn load_of_missing_file_errors() {
    let path = temp_path("missing");
    std::fs::remove_file(&path).ok();

    let result = Project::load_from_path(&path);

    assert!(result.is_err());
}
