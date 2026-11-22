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

#[test]
fn set_track_mute_and_solo_apply() {
    // Issue #39: ミュート／ソロのコマンド。
    let (mut p, track_id, _clip_id) = project_with_clip();
    assert!(!p.tracks[0].muted && !p.tracks[0].solo, "既定は両方 false");

    apply(
        &mut p,
        &Command::SetTrackMute {
            track_id,
            muted: true,
        },
    )
    .unwrap();
    assert!(p.tracks[0].muted);
    apply(
        &mut p,
        &Command::SetTrackSolo {
            track_id,
            solo: true,
        },
    )
    .unwrap();
    assert!(p.tracks[0].solo);

    // 存在しないトラックは弾いて状態を変えない
    let before = p.dump_json();
    let err = apply(
        &mut p,
        &Command::SetTrackMute {
            track_id: 9999,
            muted: true,
        },
    )
    .unwrap_err();
    assert_eq!(err, CommandError::TrackNotFound(9999));
    let err = apply(
        &mut p,
        &Command::SetTrackSolo {
            track_id: 9999,
            solo: true,
        },
    )
    .unwrap_err();
    assert_eq!(err, CommandError::TrackNotFound(9999));
    assert_eq!(p.dump_json(), before);
}

#[test]
fn set_track_volume_and_pan_apply_and_validate() {
    // Issue #15: トラック音量・パンのコマンド。
    let (mut p, track_id, _clip_id) = project_with_clip();
    // 既定値
    assert_eq!(p.tracks[0].volume, 1.0);
    assert_eq!(p.tracks[0].pan, 0.0);

    apply(
        &mut p,
        &Command::SetTrackVolume {
            track_id,
            volume: 0.25,
        },
    )
    .unwrap();
    assert_eq!(p.tracks[0].volume, 0.25);
    apply(
        &mut p,
        &Command::SetTrackPan {
            track_id,
            pan: -0.5,
        },
    )
    .unwrap();
    assert_eq!(p.tracks[0].pan, -0.5);

    // 範囲外は弾いて状態を変えない
    let before = p.dump_json();
    let err = apply(
        &mut p,
        &Command::SetTrackVolume {
            track_id,
            volume: 2.5,
        },
    )
    .unwrap_err();
    assert!(matches!(err, CommandError::InvalidRange(_)));
    let err = apply(&mut p, &Command::SetTrackPan { track_id, pan: 1.5 }).unwrap_err();
    assert!(matches!(err, CommandError::InvalidRange(_)));
    assert_eq!(p.dump_json(), before);

    // 存在しないトラック
    let err = apply(
        &mut p,
        &Command::SetTrackVolume {
            track_id: 9999,
            volume: 1.0,
        },
    )
    .unwrap_err();
    assert_eq!(err, CommandError::TrackNotFound(9999));
    // パン側も同じく（Issue #15 レビュー観察: 対称性の担保）
    let err = apply(
        &mut p,
        &Command::SetTrackPan {
            track_id: 9999,
            pan: 0.0,
        },
    )
    .unwrap_err();
    assert_eq!(err, CommandError::TrackNotFound(9999));
}

#[test]
fn track_volume_pan_json_round_trip_and_legacy_defaults() {
    // コマンドが JSON で往復できること（API／MCP の前提）
    let cmd = Command::SetTrackPan {
        track_id: 7,
        pan: 0.75,
    };
    let text = serde_json::to_string(&cmd).unwrap();
    assert!(text.contains("\"type\":\"set_track_pan\""), "{text}");
    let parsed: Command = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed, cmd);

    // volume / pan を含まない旧形式のプロジェクト JSON も読み込め、
    // 既定値（音量 1.0、パン 0.0）になること
    let legacy =
        r#"{"name":"old","tracks":[{"id":1,"name":"A1","kind":"audio","clips":[]}],"next_id":2}"#;
    let project: Project = serde_json::from_str(legacy).unwrap();
    assert_eq!(project.tracks[0].volume, 1.0);
    assert_eq!(project.tracks[0].pan, 0.0);
}

#[test]
fn set_clip_cutout_applies_and_validates() {
    // Issue #22: 人物切り抜きの適用フラグ。
    let (mut p, _track_id, clip_id) = project_with_clip();
    assert!(!p.clip(clip_id).unwrap().1.cutout, "default is off");
    apply(
        &mut p,
        &Command::SetClipCutout {
            clip_id,
            enabled: true,
        },
    )
    .unwrap();
    assert!(p.clip(clip_id).unwrap().1.cutout);
    // JSON 往復
    let cmd = Command::SetClipCutout {
        clip_id,
        enabled: true,
    };
    let text = serde_json::to_string(&cmd).unwrap();
    assert!(text.contains("\"type\":\"set_clip_cutout\""), "{text}");
    // 存在しないクリップでは状態を変えない
    let before = p.dump_json();
    let err = apply(
        &mut p,
        &Command::SetClipCutout {
            clip_id: 9999,
            enabled: true,
        },
    )
    .unwrap_err();
    assert_eq!(err, CommandError::ClipNotFound(9999));
    assert_eq!(p.dump_json(), before);
    // cutout を含まない旧形式のクリップ JSON も false で読み込める
    let legacy = r#"{"name":"old","tracks":[{"id":1,"name":"V1","kind":"video","clips":[{"id":2,"name":"c","start_ms":0,"duration_ms":1000,"source":"x.mp4","gain":1.0}],"volume":1.0,"pan":0.0}],"next_id":3}"#;
    let project: Project = serde_json::from_str(legacy).unwrap();
    assert!(!project.tracks[0].clips[0].cutout);
}

#[test]
fn set_clip_pitch_applies_and_validates() {
    // Issue #18: クリップのピッチ（半音）。方式は ADR-0009。
    let (mut p, _track_id, clip_id) = project_with_clip();
    assert_eq!(p.clip(clip_id).unwrap().1.pitch_semitones, 0.0);
    apply(
        &mut p,
        &Command::SetClipPitch {
            clip_id,
            semitones: 3.5,
        },
    )
    .unwrap();
    assert_eq!(p.clip(clip_id).unwrap().1.pitch_semitones, 3.5);
    // JSON 往復
    let cmd = Command::SetClipPitch {
        clip_id,
        semitones: -12.0,
    };
    let text = serde_json::to_string(&cmd).unwrap();
    assert!(text.contains("\"type\":\"set_clip_pitch\""), "{text}");
    // 範囲外（±24 半音を超える）は状態を変えない
    for bad in [24.5, -25.0, f32::NAN] {
        let before = p.dump_json();
        let err = apply(
            &mut p,
            &Command::SetClipPitch {
                clip_id,
                semitones: bad,
            },
        )
        .unwrap_err();
        assert!(
            matches!(err, CommandError::InvalidRange(_)),
            "bad={bad}: {err}"
        );
        assert_eq!(p.dump_json(), before);
    }
    // 境界値は通る
    apply(
        &mut p,
        &Command::SetClipPitch {
            clip_id,
            semitones: 24.0,
        },
    )
    .unwrap();
    // 存在しないクリップ
    let err = apply(
        &mut p,
        &Command::SetClipPitch {
            clip_id: 9999,
            semitones: 1.0,
        },
    )
    .unwrap_err();
    assert_eq!(err, CommandError::ClipNotFound(9999));
    // pitch を含まない旧形式 JSON は 0.0 で読み込める
    let legacy = r#"{"name":"old","tracks":[{"id":1,"name":"A1","kind":"audio","clips":[{"id":2,"name":"c","start_ms":0,"duration_ms":1000,"source":"x.wav","gain":1.0,"cutout":false}],"volume":1.0,"pan":0.0}],"next_id":3}"#;
    let project: Project = serde_json::from_str(legacy).unwrap();
    assert_eq!(project.tracks[0].clips[0].pitch_semitones, 0.0);
}

#[test]
fn rename_track_changes_only_the_name() {
    // Issue #55: 名前だけが変わり、他の状態（クリップ・音量・パン・
    // トラック数）は変わらないこと。ダンプにも新しい名前が現れる。
    let (mut p, track_id, clip_id) = project_with_clip();
    apply(
        &mut p,
        &Command::SetTrackVolume {
            track_id,
            volume: 0.5,
        },
    )
    .unwrap();
    let result = apply(
        &mut p,
        &Command::RenameTrack {
            track_id,
            name: "メイン映像".into(),
        },
    )
    .unwrap();
    assert_eq!(result, None);
    let track = p.track(track_id).unwrap();
    assert_eq!(track.name, "メイン映像");
    assert_eq!(track.volume, 0.5);
    assert_eq!(track.clips.len(), 1);
    assert_eq!(track.clips[0].id, clip_id);
    assert!(p.dump_json().contains("メイン映像"));
}

#[test]
fn rename_track_unknown_id_keeps_state() {
    // Issue #55: 存在しない track_id はエラーで、状態は一切変わらない。
    let (mut p, _, _) = project_with_clip();
    let before = p.dump_json();
    let err = apply(
        &mut p,
        &Command::RenameTrack {
            track_id: 9999,
            name: "x".into(),
        },
    )
    .unwrap_err();
    assert_eq!(err, CommandError::TrackNotFound(9999));
    assert_eq!(p.dump_json(), before);
}

#[test]
fn rename_track_rejects_blank_name() {
    // Issue #55: 空文字・空白のみの名前は InvalidRange で、状態不変。
    let (mut p, track_id, _) = project_with_clip();
    let before = p.dump_json();
    for bad in ["", " ", "  \t\n "] {
        let err = apply(
            &mut p,
            &Command::RenameTrack {
                track_id,
                name: bad.into(),
            },
        )
        .unwrap_err();
        assert!(
            matches!(err, CommandError::InvalidRange(_)),
            "bad={bad:?}: {err}"
        );
        assert_eq!(p.dump_json(), before);
    }
}

#[test]
fn set_track_color_round_trips_through_json() {
    // Issue #58 / ADR-0013: トラック色はパレット識別子で保持し、
    // プロジェクト JSON を往復しても残る。色なしへの復帰もできる。
    use studio_core::ColorTag;
    let (mut p, track_id, _) = project_with_clip();
    assert_eq!(p.track(track_id).unwrap().color, None, "既定は未設定");
    apply(
        &mut p,
        &Command::SetTrackColor {
            track_id,
            color: Some(ColorTag::Mint),
        },
    )
    .unwrap();
    assert_eq!(p.track(track_id).unwrap().color, Some(ColorTag::Mint));
    // プロジェクト JSON の往復
    let dumped = p.dump_json();
    assert!(dumped.contains("\"color\": \"mint\""), "{dumped}");
    let restored: Project = serde_json::from_str(&dumped).unwrap();
    assert_eq!(
        restored.track(track_id).unwrap().color,
        Some(ColorTag::Mint)
    );
    // コマンド自体の JSON 往復（API／MCP の前提）
    let cmd = Command::SetTrackColor {
        track_id,
        color: Some(ColorTag::Sky),
    };
    let text = serde_json::to_string(&cmd).unwrap();
    assert!(text.contains("\"type\":\"set_track_color\""), "{text}");
    assert_eq!(serde_json::from_str::<Command>(&text).unwrap(), cmd);
    // None で「なし」に戻せる
    apply(
        &mut p,
        &Command::SetTrackColor {
            track_id,
            color: None,
        },
    )
    .unwrap();
    assert_eq!(p.track(track_id).unwrap().color, None);
    // 存在しないトラックでは状態を変えない
    let before = p.dump_json();
    let err = apply(
        &mut p,
        &Command::SetTrackColor {
            track_id: 9999,
            color: Some(ColorTag::Pink),
        },
    )
    .unwrap_err();
    assert_eq!(err, CommandError::TrackNotFound(9999));
    assert_eq!(p.dump_json(), before);
}

#[test]
fn set_clip_color_applies_and_validates() {
    // Issue #58 / ADR-0013: クリップ色もパレット識別子で保持する。
    use studio_core::ColorTag;
    let (mut p, _, clip_id) = project_with_clip();
    assert_eq!(p.clip(clip_id).unwrap().1.color, None, "既定は未設定");
    apply(
        &mut p,
        &Command::SetClipColor {
            clip_id,
            color: Some(ColorTag::Lemon),
        },
    )
    .unwrap();
    assert_eq!(p.clip(clip_id).unwrap().1.color, Some(ColorTag::Lemon));
    apply(
        &mut p,
        &Command::SetClipColor {
            clip_id,
            color: None,
        },
    )
    .unwrap();
    assert_eq!(p.clip(clip_id).unwrap().1.color, None);
    let before = p.dump_json();
    let err = apply(
        &mut p,
        &Command::SetClipColor {
            clip_id: 9999,
            color: Some(ColorTag::Pink),
        },
    )
    .unwrap_err();
    assert_eq!(err, CommandError::ClipNotFound(9999));
    assert_eq!(p.dump_json(), before);
}

#[test]
fn legacy_json_without_color_loads_as_unset() {
    // Issue #58: color を含まない旧形式 JSON は未設定（None）で読める。
    let legacy = r#"{"name":"old","tracks":[{"id":1,"name":"A1","kind":"audio","clips":[{"id":2,"name":"c","start_ms":0,"duration_ms":1000,"source":"x.wav","gain":1.0}],"volume":1.0,"pan":0.0}],"next_id":3}"#;
    let project: Project = serde_json::from_str(legacy).unwrap();
    assert_eq!(project.tracks[0].color, None);
    assert_eq!(project.tracks[0].clips[0].color, None);
}

#[test]
fn split_clip_creates_right_clip_with_advanced_offset() {
    // Issue #32: 分割すると左は元の id のまま短くなり、右は新規 id で
    // 素材内開始位置（offset_ms）が分割点まで進んでいる。
    let (mut p, _track_id, clip_id) = project_with_clip();
    // project_with_clip のクリップは start 0 / duration 5000。
    let new_id = apply(
        &mut p,
        &Command::SplitClip {
            clip_id,
            at_ms: 2_000,
        },
    )
    .unwrap()
    .expect("split returns the new clip id");
    assert_ne!(new_id, clip_id);
    let (_, left) = p.clip(clip_id).unwrap();
    assert_eq!(left.start_ms, 0);
    assert_eq!(left.duration_ms, 2_000);
    assert_eq!(left.offset_ms, 0);
    let (track, right) = p.clip(new_id).unwrap();
    assert_eq!(track.clips.len(), 2);
    assert_eq!(right.start_ms, 2_000);
    assert_eq!(right.duration_ms, 3_000);
    assert_eq!(right.offset_ms, 2_000);
    assert_eq!(right.source, left.source);
    assert_eq!(right.name, left.name);
    assert_eq!(right.gain, left.gain);
}

#[test]
fn split_clip_outside_range_fails_and_keeps_state() {
    // Issue #32: 分割位置がクリップ内部（start < t < end）でない場合は
    // InvalidRange で失敗し、状態は変わらない。未知のクリップは
    // ClipNotFound。
    let (mut p, _, clip_id) = project_with_clip();
    let before = p.dump_json();
    for at_ms in [0, 5_000, -1, 6_000] {
        let err = apply(&mut p, &Command::SplitClip { clip_id, at_ms });
        assert!(
            matches!(err, Err(CommandError::InvalidRange(_))),
            "at_ms={at_ms}: {err:?}"
        );
        assert_eq!(p.dump_json(), before, "state must not change on failure");
    }
    let err = apply(
        &mut p,
        &Command::SplitClip {
            clip_id: 999,
            at_ms: 100,
        },
    );
    assert_eq!(err, Err(CommandError::ClipNotFound(999)));
    assert_eq!(p.dump_json(), before);
}

#[test]
fn split_clip_twice_accumulates_offset() {
    // Issue #32: 右クリップをさらに分割すると、offset は元の
    // オフセットから積み上がる。
    let (mut p, _, clip_id) = project_with_clip();
    let right_id = apply(
        &mut p,
        &Command::SplitClip {
            clip_id,
            at_ms: 1_000,
        },
    )
    .unwrap()
    .unwrap();
    let third_id = apply(
        &mut p,
        &Command::SplitClip {
            clip_id: right_id,
            at_ms: 3_000,
        },
    )
    .unwrap()
    .unwrap();
    let (_, middle) = p.clip(right_id).unwrap();
    assert_eq!(middle.offset_ms, 1_000);
    assert_eq!(middle.duration_ms, 2_000);
    let (_, third) = p.clip(third_id).unwrap();
    assert_eq!(third.start_ms, 3_000);
    assert_eq!(third.duration_ms, 2_000);
    assert_eq!(third.offset_ms, 3_000);
}

#[test]
fn split_clip_command_round_trips_through_json() {
    // Issue #32: コマンドは JSON 経路（API / MCP）でも往復できる。
    let cmd = Command::SplitClip {
        clip_id: 7,
        at_ms: 1_234,
    };
    let json = serde_json::to_string(&cmd).unwrap();
    let back: Command = serde_json::from_str(&json).unwrap();
    assert_eq!(cmd, back);
}

#[test]
fn legacy_json_without_offset_loads_as_zero() {
    // Issue #32: offset_ms を含まない旧形式 JSON は 0 で読める。
    let legacy = r#"{"name":"old","tracks":[{"id":1,"name":"A1","kind":"audio","clips":[{"id":2,"name":"c","start_ms":0,"duration_ms":1000,"source":"x.wav","gain":1.0}],"volume":1.0,"pan":0.0}],"next_id":3}"#;
    let project: Project = serde_json::from_str(legacy).unwrap();
    assert_eq!(project.tracks[0].clips[0].offset_ms, 0);
}

#[test]
fn set_clip_fade_applies_and_validates() {
    // Issue #34: SetClipFade は fade_in_ms / fade_out_ms を設定する。
    // 負の値とクリップ長超過は InvalidRange で状態が変わらない。
    let (mut p, _track_id, clip_id) = project_with_clip();
    apply(
        &mut p,
        &Command::SetClipFade {
            clip_id,
            fade_in_ms: 500,
            fade_out_ms: 800,
        },
    )
    .unwrap();
    let (_, clip) = p.clip(clip_id).unwrap();
    assert_eq!(clip.fade_in_ms, 500);
    assert_eq!(clip.fade_out_ms, 800);

    let before = p.dump_json();
    for (fi, fo) in [(-1, 0), (0, -5), (5_001, 0), (0, 5_001)] {
        let result = apply(
            &mut p,
            &Command::SetClipFade {
                clip_id,
                fade_in_ms: fi,
                fade_out_ms: fo,
            },
        );
        assert!(
            matches!(result, Err(CommandError::InvalidRange(_))),
            "fade {fi}/{fo}: {result:?}"
        );
        assert_eq!(p.dump_json(), before, "状態が変わらないこと");
    }
    let result = apply(
        &mut p,
        &Command::SetClipFade {
            clip_id: 999,
            fade_in_ms: 0,
            fade_out_ms: 0,
        },
    );
    assert_eq!(result, Err(CommandError::ClipNotFound(999)));
}

#[test]
fn legacy_json_without_fades_loads_as_zero() {
    // Issue #34: フェードを含まない旧形式 JSON は 0 で読める。
    let legacy = r#"{"name":"old","tracks":[{"id":1,"name":"A1","kind":"audio","clips":[{"id":2,"name":"c","start_ms":0,"duration_ms":1000,"source":"x.wav","gain":1.0}],"volume":1.0,"pan":0.0}],"next_id":3}"#;
    let project: Project = serde_json::from_str(legacy).unwrap();
    let clip = &project.tracks[0].clips[0];
    assert_eq!(clip.fade_in_ms, 0);
    assert_eq!(clip.fade_out_ms, 0);
}

#[test]
fn split_clip_keeps_fades_on_the_outer_edges_only() {
    // Issue #34: 分割すると、左は fade_in だけ（分割点は切り口なので
    // fade_out は付けない）、右は fade_out だけを引き継ぐ。各フェードは
    // 分割後の長さでクランプされる。
    let (mut p, _track_id, clip_id) = project_with_clip();
    apply(
        &mut p,
        &Command::SetClipFade {
            clip_id,
            fade_in_ms: 500,
            fade_out_ms: 700,
        },
    )
    .unwrap();
    let new_id = apply(
        &mut p,
        &Command::SplitClip {
            clip_id,
            at_ms: 2_000,
        },
    )
    .unwrap()
    .unwrap();
    let (_, left) = p.clip(clip_id).unwrap();
    assert_eq!(left.fade_in_ms, 500);
    assert_eq!(left.fade_out_ms, 0);
    let (_, right) = p.clip(new_id).unwrap();
    assert_eq!(right.fade_in_ms, 0);
    assert_eq!(right.fade_out_ms, 700);
}

#[test]
fn set_bpm_applies_and_validates_range() {
    // Issue #35: SetBpm は 20..=300 の範囲でだけ適用され、範囲外と
    // NaN では状態が変わらない。既定値は 120。
    let (mut p, _track_id, _clip_id) = project_with_clip();
    assert_eq!(p.bpm, 120.0);
    apply(&mut p, &Command::SetBpm { bpm: 90.0 }).unwrap();
    assert_eq!(p.bpm, 90.0);
    apply(&mut p, &Command::SetBpm { bpm: 20.0 }).unwrap();
    apply(&mut p, &Command::SetBpm { bpm: 300.0 }).unwrap();
    assert_eq!(p.bpm, 300.0);

    let before = p.dump_json();
    for bad in [19.9, 300.1, 0.0, -120.0, f32::NAN, f32::INFINITY] {
        let result = apply(&mut p, &Command::SetBpm { bpm: bad });
        assert!(
            matches!(result, Err(CommandError::InvalidRange(_))),
            "bpm {bad}: {result:?}"
        );
        assert_eq!(p.dump_json(), before, "状態が変わらないこと");
    }
}

#[test]
fn legacy_json_without_bpm_loads_default_120() {
    // Issue #35: bpm を含まない旧形式 JSON は既定 120 で読める。
    let legacy = r#"{"name":"old","tracks":[],"next_id":1}"#;
    let project: Project = serde_json::from_str(legacy).unwrap();
    assert_eq!(project.bpm, 120.0);
    // 往復で bpm が保存される
    let mut p = project;
    apply(&mut p, &Command::SetBpm { bpm: 128.0 }).unwrap();
    let json = p.dump_json();
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(back.bpm, 128.0);
}

#[test]
fn set_clip_title_round_trips_through_json() {
    // Issue #50（イシュー指定のテスト）: SetClipTitle で設定した
    // タイトルが、JSON へ保存→読込しても一致する。空文字で消去。
    let (mut p, _track_id, clip_id) = project_with_clip();
    let (_, clip) = p.clip(clip_id).unwrap();
    assert_eq!(clip.title_text, "", "初期値はタイトルなし（空文字）");

    apply(
        &mut p,
        &Command::SetClipTitle {
            clip_id,
            title: "Opening Scene".into(),
        },
    )
    .unwrap();
    let json = p.dump_json();
    assert!(json.contains("\"title_text\": \"Opening Scene\""), "{json}");
    let loaded: Project = serde_json::from_str(&json).unwrap();
    let (_, clip) = loaded.clip(clip_id).unwrap();
    assert_eq!(clip.title_text, "Opening Scene");

    // 旧形式（title_text フィールドなし）も空文字で読める
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    value["tracks"][0]["clips"][0]
        .as_object_mut()
        .unwrap()
        .remove("title_text");
    let legacy = serde_json::to_string(&value).unwrap();
    assert!(!legacy.contains("title_text"));
    let loaded_legacy: Project = serde_json::from_str(&legacy).unwrap();
    let (_, clip) = loaded_legacy.clip(clip_id).unwrap();
    assert_eq!(clip.title_text, "");

    // 空文字で消去できる
    apply(
        &mut p,
        &Command::SetClipTitle {
            clip_id,
            title: String::new(),
        },
    )
    .unwrap();
    let (_, clip) = p.clip(clip_id).unwrap();
    assert_eq!(clip.title_text, "");

    // 存在しないクリップはエラーで、状態は変わらない
    let before = p.dump_json();
    let result = apply(
        &mut p,
        &Command::SetClipTitle {
            clip_id: 999_999,
            title: "x".into(),
        },
    );
    assert!(result.is_err());
    assert_eq!(p.dump_json(), before);
}

#[test]
fn set_clip_transition_validates_and_round_trips() {
    // Issue #49（ADR-0014）: SetClipTransition は後続クリップの属性
    // として持ち、種類で検証を分ける。cross_dissolve は
    // 0 < duration <= クリップ長、none は duration 0 のみ（解除）。
    use studio_core::TransitionKind;
    let (mut p, _track_id, clip_id) = project_with_clip();
    let (_, clip) = p.clip(clip_id).unwrap();
    assert_eq!(clip.transition_in_kind, TransitionKind::None);
    assert_eq!(clip.transition_in_ms, 0);

    apply(
        &mut p,
        &Command::SetClipTransition {
            clip_id,
            kind: TransitionKind::CrossDissolve,
            duration_ms: 500,
        },
    )
    .unwrap();
    let json = p.dump_json();
    assert!(
        json.contains("\"transition_in_kind\": \"cross_dissolve\""),
        "{json}"
    );
    let loaded: Project = serde_json::from_str(&json).unwrap();
    let (_, clip) = loaded.clip(clip_id).unwrap();
    assert_eq!(clip.transition_in_kind, TransitionKind::CrossDissolve);
    assert_eq!(clip.transition_in_ms, 500);

    // 旧形式（フィールドなし）は none / 0 で読める
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let obj = value["tracks"][0]["clips"][0].as_object_mut().unwrap();
    obj.remove("transition_in_kind");
    obj.remove("transition_in_ms");
    let legacy = serde_json::to_string(&value).unwrap();
    let loaded_legacy: Project = serde_json::from_str(&legacy).unwrap();
    let (_, clip) = loaded_legacy.clip(clip_id).unwrap();
    assert_eq!(clip.transition_in_kind, TransitionKind::None);
    assert_eq!(clip.transition_in_ms, 0);

    // 検証: 0 以下・クリップ長超過はエラーで状態不変
    let before = p.dump_json();
    for bad in [0, -100, 5_001] {
        assert!(
            apply(
                &mut p,
                &Command::SetClipTransition {
                    clip_id,
                    kind: TransitionKind::CrossDissolve,
                    duration_ms: bad,
                },
            )
            .is_err(),
            "duration {bad} must be rejected"
        );
    }
    // none に長さを持たせることはできない（解除は 0 のみ）
    assert!(
        apply(
            &mut p,
            &Command::SetClipTransition {
                clip_id,
                kind: TransitionKind::None,
                duration_ms: 500,
            },
        )
        .is_err()
    );
    assert_eq!(p.dump_json(), before, "rejected calls keep state");

    // 解除（none + 0）で属性が消える
    apply(
        &mut p,
        &Command::SetClipTransition {
            clip_id,
            kind: TransitionKind::None,
            duration_ms: 0,
        },
    )
    .unwrap();
    let (_, clip) = p.clip(clip_id).unwrap();
    assert_eq!(clip.transition_in_kind, TransitionKind::None);
    assert_eq!(clip.transition_in_ms, 0);

    // 存在しないクリップはエラーで状態不変
    let before = p.dump_json();
    assert!(
        apply(
            &mut p,
            &Command::SetClipTransition {
                clip_id: 9999,
                kind: TransitionKind::CrossDissolve,
                duration_ms: 500,
            },
        )
        .is_err()
    );
    assert_eq!(p.dump_json(), before);
}

#[test]
fn split_clip_clears_right_clip_transition_in() {
    // Issue #49（ADR-0014）: トランジションは境界の属性。分割で
    // 新しく生じた境界は素の切り替わりにするため、右クリップの
    // transition_in は引き継がない（フェードの外側保持と同じ）。
    use studio_core::TransitionKind;
    let (mut p, _track_id, clip_id) = project_with_clip();
    apply(
        &mut p,
        &Command::SetClipTransition {
            clip_id,
            kind: TransitionKind::CrossDissolve,
            duration_ms: 500,
        },
    )
    .unwrap();
    apply(
        &mut p,
        &Command::SplitClip {
            clip_id,
            at_ms: 2_000,
        },
    )
    .unwrap();
    let clips = &p.tracks[0].clips;
    assert_eq!(clips.len(), 2);
    let (left, right) = (&clips[0], &clips[1]);
    // 左（元の先頭側）は前の境界の属性を保持する
    assert_eq!(left.transition_in_kind, TransitionKind::CrossDissolve);
    assert_eq!(left.transition_in_ms, 500);
    // 右の先頭は新しい境界なので属性なし
    assert_eq!(right.transition_in_kind, TransitionKind::None);
    assert_eq!(right.transition_in_ms, 0);
}
