//! video-editor: AviUtl 風の動画編集ソフト（nannou）。
//!
//! 本ファイルは調査 PBI #2「nannou のビルドと最小ウィンドウ表示の検証」の
//! 最小アプリ。ウィンドウを開き、図形を1つ描くことだけを行う。
//! 本格的な GUI は M1 の後続 PBI（#8 以降）で実装する。
//! 状態の変更は studio-core のコマンド層経由でのみ行う（ADR-0004）。

use nannou::prelude::*;

struct Model {
    _window: WindowId,
}

fn main() {
    // アプリの状態は studio-core の Project が正（ADR-0004）。
    // この最小アプリでは起動確認としてダンプを出すだけだが、後続 PBI の
    // GUI はこの Project をコマンド経由で操作する。
    let project = studio_core::Project::new("untitled video");
    println!("video-editor (issue #2: nannou smoke)");
    println!("{}", project.dump_json());
    nannou::app(model).run();
}

fn model(app: &App) -> Model {
    let window = app
        .new_window()
        .size(640, 480)
        .title("video-editor (issue #2: nannou smoke)")
        .view(view)
        .build()
        .unwrap();
    Model { _window: window }
}

fn view(app: &App, _model: &Model, frame: Frame) {
    let draw = app.draw();
    draw.background().color(rgb(0.09, 0.09, 0.11));
    // 描画確認用の図形を1つ（楕円）描く。
    draw.ellipse()
        .x_y(0.0, 0.0)
        .w_h(180.0, 180.0)
        .color(rgb(0.88, 0.66, 0.11));
    draw.to_frame(app, &frame).unwrap();
}
