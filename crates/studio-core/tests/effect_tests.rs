//! トラックエフェクト（Issue #92、EPIC #91）のコマンド層テスト。
//!
//! モデル（TrackEffect / EffectSlot）とコマンド5種（追加・削除・
//! 並べ替え・差し替え・バイパス）の振る舞いを、状態ダンプと
//! 失敗時の状態不変で固定する。

use studio_core::{
    apply, Command, CommandError, EffectSlot, History, Project, TrackEffect, TrackKind,
};

fn project_with_track() -> (Project, u64) {
    let mut p = Project::new("fx");
    let track_id = apply(
        &mut p,
        &Command::AddTrack {
            name: "A1".into(),
            kind: TrackKind::Audio,
        },
    )
    .unwrap()
    .unwrap();
    (p, track_id)
}

fn eq3() -> TrackEffect {
    TrackEffect::Eq3 {
        low_gain_db: 3.0,
        mid_gain_db: -2.0,
        high_gain_db: 1.5,
    }
}

#[test]
fn add_track_effect_appends_and_appears_in_dump() {
    let (mut p, track_id) = project_with_track();
    let index = apply(
        &mut p,
        &Command::AddTrackEffect {
            track_id,
            effect: eq3(),
        },
    )
    .unwrap();
    assert_eq!(index, Some(0), "戻り値は追加されたスロットの位置");
    let track = p.track(track_id).unwrap();
    assert_eq!(
        track.effects,
        vec![EffectSlot {
            effect: eq3(),
            bypassed: false,
        }]
    );
    let dump = p.dump_json();
    assert!(dump.contains("\"effects\""), "dump: {dump}");
    assert!(dump.contains("\"eq3\""), "dump: {dump}");
    assert!(dump.contains("\"low_gain_db\": 3.0"), "dump: {dump}");
    // 2つ目は末尾に付く
    let index = apply(
        &mut p,
        &Command::AddTrackEffect {
            track_id,
            effect: TrackEffect::Delay {
                time_ms: 250,
                feedback: 0.3,
                mix: 0.3,
            },
        },
    )
    .unwrap();
    assert_eq!(index, Some(1));
    assert_eq!(p.track(track_id).unwrap().effects.len(), 2);
}

#[test]
fn effect_commands_validate_ranges_and_leave_state_unchanged() {
    let (mut p, track_id) = project_with_track();
    let before = p.dump_json();
    let invalid = [
        TrackEffect::Reverb {
            decay: 1.5,
            mix: 0.3,
        },
        TrackEffect::Reverb {
            decay: 0.5,
            mix: f32::NAN,
        },
        TrackEffect::Delay {
            time_ms: 0,
            feedback: 0.3,
            mix: 0.3,
        },
        TrackEffect::Delay {
            time_ms: 2001,
            feedback: 0.3,
            mix: 0.3,
        },
        TrackEffect::Delay {
            time_ms: 250,
            feedback: 0.95,
            mix: 0.3,
        },
        TrackEffect::Delay {
            time_ms: 250,
            feedback: 0.3,
            mix: -0.1,
        },
        TrackEffect::Eq3 {
            low_gain_db: 24.5,
            mid_gain_db: 0.0,
            high_gain_db: 0.0,
        },
        TrackEffect::Eq3 {
            low_gain_db: 0.0,
            mid_gain_db: f32::INFINITY,
            high_gain_db: 0.0,
        },
    ];
    for effect in invalid {
        let result = apply(&mut p, &Command::AddTrackEffect { track_id, effect });
        assert_eq!(
            result,
            Err(CommandError::InvalidRange("effect parameter out of range")),
            "effect: {effect:?}"
        );
        assert_eq!(p.dump_json(), before, "失敗時は状態が変わらない");
    }
    // 境界値は通る
    for effect in [
        TrackEffect::Reverb {
            decay: 0.0,
            mix: 1.0,
        },
        TrackEffect::Delay {
            time_ms: 1,
            feedback: 0.0,
            mix: 0.0,
        },
        TrackEffect::Delay {
            time_ms: 2000,
            feedback: 0.9,
            mix: 1.0,
        },
        TrackEffect::Eq3 {
            low_gain_db: -24.0,
            mid_gain_db: 24.0,
            high_gain_db: 0.0,
        },
    ] {
        assert!(
            apply(&mut p, &Command::AddTrackEffect { track_id, effect }).is_ok(),
            "effect: {effect:?}"
        );
    }
}

#[test]
fn effect_commands_reject_unknown_track_and_bad_index() {
    let (mut p, track_id) = project_with_track();
    assert_eq!(
        apply(
            &mut p,
            &Command::AddTrackEffect {
                track_id: 999,
                effect: eq3(),
            }
        ),
        Err(CommandError::TrackNotFound(999))
    );
    let before = p.dump_json();
    for command in [
        Command::RemoveTrackEffect { track_id, index: 0 },
        Command::MoveTrackEffect {
            track_id,
            from_index: 0,
            to_index: 0,
        },
        Command::SetTrackEffect {
            track_id,
            index: 0,
            effect: eq3(),
        },
        Command::SetTrackEffectBypass {
            track_id,
            index: 0,
            bypassed: true,
        },
    ] {
        assert!(
            apply(&mut p, &command).is_err(),
            "空チェーンへのインデックス操作は失敗する: {command:?}"
        );
        assert_eq!(p.dump_json(), before);
    }
}

#[test]
fn remove_and_move_track_effect_reorder_chain() {
    let (mut p, track_id) = project_with_track();
    for effect in [
        TrackEffect::Reverb {
            decay: 0.5,
            mix: 0.3,
        },
        TrackEffect::Delay {
            time_ms: 250,
            feedback: 0.3,
            mix: 0.3,
        },
        eq3(),
    ] {
        apply(&mut p, &Command::AddTrackEffect { track_id, effect }).unwrap();
    }
    // 先頭を末尾へ
    apply(
        &mut p,
        &Command::MoveTrackEffect {
            track_id,
            from_index: 0,
            to_index: 2,
        },
    )
    .unwrap();
    let kinds: Vec<String> = p
        .track(track_id)
        .unwrap()
        .effects
        .iter()
        .map(|s| format!("{:?}", s.effect))
        .collect();
    assert!(kinds[0].starts_with("Delay"), "{kinds:?}");
    assert!(kinds[1].starts_with("Eq3"), "{kinds:?}");
    assert!(kinds[2].starts_with("Reverb"), "{kinds:?}");
    // 同位置への移動は無害な成功
    apply(
        &mut p,
        &Command::MoveTrackEffect {
            track_id,
            from_index: 1,
            to_index: 1,
        },
    )
    .unwrap();
    assert_eq!(p.track(track_id).unwrap().effects.len(), 3);
    // 削除で詰まる
    apply(&mut p, &Command::RemoveTrackEffect { track_id, index: 0 }).unwrap();
    let effects = &p.track(track_id).unwrap().effects;
    assert_eq!(effects.len(), 2);
    assert!(format!("{:?}", effects[0].effect).starts_with("Eq3"));
}

#[test]
fn set_track_effect_replaces_params_and_bypass_toggles() {
    let (mut p, track_id) = project_with_track();
    apply(
        &mut p,
        &Command::AddTrackEffect {
            track_id,
            effect: TrackEffect::Delay {
                time_ms: 250,
                feedback: 0.3,
                mix: 0.3,
            },
        },
    )
    .unwrap();
    apply(
        &mut p,
        &Command::SetTrackEffectBypass {
            track_id,
            index: 0,
            bypassed: true,
        },
    )
    .unwrap();
    // 差し替えはパラメータだけを変え、バイパス状態は保つ
    apply(
        &mut p,
        &Command::SetTrackEffect {
            track_id,
            index: 0,
            effect: TrackEffect::Delay {
                time_ms: 400,
                feedback: 0.5,
                mix: 0.2,
            },
        },
    )
    .unwrap();
    let slot = &p.track(track_id).unwrap().effects[0];
    assert_eq!(
        slot.effect,
        TrackEffect::Delay {
            time_ms: 400,
            feedback: 0.5,
            mix: 0.2,
        }
    );
    assert!(slot.bypassed, "差し替えでバイパスは解除されない");
    // 種類ごとの差し替えもできる（スロットの差し替えとして扱う）
    apply(
        &mut p,
        &Command::SetTrackEffect {
            track_id,
            index: 0,
            effect: TrackEffect::Reverb {
                decay: 0.8,
                mix: 0.4,
            },
        },
    )
    .unwrap();
    assert!(matches!(
        p.track(track_id).unwrap().effects[0].effect,
        TrackEffect::Reverb { .. }
    ));
    // 差し替え時の範囲検証も効き、失敗時は元のまま
    let before = p.dump_json();
    assert!(apply(
        &mut p,
        &Command::SetTrackEffect {
            track_id,
            index: 0,
            effect: TrackEffect::Reverb {
                decay: 2.0,
                mix: 0.4,
            },
        },
    )
    .is_err());
    assert_eq!(p.dump_json(), before);
}

#[test]
fn effect_commands_support_undo_and_redo() {
    let (p, track_id) = project_with_track();
    let mut history = History::new(p.clone());
    history
        .apply(&Command::AddTrackEffect {
            track_id,
            effect: eq3(),
        })
        .unwrap();
    assert_eq!(history.project().track(track_id).unwrap().effects.len(), 1);
    assert!(history.undo());
    assert!(history
        .project()
        .track(track_id)
        .unwrap()
        .effects
        .is_empty());
    assert!(history.redo());
    assert_eq!(history.project().track(track_id).unwrap().effects.len(), 1);
    // p 側は履歴と無関係に空のまま（履歴は複製で始めた）
    assert!(p.track(track_id).unwrap().effects.is_empty());
}

#[test]
fn legacy_project_json_without_effects_loads_with_empty_chain() {
    // 旧形式: Track に effects フィールドが無い JSON。
    let json = r#"{
        "name": "legacy",
        "tracks": [
            {"id": 1, "name": "A1", "kind": "audio", "clips": []}
        ],
        "next_id": 2
    }"#;
    let p: Project = serde_json::from_str(json).unwrap();
    assert!(p.tracks[0].effects.is_empty());
    // 保存し直すと effects が空配列として現れる
    let dump = p.dump_json();
    assert!(dump.contains("\"effects\": []"), "dump: {dump}");
}

#[test]
fn effect_defaults_are_neutral_starting_points() {
    assert_eq!(
        TrackEffect::default_eq3(),
        TrackEffect::Eq3 {
            low_gain_db: 0.0,
            mid_gain_db: 0.0,
            high_gain_db: 0.0,
        }
    );
    assert_eq!(
        TrackEffect::default_delay(),
        TrackEffect::Delay {
            time_ms: 250,
            feedback: 0.3,
            mix: 0.3,
        }
    );
    assert_eq!(
        TrackEffect::default_reverb(),
        TrackEffect::Reverb {
            decay: 0.5,
            mix: 0.3,
        }
    );
}
