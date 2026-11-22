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
/// 素材時刻は「offset_ms + クリップ内位置」（Issue #43）。分割で
/// 生じた右クリップは offset が分割点まで進んでいるため、これを
/// 加味しないと素材の頭から再生し直してしまう（なお現行コアの
/// TrimClip は offset を進めないので、左トリムした右クリップでは
/// トリム量ぶんずれる制限が別に残る）。
pub fn source_time_ms(clip: &Clip, playhead_ms: i64) -> Option<i64> {
    if playhead_ms >= clip.start_ms && playhead_ms < clip.end_ms() {
        Some(clip.offset_ms + (playhead_ms - clip.start_ms))
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

/// トランジションのブレンド係数（Issue #49、ADR-0014）。
/// 後続クリップの先頭の時間窓 [start, start + transition_in_ms)
/// の中では位置に応じて線形に 0 → 1 へ進む。窓の外・種類が
/// cross_dissolve でない・長さ 0 の場合は None（ブレンドしない）。
pub fn blend_factor(clip: &Clip, playhead_ms: i64) -> Option<f32> {
    if clip.transition_in_kind != studio_core::TransitionKind::CrossDissolve
        || clip.transition_in_ms <= 0
    {
        return None;
    }
    let elapsed = playhead_ms - clip.start_ms;
    if elapsed < 0 || elapsed >= clip.transition_in_ms {
        return None;
    }
    Some(elapsed as f32 / clip.transition_in_ms as f32)
}

/// プレビューでブレンドすべき2フレームの指定（Issue #49）。
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionBlend {
    pub prev_source: String,
    /// 前クリップの素材時刻。タイムライン上の終了位置を過ぎた
    /// 続き（offset 込み）で、フレーム取得側で末尾にクランプされる。
    pub prev_source_time_ms: i64,
    pub next_source: String,
    pub next_source_time_ms: i64,
    pub alpha: f32,
}

/// 再生ヘッド位置でトランジションのブレンドが成立するか
/// （Issue #49、ADR-0014）。成立条件は、アクティブな映像
/// クリップにブレンド係数があり、同一トラックに直接隣接する
/// 前のクリップ（end == start）が存在すること。隙間がある・
/// 前のクリップがない場合は None（従来どおり単一クリップ）。
pub fn transition_blend(project: &Project, playhead_ms: i64) -> Option<TransitionBlend> {
    let clip = active_video_clip(project, playhead_ms)?;
    let alpha = blend_factor(clip, playhead_ms)?;
    let track = project
        .tracks
        .iter()
        .find(|t| t.clips.iter().any(|c| c.id == clip.id))?;
    let prev = track
        .clips
        .iter()
        .filter(|c| c.id != clip.id && c.end_ms() == clip.start_ms)
        .max_by_key(|c| c.start_ms)?;
    Some(TransitionBlend {
        prev_source: prev.source.clone(),
        prev_source_time_ms: prev.offset_ms + (playhead_ms - prev.start_ms),
        next_source: clip.source.clone(),
        next_source_time_ms: source_time_ms(clip, playhead_ms)?,
        alpha,
    })
}

/// 2枚のフレーム画像を α でブレンドした PNG を書き出す
/// （Issue #49、ADR-0014 の CPU ブレンド）。式は切り抜き合成と
/// 同じ out = prev * (1 - α) + next * α。寸法が違う場合は前
/// フレームの寸法に合わせて後フレームをリサイズする。
pub fn blend_frame_files(
    prev_path: &Path,
    next_path: &Path,
    alpha: f32,
    out_path: &Path,
) -> std::io::Result<()> {
    let prev = image::open(prev_path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
        .to_rgba8();
    let next_img = image::open(next_path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let next = if next_img.width() == prev.width() && next_img.height() == prev.height() {
        next_img.to_rgba8()
    } else {
        image::imageops::resize(
            &next_img.to_rgba8(),
            prev.width(),
            prev.height(),
            image::imageops::FilterType::Triangle,
        )
    };
    let a = alpha.clamp(0.0, 1.0);
    // out は prev の複製なので、書き込む前の op[ch] が prev の値。
    let mut out = prev.clone();
    for (op, np) in out.pixels_mut().zip(next.pixels()) {
        for ch in 0..4 {
            op[ch] = ((op[ch] as f32) * (1.0 - a) + (np[ch] as f32) * a)
                .round()
                .clamp(0.0, 255.0) as u8;
        }
    }
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    out.save(out_path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// ブレンド済みフレームのキャッシュパス（Issue #49）。
/// フレームの組と α の量子化段（0.05 刻み）で決まる。
pub fn blend_cache_path(prev_frame: &Path, next_frame: &Path, alpha: f32) -> PathBuf {
    let alpha_q = (alpha.clamp(0.0, 1.0) * 20.0).round() as u8;
    let key = format!(
        "{}|{}|{}",
        prev_frame.display(),
        next_frame.display(),
        alpha_q
    );
    media_dir()
        .join("blend")
        .join(format!("b{:016x}.png", fnv1a(key.as_bytes())))
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
    fn blend_factor_is_linear_across_transition() {
        // Issue #49（イシュー指定のテスト、ADR-0014）: ブレンド係数は
        // 後続クリップ先頭の時間窓で線形に 0 → 1 へ進む。
        let mut p = project();
        // interview は 2500 開始。先頭に 500ms のディゾルブを設定。
        let interview = active_video_clip(&p, 3_000).unwrap().clone();
        apply(
            &mut p,
            &Command::SetClipTransition {
                clip_id: interview.id,
                kind: studio_core::TransitionKind::CrossDissolve,
                duration_ms: 500,
            },
        )
        .unwrap();
        let clip = active_video_clip(&p, 3_000).unwrap();
        assert_eq!(blend_factor(clip, 2_500), Some(0.0));
        assert_eq!(blend_factor(clip, 2_750), Some(0.5));
        let near_end = blend_factor(clip, 2_999).unwrap();
        assert!(near_end > 0.99 && near_end < 1.0, "{near_end}");
        // 窓の外はブレンドしない
        assert_eq!(blend_factor(clip, 2_499), None);
        assert_eq!(blend_factor(clip, 3_000), None);
        // 種類が none のクリップは窓内でもブレンドしない
        let opening = active_video_clip(&p, 0).unwrap();
        assert_eq!(blend_factor(opening, 0), None);
    }

    #[test]
    fn transition_blend_needs_directly_adjacent_previous_clip() {
        // Issue #49（ADR-0014）: ブレンドは同一トラックで直接隣接
        // する境界にだけ適用される。前クリップ側の素材時刻は
        // 終了位置を過ぎた続き（offset 込み）を使う。
        let mut p = project();
        let interview = active_video_clip(&p, 3_000).unwrap().clone();
        apply(
            &mut p,
            &Command::SetClipTransition {
                clip_id: interview.id,
                kind: studio_core::TransitionKind::CrossDissolve,
                duration_ms: 500,
            },
        )
        .unwrap();
        // project() では opening(0..2000) と interview(2500..) の間に
        // 隙間があるため、隣接しておらずブレンドは成立しない。
        assert!(transition_blend(&p, 2_750).is_none());
        // 隙間をなくす: opening を 2500 まで伸ばす代わりに、
        // interview を 2000 開始へ移動して直接隣接させる。
        apply(
            &mut p,
            &Command::MoveClip {
                clip_id: interview.id,
                new_start_ms: 2_000,
            },
        )
        .unwrap();
        let blend = transition_blend(&p, 2_250).expect("adjacent blend");
        assert_eq!(blend.alpha, 0.5);
        assert_eq!(blend.next_source, "demo/interview.mp4");
        assert_eq!(blend.next_source_time_ms, 250);
        assert_eq!(blend.prev_source, "demo/opening.mp4");
        // 前クリップは終了（2000）を過ぎた続きの素材時刻になる
        assert_eq!(blend.prev_source_time_ms, 2_250);
    }

    #[test]
    fn frame_at_uses_clip_offset() {
        // Issue #43: 分割した右クリップは素材の続きを表示する。
        // フレーム選択は「素材時刻 = offset_ms + クリップ内位置」。
        let mut p = project();
        let opening = active_video_clip(&p, 0).unwrap().clone();
        // opening は 0..2000。これを 500 で分割すると右クリップは
        // start 500 / offset 500 になる。
        apply(
            &mut p,
            &Command::SplitClip {
                clip_id: opening.id,
                at_ms: 500,
            },
        )
        .unwrap();
        let right = p.tracks[0]
            .clips
            .iter()
            .find(|c| c.start_ms == 500)
            .expect("right clip exists");
        assert_eq!(right.offset_ms, 500);
        // 右クリップの先頭（再生ヘッド 500）は素材時刻 500
        // ＝フレーム 5（10fps）であって、素材の頭（フレーム 0）
        // ではない。
        let st = source_time_ms(right, 500).unwrap();
        assert_eq!(st, 500);
        assert_eq!(frame_index(st, 10), 5);
        // 右クリップの途中も続きになる。
        let st = source_time_ms(right, 1_500).unwrap();
        assert_eq!(st, 1_500);
        assert_eq!(frame_index(st, 10), 15);
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
