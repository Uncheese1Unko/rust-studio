//! 人物切り抜き（Issue #22）: MODNet（ONNX）によるオフライン処理。
//!
//! 方式は調査 #21 の推奨（MODNet、Apache-2.0）に従う。推論は
//! ONNX Runtime を `ort` クレート（load-dynamic）で読み込んで行い、
//! 処理はワーカースレッドで進める。進捗は共有状態（JobState）で
//! GUI に見せ、UI は固まらない。
//!
//! 実行には以下が要る（無い場合は「利用不可」として扱い、アプリは
//! 通常のプレビューを続ける）:
//! - モデルファイル: 環境変数 `STUDIO_MODNET_MODEL`、なければ
//!   メディア置き場の `modnet.onnx`
//! - libonnxruntime: 環境変数 `ORT_DYLIB_PATH`、なければ候補パスを探す

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::media;

/// 切り抜きジョブの状態。GUI はこれを読んで進捗を表示する。
#[derive(Debug, Clone, PartialEq)]
pub enum JobStatus {
    Running,
    Done,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct JobState {
    pub clip_id: u64,
    pub total: usize,
    pub done: usize,
    pub status: JobStatus,
}

/// マスク列の置き場名（フレーム列の置き場名に -mask を付ける）。
pub fn mask_dir_name(source: &str) -> String {
    format!("{}-mask", media::frames_dir_name(source))
}

/// 合成済みフレーム列の置き場名。
pub fn composite_dir_name(source: &str) -> String {
    format!("{}-cutout", media::frames_dir_name(source))
}

/// 1画素の合成: fg * alpha + bg * (1 - alpha)。
pub fn composite_pixel(fg: [u8; 3], bg: [u8; 3], alpha: f32) -> [u8; 3] {
    let a = alpha.clamp(0.0, 1.0);
    let mut out = [0u8; 3];
    for i in 0..3 {
        out[i] = (fg[i] as f32 * a + bg[i] as f32 * (1.0 - a)).round() as u8;
    }
    out
}

/// RGB 画像（HWC, u8）を MODNet の入力（CHW, f32, -1..1）に変換する。
/// リサイズは呼び出し側で行う。
pub fn normalize_input(rgb: &[u8], width: usize, height: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; 3 * width * height];
    for c in 0..3 {
        for i in 0..width * height {
            out[c * width * height + i] = (rgb[i * 3 + c] as f32 / 255.0 - 0.5) / 0.5;
        }
    }
    out
}

/// モデルファイルの場所を解決する。
pub fn find_model() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("STUDIO_MODNET_MODEL") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    let candidate = media::media_dir().join("modnet.onnx");
    if candidate.exists() {
        return Some(candidate);
    }
    None
}

/// libonnxruntime の場所を解決する。
pub fn find_onnxruntime() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("ORT_DYLIB_PATH") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    let mut candidates = vec![
        PathBuf::from("/usr/lib/x86_64-linux-gnu/libonnxruntime.so"),
        PathBuf::from("/usr/local/lib/libonnxruntime.so"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(
            PathBuf::from(home).join(
                ".local/lib/python3.12/site-packages/onnxruntime/capi/libonnxruntime.so.1.32.0",
            ),
        );
    }
    candidates.into_iter().find(|p| p.exists())
}

/// 切り抜き処理の入力一式。
pub struct JobRequest {
    pub clip_id: u64,
    /// 前景フレーム列（時刻順）。
    pub frames: Vec<PathBuf>,
    /// 背景フレーム列（frames と同じ長さ。無い区間は None = 単色背景）。
    pub bg_frames: Vec<Option<PathBuf>>,
    pub mask_dir: PathBuf,
    pub composite_dir: PathBuf,
    pub model_path: PathBuf,
    pub dylib_path: PathBuf,
}

/// 背景の単色（背景クリップが無い区間の差し替え色）。
pub const FALLBACK_BG: [u8; 3] = [24, 40, 72];

/// ジョブを開始し、共有状態を返す。実際の推論はワーカースレッドで行う。
pub fn start_job(request: JobRequest) -> Arc<Mutex<JobState>> {
    let state = Arc::new(Mutex::new(JobState {
        clip_id: request.clip_id,
        total: request.frames.len(),
        done: 0,
        status: JobStatus::Running,
    }));
    let shared = Arc::clone(&state);
    std::thread::spawn(move || {
        let result = run_job(&request, &shared);
        let mut guard = shared.lock().unwrap();
        match result {
            Ok(()) => {
                guard.done = guard.total;
                guard.status = JobStatus::Done;
            }
            Err(e) => {
                guard.status = JobStatus::Failed(e);
            }
        }
    });
    state
}

fn run_job(request: &JobRequest, state: &Arc<Mutex<JobState>>) -> Result<(), String> {
    std::fs::create_dir_all(&request.mask_dir).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&request.composite_dir).map_err(|e| e.to_string())?;

    let env = ort::init_from(&request.dylib_path)
        .map_err(|e| format!("onnxruntime load failed: {e}"))?
        .with_name("rust-studio-cutout")
        .build()
        .map_err(|e| format!("onnxruntime init failed: {e}"))?;
    let mut session = ort::session::Session::builder(&env)
        .map_err(|e| e.to_string())?
        .commit_from_file(&request.model_path)
        .map_err(|e| format!("model load failed: {e}"))?;

    for (i, frame_path) in request.frames.iter().enumerate() {
        process_frame(
            &mut session,
            frame_path,
            request.bg_frames.get(i).and_then(|p| p.as_deref()),
            &request.mask_dir.join(format!("f{i:03}.png")),
            &request.composite_dir.join(format!("f{i:03}.png")),
        )?;
        state.lock().unwrap().done = i + 1;
    }
    Ok(())
}

/// 1フレーム分の推論と合成。
fn process_frame(
    session: &mut ort::session::Session,
    frame_path: &std::path::Path,
    bg_path: Option<&std::path::Path>,
    mask_out: &std::path::Path,
    composite_out: &std::path::Path,
) -> Result<(), String> {
    let fg = image::open(frame_path)
        .map_err(|e| e.to_string())?
        .to_rgb8();
    let (w, h) = (fg.width() as usize, fg.height() as usize);
    // MODNet 入力: 512x512 にリサイズして正規化
    let small = image::imageops::resize(&fg, 512, 512, image::imageops::FilterType::Triangle);
    let input = normalize_input(&small, 512, 512);
    let tensor = ort::value::Tensor::from_array(
        ndarray::Array4::from_shape_vec((1, 3, 512, 512), input).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let outputs = session
        .run(ort::inputs![tensor])
        .map_err(|e| format!("inference failed: {e}"))?;
    let (_shape, data) = outputs[0]
        .try_extract_tensor::<f32>()
        .map_err(|e| e.to_string())?;
    if data.len() != 512 * 512 {
        return Err(format!("unexpected model output size: {}", data.len()));
    }
    // アルファを元解像度へ戻す
    let alpha_small = image::GrayImage::from_fn(512, 512, |x, y| {
        image::Luma([(data[(y * 512 + x) as usize].clamp(0.0, 1.0) * 255.0) as u8])
    });
    let alpha = image::imageops::resize(
        &alpha_small,
        w as u32,
        h as u32,
        image::imageops::FilterType::Triangle,
    );
    alpha.save(mask_out).map_err(|e| e.to_string())?;

    // 背景と合成
    let bg_img = match bg_path {
        Some(path) => image::open(path).map_err(|e| e.to_string())?.to_rgb8(),
        None => image::RgbImage::from_pixel(w as u32, h as u32, image::Rgb(FALLBACK_BG)),
    };
    let mut out = image::RgbImage::new(w as u32, h as u32);
    for y in 0..h {
        for x in 0..w {
            let a = alpha.get_pixel(x as u32, y as u32)[0] as f32 / 255.0;
            let fgp = fg.get_pixel(x as u32, y as u32).0;
            let bgp = if (x as u32) < bg_img.width() && (y as u32) < bg_img.height() {
                bg_img.get_pixel(x as u32, y as u32).0
            } else {
                FALLBACK_BG
            };
            out.put_pixel(x as u32, y as u32, image::Rgb(composite_pixel(fgp, bgp, a)));
        }
    }
    out.save(composite_out).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite_pixel_blends_by_alpha() {
        assert_eq!(
            composite_pixel([200, 100, 0], [0, 100, 200], 1.0),
            [200, 100, 0]
        );
        assert_eq!(
            composite_pixel([200, 100, 0], [0, 100, 200], 0.0),
            [0, 100, 200]
        );
        assert_eq!(
            composite_pixel([200, 100, 0], [0, 100, 200], 0.5),
            [100, 100, 100]
        );
        // 範囲外のアルファはクランプされる
        assert_eq!(
            composite_pixel([200, 100, 0], [0, 100, 200], 1.5),
            [200, 100, 0]
        );
    }

    #[test]
    fn normalize_input_maps_to_minus_one_to_one_chw() {
        // 2x1 画像: 赤画素と青画素
        let rgb = [255u8, 0, 0, 0, 0, 255];
        let out = normalize_input(&rgb, 2, 1);
        assert_eq!(out.len(), 3 * 2);
        // R 平面: [1.0, -1.0], G 平面: [-1.0, -1.0], B 平面: [-1.0, 1.0]
        assert!((out[0] - 1.0).abs() < 1e-6);
        assert!((out[1] + 1.0).abs() < 1e-6);
        assert!((out[2] + 1.0).abs() < 1e-6);
        assert!((out[5] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn dir_names_derive_from_frames_dir() {
        let source = "demo/interview.mp4";
        assert!(mask_dir_name(source).ends_with("-mask"));
        assert!(composite_dir_name(source).ends_with("-cutout"));
        assert_ne!(mask_dir_name(source), composite_dir_name(source));
        // 別ディレクトリの同名素材と衝突しない（フレーム置き場と同じ性質）
        assert_ne!(mask_dir_name("a/x.mp4"), mask_dir_name("b/x.mp4"));
    }
}
