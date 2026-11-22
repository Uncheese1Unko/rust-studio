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
    assert!(apply(
        &mut p,
        &Command::SetClipTransition {
            clip_id,
            kind: TransitionKind::None,
            duration_ms: 500,
        },
    )
    .is_err());
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
    assert!(apply(
        &mut p,
        &Command::SetClipTransition {
            clip_id: 9999,
            kind: TransitionKind::CrossDissolve,
            duration_ms: 500,
        },
    )
    .is_err());
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

#[test]
fn trim_clip_clamps_transition_in_to_new_duration() {
    // Issue #49 レビュー指摘: SetClipTransition の不変条件
    // （0 < transition_in_ms <= クリップ長）は、トリムで短くした
    // 後も保たなければならない（フェードと同じクランプ）。
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
        &Command::TrimClip {
            clip_id,
            new_start_ms: 0,
            new_duration_ms: 300,
        },
    )
    .unwrap();
    let (_, clip) = p.clip(clip_id).unwrap();
    assert_eq!(clip.duration_ms, 300);
    assert_eq!(clip.transition_in_ms, 300, "clamped to the new duration");
    assert_eq!(clip.transition_in_kind, TransitionKind::CrossDissolve);
}

#[test]
fn split_clip_clamps_left_transition_in() {
    // Issue #49 レビュー指摘: 分割で左クリップが短くなった場合も
    // transition_in_ms を左の長さへクランプする。
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
            at_ms: 200,
        },
    )
    .unwrap();
    let left = &p.tracks[0].clips[0];
    assert_eq!(left.duration_ms, 200);
    assert_eq!(left.transition_in_ms, 200, "clamped to the left duration");
}

#[test]
fn set_clip_offset_applies_and_validates() {
    // Issue #62: SetClipOffset は素材内開始位置（offset_ms）を設定する。
    // 負の値は InvalidRange で状態が変わらない。未知のクリップは
    // ClipNotFound。
    let (mut p, _track_id, clip_id) = project_with_clip();
    apply(
        &mut p,
        &Command::SetClipOffset {
            clip_id,
            offset_ms: 1_500,
        },
    )
    .unwrap();
    let (_, clip) = p.clip(clip_id).unwrap();
    assert_eq!(clip.offset_ms, 1_500);
    // 0 に戻すこともできる（素材の先頭から再生）。
    apply(
        &mut p,
        &Command::SetClipOffset {
            clip_id,
            offset_ms: 0,
        },
    )
    .unwrap();
    let (_, clip) = p.clip(clip_id).unwrap();
    assert_eq!(clip.offset_ms, 0);

    let before = p.dump_json();
    let err = apply(
        &mut p,
        &Command::SetClipOffset {
            clip_id,
            offset_ms: -1,
        },
    );
    assert!(
        matches!(err, Err(CommandError::InvalidRange(_))),
        "negative offset: {err:?}"
    );
    assert_eq!(p.dump_json(), before, "state must not change on failure");
    let err = apply(
        &mut p,
        &Command::SetClipOffset {
            clip_id: 999,
            offset_ms: 100,
        },
    );
    assert_eq!(err, Err(CommandError::ClipNotFound(999)));
    assert_eq!(p.dump_json(), before);
}

#[test]
fn set_clip_offset_command_round_trips_through_json() {
    // Issue #62: コマンドは JSON 経路（API / MCP）でも往復できる。
    // MCP のツール名は type タグ "set_clip_offset"。
    let cmd = Command::SetClipOffset {
        clip_id: 7,
        offset_ms: 1_234,
    };
    let json = serde_json::to_string(&cmd).unwrap();
    assert!(json.contains("\"set_clip_offset\""), "{json}");
    let back: Command = serde_json::from_str(&json).unwrap();
    assert_eq!(cmd, back);
}

// ---- Issue #68 / ADR-0016: ノートと楽器のモデルとコマンド ----

use studio_core::{Instrument, Note};

fn project_with_note_clip() -> (Project, u64, u64) {
    let mut p = Project::new("notes");
    let track_id = apply(
        &mut p,
        &Command::AddTrack {
            name: "A1".into(),
            kind: TrackKind::Audio,
        },
    )
    .unwrap()
    .unwrap();
    let clip_id = apply(
        &mut p,
        &Command::AddClip {
            track_id,
            name: "melody".into(),
            start_ms: 0,
            duration_ms: 4_000,
            source: String::new(),
        },
    )
    .unwrap()
    .unwrap();
    (p, track_id, clip_id)
}

fn add_note(p: &mut Project, clip_id: u64, pitch: u8, start_ms: i64, duration_ms: i64) {
    apply(
        p,
        &Command::AddNote {
            clip_id,
            pitch,
            start_ms,
            duration_ms,
            velocity: 100,
        },
    )
    .unwrap();
}

#[test]
fn set_track_instrument_applies_and_clears() {
    // Issue #68 / ADR-0016: トラックへサンプラー楽器を設定でき、
    // None で解除できる。状態ダンプでも観測できる。
    let (mut p, track_id, _) = project_with_note_clip();
    apply(
        &mut p,
        &Command::SetTrackInstrument {
            track_id,
            instrument: Some(Instrument::Sampler {
                source: "media/piano.wav".into(),
                root_note: 60,
            }),
        },
    )
    .unwrap();
    assert_eq!(
        p.track(track_id).unwrap().instrument,
        Some(Instrument::Sampler {
            source: "media/piano.wav".into(),
            root_note: 60,
        })
    );
    assert!(
        p.dump_json().contains("\"sampler\""),
        "dump must expose the instrument: {}",
        p.dump_json()
    );
    apply(
        &mut p,
        &Command::SetTrackInstrument {
            track_id,
            instrument: None,
        },
    )
    .unwrap();
    assert_eq!(p.track(track_id).unwrap().instrument, None);
}

#[test]
fn set_track_instrument_unknown_track_errors_and_keeps_state() {
    let (mut p, _, _) = project_with_note_clip();
    let before = p.dump_json();
    let err = apply(
        &mut p,
        &Command::SetTrackInstrument {
            track_id: 999,
            instrument: Some(Instrument::Sampler {
                source: "x.wav".into(),
                root_note: 60,
            }),
        },
    );
    assert_eq!(err, Err(CommandError::TrackNotFound(999)));
    assert_eq!(p.dump_json(), before);
}

#[test]
fn note_commands_round_trip_through_json() {
    // Issue #68: コマンドは JSON 経路（API / MCP）でも往復できる。
    let cmd = Command::SetTrackInstrument {
        track_id: 3,
        instrument: Some(Instrument::Sampler {
            source: "piano.wav".into(),
            root_note: 60,
        }),
    };
    let json = serde_json::to_string(&cmd).unwrap();
    assert!(json.contains("\"set_track_instrument\""), "{json}");
    let back: Command = serde_json::from_str(&json).unwrap();
    assert_eq!(cmd, back);

    let cmd = Command::AddNote {
        clip_id: 7,
        pitch: 62,
        start_ms: 250,
        duration_ms: 500,
        velocity: 90,
    };
    let json = serde_json::to_string(&cmd).unwrap();
    assert!(json.contains("\"add_note\""), "{json}");
    let back: Command = serde_json::from_str(&json).unwrap();
    assert_eq!(cmd, back);
}

#[test]
fn add_note_appends_notes_to_clip() {
    let (mut p, _, clip_id) = project_with_note_clip();
    add_note(&mut p, clip_id, 60, 0, 500);
    add_note(&mut p, clip_id, 64, 500, 500);
    let (_, clip) = p.clip(clip_id).unwrap();
    assert_eq!(
        clip.notes,
        vec![
            Note {
                pitch: 60,
                start_ms: 0,
                duration_ms: 500,
                velocity: 100,
            },
            Note {
                pitch: 64,
                start_ms: 500,
                duration_ms: 500,
                velocity: 100,
            },
        ]
    );
}

#[test]
fn add_note_rejects_invalid_values_and_keeps_state() {
    // Issue #68 / ADR-0016: ノートはクリップ範囲内に収まることが
    // 不変条件。範囲外の値はエラーで、状態は変わらない。
    let (mut p, _, clip_id) = project_with_note_clip();
    let before = p.dump_json();
    for cmd in [
        Command::AddNote {
            clip_id,
            pitch: 128,
            start_ms: 0,
            duration_ms: 100,
            velocity: 100,
        },
        Command::AddNote {
            clip_id,
            pitch: 60,
            start_ms: 0,
            duration_ms: 100,
            velocity: 128,
        },
        Command::AddNote {
            clip_id,
            pitch: 60,
            start_ms: -1,
            duration_ms: 100,
            velocity: 100,
        },
        Command::AddNote {
            clip_id,
            pitch: 60,
            start_ms: 0,
            duration_ms: 0,
            velocity: 100,
        },
        Command::AddNote {
            clip_id,
            pitch: 60,
            start_ms: 3_900,
            duration_ms: 200,
            velocity: 100,
        },
    ] {
        assert!(
            matches!(apply(&mut p, &cmd), Err(CommandError::InvalidRange(_))),
            "must reject: {cmd:?}"
        );
    }
    assert_eq!(p.dump_json(), before);
    let err = apply(
        &mut p,
        &Command::AddNote {
            clip_id: 999,
            pitch: 60,
            start_ms: 0,
            duration_ms: 100,
            velocity: 100,
        },
    );
    assert_eq!(err, Err(CommandError::ClipNotFound(999)));
    assert_eq!(p.dump_json(), before);
}

#[test]
fn split_clip_cuts_note_crossing_the_boundary() {
    // Issue #68 / ADR-0016: 分割点でノートを振り分ける。境界を跨ぐ
    // ノートは分割点で切れ、分割前後で鳴るメロディーが変わらない。
    let (mut p, _, clip_id) = project_with_note_clip();
    add_note(&mut p, clip_id, 60, 0, 1_000);
    add_note(&mut p, clip_id, 62, 1_500, 1_000); // 2000 を跨ぐ
    add_note(&mut p, clip_id, 64, 3_000, 500);
    let right_id = apply(
        &mut p,
        &Command::SplitClip {
            clip_id,
            at_ms: 2_000,
        },
    )
    .unwrap()
    .unwrap();
    let (_, left) = p.clip(clip_id).unwrap();
    assert_eq!(
        left.notes,
        vec![
            Note {
                pitch: 60,
                start_ms: 0,
                duration_ms: 1_000,
                velocity: 100,
            },
            Note {
                pitch: 62,
                start_ms: 1_500,
                duration_ms: 500,
                velocity: 100,
            },
        ]
    );
    let (_, right) = p.clip(right_id).unwrap();
    assert_eq!(
        right.notes,
        vec![
            Note {
                pitch: 62,
                start_ms: 0,
                duration_ms: 500,
                velocity: 100,
            },
            Note {
                pitch: 64,
                start_ms: 1_000,
                duration_ms: 500,
                velocity: 100,
            },
        ]
    );
}

#[test]
fn trim_clip_drops_out_of_range_notes_and_clamps() {
    // Issue #68 / ADR-0016: トリムで新しい長さの範囲外に出たノートは
    // 削除し、終端がはみ出すノートは収まるようクランプする
    // （フェード／トランジションのクランプ規則と揃える）。
    let (mut p, _, clip_id) = project_with_note_clip();
    add_note(&mut p, clip_id, 60, 0, 1_000);
    add_note(&mut p, clip_id, 62, 3_000, 1_000);
    add_note(&mut p, clip_id, 64, 3_500, 500);
    apply(
        &mut p,
        &Command::TrimClip {
            clip_id,
            new_start_ms: 0,
            new_duration_ms: 3_200,
        },
    )
    .unwrap();
    let (_, clip) = p.clip(clip_id).unwrap();
    assert_eq!(
        clip.notes,
        vec![
            Note {
                pitch: 60,
                start_ms: 0,
                duration_ms: 1_000,
                velocity: 100,
            },
            Note {
                pitch: 62,
                start_ms: 3_000,
                duration_ms: 200,
                velocity: 100,
            },
        ]
    );
}

#[test]
fn legacy_json_without_notes_and_instrument_loads_with_defaults() {
    // Issue #68 / ADR-0016: notes / instrument の無い旧形式 JSON は
    // 既定値（空ノート列・楽器なし）で読める。
    let json = r#"{
        "name": "legacy",
        "tracks": [
            {
                "id": 1,
                "name": "A1",
                "kind": "audio",
                "clips": [
                    {"id": 2, "name": "c", "start_ms": 0, "duration_ms": 1000,
                     "source": "a.wav", "gain": 1.0}
                ]
            }
        ],
        "next_id": 3
    }"#;
    let path = std::env::temp_dir().join(format!(
        "studio-core-legacy-notes-{}.json",
        std::process::id()
    ));
    std::fs::write(&path, json).unwrap();
    let p = Project::load_from_path(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(p.tracks[0].clips[0].notes.is_empty());
    assert_eq!(p.tracks[0].instrument, None);
}

#[test]
fn add_note_rejects_overflowing_range_and_keeps_state() {
    // レビュー Major-1（Issue #68）: start_ms + duration_ms の加算が
    // オーバーフローすると検証を素通り／panic する。巨大値でも
    // InvalidRange で状態不変でなければならない。
    let (mut project, _track, clip) = project_with_note_clip();
    let before = project.dump_json();
    let err = apply(
        &mut project,
        &Command::AddNote {
            clip_id: clip,
            pitch: 60,
            start_ms: i64::MAX - 10,
            duration_ms: 100,
            velocity: 100,
        },
    )
    .unwrap_err();
    assert_eq!(
        err,
        CommandError::InvalidRange("note must fit within the clip")
    );
    assert_eq!(project.dump_json(), before);
}

#[test]
fn set_track_instrument_rejects_out_of_range_root_note() {
    // レビュー Major-2（Issue #68）: root_note は MIDI ノート番号
    // （0–127）。範囲外は InvalidRange で状態不変。
    let (mut project, track, _clip) = project_with_note_clip();
    let before = project.dump_json();
    let err = apply(
        &mut project,
        &Command::SetTrackInstrument {
            track_id: track,
            instrument: Some(Instrument::Sampler {
                source: "x.wav".into(),
                root_note: 200,
            }),
        },
    )
    .unwrap_err();
    assert_eq!(
        err,
        CommandError::InvalidRange("root_note must be within 0..=127")
    );
    assert_eq!(project.dump_json(), before);
}

#[test]
fn move_note_changes_pitch_and_start_keeping_duration() {
    // Issue #69 / ADR-0016: MoveNote は音高と開始だけを変え、
    // 長さとベロシティは保つ。
    let (mut p, _, clip_id) = project_with_note_clip();
    add_note(&mut p, clip_id, 60, 0, 500);
    apply(
        &mut p,
        &Command::MoveNote {
            clip_id,
            note_index: 0,
            new_pitch: 65,
            new_start_ms: 300,
        },
    )
    .unwrap();
    let (_, clip) = p.clip(clip_id).unwrap();
    assert_eq!(
        clip.notes,
        vec![Note {
            pitch: 65,
            start_ms: 300,
            duration_ms: 500,
            velocity: 100,
        }]
    );
}

#[test]
fn move_note_rejects_invalid_values_and_keeps_state() {
    // Issue #69 / ADR-0016: 移動後もノートはクリップ範囲内に収まる
    // ことが不変条件。インデックス範囲外・未知のクリップもエラーで
    // 状態不変。巨大な開始位置でもオーバーフローしない。
    let (mut p, _, clip_id) = project_with_note_clip();
    add_note(&mut p, clip_id, 60, 0, 500);
    let before = p.dump_json();
    for cmd in [
        Command::MoveNote {
            clip_id,
            note_index: 0,
            new_pitch: 128,
            new_start_ms: 0,
        },
        Command::MoveNote {
            clip_id,
            note_index: 0,
            new_pitch: 60,
            new_start_ms: -1,
        },
        Command::MoveNote {
            clip_id,
            note_index: 0,
            new_pitch: 60,
            new_start_ms: 3_800, // 3800 + 500 > 4000（クリップ長）
        },
        Command::MoveNote {
            clip_id,
            note_index: 0,
            new_pitch: 60,
            new_start_ms: i64::MAX,
        },
        Command::MoveNote {
            clip_id,
            note_index: 9,
            new_pitch: 60,
            new_start_ms: 0,
        },
    ] {
        assert!(apply(&mut p, &cmd).is_err(), "must reject: {cmd:?}");
        assert_eq!(p.dump_json(), before, "state must not change: {cmd:?}");
    }
    let err = apply(
        &mut p,
        &Command::MoveNote {
            clip_id: 999,
            note_index: 0,
            new_pitch: 60,
            new_start_ms: 0,
        },
    )
    .unwrap_err();
    assert_eq!(err, CommandError::ClipNotFound(999));
}

#[test]
fn remove_note_deletes_by_index() {
    // Issue #69 / ADR-0016: RemoveNote はインデックスで1音を消す。
    let (mut p, _, clip_id) = project_with_note_clip();
    add_note(&mut p, clip_id, 60, 0, 500);
    add_note(&mut p, clip_id, 64, 500, 500);
    apply(
        &mut p,
        &Command::RemoveNote {
            clip_id,
            note_index: 0,
        },
    )
    .unwrap();
    let (_, clip) = p.clip(clip_id).unwrap();
    assert_eq!(
        clip.notes,
        vec![Note {
            pitch: 64,
            start_ms: 500,
            duration_ms: 500,
            velocity: 100,
        }]
    );
}

#[test]
fn remove_note_rejects_bad_index_and_unknown_clip() {
    let (mut p, _, clip_id) = project_with_note_clip();
    add_note(&mut p, clip_id, 60, 0, 500);
    let before = p.dump_json();
    assert!(apply(
        &mut p,
        &Command::RemoveNote {
            clip_id,
            note_index: 1,
        }
    )
    .is_err());
    assert_eq!(
        apply(
            &mut p,
            &Command::RemoveNote {
                clip_id: 999,
                note_index: 0,
            }
        )
        .unwrap_err(),
        CommandError::ClipNotFound(999)
    );
    assert_eq!(p.dump_json(), before);
}

#[test]
fn move_and_remove_note_round_trip_through_json() {
    // Issue #69: コマンドの JSON 往復（タグは move_note / remove_note）。
    for cmd in [
        Command::MoveNote {
            clip_id: 7,
            note_index: 2,
            new_pitch: 65,
            new_start_ms: 300,
        },
        Command::RemoveNote {
            clip_id: 7,
            note_index: 2,
        },
    ] {
        let json = serde_json::to_string(&cmd).unwrap();
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd, "round trip failed for {json}");
    }
    let json = serde_json::to_string(&Command::MoveNote {
        clip_id: 7,
        note_index: 2,
        new_pitch: 65,
        new_start_ms: 300,
    })
    .unwrap();
    assert!(json.contains("\"move_note\""), "{json}");
}

// ---- Issue #72 / ADR-0017: Plugin 楽器（CLAP ヘッドレス連携） ----

use studio_core::PluginFormat;

fn plugin_instrument() -> Instrument {
    Instrument::Plugin {
        format: PluginFormat::Clap,
        plugin_path: "/plugins/Vital.clap".into(),
        preset_path: Some("/presets/lead.vital".into()),
    }
}

#[test]
fn set_track_instrument_plugin_applies_and_shows_in_dump() {
    // Issue #72: Plugin 楽器を設定でき、状態ダンプで観測できる。
    let (mut p, track_id, _clip_id) = project_with_note_clip();
    apply(
        &mut p,
        &Command::SetTrackInstrument {
            track_id,
            instrument: Some(plugin_instrument()),
        },
    )
    .unwrap();
    assert_eq!(
        p.track(track_id).unwrap().instrument,
        Some(plugin_instrument())
    );
    let dump = p.dump_json();
    assert!(
        dump.contains("\"plugin\""),
        "dump must expose plugin: {dump}"
    );
    assert!(dump.contains("/plugins/Vital.clap"), "{dump}");
    assert!(dump.contains("/presets/lead.vital"), "{dump}");
    assert!(dump.contains("\"clap\""), "{dump}");
}

#[test]
fn set_track_instrument_plugin_rejects_empty_plugin_path() {
    // プラグインを指せない空パスは InvalidRange で状態不変。
    let (mut p, track_id, _clip_id) = project_with_note_clip();
    let before = p.dump_json();
    let err = apply(
        &mut p,
        &Command::SetTrackInstrument {
            track_id,
            instrument: Some(Instrument::Plugin {
                format: PluginFormat::Clap,
                plugin_path: "  ".into(),
                preset_path: None,
            }),
        },
    )
    .unwrap_err();
    assert_eq!(
        err,
        CommandError::InvalidRange("plugin_path must not be empty")
    );
    assert_eq!(p.dump_json(), before);
}

#[test]
fn set_track_instrument_plugin_rejects_empty_preset_path() {
    // プリセットパスは None（既定状態）か非空のパスのどちらか。
    // 空白のみも plugin_path と同じく弾く（Issue #72 レビュー Minor）。
    let (mut p, track_id, _clip_id) = project_with_note_clip();
    let before = p.dump_json();
    for preset in ["", "   "] {
        let err = apply(
            &mut p,
            &Command::SetTrackInstrument {
                track_id,
                instrument: Some(Instrument::Plugin {
                    format: PluginFormat::Clap,
                    plugin_path: "/plugins/Vital.clap".into(),
                    preset_path: Some(preset.into()),
                }),
            },
        )
        .unwrap_err();
        assert_eq!(
            err,
            CommandError::InvalidRange("preset_path must not be empty")
        );
    }
    assert_eq!(p.dump_json(), before);
}

#[test]
fn plugin_instrument_survives_project_json_roundtrip() {
    // Issue #72: プリセット指定を含む楽器設定がプロジェクトの
    // 保存→再読込で往復する。
    let (mut p, track_id, _clip_id) = project_with_note_clip();
    apply(
        &mut p,
        &Command::SetTrackInstrument {
            track_id,
            instrument: Some(plugin_instrument()),
        },
    )
    .unwrap();
    let json = p.dump_json();
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(back, p);
    assert_eq!(
        back.track(track_id).unwrap().instrument,
        Some(plugin_instrument())
    );
}

#[test]
fn plugin_command_json_roundtrip_uses_plugin_tag() {
    let cmd = Command::SetTrackInstrument {
        track_id: 3,
        instrument: Some(plugin_instrument()),
    };
    let json = serde_json::to_string(&cmd).unwrap();
    assert!(json.contains("\"plugin\""), "{json}");
    let back: Command = serde_json::from_str(&json).unwrap();
    assert_eq!(cmd, back);
}

// ---------- Issue #73: 音色（Tone）の管理 ----------

fn sampler_tone(root_note: u8) -> Instrument {
    Instrument::Sampler {
        source: "tone.wav".into(),
        root_note,
    }
}

#[test]
fn add_tone_applies_and_shows_in_dump() {
    // 音色を作成すると Project.tones に入り、ダンプで観測できる。
    let mut p = Project::new("tones");
    let tone_id = apply(
        &mut p,
        &Command::AddTone {
            name: "Warm Pad".into(),
            instrument: sampler_tone(60),
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(p.tones.len(), 1);
    assert_eq!(p.tones[0].id, tone_id);
    assert_eq!(p.tones[0].name, "Warm Pad");
    assert_eq!(p.tones[0].instrument, sampler_tone(60));
    let dump = p.dump_json();
    assert!(dump.contains("\"Warm Pad\""), "{dump}");
    assert!(dump.contains("\"tones\""), "{dump}");
}

#[test]
fn add_tone_rejects_blank_name_and_invalid_instrument() {
    // 名前が空白のみ、または楽器の不変条件違反はエラーで状態不変。
    let mut p = Project::new("tones");
    let before = p.dump_json();
    let err = apply(
        &mut p,
        &Command::AddTone {
            name: "   ".into(),
            instrument: sampler_tone(60),
        },
    )
    .unwrap_err();
    assert_eq!(
        err,
        CommandError::InvalidRange("tone name must not be empty")
    );
    let err = apply(
        &mut p,
        &Command::AddTone {
            name: "Bad".into(),
            instrument: sampler_tone(200),
        },
    )
    .unwrap_err();
    assert_eq!(
        err,
        CommandError::InvalidRange("root_note must be within 0..=127")
    );
    assert_eq!(p.dump_json(), before);
}

#[test]
fn rename_and_update_tone_change_stored_values() {
    let mut p = Project::new("tones");
    let tone_id = apply(
        &mut p,
        &Command::AddTone {
            name: "A".into(),
            instrument: sampler_tone(60),
        },
    )
    .unwrap()
    .unwrap();
    apply(
        &mut p,
        &Command::RenameTone {
            tone_id,
            name: "B".into(),
        },
    )
    .unwrap();
    apply(
        &mut p,
        &Command::UpdateTone {
            tone_id,
            instrument: sampler_tone(48),
        },
    )
    .unwrap();
    assert_eq!(p.tones[0].name, "B");
    assert_eq!(p.tones[0].instrument, sampler_tone(48));
    // 存在しない音色はエラーで状態不変。
    let before = p.dump_json();
    let err = apply(
        &mut p,
        &Command::RenameTone {
            tone_id: 999,
            name: "X".into(),
        },
    )
    .unwrap_err();
    assert_eq!(err, CommandError::ToneNotFound(999));
    assert_eq!(p.dump_json(), before);
}

#[test]
fn set_track_tone_assigns_switches_and_clears() {
    let (mut p, track_id, _clip_id) = project_with_note_clip();
    let t1 = apply(
        &mut p,
        &Command::AddTone {
            name: "T1".into(),
            instrument: sampler_tone(60),
        },
    )
    .unwrap()
    .unwrap();
    let t2 = apply(
        &mut p,
        &Command::AddTone {
            name: "T2".into(),
            instrument: sampler_tone(48),
        },
    )
    .unwrap()
    .unwrap();
    apply(
        &mut p,
        &Command::SetTrackTone {
            track_id,
            tone_id: Some(t1),
        },
    )
    .unwrap();
    assert_eq!(p.track(track_id).unwrap().tone_id, Some(t1));
    // 切り替え。
    apply(
        &mut p,
        &Command::SetTrackTone {
            track_id,
            tone_id: Some(t2),
        },
    )
    .unwrap();
    assert_eq!(p.track(track_id).unwrap().tone_id, Some(t2));
    // 解除。
    apply(
        &mut p,
        &Command::SetTrackTone {
            track_id,
            tone_id: None,
        },
    )
    .unwrap();
    assert_eq!(p.track(track_id).unwrap().tone_id, None);
    // 存在しない音色の割り当てはエラーで状態不変。
    let before = p.dump_json();
    let err = apply(
        &mut p,
        &Command::SetTrackTone {
            track_id,
            tone_id: Some(999),
        },
    )
    .unwrap_err();
    assert_eq!(err, CommandError::ToneNotFound(999));
    assert_eq!(p.dump_json(), before);
}

#[test]
fn remove_tone_clears_track_references() {
    let (mut p, track_id, _clip_id) = project_with_note_clip();
    let tone_id = apply(
        &mut p,
        &Command::AddTone {
            name: "T".into(),
            instrument: sampler_tone(60),
        },
    )
    .unwrap()
    .unwrap();
    apply(
        &mut p,
        &Command::SetTrackTone {
            track_id,
            tone_id: Some(tone_id),
        },
    )
    .unwrap();
    apply(&mut p, &Command::RemoveTone { tone_id }).unwrap();
    assert!(p.tones.is_empty());
    assert_eq!(p.track(track_id).unwrap().tone_id, None);
    let err = apply(&mut p, &Command::RemoveTone { tone_id }).unwrap_err();
    assert_eq!(err, CommandError::ToneNotFound(tone_id));
}

#[test]
fn set_track_instrument_clears_tone_reference() {
    // 直接の楽器設定が音色参照より優先される（二重管理の回避）。
    let (mut p, track_id, _clip_id) = project_with_note_clip();
    let tone_id = apply(
        &mut p,
        &Command::AddTone {
            name: "T".into(),
            instrument: sampler_tone(60),
        },
    )
    .unwrap()
    .unwrap();
    apply(
        &mut p,
        &Command::SetTrackTone {
            track_id,
            tone_id: Some(tone_id),
        },
    )
    .unwrap();
    apply(
        &mut p,
        &Command::SetTrackInstrument {
            track_id,
            instrument: Some(sampler_tone(72)),
        },
    )
    .unwrap();
    assert_eq!(p.track(track_id).unwrap().tone_id, None);
    assert_eq!(
        p.track(track_id).unwrap().instrument,
        Some(sampler_tone(72))
    );
}

#[test]
fn effective_instrument_prefers_tone_and_falls_back() {
    let (mut p, track_id, _clip_id) = project_with_note_clip();
    apply(
        &mut p,
        &Command::SetTrackInstrument {
            track_id,
            instrument: Some(sampler_tone(72)),
        },
    )
    .unwrap();
    let tone_id = apply(
        &mut p,
        &Command::AddTone {
            name: "T".into(),
            instrument: sampler_tone(48),
        },
    )
    .unwrap()
    .unwrap();
    // 未割り当てでは直接の楽器。
    let track = p.track(track_id).unwrap().clone();
    assert_eq!(p.effective_instrument(&track), Some(sampler_tone(72)));
    // 割り当てると音色が優先される。
    apply(
        &mut p,
        &Command::SetTrackTone {
            track_id,
            tone_id: Some(tone_id),
        },
    )
    .unwrap();
    let track = p.track(track_id).unwrap().clone();
    assert_eq!(p.effective_instrument(&track), Some(sampler_tone(48)));
}

#[test]
fn tones_and_assignment_survive_project_json_roundtrip() {
    let (mut p, track_id, _clip_id) = project_with_note_clip();
    let tone_id = apply(
        &mut p,
        &Command::AddTone {
            name: "Vital Lead".into(),
            instrument: plugin_instrument(),
        },
    )
    .unwrap()
    .unwrap();
    apply(
        &mut p,
        &Command::SetTrackTone {
            track_id,
            tone_id: Some(tone_id),
        },
    )
    .unwrap();
    let json = p.dump_json();
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(back, p);
    assert_eq!(back.tones.len(), 1);
    assert_eq!(back.track(track_id).unwrap().tone_id, Some(tone_id));
}
