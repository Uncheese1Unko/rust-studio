//! テスト用 CLAP シンセフィクスチャ。
//! ステートは JSON `{"gain": f32, "square": bool}`。ノートイベントで
//! サイン波（square=true で矩形波）を鳴らす。

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use clack_common::events::event_types::{NoteOffEvent, NoteOnEvent};
use clack_common::stream::{InputStream, OutputStream};
use clack_extensions::state::{PluginState, PluginStateImpl};
use clack_plugin::prelude::*;

#[derive(Clone, Copy, PartialEq, Debug)]
struct Settings {
    gain: f32,
    square: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            gain: 0.5,
            square: false,
        }
    }
}

fn settings_to_json(s: &Settings) -> String {
    format!("{{\"gain\": {}, \"square\": {}}}", s.gain, s.square)
}

fn settings_from_json(bytes: &[u8]) -> Option<Settings> {
    let text = std::str::from_utf8(bytes).ok()?;
    let gain = extract_number(text, "\"gain\"")? as f32;
    let square = text.contains("\"square\": true") || text.contains("\"square\":true");
    Some(Settings { gain, square })
}

fn extract_number(text: &str, key: &str) -> Option<f64> {
    let at = text.find(key)?;
    let rest = &text[at + key.len()..];
    let rest = rest.trim_start_matches([':', ' ', '\t']);
    let end = rest
        .find(|c: char| {
            !(c.is_ascii_digit() || c == '.' || c == '-' || c == 'e' || c == 'E' || c == '+')
        })
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

pub struct TestSynthShared {
    settings: Arc<Mutex<Settings>>,
}

impl PluginShared<'_> for TestSynthShared {}

pub struct TestSynthMainThread {
    settings: Arc<Mutex<Settings>>,
}

impl PluginMainThread<'_, TestSynthShared> for TestSynthMainThread {}

impl PluginStateImpl for TestSynthMainThread {
    fn save(&self, output: &mut OutputStream) -> Result<(), PluginError> {
        let settings = *self.settings.lock().unwrap();
        output.write_all(settings_to_json(&settings).as_bytes())?;
        Ok(())
    }

    fn load(&self, input: &mut InputStream) -> Result<(), PluginError> {
        let mut buf = Vec::new();
        input.read_to_end(&mut buf)?;
        let parsed =
            settings_from_json(&buf).ok_or_else(|| PluginError::Message("invalid state JSON"))?;
        *self.settings.lock().unwrap() = parsed;
        Ok(())
    }
}

struct Voice {
    key: u8,
    phase: f64,
    velocity: f32,
}

pub struct TestSynthAudioProcessor {
    settings: Arc<Mutex<Settings>>,
    sample_rate: f64,
    voices: Vec<Voice>,
}

impl<'a> PluginAudioProcessor<'a, TestSynthShared, TestSynthMainThread>
    for TestSynthAudioProcessor
{
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main_thread: &TestSynthMainThread,
        shared: &'a TestSynthShared,
        audio_config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        Ok(Self {
            settings: shared.settings.clone(),
            sample_rate: audio_config.sample_rate,
            voices: Vec::new(),
        })
    }

    fn process(
        &mut self,
        _process: Process,
        mut audio: Audio,
        events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        let frames = audio.frames_count() as usize;
        // イベントを (時刻, オン/オフ, キー, ベロシティ) に集める。
        let mut evs: Vec<(u32, bool, u8, f32)> = Vec::new();
        for event in events.input.iter() {
            if let Some(on) = event.as_event::<NoteOnEvent>() {
                evs.push((
                    on.header().time(),
                    true,
                    on.pckn().key.as_specific().copied().unwrap_or(0) as u8,
                    on.velocity() as f32,
                ));
            } else if let Some(off) = event.as_event::<NoteOffEvent>() {
                evs.push((
                    off.header().time(),
                    false,
                    off.pckn().key.as_specific().copied().unwrap_or(0) as u8,
                    0.0,
                ));
            }
        }
        let settings = *self.settings.lock().unwrap();
        let Some(mut port) = audio.output_port(0) else {
            return Ok(ProcessStatus::Sleep);
        };
        let Some(channels) = port.channels()?.into_f32() else {
            return Ok(ProcessStatus::Sleep);
        };
        let mut bufs: Vec<&mut [f32]> = channels.into_iter().collect();
        let mut ev_idx = 0;
        for i in 0..frames {
            while ev_idx < evs.len() && evs[ev_idx].0 as usize <= i {
                let (_, on, key, vel) = evs[ev_idx];
                if on {
                    self.voices.push(Voice {
                        key,
                        phase: 0.0,
                        velocity: vel,
                    });
                } else {
                    self.voices.retain(|v| v.key != key);
                }
                ev_idx += 1;
            }
            let mut sample = 0.0f32;
            for voice in &mut self.voices {
                let freq = 440.0 * 2f64.powf((voice.key as f64 - 69.0) / 12.0);
                let ph = voice.phase;
                let v = if settings.square {
                    if ph.fract() < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                } else {
                    (ph * 2.0 * std::f64::consts::PI).sin()
                };
                sample += v as f32 * voice.velocity;
                voice.phase += freq / self.sample_rate;
            }
            sample *= settings.gain;
            for buf in bufs.iter_mut() {
                if i < buf.len() {
                    buf[i] = sample;
                }
            }
        }
        Ok(ProcessStatus::Continue)
    }
}

pub struct TestSynth;

impl Plugin for TestSynth {
    type AudioProcessor<'a> = TestSynthAudioProcessor;
    type Shared<'a> = TestSynthShared;
    type MainThread<'a> = TestSynthMainThread;

    fn declare_extensions(
        builder: &mut PluginExtensions<Self>,
        _shared: Option<&Self::Shared<'_>>,
    ) {
        builder.register::<PluginState>();
    }
}

impl DefaultPluginFactory for TestSynth {
    fn get_descriptor() -> PluginDescriptor {
        use clack_plugin::plugin::features::*;
        PluginDescriptor::new("dev.rust-studio.test-synth", "Rust Studio Test Synth")
            .with_features([SYNTHESIZER, STEREO])
    }

    fn new_shared(_host: HostSharedHandle<'_>) -> Result<Self::Shared<'_>, PluginError> {
        Ok(TestSynthShared {
            settings: Arc::new(Mutex::new(Settings::default())),
        })
    }

    fn new_main_thread<'a>(
        _host: HostMainThreadHandle<'a>,
        shared: &'a Self::Shared<'a>,
    ) -> Result<Self::MainThread<'a>, PluginError> {
        Ok(TestSynthMainThread {
            settings: shared.settings.clone(),
        })
    }
}

clack_export_entry!(SinglePluginEntry<TestSynth>);
