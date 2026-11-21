//! daw: Reaper 風の DAW。
//!
//! Issue #13 で音声再生エンジン（`audio` モジュール）を実装した。
//! このバイナリはデモプロジェクトをコマンド経由で組み立て、デモ音源を
//! デコードし、ミックスを WAV に書き出し、再生状態機械の遷移を表示する。
//! GUI は #14、実デバイスへの音声出力は実機確認のうえ別途行う
//! （本環境に音声デバイスはなく、実音は未検証）。
//! 状態の変更は studio-core のコマンド層経由でのみ行う（ADR-0004）。

mod audio;

use std::collections::HashMap;

use studio_core::{apply, Command, Project, TrackKind};

const RATE: u32 = 44100;

fn write_tone(path: &std::path::Path, freq: f32, ms: i64) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    let frames = RATE as i64 * ms / 1000;
    for i in 0..frames {
        let s = 0.4 * (2.0 * std::f32::consts::PI * freq * i as f32 / RATE as f32).sin();
        writer.write_sample((s * i16::MAX as f32) as i16).unwrap();
    }
    writer.finalize().unwrap();
}

fn demo_project(dir: &std::path::Path) -> Project {
    let mut project = Project::new("daw demo session");
    let track = apply(
        &mut project,
        &Command::AddTrack {
            name: "A1".into(),
            kind: TrackKind::Audio,
        },
    )
    .unwrap()
    .unwrap();
    let bass = apply(
        &mut project,
        &Command::AddClip {
            track_id: track,
            name: "tone-220".into(),
            start_ms: 0,
            duration_ms: 2000,
            source: dir.join("tone220.wav").to_string_lossy().into_owned(),
        },
    )
    .unwrap()
    .unwrap();
    apply(
        &mut project,
        &Command::AddClip {
            track_id: track,
            name: "tone-440".into(),
            start_ms: 1000,
            duration_ms: 2000,
            source: dir.join("tone440.wav").to_string_lossy().into_owned(),
        },
    )
    .unwrap()
    .unwrap();
    apply(
        &mut project,
        &Command::SetClipGain {
            clip_id: bass,
            gain: 0.5,
        },
    )
    .unwrap();
    project
}

fn main() {
    let dir = std::env::temp_dir().join("rust-studio-daw-media");
    std::fs::create_dir_all(&dir).unwrap();
    write_tone(&dir.join("tone220.wav"), 220.0, 2500);
    write_tone(&dir.join("tone440.wav"), 440.0, 2500);

    let project = demo_project(&dir);
    println!("daw (issue #13: audio engine)");
    println!("{}", project.dump_json());

    // 素材をデコードし、ミックスを書き出す
    let mut audio_map = HashMap::new();
    for track in &project.tracks {
        for clip in &track.clips {
            let decoded = audio::decode_audio(std::path::Path::new(&clip.source))
                .expect("demo source must decode");
            println!(
                "decoded {}: {} Hz, {} ch, {} ms",
                clip.source,
                decoded.sample_rate,
                decoded.channels,
                decoded.duration_ms()
            );
            audio_map.entry(clip.source.clone()).or_insert(decoded);
        }
    }
    let end_ms = 3000;
    let mix = audio::render_mix(&project, &audio_map, 0, end_ms, RATE);
    let mix_path = dir.join("mixdown.wav");
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&mix_path, spec).unwrap();
    for s in &mix {
        writer.write_sample((s * i16::MAX as f32) as i16).unwrap();
    }
    writer.finalize().unwrap();
    println!("mixdown written: {}", mix_path.display());
    for sec in 0..3 {
        let a = (sec * RATE as usize) * 2;
        let b = ((sec + 1) * RATE as usize) * 2;
        let rms = (mix[a..b].iter().map(|s| s * s).sum::<f32>() / (b - a) as f32).sqrt();
        println!("mix rms {sec}s-{}s: {rms:.4}", sec + 1);
    }

    // 再生状態機械の遷移（実時間出力は実機未検証）
    let mut player = audio::Player::new(end_ms);
    let report = |p: &audio::Player| {
        println!("player: position={}ms playing={}", p.position_ms, p.playing);
    };
    player.play();
    for _ in 0..3 {
        player.advance(500);
        report(&player);
    }
    player.pause();
    player.advance(500);
    println!("player: paused, advance is ignored");
    report(&player);
    player.seek(0);
    player.play();
    for _ in 0..6 {
        player.advance(500);
        report(&player);
    }
}
