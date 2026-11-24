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

/// スクリプトの置き場（Issue #81）。スクリプト参照は原則ここで
/// 解決する。発見・一覧・有効／無効の管理は `extensions`（#82）。
pub fn scripts_dir() -> PathBuf {
    media_dir().join("scripts")
}

/// スクリプト参照を実ファイルに解決する。絶対パスはそのまま、
/// それ以外はスクリプトディレクトリ内の名前として解決する。
pub fn resolve_script_in(dir: &Path, reference: &str) -> PathBuf {
    let as_given = Path::new(reference);
    if as_given.is_absolute() {
        return as_given.to_path_buf();
    }
    dir.join(reference)
}

/// クリップのスクリプトエフェクトをベースフレームへ適用し、適用後の
/// PNG パスを返す（Issue #81）。プレビューと書き出しで共通の規則に
/// するため、両経路はこの関数を通す。エフェクトが無いクリップでは
/// ベースをそのまま返す。スクリプトの読み込み・実行に失敗したら
/// Err（呼び出し側はエフェクトなしとして扱い、エラーを表示する）。
/// 適用結果は（ベースパス＋スクリプト内容＋パラメータ＋寸法）の
/// ハッシュでキャッシュする。
pub fn apply_script_effect(
    base: &Path,
    clip: &Clip,
    at_ms: i64,
    out_dir: &Path,
) -> Result<PathBuf, String> {
    apply_script_effect_with_dirs(base, clip, at_ms, &scripts_dir(), out_dir)
}

/// `apply_script_effect` のディレクトリ明示版（テスト用）。
pub fn apply_script_effect_with_dirs(
    base: &Path,
    clip: &Clip,
    at_ms: i64,
    scripts_dir: &Path,
    out_dir: &Path,
) -> Result<PathBuf, String> {
    let Some(effect) = &clip.script_effect else {
        return Ok(base.to_path_buf());
    };
    let script_path = resolve_script_in(scripts_dir, &effect.script);
    // Issue #82: 無効化したスクリプトは適用できない。適用済み
    // クリップでは呼び出し側がこのエラーを表示し、無効であることが
    // 観測できる。無効リストはファイル名で効く（一覧の管理単位）。
    let script_file_name = script_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !crate::extensions::is_enabled(scripts_dir, &script_file_name) {
        return Err(format!(
            "スクリプト {} は無効化されているため適用できません",
            effect.script
        ));
    }
    let source = std::fs::read_to_string(&script_path).map_err(|e| {
        format!(
            "スクリプト {} が読み込めません（{}）: {e}",
            effect.script,
            script_path.display()
        )
    })?;
    let img = image::open(base)
        .map_err(|e| format!("フレーム {} が開けません: {e}", base.display()))?
        .to_rgba8();
    let (width, height) = img.dimensions();
    // キャッシュキーにはスクリプトが読める入力をすべて含める
    // （Issue #81 レビュー round 1 Major: クリップ相対時刻・名前・
    // 長さがキーに無いと、時間依存スクリプトの結果が stale になる）。
    let key = format!(
        "{}|{}|{:?}|{}x{}|time={}|name={}|dur={}",
        base.display(),
        source,
        effect.params,
        width,
        height,
        at_ms - clip.start_ms,
        clip.name,
        clip.duration_ms
    );
    let out = out_dir.join(format!("s{:016x}.png", fnv1a(key.as_bytes())));
    if out.exists() {
        return Ok(out);
    }
    let params: crate::script::ScriptParams = effect
        .params
        .iter()
        .map(|(k, v)| {
            let value = match v {
                studio_core::EffectParam::Number(n) => crate::script::ScriptParam::Number(*n),
                studio_core::EffectParam::Bool(b) => crate::script::ScriptParam::Bool(*b),
                studio_core::EffectParam::Text(t) => crate::script::ScriptParam::Text(t.clone()),
            };
            (k.clone(), value)
        })
        .collect();
    let info = crate::script::ClipInfo {
        name: clip.name.clone(),
        duration_ms: clip.duration_ms,
        source: Some(clip.source.clone()),
        time_ms: at_ms - clip.start_ms,
    };
    let mut pixels = img.into_raw();
    crate::script::ScriptEngine::new()
        .run_frame_effect(&source, &info, &params, width, height, &mut pixels)
        .map_err(|e| format!("スクリプト {} の実行エラー: {}", effect.script, e.message))?;
    let img = image::RgbaImage::from_raw(width, height, pixels)
        .ok_or_else(|| "エフェクト後のフレーム再構成に失敗しました".to_string())?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("エフェクト出力先が作れません: {e}"))?;
    }
    img.save(&out)
        .map_err(|e| format!("エフェクト後フレームが保存できません: {e}"))?;
    Ok(out)
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
    extract_frames_sized(media_file, out_dir, FRAME_FPS, FRAME_WIDTH, FRAME_HEIGHT)
}

fn extract_frames_sized(
    media_file: &Path,
    out_dir: &Path,
    fps: u32,
    width: u32,
    height: u32,
) -> Vec<PathBuf> {
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
    let pattern = out_dir.join("f%05d.png");
    let status = ProcCommand::new("ffmpeg")
        .arg("-y")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(media_file)
        .arg("-vf")
        .arg(format!("fps={fps},scale={width}:{height}"))
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
    sort_frame_paths(&mut paths);
    paths
}

/// フレーム列を数値順に並べる（Issue #54 レビュー Medium）。
/// 文字列ソートでは f1000.png が f101.png より前に来てしまう。
pub fn sort_frame_paths(paths: &mut [PathBuf]) {
    fn frame_number(p: &Path) -> u64 {
        p.file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_prefix('f'))
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(u64::MAX)
    }
    paths.sort_by_key(|p| frame_number(p));
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

// ---------- Issue #54: mp4 書き出し（ADR-0015） ----------
//
// 方式は ADR-0015 のとおり: アプリ側で PNG フレーム列とミックス済み
// WAV を作り、外部 ffmpeg で H.264/AAC の mp4 にエンコードする。
// 書き出し設定はパラメータで、既定は 1280x720 / 30fps（プレビュー
// の 320x180 / 10fps とは別）。映像の選択規則はプレビューと同じ
// （トラック順で最初に覆う映像クリップ、トランジションは
// ADR-0014 の α ブレンド、覆われない時刻は黒）。音声は音声
// トラックを DAW の render_mix と同じ規則でステレオ 48kHz に
// ミックスする（ゲイン×トラック音量、パン、フェード、offset。
// ピッチシフトは ADR-0015 の規則に含まれないため行わない）。
// 映像ソースに埋め込まれた音声は対象外（ADR-0015 の明記どおり）。

/// 書き出し音声のサンプルレート（ADR-0015: ステレオ 48kHz）。
pub const EXPORT_SAMPLE_RATE: u32 = 48_000;

/// 書き出し設定（ADR-0015 のパラメータ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            fps: 30,
        }
    }
}

/// 書き出し計画: 全長とフレーム数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportPlan {
    pub settings: ExportSettings,
    pub duration_ms: i64,
    pub frame_count: usize,
}

/// 全長は全トラックを通じたクリップ終端の最大値（ADR-0015）。
pub fn export_plan(project: &Project, settings: ExportSettings) -> ExportPlan {
    let duration_ms = project
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.start_ms + c.duration_ms))
        .max()
        .unwrap_or(0)
        .max(0);
    let frame_count = if duration_ms <= 0 || settings.fps == 0 {
        0
    } else {
        ((duration_ms as u64 * settings.fps as u64).div_ceil(1000)) as usize
    };
    ExportPlan {
        settings,
        duration_ms,
        frame_count,
    }
}

/// 書き出し時刻のフレーム選択（プレビューと同じ規則）。
#[derive(Debug, Clone, PartialEq)]
pub enum ExportFrame {
    /// どの映像クリップにも覆われていない（黒フレーム）。
    Black,
    Clip {
        source: String,
        source_time_ms: i64,
    },
    Blend(TransitionBlend),
    Cutout {
        source: String,
        source_time_ms: i64,
    },
}

pub fn export_frame_at(project: &Project, t_ms: i64) -> ExportFrame {
    let Some(clip) = active_video_clip(project, t_ms) else {
        return ExportFrame::Black;
    };
    let source_time_ms =
        source_time_ms(clip, t_ms).unwrap_or_else(|| clip.offset_ms + (t_ms - clip.start_ms));
    // プレビューの優先順位と同じ: 切り抜き合成 ＞ ブレンド ＞ 素フレーム
    if clip.cutout {
        ExportFrame::Cutout {
            source: clip.source.clone(),
            source_time_ms,
        }
    } else if let Some(blend) = transition_blend(project, t_ms) {
        ExportFrame::Blend(blend)
    } else {
        ExportFrame::Clip {
            source: clip.source.clone(),
            source_time_ms,
        }
    }
}

struct DecodedAudio {
    channels: u16,
    sample_rate: u32,
    samples: Vec<f32>,
}

fn decode_wav(path: &Path) -> Result<DecodedAudio, String> {
    let mut reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?,
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|r| r.map(|v| v as f32 / max))
                .collect::<Result<_, _>>()
                .map_err(|e| e.to_string())?
        }
    };
    Ok(DecodedAudio {
        channels: spec.channels.max(1),
        sample_rate: spec.sample_rate,
        samples,
    })
}

/// DAW の fade_envelope と同じ線形ランプ。
fn fade_envelope(clip: &Clip, within_ms: f64) -> f32 {
    let mut env = 1.0f64;
    if clip.fade_in_ms > 0 {
        env = env.min((within_ms / clip.fade_in_ms as f64).clamp(0.0, 1.0));
    }
    if clip.fade_out_ms > 0 {
        let rest = clip.duration_ms as f64 - within_ms;
        env = env.min((rest / clip.fade_out_ms as f64).clamp(0.0, 1.0));
    }
    env as f32
}

fn sample_at(audio: &DecodedAudio, pos: f64, out_ch: usize) -> f32 {
    let channels = audio.channels as usize;
    if channels == 0 || audio.samples.is_empty() {
        return 0.0;
    }
    let frames = audio.samples.len() / channels;
    if frames == 0 {
        return 0.0;
    }
    let i0 = pos.floor().max(0.0) as usize;
    let frac = (pos - pos.floor()) as f32;
    let pick = |frame: usize| -> f32 {
        if frame >= frames {
            return 0.0; // 素材の終端を超えたら無音（DAW と同じ）
        }
        let src_ch = if channels == 1 {
            0
        } else {
            out_ch.min(channels - 1)
        };
        audio.samples[frame * channels + src_ch]
    };
    let a = pick(i0);
    let b = pick(i0 + 1);
    a + (b - a) * frac
}

/// 音声トラックをステレオ 48kHz にミックスする（インターリーブ）。
/// DAW の render_mix と同じ規則（ミュート／ソロ、クリップゲイン×
/// トラック音量、パン、フェード、offset）。デコード
/// できない素材は理由を出して飛ばす。全体は ±1 にクランプする。
pub fn mix_audio(project: &Project, duration_ms: i64) -> Vec<f32> {
    let total_frames = ((duration_ms.max(0) as u64 * EXPORT_SAMPLE_RATE as u64) / 1000) as usize;
    let mut out = vec![0.0f32; total_frames * 2];
    let any_solo = project.tracks.iter().any(|t| t.solo);
    let dir = media_dir();
    let mut decoded: BTreeMap<String, Option<DecodedAudio>> = BTreeMap::new();
    for track in &project.tracks {
        if track.kind != TrackKind::Audio {
            continue;
        }
        if track.muted || (any_solo && !track.solo) {
            continue;
        }
        let channel_factor = [
            track.volume * (1.0 - track.pan.max(0.0)),
            track.volume * (1.0 + track.pan.min(0.0)),
        ];
        for clip in &track.clips {
            let entry = decoded.entry(clip.source.clone()).or_insert_with(|| {
                let file = resolve_source(&dir, &clip.source);
                match decode_wav(&file) {
                    Ok(a) => Some(a),
                    Err(e) => {
                        println!("export: skipping undecodable audio {}: {e}", clip.source);
                        None
                    }
                }
            });
            let Some(audio) = entry.as_ref() else {
                continue;
            };
            let start_frame =
                (clip.start_ms.max(0) as u64 * EXPORT_SAMPLE_RATE as u64 / 1000) as usize;
            let clip_frames =
                (clip.duration_ms.max(0) as u64 * EXPORT_SAMPLE_RATE as u64 / 1000) as usize;
            for i in 0..clip_frames {
                let out_idx = start_frame + i;
                if out_idx >= total_frames {
                    break;
                }
                let within_ms = i as f64 * 1000.0 / EXPORT_SAMPLE_RATE as f64;
                let src_pos =
                    (within_ms + clip.offset_ms as f64) * audio.sample_rate as f64 / 1000.0;
                let env = fade_envelope(clip, within_ms) * clip.gain;
                for ch in 0..2 {
                    out[out_idx * 2 + ch] +=
                        sample_at(audio, src_pos, ch) * env * channel_factor[ch];
                }
            }
        }
    }
    for v in &mut out {
        *v = v.clamp(-1.0, 1.0);
    }
    out
}

fn write_mix_wav(path: &Path, samples: &[f32]) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: EXPORT_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    for s in samples {
        writer
            .write_sample((s.clamp(-1.0, 1.0) * 32_767.0) as i16)
            .map_err(|e| e.to_string())?;
    }
    writer.finalize().map_err(|e| e.to_string())
}

/// 書き出し解像度・fps で映像ソースのフレームを展開する。
fn prepare_export_frames(project: &Project, settings: &ExportSettings, work: &Path) -> FrameStore {
    let dir = media_dir();
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
            let out_dir = work.join("src-frames").join(frames_dir_name(&clip.source));
            let frames = if file.exists() {
                extract_frames_sized(
                    &file,
                    &out_dir,
                    settings.fps,
                    settings.width,
                    settings.height,
                )
            } else {
                println!("export: media file not found: {}", file.display());
                Vec::new()
            };
            map.insert(clip.source.clone(), frames);
        }
    }
    FrameStore::from_frames(settings.fps, map)
}

/// 書き出し1フレーム分の PNG ファイルを決める。
#[allow(clippy::too_many_arguments)]
fn compose_export_frame(
    project: &Project,
    store: &FrameStore,
    preview_store: &FrameStore,
    t_ms: i64,
    settings: &ExportSettings,
    work: &Path,
    black: &Path,
) -> PathBuf {
    let composed = match export_frame_at(project, t_ms) {
        ExportFrame::Black => black.to_path_buf(),
        ExportFrame::Clip {
            source,
            source_time_ms,
        } => store
            .frame_path(&source, source_time_ms)
            .map_or_else(|| black.to_path_buf(), Path::to_path_buf),
        ExportFrame::Blend(b) => {
            let pair = (
                store.frame_path(&b.prev_source, b.prev_source_time_ms),
                store.frame_path(&b.next_source, b.next_source_time_ms),
            );
            match pair {
                (Some(prev), Some(next)) => {
                    let alpha_q = (b.alpha * 20.0).round() / 20.0;
                    let hash = fnv1a(
                        format!(
                            "{}|{}|{}|{}",
                            prev.display(),
                            next.display(),
                            (alpha_q * 100.0) as i32,
                            settings.width
                        )
                        .as_bytes(),
                    );
                    let out = work.join("blend").join(format!("b{hash:016x}.png"));
                    match blend_frame_files(prev, next, b.alpha, &out) {
                        Ok(()) => out,
                        Err(_) => black.to_path_buf(),
                    }
                }
                _ => black.to_path_buf(),
            }
        }
        ExportFrame::Cutout {
            source,
            source_time_ms,
        } => {
            // プレビューが作った合成済みフレーム（320x180）を書き出し
            // サイズへ拡大して使う。合成がまだ無い場合は素のフレーム
            // で代替する（記録: 書き出し側で合成は再実行しない）。
            let dir = media_dir().join(crate::cutout::composite_dir_name(&source));
            let idx = frame_index(source_time_ms, FRAME_FPS);
            let composite = dir.join(format!("f{idx:03}.png"));
            let base = if composite.exists() {
                Some(composite)
            } else {
                preview_store
                    .frame_path(&source, source_time_ms)
                    .map(Path::to_path_buf)
            };
            match base {
                None => black.to_path_buf(),
                Some(base) => {
                    let out = work.join("cutout").join(format!(
                        "c{:016x}.png",
                        fnv1a(base.to_string_lossy().as_bytes())
                    ));
                    let mut result = out.clone();
                    if !out.exists() {
                        if let Some(parent) = out.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        let resized = image::open(&base).and_then(|img| {
                            img.resize_exact(
                                settings.width,
                                settings.height,
                                image::imageops::FilterType::Triangle,
                            )
                            .save(&out)
                        });
                        if resized.is_err() {
                            // Issue #54 レビュー round 2 残件: 失敗時に
                            // プレビュー解像度（320x180）のファイルを返すと
                            // 書き出し列に寸法が混在する。書き出し解像度の
                            // 素フレーム（無ければ黒）で代替する。
                            result = store
                                .frame_path(&source, source_time_ms)
                                .map_or_else(|| black.to_path_buf(), Path::to_path_buf);
                        }
                    }
                    result
                }
            }
        }
    };
    // スクリプトエフェクト（Issue #81）: プレビューと同じ規則で、
    // アクティブなクリップのエフェクトを合成済みフレームへ適用する。
    // 失敗したらエフェクトなしのフレームで書き出しを続ける。
    // 黒フォールバック（フレーム欠落時）には適用しない: プレビューは
    // ベースが無いと何も描かないため（レビュー round 1 Minor）。
    if composed.as_path() != black {
        if let Some(clip) = active_video_clip(project, t_ms) {
            if clip.script_effect.is_some() {
                match apply_script_effect(&composed, clip, t_ms, &work.join("scriptfx")) {
                    Ok(path) => return path,
                    Err(e) => println!("export script effect failed: {e}"),
                }
            }
        }
    }
    composed
}

/// 書き出し結果。
#[derive(Debug)]
pub struct ExportReport {
    pub duration_ms: i64,
    pub frame_count: usize,
    pub out_path: PathBuf,
}

/// 書き出しの作業ディレクトリ（出力先＋設定から決定的に決まる）。
pub fn export_work_dir(out_path: &Path, settings: &ExportSettings) -> PathBuf {
    media_dir().join(format!(
        "export-{:016x}",
        fnv1a(
            format!(
                "{}-{}x{}@{}",
                out_path.display(),
                settings.width,
                settings.height,
                settings.fps
            )
            .as_bytes()
        )
    ))
}

/// タイムライン全体を mp4 に書き出す（ADR-0015）。進捗は
/// （済みフレーム数, 全フレーム数）でコールバックへ知らせる。
/// 出力は一時ファイルへ書いてからリネームする。
pub fn export_mp4(
    project: &Project,
    out_path: &Path,
    settings: ExportSettings,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<ExportReport, String> {
    let plan = export_plan(project, settings);
    if plan.frame_count == 0 {
        return Err("書き出す内容がありません（タイムラインが空です）".into());
    }
    let work = export_work_dir(out_path, &settings);
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let store = prepare_export_frames(project, &settings, &work);
    let preview_store = prepare_media(project);
    let black = work.join("black.png");
    image::RgbImage::new(settings.width, settings.height)
        .save(&black)
        .map_err(|e| e.to_string())?;
    let frames_dir = work.join("frames");
    // Issue #54 レビュー Major: 前回の書き出しのフレームが残ると
    // 連番に混入して出力が長くなる。開始時に必ず空にする。
    if frames_dir.exists() {
        std::fs::remove_dir_all(&frames_dir).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&frames_dir).map_err(|e| e.to_string())?;
    for i in 0..plan.frame_count {
        let t_ms = (i as i64 * 1000) / settings.fps as i64;
        let src = compose_export_frame(
            project,
            &store,
            &preview_store,
            t_ms,
            &settings,
            &work,
            &black,
        );
        let dst = frames_dir.join(format!("f{i:05}.png"));
        std::fs::copy(&src, &dst).map_err(|e| e.to_string())?;
        progress(i + 1, plan.frame_count);
    }
    // 書き出し先のディレクトリはエンコード前に用意する
    // （一時ファイルも同じ場所に作るため）。
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    let mix = mix_audio(project, plan.duration_ms);
    let wav = work.join("mix.wav");
    write_mix_wav(&wav, &mix)?;
    // 一時ファイルは書き出し先と同じディレクトリに作る
    // （別ファイルシステム越しの rename は失敗するため。ADR-0015）
    let tmp_out = out_path.with_extension("tmp.mp4");
    let pattern = frames_dir.join("f%05d.png");
    let status = ProcCommand::new("ffmpeg")
        .arg("-y")
        .arg("-loglevel")
        .arg("error")
        .arg("-framerate")
        .arg(settings.fps.to_string())
        .arg("-i")
        .arg(&pattern)
        .arg("-i")
        .arg(&wav)
        .arg("-c:v")
        .arg("libx264")
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg("-c:a")
        .arg("aac")
        .arg("-movflags")
        .arg("+faststart")
        .arg(&tmp_out)
        .status()
        .map_err(|e| format!("ffmpeg を起動できません: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_file(&tmp_out);
        return Err(format!("ffmpeg が失敗しました（{status}）"));
    }
    std::fs::rename(&tmp_out, out_path).map_err(|e| e.to_string())?;
    Ok(ExportReport {
        duration_ms: plan.duration_ms,
        frame_count: plan.frame_count,
        out_path: out_path.to_path_buf(),
    })
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

    #[test]
    fn export_plan_covers_timeline_length() {
        // Issue #54（イシュー指定のテスト、ADR-0015）: 書き出し計画は
        // 全トラックを通じたクリップ終端の最大値までを覆う。
        let mut p = project();
        // project(): opening 0..2000（映像）、interview 2500..6500
        // （映像）、bgm 0..8000（音声）。全長は音声の 8000。
        let plan = export_plan(&p, ExportSettings::default());
        assert_eq!(plan.duration_ms, 8_000);
        assert_eq!(plan.frame_count, 240); // 8000ms * 30fps
        assert_eq!(plan.settings.width, 1280);
        assert_eq!(plan.settings.height, 720);
        assert_eq!(plan.settings.fps, 30);
        // 空のプロジェクトは長さ 0
        let empty = Project::new("empty");
        let plan = export_plan(&empty, ExportSettings::default());
        assert_eq!(plan.duration_ms, 0);
        assert_eq!(plan.frame_count, 0);
        let _ = &mut p;
    }

    #[test]
    fn export_frame_at_follows_preview_rules() {
        // Issue #54（ADR-0015）: 書き出しフレームの選択はプレビューと
        // 同じ規則（覆われない時刻は黒、素材時刻は offset 込み、
        // トランジション窓ではブレンド）。
        let p = project();
        // 隙間（2200）は黒フレーム
        assert_eq!(export_frame_at(&p, 2_200), ExportFrame::Black);
        // opening の中は通常フレーム
        assert_eq!(
            export_frame_at(&p, 500),
            ExportFrame::Clip {
                source: "demo/opening.mp4".into(),
                source_time_ms: 500,
            }
        );
        // interview にトランジションを付けると窓内はブレンドになる
        let mut p2 = project();
        let interview = active_video_clip(&p2, 3_000).unwrap().clone();
        apply(
            &mut p2,
            &Command::MoveClip {
                clip_id: interview.id,
                new_start_ms: 2_000,
            },
        )
        .unwrap();
        apply(
            &mut p2,
            &Command::SetClipTransition {
                clip_id: interview.id,
                kind: studio_core::TransitionKind::CrossDissolve,
                duration_ms: 500,
            },
        )
        .unwrap();
        match export_frame_at(&p2, 2_250) {
            ExportFrame::Blend(b) => assert_eq!(b.alpha, 0.5),
            other => panic!("expected Blend, got {other:?}"),
        }
    }

    #[test]
    fn mix_audio_applies_gain_fade_and_track_settings() {
        // Issue #54（ADR-0015）: 音声ミックスは DAW の render_mix と
        // 同じ規則（クリップゲイン×トラック音量、パン、フェード、
        // offset）。一定振幅の WAV で数値を確認する。
        let dir = std::env::temp_dir().join(format!("rust-studio-test-mix-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav_path = dir.join("tone.wav");
        {
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: 48_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut writer = hound::WavWriter::create(&wav_path, spec).unwrap();
            for _ in 0..48_000 * 2 {
                writer.write_sample((0.8f32 * 32_767.0) as i16).unwrap();
            }
            writer.finalize().unwrap();
        }
        let mut p = Project::new("mix");
        let a = apply(
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
                track_id: a,
                name: "tone".into(),
                start_ms: 0,
                duration_ms: 2_000,
                source: wav_path.to_string_lossy().into_owned(),
            },
        )
        .unwrap()
        .unwrap();
        apply(&mut p, &Command::SetClipGain { clip_id, gain: 0.5 }).unwrap();
        apply(
            &mut p,
            &Command::SetClipFade {
                clip_id,
                fade_in_ms: 1_000,
                fade_out_ms: 0,
            },
        )
        .unwrap();
        let mix = mix_audio(&p, 2_000);
        assert_eq!(mix.len(), 48_000 * 2 * 2); // 2秒 * 48kHz * ステレオ
        let at = |ms: i64| mix[(ms as usize * 48) * 2];
        // フェードイン中点（500ms）: 0.8 * 0.5(gain) * 0.5(fade) = 0.2
        assert!((at(500) - 0.2).abs() < 0.01, "at 500ms: {}", at(500));
        // フェード後（1500ms）: 0.8 * 0.5 = 0.4
        assert!((at(1500) - 0.4).abs() < 0.01, "at 1500ms: {}", at(1500));
        // 左チャンネルと右チャンネルは同じ（パン中央、モノラル素材）
        assert_eq!(mix[(500 * 48) * 2], mix[(500 * 48) * 2 + 1]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sort_frame_paths_orders_numerically() {
        // Issue #54 レビュー Medium: フレーム列の並びは数値順。
        // 文字列ソートでは f1000.png が f101.png より前に来て、
        // 30fps では約33秒超の素材で順序が入れ替わってしまう。
        let mut paths: Vec<PathBuf> = ["f1000.png", "f101.png", "f099.png", "f100.png", "f009.png"]
            .iter()
            .map(PathBuf::from)
            .collect();
        sort_frame_paths(&mut paths);
        let names: Vec<String> = paths
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            ["f009.png", "f099.png", "f100.png", "f101.png", "f1000.png"]
        );
    }

    #[test]
    fn sample_at_returns_silence_beyond_source_end() {
        // Issue #54 レビュー Minor: 素材の終端を超えたら DAW の
        // render_mix と同じく無音（最後のサンプルの保持ではない）。
        let audio = DecodedAudio {
            channels: 1,
            sample_rate: 48_000,
            samples: vec![0.5; 4],
        };
        assert_eq!(sample_at(&audio, 1.0, 0), 0.5);
        assert_eq!(sample_at(&audio, 10.0, 0), 0.0);
    }

    #[test]
    fn export_frame_at_prefers_cutout_over_blend() {
        // Issue #54 レビュー Minor: プレビューの優先順位は
        // 切り抜き合成 ＞ ブレンド。書き出しも同じ規則にする。
        let mut p = project();
        let interview = active_video_clip(&p, 3_000).unwrap().clone();
        apply(
            &mut p,
            &Command::MoveClip {
                clip_id: interview.id,
                new_start_ms: 2_000,
            },
        )
        .unwrap();
        apply(
            &mut p,
            &Command::SetClipTransition {
                clip_id: interview.id,
                kind: studio_core::TransitionKind::CrossDissolve,
                duration_ms: 500,
            },
        )
        .unwrap();
        for track in &mut p.tracks {
            for clip in &mut track.clips {
                if clip.id == interview.id {
                    clip.cutout = true;
                }
            }
        }
        assert!(matches!(
            export_frame_at(&p, 2_250),
            ExportFrame::Cutout { .. }
        ));
    }

    #[test]
    fn export_mp4_reexport_does_not_mix_stale_frames() {
        // Issue #54 レビュー Major: 同じ出力先へ短くして再書き出し
        // しても、前回のフレームが作業ディレクトリに残って混入
        // しない（開始時にフレーム置き場を空にする）。
        let settings = ExportSettings {
            width: 160,
            height: 90,
            fps: 10,
        };
        let out = std::env::temp_dir().join(format!(
            "rust-studio-test-reexport-{}.mp4",
            std::process::id()
        ));
        let work = export_work_dir(&out, &settings);
        let frames_dir = work.join("frames");
        std::fs::create_dir_all(&frames_dir).unwrap();
        // 前回の長い書き出しの残骸を置く
        for i in 0..20 {
            std::fs::write(frames_dir.join(format!("f{i:05}.png")), b"stale").unwrap();
        }
        // 500ms のプロジェクト（10fps で 5 フレーム）
        let mut p = Project::new("short");
        let v = apply(
            &mut p,
            &Command::AddTrack {
                name: "V1".into(),
                kind: TrackKind::Video,
            },
        )
        .unwrap()
        .unwrap();
        apply(
            &mut p,
            &Command::AddClip {
                track_id: v,
                name: "c".into(),
                start_ms: 0,
                duration_ms: 500,
                source: "demo/opening.mp4".into(),
            },
        )
        .unwrap();
        // エンコード成否は環境依存のため見ない（フレーム列の衛生が対象）
        let _ = export_mp4(&p, &out, settings, &mut |_, _| {});
        let remaining: Vec<String> = std::fs::read_dir(&frames_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".png"))
            .collect();
        assert_eq!(remaining.len(), 5, "残ったフレーム: {remaining:?}");
        std::fs::remove_dir_all(&work).ok();
        std::fs::remove_file(&out).ok();
    }
    // ---- スクリプトエフェクト（Issue #81） ----

    fn fx_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("studio-media-fx-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn clip_with_effect(script: &str) -> Clip {
        let mut p = project();
        let clip_id = p.tracks[0].clips[0].id;
        apply(
            &mut p,
            &Command::SetClipScriptEffect {
                clip_id,
                effect: Some(studio_core::ScriptEffect {
                    script: script.to_string(),
                    params: Default::default(),
                }),
            },
        )
        .unwrap();
        p.tracks[0].clips[0].clone()
    }

    #[test]
    fn script_effect_applies_to_frame_pixels() {
        let dir = fx_dir("apply");
        let scripts = dir.join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(
            scripts.join("invert.lua"),
            "for x = 0, frame.width - 1 do
                 local r, g, b, a = frame.get(x, 0)
                 frame.set(x, 0, 255 - r, 255 - g, 255 - b, a)
             end",
        )
        .unwrap();
        let base = dir.join("base.png");
        image::RgbaImage::from_raw(2, 1, vec![10, 20, 30, 255, 200, 150, 100, 255])
            .unwrap()
            .save(&base)
            .unwrap();
        let clip = clip_with_effect("invert.lua");
        let out_dir = dir.join("out");
        let derived = apply_script_effect_with_dirs(&base, &clip, 0, &scripts, &out_dir).unwrap();
        assert_ne!(derived, base);
        let img = image::open(&derived).unwrap().to_rgba8();
        assert_eq!(img.as_raw(), &vec![245, 235, 225, 255, 55, 105, 155, 255]);
        // 2回目はキャッシュ（同じパス）が返る
        let again = apply_script_effect_with_dirs(&base, &clip, 0, &scripts, &out_dir).unwrap();
        assert_eq!(again, derived);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn script_effect_disabled_script_is_not_applied() {
        // Issue #82: 無効化したスクリプトは、適用済みクリップでも
        // エフェクトとして適用できない（無効である旨が返る）。
        let dir = fx_dir("disabled");
        let scripts = dir.join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(
            scripts.join("invert.lua"),
            "local r, g, b, a = frame.get(0, 0)\n             frame.set(0, 0, 255 - r, 255 - g, 255 - b, a)",
        )
        .unwrap();
        let base = dir.join("base.png");
        image::RgbaImage::from_raw(1, 1, vec![10, 20, 30, 255])
            .unwrap()
            .save(&base)
            .unwrap();
        let clip = clip_with_effect("invert.lua");
        crate::extensions::set_enabled(&scripts, "invert.lua", false).unwrap();
        let err =
            apply_script_effect_with_dirs(&base, &clip, 0, &scripts, &dir.join("out")).unwrap_err();
        assert!(err.contains("無効"), "{err}");
        // 有効に戻せば適用できる
        crate::extensions::set_enabled(&scripts, "invert.lua", true).unwrap();
        let derived =
            apply_script_effect_with_dirs(&base, &clip, 0, &scripts, &dir.join("out")).unwrap();
        let img = image::open(&derived).unwrap().to_rgba8();
        assert_eq!(img.as_raw(), &vec![245, 235, 225, 255]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn script_effect_missing_script_is_error() {
        let dir = fx_dir("missing");
        let scripts = dir.join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        let base = dir.join("base.png");
        image::RgbaImage::from_raw(1, 1, vec![0, 0, 0, 255])
            .unwrap()
            .save(&base)
            .unwrap();
        let clip = clip_with_effect("nope.lua");
        let err =
            apply_script_effect_with_dirs(&base, &clip, 0, &scripts, &dir.join("out")).unwrap_err();
        assert!(err.contains("nope.lua"), "{err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn script_effect_broken_script_is_error() {
        let dir = fx_dir("broken");
        let scripts = dir.join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(scripts.join("bad.lua"), "error('bad fx')").unwrap();
        let base = dir.join("base.png");
        image::RgbaImage::from_raw(1, 1, vec![0, 0, 0, 255])
            .unwrap()
            .save(&base)
            .unwrap();
        let clip = clip_with_effect("bad.lua");
        let err =
            apply_script_effect_with_dirs(&base, &clip, 0, &scripts, &dir.join("out")).unwrap_err();
        assert!(err.contains("bad fx"), "{err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn script_effect_absent_returns_base() {
        let p = project();
        let clip = &p.tracks[0].clips[0];
        let base = PathBuf::from("some/base.png");
        let derived = apply_script_effect_with_dirs(
            &base,
            clip,
            0,
            Path::new("no-scripts"),
            Path::new("no-out"),
        )
        .unwrap();
        assert_eq!(derived, base);
    }

    #[test]
    fn script_effect_cache_key_includes_clip_time() {
        // Issue #81 レビュー round 1 (Major): clip.time_ms に依存する
        // スクリプトは、同じベースフレームでも時刻が違えば違う結果を
        // 返さなければならない（キャッシュキーが時刻を含む）。
        let dir = fx_dir("timekey");
        let scripts = dir.join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(
            scripts.join("bytime.lua"),
            "local v = math.floor(clip.time_ms / 10) % 256
             frame.fill_rect(0, 0, frame.width, frame.height, v, 0, 0)",
        )
        .unwrap();
        let base = dir.join("base.png");
        image::RgbaImage::from_raw(2, 1, vec![0, 0, 0, 255, 0, 0, 0, 255])
            .unwrap()
            .save(&base)
            .unwrap();
        let clip = clip_with_effect("bytime.lua");
        let out_dir = dir.join("out");
        let at0 = apply_script_effect_with_dirs(&base, &clip, 0, &scripts, &out_dir).unwrap();
        let at500 = apply_script_effect_with_dirs(&base, &clip, 500, &scripts, &out_dir).unwrap();
        assert_ne!(at0, at500, "時刻が違えばキャッシュも別になる");
        let img500 = image::open(&at500).unwrap().to_rgba8();
        assert_eq!(img500.get_pixel(0, 0), &image::Rgba([50, 0, 0, 255]));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn compose_export_frame_skips_effect_on_black_fallback() {
        // Issue #81 レビュー round 1 (Minor): フレームが無い場合の
        // 黒フォールバックにはエフェクトを適用しない（プレビューは
        // ベースが無いと何も描かないので、書き出しも合わせる）。
        let dir = fx_dir("blackskip");
        let scripts = scripts_dir();
        std::fs::create_dir_all(&scripts).unwrap();
        let script_name = format!("fillall-{}.lua", std::process::id());
        std::fs::write(
            scripts.join(&script_name),
            "frame.fill_rect(0, 0, frame.width, frame.height, 255, 0, 0)",
        )
        .unwrap();
        let mut p = project();
        let clip_id = p.tracks[0].clips[0].id;
        apply(
            &mut p,
            &Command::SetClipScriptEffect {
                clip_id,
                effect: Some(studio_core::ScriptEffect {
                    script: script_name.clone(),
                    params: Default::default(),
                }),
            },
        )
        .unwrap();
        // フレームが 1 枚も無いストア → compose は黒に落ちる
        let store = FrameStore::from_frames(10, BTreeMap::new());
        let preview_store = FrameStore::from_frames(10, BTreeMap::new());
        let black = dir.join("black.png");
        image::RgbaImage::from_raw(2, 1, vec![0, 0, 0, 255, 0, 0, 0, 255])
            .unwrap()
            .save(&black)
            .unwrap();
        let out = compose_export_frame(
            &p,
            &store,
            &preview_store,
            0,
            &ExportSettings::default(),
            &dir.join("work"),
            &black,
        );
        assert_eq!(out, black, "黒フォールバックにはエフェクトを適用しない");
        let _ = std::fs::remove_file(scripts.join(&script_name));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn compose_export_frame_applies_script_effect() {
        // Issue #81: 書き出しでもプレビューと同じスクリプトエフェクトが
        // 同じ意味で反映される。同一ベースフレームに対する結果が
        // プレビュー経路（apply_script_effect）とピクセル一致する。
        let dir = fx_dir("export");
        let scripts = scripts_dir();
        std::fs::create_dir_all(&scripts).unwrap();
        let script_name = format!("invert-export-{}.lua", std::process::id());
        std::fs::write(
            scripts.join(&script_name),
            "for x = 0, frame.width - 1 do
                 local r, g, b, a = frame.get(x, 0)
                 frame.set(x, 0, 255 - r, 255 - g, 255 - b, a)
             end",
        )
        .unwrap();
        let mut p = project();
        let clip_id = p.tracks[0].clips[0].id;
        apply(
            &mut p,
            &Command::SetClipScriptEffect {
                clip_id,
                effect: Some(studio_core::ScriptEffect {
                    script: script_name.clone(),
                    params: Default::default(),
                }),
            },
        )
        .unwrap();
        let frame = dir.join("f000.png");
        image::RgbaImage::from_raw(2, 1, vec![10, 20, 30, 255, 200, 150, 100, 255])
            .unwrap()
            .save(&frame)
            .unwrap();
        let mut map = BTreeMap::new();
        map.insert("demo/opening.mp4".to_string(), vec![frame.clone()]);
        let store = FrameStore::from_frames(10, map);
        let preview_store = FrameStore::from_frames(10, BTreeMap::new());
        let black = dir.join("black.png");
        image::RgbaImage::from_raw(2, 1, vec![0, 0, 0, 255, 0, 0, 0, 255])
            .unwrap()
            .save(&black)
            .unwrap();
        let out = compose_export_frame(
            &p,
            &store,
            &preview_store,
            0,
            &ExportSettings::default(),
            &dir.join("work"),
            &black,
        );
        let exported = image::open(&out).unwrap().to_rgba8();
        assert_eq!(
            exported.as_raw(),
            &vec![245, 235, 225, 255, 55, 105, 155, 255],
            "書き出しフレームにエフェクトが反映される"
        );
        // プレビュー経路とピクセル一致
        let clip = &p.tracks[0].clips[0];
        let preview_out = apply_script_effect(&frame, clip, 0, &dir.join("pv")).unwrap();
        let preview = image::open(&preview_out).unwrap().to_rgba8();
        assert_eq!(preview.as_raw(), exported.as_raw());
        let _ = std::fs::remove_file(scripts.join(&script_name));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn script_effect_params_reach_script() {
        let dir = fx_dir("params");
        let scripts = dir.join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(
            scripts.join("level.lua"),
            "frame.set(0, 0, params.level, 0, 0, 255)",
        )
        .unwrap();
        let base = dir.join("base.png");
        image::RgbaImage::from_raw(1, 1, vec![0, 0, 0, 255])
            .unwrap()
            .save(&base)
            .unwrap();
        let mut clip = clip_with_effect("level.lua");
        clip.script_effect
            .as_mut()
            .unwrap()
            .params
            .insert("level".to_string(), studio_core::EffectParam::Number(9.0));
        let derived =
            apply_script_effect_with_dirs(&base, &clip, 0, &scripts, &dir.join("out")).unwrap();
        let img = image::open(&derived).unwrap().to_rgba8();
        assert_eq!(img.as_raw(), &vec![9, 0, 0, 255]);
        std::fs::remove_dir_all(&dir).unwrap();
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
