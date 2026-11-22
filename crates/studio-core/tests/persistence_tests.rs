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
fn mute_and_solo_survive_save_load_and_legacy_json_defaults() {
    // Issue #39: ミュート／ソロが保存・復元され、旧 JSON（フィールド
    // なし）では false として読める。
    let mut p = sample_project();
    let track_id = p.tracks[0].id;
    apply(
        &mut p,
        &Command::SetTrackMute {
            track_id,
            muted: true,
        },
    )
    .unwrap();
    apply(
        &mut p,
        &Command::SetTrackSolo {
            track_id,
            solo: true,
        },
    )
    .unwrap();
    let path = temp_path("mute-solo");
    p.save_to_path(&path).unwrap();
    let loaded = Project::load_from_path(&path).unwrap();
    assert!(loaded.tracks[0].muted && loaded.tracks[0].solo);
    let _ = std::fs::remove_file(&path);

    let legacy = r#"{
        "name": "legacy", "bpm": 120.0,
        "tracks": [{"id": 1, "name": "A1", "kind": "audio", "clips": [],
                    "volume": 1.0, "pan": 0.0, "color": null}],
        "next_id": 2
    }"#;
    let parsed: Project = serde_json::from_str(legacy).unwrap();
    assert!(!parsed.tracks[0].muted && !parsed.tracks[0].solo);
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

#[test]
fn save_is_atomic_and_leaves_no_temp_file() {
    // Issue #11 レビュー対応: 保存は tmp + rename で行い、書き込み途中の
    // 壊れたファイルが読者に見えないこと。tmp ファイルも残らない。
    let project = sample_project();
    let path = temp_path("atomic");
    project.save_to_path(&path).unwrap();
    assert!(path.exists());
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    assert!(!tmp.exists(), "temp file must be renamed away");
    let loaded = Project::load_from_path(&path).unwrap();
    assert_eq!(loaded, project);
    let _ = std::fs::remove_file(&path);
}
