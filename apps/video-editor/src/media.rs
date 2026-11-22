//! アプリ側メディア層（Issue #10）。
//!
//! デコード等の重いメディア処理はコアに入れず、ここ（アプリ側）に閉じる
//! （architecture ルール）。M1 の最小経路として、ffmpeg でサンプル素材を
//! 生成し、映像クリップのフレームを 10fps の PNG 列に事前展開して、
//! 再生ヘッドの時刻に対応するフレームを表示する。
//! 時刻計算とフレーム選択は純粋関数として単体テストで固定する。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command as ProcCommand;

use studio_core::{Clip, Project, TrackKind};

/// フレーム展開のフレームレート。
pub const FRAME_FPS: u32 = 10;
/// フレーム画像の大きさ。
pub const FRAME_WIDTH: u32 = 320;
pub const FRAME_HEIGHT: u32 = 180;

/// メディア作業ディレクトリ（サンプル素材とフレーム列の置き場）。
pub fn media_dir() -> PathBuf {
    std::env::temp_dir().join("rust-studio-video-editor-media")
}

/// ソース参照を実ファイルに解決する。そのままのパスが存在すればそれを、
/// なければメディアディレクトリ内の同名ファイルを使う。
pub fn resolve_source(dir: &Path, source: &str) -> PathBuf {
    let as_given = Path::new(source);
    if as_given.exists() {
        return as_given.to_path_buf();
    }
    match as_given.file_name() {
        Some(name) => dir.join(name),
        None => dir.join(source),
    }
}

/// メディアビンに並べる素材の種別（Issue #47）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Video,
    Image,
}

/// メディアビンの素材1件（Issue #47）。名前はファイル名で、
/// AddClip の source にはこの名前をそのまま使う（resolve_source が
/// メディアディレクトリ内で解決する）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaItem {
    pub name: String,
    pub kind: MediaKind,
}

/// メディアディレクトリ内の対応ファイルを名前順に列挙する
/// （Issue #47）。対応拡張子は resolve_source の解決対象のうち
/// 動画（mp4 / mov）と画像（png / jpg）。音声やフレーム置き場の
/// ようなディレクトリは含めない。ディレクトリが無ければ空。
pub fn list_media(dir: &Path) -> Vec<MediaItem> {
    let mut items: Vec<MediaItem> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase());
            let kind = match ext.as_deref() {
                Some("mp4") | Some("mov") => MediaKind::Video,
                Some("png") | Some("jpg") => MediaKind::Image,
                _ => continue,
            };
            if let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) {
                items.push(MediaItem { name, kind });
            }
        }
    }
    items.sort_by(|a, b| a.name.cmp(&b.name));
    items
}

/// ffprobe の format=duration 出力（秒の文字列）をミリ秒にする
/// （Issue #47）。解釈できなければ None。
pub fn parse_duration_output(text: &str) -> Option<i64> {
    let secs: f64 = text.trim().parse().ok()?;
    if secs.is_sign_negative() {
        return None;
    }
    Some((secs * 1_000.0).round() as i64)
}

/// 素材ファイルの実尺を ffprobe で調べる（Issue #47）。ffprobe が
/// 無い・失敗した・解釈できない場合は None（呼び出し側は既定の
/// クリップ長にフォールバックする）。
pub fn probe_duration_ms(path: &Path) -> Option<i64> {
    let output = ProcCommand::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_duration_output(&String::from_utf8_lossy(&output.stdout))
}

/// 再生ヘッド位置にある映像クリップを返す（トラック順で最初に覆うもの）。
/// 音声トラックは対象外。どの映像クリップの範囲にもなければ None。
pub fn active_video_clip(project: &Project, playhead_ms: i64) -> Option<&Clip> {
    for track in &project.tracks {
        if track.kind != TrackKind::Video {
            continue;
        }
        for clip in &track.clips {
            if playhead_ms >= clip.start_ms && playhead_ms < clip.end_ms() {
                return Some(clip);
            }
        }
    }
    None
}

/// クリップ内でのソース時刻。範囲外なら None。
pub fn source_time_ms(clip: &Clip, playhead_ms: i64) -> Option<i64> {
    if playhead_ms >= clip.start_ms && playhead_ms < clip.end_ms() {
        Some(playhead_ms - clip.start_ms)
    } else {
        None
    }
}

/// ソース時刻に対応するフレーム番号（0 始まり）。
pub fn frame_index(source_time_ms: i64, fps: u32) -> usize {
    if source_time_ms <= 0 {
        return 0;
    }
    (source_time_ms as u64 * fps as u64 / 1_000) as usize
}

/// 展開済みフレーム列のストア。キーはクリップの source 文字列。
#[derive(Debug, Default)]
pub struct FrameStore {
    pub fps: u32,
    frames: BTreeMap<String, Vec<PathBuf>>,
}

impl FrameStore {
    pub fn from_frames(fps: u32, frames: BTreeMap<String, Vec<PathBuf>>) -> Self {
        FrameStore { fps, frames }
    }

    /// ソース時刻に対応するフレーム画像のパス。範囲を超えたら最後の
    /// フレームにクランプする。未展開なら None。
    pub fn frame_path(&self, source: &str, source_time_ms: i64) -> Option<&Path> {
        let list = self.frames.get(source)?;
        if list.is_empty() {
            return None;
        }
        let idx = frame_index(source_time_ms, self.fps).min(list.len() - 1);
        Some(&list[idx])
    }

    pub fn frame_count(&self, source: &str) -> usize {
        self.frames.get(source).map_or(0, |v| v.len())
    }

    /// 展開済みのソース一覧。
    pub fn sources(&self) -> Vec<String> {
        self.frames.keys().cloned().collect()
    }

    /// ソースの全フレームパス（時刻順）。
    pub fn frames_for(&self, source: &str) -> Vec<PathBuf> {
        self.frames.get(source).cloned().unwrap_or_default()
    }
}

/// デモ用のサンプル素材をメディアディレクトリに用意する（なければ生成）。
/// ffmpeg が無い・失敗した場合は警告を出すだけで、アプリは止めない。
fn ensure_sample_media(dir: &Path) {
    let samples: [(&str, Vec<String>); 4] = [
        (
            "opening.mp4",
            vec![
                "-f".into(),
                "lavfi".into(),
                "-i".into(),
                "testsrc2=duration=4:size=320x180:rate=30".into(),
                "-c:v".into(),
                "libx264".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
            ],
        ),
        (
            "interview.mp4",
            vec![
                "-f".into(),
                "lavfi".into(),
                "-i".into(),
                "testsrc2=duration=6:size=320x180:rate=30".into(),
                "-vf".into(),
                "hue=h=90".into(),
                "-c:v".into(),
                "libx264".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
            ],
        ),
        (
            "bgm.wav",
            vec![
                "-f".into(),
                "lavfi".into(),
                "-i".into(),
                "sine=frequency=440:duration=8".into(),
            ],
        ),
        (
            // 切り抜き合成用の背景素材（Issue #22）: 人物のいない単調な映像
            "bg.mp4",
            vec![
                "-f".into(),
                "lavfi".into(),
                "-i".into(),
                "testsrc=duration=8:size=320x180:rate=30".into(),
                "-vf".into(),
                "hue=h=200".into(),
                "-c:v".into(),
                "libx264".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
            ],
        ),
    ];
    for (name, args) in samples {
        let out = dir.join(name);
        if out.exists() {
            continue;
        }
        let status = ProcCommand::new("ffmpeg")
            .arg("-y")
            .arg("-loglevel")
            .arg("error")
            .args(&args)
            .arg(&out)
            .status();
        match status {
            Ok(s) if s.success() => println!("sample media generated: {}", out.display()),
            Ok(s) => println!("warning: ffmpeg exited with {s} for sample {name}"),
            Err(e) => println!("warning: ffmpeg not available ({e}); preview disabled"),
        }
    }
}

/// FNV-1a 64bit（決定的な簡易ハッシュ。依存を増やさない）。
/// フレーム置き場の名前付けと、プロジェクトファイルの変更検出で使う。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// フレーム列の置き場の名前。素材の stem だけだと別ディレクトリの同名
/// 素材が衝突するため、ソース文字列全体のハッシュを付ける。
pub fn frames_dir_name(source: &str) -> String {
    let stem = Path::new(source)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "clip".to_string());
    format!("{stem}-{:08x}", fnv1a(source.as_bytes()))
}

/// 展開先にある既存のフレーム PNG を消す。再展開時に古いフレームが
/// 残ってフレーム数がずれるのを防ぐ（レビュー観察への対応）。
pub fn clear_frame_files(dir: &Path) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            let is_frame = path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('f'))
                && path.extension().is_some_and(|x| x == "png");
            if is_frame {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// 映像ファイルをフレーム PNG 列に展開し、時刻順のパス一覧を返す。
fn extract_frames(media_file: &Path, out_dir: &Path) -> Vec<PathBuf> {
    if !out_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(out_dir) {
            println!(
                "warning: cannot create frame dir {}: {e}",
                out_dir.display()
            );
            return Vec::new();
        }
    }
    clear_frame_files(out_dir);
    let pattern = out_dir.join("f%03d.png");
    let status = ProcCommand::new("ffmpeg")
        .arg("-y")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(media_file)
        .arg("-vf")
        .arg(format!(
            "fps={FRAME_FPS},scale={FRAME_WIDTH}:{FRAME_HEIGHT}"
        ))
        .arg(&pattern)
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            println!(
                "warning: ffmpeg frame extraction exited with {s} for {}",
                media_file.display()
            );
            return Vec::new();
        }
        Err(e) => {
            println!("warning: ffmpeg not available ({e}); preview disabled");
            return Vec::new();
        }
    }
    let mut paths: Vec<PathBuf> = std::fs::read_dir(out_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "png"))
                .collect()
        })
        .unwrap_or_default();
    paths.sort();
    paths
}

/// プロジェクトの映像クリップについてフレーム列を準備する。
/// 音声クリップは展開しない。失敗しても空のストアを返し、アプリは続行する。
pub fn prepare_media(project: &Project) -> FrameStore {
    let dir = media_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        println!("warning: cannot create media dir {}: {e}", dir.display());
        return FrameStore::from_frames(FRAME_FPS, BTreeMap::new());
    }
    ensure_sample_media(&dir);
    let frames_root = dir.join("frames");
    let mut map: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for track in &project.tracks {
        if track.kind != TrackKind::Video {
            continue;
        }
        for clip in &track.clips {
            if map.contains_key(&clip.source) {
                continue;
            }
            let file = resolve_source(&dir, &clip.source);
            let out_dir = frames_root.join(frames_dir_name(&clip.source));
            let frames = if file.exists() {
                extract_frames(&file, &out_dir)
            } else {
                println!("warning: media file not found: {}", file.display());
                Vec::new()
            };
            println!("media prepared: {} -> {} frames", clip.source, frames.len());
            map.insert(clip.source.clone(), frames);
        }
    }
    FrameStore::from_frames(FRAME_FPS, map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::{apply, Command};

    fn project() -> Project {
        let mut p = Project::new("test");
        let v = apply(
            &mut p,
            &Command::AddTrack {
                name: "V1".into(),
                kind: TrackKind::Video,
            },
        )
        .unwrap()
        .unwrap();
        let a = apply(
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
                track_id: v,
                name: "opening".into(),
                start_ms: 0,
                duration_ms: 2_000,
                source: "demo/opening.mp4".into(),
            },
        )
        .unwrap();
        apply(
            &mut p,
            &Command::AddClip {
                track_id: v,
                name: "interview".into(),
                start_ms: 2_500,
                duration_ms: 4_000,
                source: "demo/interview.mp4".into(),
            },
        )
        .unwrap();
        apply(
            &mut p,
            &Command::AddClip {
                track_id: a,
                name: "bgm".into(),
                start_ms: 0,
                duration_ms: 8_000,
                source: "demo/bgm.wav".into(),
            },
        )
        .unwrap();
        p
    }

    #[test]
    fn active_video_clip_follows_playhead() {
        let p = project();
        assert_eq!(active_video_clip(&p, 0).unwrap().name, "opening");
        assert_eq!(active_video_clip(&p, 1_999).unwrap().name, "opening");
        // 2000〜2500 は映像クリップの隙間（音声だけが鳴っている時間帯）
        assert!(active_video_clip(&p, 2_200).is_none());
        assert_eq!(active_video_clip(&p, 2_500).unwrap().name, "interview");
        assert!(active_video_clip(&p, 6_500).is_none());
    }

    #[test]
    fn source_time_is_relative_to_clip_start() {
        let p = project();
        let interview = active_video_clip(&p, 3_000).unwrap();
        assert_eq!(source_time_ms(interview, 3_000), Some(500));
        assert_eq!(source_time_ms(interview, 2_499), None);
        assert_eq!(source_time_ms(interview, 6_500), None);
    }

    #[test]
    fn frame_index_uses_fps() {
        assert_eq!(frame_index(0, 10), 0);
        assert_eq!(frame_index(99, 10), 0);
        assert_eq!(frame_index(100, 10), 1);
        assert_eq!(frame_index(2_500, 10), 25);
        assert_eq!(frame_index(-100, 10), 0);
    }

    #[test]
    fn frame_path_clamps_to_last_frame() {
        let frames: Vec<PathBuf> = (0..40)
            .map(|i| PathBuf::from(format!("f{i:03}.png")))
            .collect();
        let mut map = BTreeMap::new();
        map.insert("demo/opening.mp4".to_string(), frames);
        let store = FrameStore::from_frames(10, map);
        assert_eq!(
            store.frame_path("demo/opening.mp4", 2_500),
            Some(Path::new("f025.png"))
        );
        // クリップ長を超える時刻では最後のフレームにクランプ
        assert_eq!(
            store.frame_path("demo/opening.mp4", 99_999),
            Some(Path::new("f039.png"))
        );
        assert_eq!(store.frame_path("demo/unknown.mp4", 0), None);
        assert_eq!(store.frame_count("demo/opening.mp4"), 40);
    }
}

#[cfg(test)]
mod bin_tests {
    use super::*;

    #[test]
    fn list_media_filters_by_extension() {
        // Issue #47（イシュー指定のテスト）: 対応拡張子（動画 mp4/mov、
        // 画像 png/jpg）のファイルだけが名前順に残る。音声・その他・
        // ディレクトリは含めない。
        let dir = std::env::temp_dir().join(format!("rust-studio-test-bin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in [
            "b.mov", "a.mp4", "c.png", "d.jpg", "e.wav", "f.txt", "noext", "UP.MP4",
        ] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        std::fs::create_dir_all(dir.join("g.mp4")).unwrap();
        let items = list_media(&dir);
        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["UP.MP4", "a.mp4", "b.mov", "c.png", "d.jpg"]);
        assert_eq!(items[1].kind, MediaKind::Video);
        assert_eq!(items[3].kind, MediaKind::Image);
        // 存在しないディレクトリは空
        assert!(list_media(&dir.join("missing")).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn parse_duration_output_to_ms() {
        // Issue #47: ffprobe の format=duration 出力（秒）を ms にする。
        assert_eq!(parse_duration_output("6.000000\n"), Some(6_000));
        assert_eq!(parse_duration_output("2.5"), Some(2_500));
        assert_eq!(parse_duration_output("0.04"), Some(40));
        assert_eq!(parse_duration_output("N/A"), None);
        assert_eq!(parse_duration_output(""), None);
    }
}

#[cfg(test)]
mod hygiene_tests {
    use super::*;

    #[test]
    fn frames_dir_name_distinguishes_same_stem_sources() {
        // a/opening.mp4 と b/opening.mp4 が同じフレーム置き場を共有しないこと
        let a = frames_dir_name("a/opening.mp4");
        let b = frames_dir_name("b/opening.mp4");
        assert_ne!(a, b);
        assert!(a.starts_with("opening-"), "{a}");
        // 同じソースからは常に同じ名前（決定的）
        assert_eq!(a, frames_dir_name("a/opening.mp4"));
    }

    #[test]
    fn clear_frame_files_removes_only_frame_pngs() {
        let dir =
            std::env::temp_dir().join(format!("rust-studio-test-clear-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("f001.png"), b"x").unwrap();
        std::fs::write(dir.join("f002.png"), b"x").unwrap();
        std::fs::write(dir.join("keep.txt"), b"x").unwrap();
        clear_frame_files(&dir);
        assert!(!dir.join("f001.png").exists());
        assert!(!dir.join("f002.png").exists());
        assert!(dir.join("keep.txt").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
