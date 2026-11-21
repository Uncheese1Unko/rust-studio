use studio_core::{apply, Command, CommandError, Project, TrackKind};

fn project_with_clip() -> (Project, u64, u64) {
    let mut p = Project::new("test");
    let track_id = apply(
        &mut p,
        &Command::AddTrack {
            name: "V1".into(),
            kind: TrackKind::Video,
        },
    )
    .unwrap()
    .unwrap();
    let clip_id = apply(
        &mut p,
        &Command::AddClip {
            track_id,
            name: "opening".into(),
            start_ms: 0,
            duration_ms: 5_000,
            source: "media/opening.mp4".into(),
        },
    )
    .unwrap()
    .unwrap();
    (p, track_id, clip_id)
}

#[test]
fn add_track_and_clip() {
    let (p, track_id, clip_id) = project_with_clip();
    assert_eq!(p.tracks.len(), 1);
    let (_, clip) = p.clip(clip_id).unwrap();
    assert_eq!(clip.name, "opening");
    assert_eq!(clip.end_ms(), 5_000);
    assert_eq!(p.track(track_id).unwrap().clips.len(), 1);
}

#[test]
fn move_and_gain_and_remove_clip() {
    let (mut p, _, clip_id) = project_with_clip();
    apply(
        &mut p,
        &Command::MoveClip {
            clip_id,
            new_start_ms: 2_500,
        },
    )
    .unwrap();
    apply(&mut p, &Command::SetClipGain { clip_id, gain: 0.5 }).unwrap();
    let (_, clip) = p.clip(clip_id).unwrap();
    assert_eq!(clip.start_ms, 2_500);
    assert_eq!(clip.gain, 0.5);

    apply(&mut p, &Command::RemoveClip { clip_id }).unwrap();
    assert!(p.clip(clip_id).is_none());
}

#[test]
fn failed_command_leaves_state_unchanged() {
    let (mut p, track_id, _) = project_with_clip();
    let before = p.dump_json();

    let err = apply(
        &mut p,
        &Command::AddClip {
            track_id,
            name: "bad".into(),
            start_ms: -1,
            duration_ms: 100,
            source: "x".into(),
        },
    );
    assert_eq!(
        err,
        Err(CommandError::InvalidRange("start_ms must be >= 0"))
    );

    let err = apply(
        &mut p,
        &Command::MoveClip {
            clip_id: 9999,
            new_start_ms: 0,
        },
    );
    assert_eq!(err, Err(CommandError::ClipNotFound(9999)));

    let err = apply(&mut p, &Command::RemoveTrack { track_id: 4242 });
    assert_eq!(err, Err(CommandError::TrackNotFound(4242)));

    assert_eq!(p.dump_json(), before);
}

#[test]
fn commands_and_state_round_trip_through_json() {
    // API / MCP から JSON で受けられること（ADR-0004）
    let json = r#"{"type":"add_clip","track_id":1,"name":"a","start_ms":100,"duration_ms":900,"source":"s.wav"}"#;
    let cmd: Command = serde_json::from_str(json).unwrap();
    assert!(matches!(cmd, Command::AddClip { .. }));

    let (p, _, _) = project_with_clip();
    let dumped = p.dump_json();
    let restored: Project = serde_json::from_str(&dumped).unwrap();
    assert_eq!(restored, p);
}

#[test]
fn trim_clip_changes_start_and_duration() {
    let (mut p, _track_id, clip_id) = project_with_clip();
    apply(
        &mut p,
        &Command::TrimClip {
            clip_id,
            new_start_ms: 500,
            new_duration_ms: 3_000,
        },
    )
    .unwrap();
    let (_, clip) = p.clip(clip_id).unwrap();
    assert_eq!(clip.start_ms, 500);
    assert_eq!(clip.duration_ms, 3_000);
    assert_eq!(clip.end_ms(), 3_500);
}

#[test]
fn trim_clip_rejects_invalid_ranges_and_keeps_state() {
    let (mut p, _track_id, clip_id) = project_with_clip();
    let before = p.dump_json();
    for (start, duration) in [(-1, 3_000), (500, 0), (500, -100)] {
        let err = apply(
            &mut p,
            &Command::TrimClip {
                clip_id,
                new_start_ms: start,
                new_duration_ms: duration,
            },
        )
        .unwrap_err();
        assert!(matches!(err, CommandError::InvalidRange(_)), "{err}");
        assert_eq!(p.dump_json(), before, "失敗したコマンドが状態を変えた");
    }
}

#[test]
fn trim_clip_unknown_clip_errors_and_keeps_state() {
    let (mut p, _track_id, _clip_id) = project_with_clip();
    let before = p.dump_json();
    let err = apply(
        &mut p,
        &Command::TrimClip {
            clip_id: 9999,
            new_start_ms: 0,
            new_duration_ms: 1_000,
        },
    )
    .unwrap_err();
    assert_eq!(err, CommandError::ClipNotFound(9999));
    assert_eq!(p.dump_json(), before);
}

#[test]
fn trim_clip_command_round_trips_through_json() {
    // API／MCP から JSON で受けられる形であること（architecture ルール）。
    let cmd = Command::TrimClip {
        clip_id: 42,
        new_start_ms: 500,
        new_duration_ms: 3_000,
    };
    let text = serde_json::to_string(&cmd).unwrap();
    assert!(text.contains("\"type\":\"trim_clip\""), "{text}");
    let parsed: Command = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed, cmd);
    // デシリアライズしたコマンドがそのまま適用できること
    let (mut p, _track_id, clip_id) = project_with_clip();
    let parsed = Command::TrimClip {
        clip_id,
        new_start_ms: 500,
        new_duration_ms: 3_000,
    };
    apply(&mut p, &parsed).unwrap();
    let (_, clip) = p.clip(clip_id).unwrap();
    assert_eq!((clip.start_ms, clip.duration_ms), (500, 3_000));
}
