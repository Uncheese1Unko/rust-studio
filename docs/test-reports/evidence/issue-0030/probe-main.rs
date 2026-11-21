//! Issue #30 実験用アプリ（使い捨て）: nannou 0.19 で OS のファイル
//! ドロップイベントが取得できるか、ドロップ時にマウス位置が分かるか
//! を確認する。イベントはすべて標準出力に記録する。

use nannou::prelude::*;
use std::path::PathBuf;

struct Model {
    log: Vec<String>,
}

fn model(app: &App) -> Model {
    app.new_window()
        .size(960, 720)
        .title("dnd-probe")
        .hovered_file(hovered)
        .hovered_file_cancelled(hover_cancelled)
        .dropped_file(dropped)
        .view(view)
        .build()
        .unwrap();
    println!("probe ready");
    Model { log: Vec::new() }
}

fn hovered(_app: &App, model: &mut Model, path: PathBuf) {
    let line = format!("HOVERED: {}", path.display());
    println!("{line}");
    model.log.push(line);
}

fn hover_cancelled(_app: &App, model: &mut Model) {
    println!("HOVER CANCELLED");
    model.log.push("HOVER CANCELLED".into());
}

fn dropped(app: &App, model: &mut Model, path: PathBuf) {
    // ドロップ位置への変換にマウス位置が使えるかを確認するため、
    // コールバック時点の app.mouse も記録する（中心原点座標）。
    let line = format!(
        "DROPPED: {} mouse=({:.0}, {:.0})",
        path.display(),
        app.mouse.x,
        app.mouse.y
    );
    println!("{line}");
    model.log.push(line);
}

fn update(_app: &App, _model: &mut Model, _update: Update) {}

fn view(app: &App, model: &Model, frame: Frame) {
    let draw = app.draw();
    draw.background().color(rgb(0.10, 0.10, 0.12));
    let win = app.window_rect();
    draw.text("drop files here")
        .x_y(0.0, 100.0)
        .font_size(20)
        .color(rgb(0.9, 0.9, 0.9));
    for (i, line) in model.log.iter().enumerate() {
        draw.text(line)
            .x_y(-win.w() / 2.0 + 300.0, 40.0 - i as f32 * 22.0)
            .font_size(13)
            .color(rgb(0.6, 0.95, 0.6));
    }
    draw.to_frame(app, &frame).unwrap();
}

fn main() {
    nannou::app(model).update(update).run();
}
