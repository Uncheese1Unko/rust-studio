//! プラグインヘルパー（Issue #72、ADR-0017）: CLAP プラグインを
//! ヘッドレスでホスティングする別プロセス。
//!
//! 本体（daw）とは疎結合で、プロトコルは次のとおり:
//! - 入力: stdin から JSON リクエストを1つ読む
//!   （plugin_path / sample_rate / duration_ms / state_json /
//!   notes[{start_ms, duration_ms, pitch, velocity}] / output_path）
//! - 処理: プラグインをロードし、state_json があれば CLAP の
//!   ステート拡張で注入し、ノート列をオフラインでレンダリングする
//! - 出力: raw f32 リトルエンディアン・インターリーブ PCM を
//!   output_path へ書き、stdout に JSON ステータスを1行出す
//!   （{"ok": true, "frames": N, "channels": 2, "sample_rate": R}
//!   または {"ok": false, "error": "..."}、失敗時は終了コード 1）
//!
//! 別プロセスに分ける理由は ADR-0017: GPLv3 の結合解釈と
//! プラグインのクラッシュから本体を切り離す。プラグインの
//! バイナリは同梱しない（利用者が入手したものへのパスを受け取る）。

use std::io::Read as _;

use clack_common::events::event_types::{NoteOffEvent, NoteOnEvent};
use clack_common::events::io::EventBuffer;
use clack_common::events::Pckn;
use clack_extensions::state::PluginState;
use clack_host::prelude::*;
use serde::Deserialize;

#[derive(Deserialize)]
struct NoteReq {
    start_ms: f64,
    duration_ms: f64,
    pitch: u8,
    velocity: u8,
}

#[derive(Deserialize)]
struct Request {
    plugin_path: String,
    sample_rate: u32,
    duration_ms: f64,
    state_json: Option<String>,
    notes: Vec<NoteReq>,
    output_path: String,
}

struct HostShared;

impl SharedHandler<'_> for HostShared {
    fn request_restart(&self) {}
    fn request_process(&self) {}
    fn request_callback(&self) {}
}

struct Host;

impl HostHandlers for Host {
    type Shared<'a> = HostShared;
    type MainThread<'a> = ();
    type AudioProcessor<'a> = ();
}

const BLOCK: usize = 1024;

fn fail(msg: impl std::fmt::Display) -> ! {
    println!(
        "{}",
        serde_json::json!({"ok": false, "error": msg.to_string()})
    );
    std::process::exit(1);
}

fn main() {
    let mut text = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut text) {
        fail(e);
    }
    let req: Request = match serde_json::from_str(&text) {
        Ok(r) => r,
        Err(e) => fail(format!("invalid request JSON: {e}")),
    };
    if let Err(e) = run(&req) {
        fail(e);
    }
}

fn run(req: &Request) -> Result<(), Box<dyn std::error::Error>> {
    let host_info = HostInfo::new("rust-studio", "rust-studio", "", "0.1.0")?;
    // SAFETY: パスは呼び出し側（DAW 本体）が指定したプラグイン。
    let entry = unsafe { PluginEntry::load(&req.plugin_path) }?;
    let factory = entry
        .get_plugin_factory()
        .ok_or("plugin factory not found")?;
    // バンドル内の最初のプラグインを使う（Issue #72 の範囲では
    // 単一プラグインのバンドルを対象とする）。
    let descriptor = factory
        .plugin_descriptors()
        .next()
        .ok_or("no plugin descriptor")?;
    let plugin_id = descriptor.id().ok_or("descriptor has no id")?;

    let mut instance =
        PluginInstance::<Host>::new(|_| HostShared, |_| (), &entry, plugin_id, &host_info)?;

    // ステート注入（プリセット JSON）。拡張が無いプラグインでは失敗扱い。
    if let Some(state) = &req.state_json {
        let handle = instance.plugin_handle();
        let ext = handle
            .get_extension::<PluginState>()
            .ok_or("plugin does not support the state extension")?;
        let mut cursor = std::io::Cursor::new(state.as_bytes());
        ext.load(&handle, &mut cursor)
            .map_err(|_| "plugin rejected the preset state".to_string())?;
    }

    let config = PluginAudioConfiguration {
        sample_rate: req.sample_rate as f64,
        min_frames_count: 32,
        max_frames_count: BLOCK as u32,
    };
    let audio_processor = instance.activate(|_, _| (), config)?;

    let total_frames = (req.duration_ms * req.sample_rate as f64 / 1000.0).round() as usize;
    let mut pcm: Vec<f32> = Vec::with_capacity(total_frames * 2);

    let mut output_ports = AudioPorts::with_capacity(2, 1);
    let mut out_events_buffer = EventBuffer::new();

    let mut started = audio_processor
        .start_processing()
        .map_err(|_| "start_processing failed")?;

    let mut frame_pos = 0usize;
    while frame_pos < total_frames {
        let block_frames = BLOCK.min(total_frames - frame_pos);
        // このブロックにかかるノートイベントを集める。
        let mut events = EventBuffer::new();
        // ノート ID は連番にする（Issue #72 レビュー Minor: 音高を
        // 流用すると同音高の重なりで ID が衝突し、実プラグインが
        // 2つ目を無視したり両方止めたりしうる）。
        for (index, note) in req.notes.iter().enumerate() {
            let note_id = index as u32;
            let on = (note.start_ms * req.sample_rate as f64 / 1000.0).round() as i64;
            let off = ((note.start_ms + note.duration_ms) * req.sample_rate as f64 / 1000.0).round()
                as i64;
            let block_start = frame_pos as i64;
            let block_end = block_start + block_frames as i64;
            if on >= block_start && on < block_end {
                events.push(&NoteOnEvent::new(
                    (on - block_start) as u32,
                    Pckn::new(0u16, 0u16, note.pitch as u16, note_id),
                    note.velocity as f64 / 127.0,
                ));
            }
            if off >= block_start && off < block_end {
                events.push(&NoteOffEvent::new(
                    (off - block_start) as u32,
                    Pckn::new(0u16, 0u16, note.pitch as u16, note_id),
                    0.0,
                ));
            }
        }
        events.sort();

        let mut out_l = vec![0.0f32; block_frames];
        let mut out_r = vec![0.0f32; block_frames];
        let input_events = InputEvents::from_buffer(&events);
        let mut output_events = OutputEvents::from_buffer(&mut out_events_buffer);
        let input_audio = InputAudioBuffers::empty();
        let mut output_audio = output_ports.with_output_buffers([AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_output_only(
                [out_l.as_mut_slice(), out_r.as_mut_slice()].into_iter(),
            ),
        }]);
        started.process(
            &input_audio,
            &mut output_audio,
            &input_events,
            &mut output_events,
            Some(frame_pos as u64),
            None,
        )?;
        for i in 0..block_frames {
            pcm.push(out_l[i]);
            pcm.push(out_r[i]);
        }
        frame_pos += block_frames;
    }

    let stopped = started.stop_processing();
    instance.deactivate(stopped);

    let mut bytes = Vec::with_capacity(pcm.len() * 4);
    for s in &pcm {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(&req.output_path, bytes)?;
    println!(
        "{}",
        serde_json::json!({
            "ok": true,
            "frames": total_frames,
            "channels": 2,
            "sample_rate": req.sample_rate,
        })
    );
    Ok(())
}
