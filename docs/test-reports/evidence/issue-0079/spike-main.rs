// Issue #79 spike (throwaway): embed Lua 5.1 via mlua (vendored),
// parse ExEdit-style parameter directives, run an .anm-like script
// against a mock `obj` table backed by a real RGBA pixel buffer.
use mlua::{Lua, Table, Value};
use std::cell::RefCell;
use std::rc::Rc;

const W: usize = 64;
const H: usize = 36;

// A script modeled on real .anm files (directives + obj.* calls).
// The effect name is injected as raw SJIS bytes by the host (see below).
const SCRIPT_BODY: &str = r#"
obj.load("figure", FIGURE_NAME, col, obj.track1)
for y = 0, obj.h - 1 do
  for x = 0, obj.w - 1 do
    local r, g, b, a = obj.getpixel(x, y, "rgb")
    if obj.check0 then
      obj.putpixel(x, y, 255 - r, g, b, a)
    end
  end
end
obj.effect(EFFECT_NAME, PARAM_NAME, 100 + obj.track0)
obj.draw(obj.track0 * obj.time, 0)
"#;

fn parse_directives(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in src.lines() {
        let line = line.trim();
        if line.starts_with("--track") || line.starts_with("--check") {
            out.push(format!("directive: {line}"));
        } else if let Some(rest) = line.strip_prefix("--dialog:") {
            out.push(format!("dialog: {rest}"));
        } else if !line.is_empty() && !line.starts_with("--") {
            break;
        }
    }
    out
}

fn main() -> mlua::Result<()> {
    // 1) Directives are declared in comment lines at the top of the file.
    let header = "--track0:速度,-100,100,10\n--track1:サイズ,10,200,64,1\n--check0:反転,1\n--dialog:色/col,col=0xff0000;\n";
    for d in parse_directives(header) {
        println!("{d}");
    }

    // 2) Pixel buffer (RGBA8), starts as a deterministic gradient.
    let buf: Rc<RefCell<Vec<u8>>> = Rc::new(RefCell::new(vec![0u8; W * H * 4]));
    {
        let mut b = buf.borrow_mut();
        for y in 0..H {
            for x in 0..W {
                let i = (y * W + x) * 4;
                b[i] = (x * 4) as u8;
                b[i + 1] = (y * 7) as u8;
                b[i + 2] = 128;
                b[i + 3] = 255;
            }
        }
    }
    let calls: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

    let lua = Lua::new();
    let obj = lua.create_table()?;
    obj.set("ox", 0.0)?;
    obj.set("oy", 0.0)?;
    obj.set("time", 0.5)?;
    obj.set("frame", 15)?;
    obj.set("w", W as i64)?;
    obj.set("h", H as i64)?;
    obj.set("track0", 10.0)?; // from --track0 default
    obj.set("track1", 64.0)?; // from --track1 default
    obj.set("check0", true)?; // from --check0 default 1

    // obj.load("figure", name, color, size): record + fill buffer with color.
    {
        let calls = calls.clone();
        let buf = buf.clone();
        obj.set(
            "load",
            lua.create_function(move |_, args: mlua::MultiValue| {
                let kind = match args.front() {
                    Some(Value::String(s)) => s.to_str()?.to_string(),
                    _ => "?".into(),
                };
                calls.borrow_mut().push(format!("load kind={kind}"));
                if kind == "figure" {
                    // 4th arg in this mock call pattern is the color value
                    // passed via the dialog variable `col`.
                    let mut b = buf.borrow_mut();
                    for px in b.chunks_exact_mut(4) {
                        px[0] = 0x11;
                        px[1] = 0x22;
                        px[2] = 0x33;
                        px[3] = 0xff;
                    }
                }
                Ok(())
            })?,
        )?;
    }
    // obj.getpixel / obj.putpixel over the RGBA buffer ("rgb" type).
    {
        let buf = buf.clone();
        obj.set(
            "getpixel",
            lua.create_function(move |_, (x, y, _t): (i64, i64, Option<String>)| {
                let b = buf.borrow();
                let i = ((y as usize * W + x as usize) * 4).min(b.len() - 4);
                Ok((b[i] as i64, b[i + 1] as i64, b[i + 2] as i64, b[i + 3] as i64))
            })?,
        )?;
    }
    {
        let buf = buf.clone();
        obj.set(
            "putpixel",
            lua.create_function(
                move |_, (x, y, r, g, bl, a): (i64, i64, i64, i64, i64, i64)| {
                    let mut b = buf.borrow_mut();
                    let i = (y as usize * W + x as usize) * 4;
                    if i + 3 < b.len() {
                        b[i] = r as u8;
                        b[i + 1] = g as u8;
                        b[i + 2] = bl as u8;
                        b[i + 3] = a as u8;
                    }
                    Ok(())
                },
            )?,
        )?;
    }
    // obj.effect(name, param, value): record raw bytes of the name.
    {
        let calls = calls.clone();
        obj.set(
            "effect",
            lua.create_function(move |_, args: mlua::MultiValue| {
                if let Some(Value::String(s)) = args.front() {
                    let bytes = s.as_bytes();
                    calls.borrow_mut().push(format!(
                        "effect name_bytes={:02x?} param_count={}",
                        bytes,
                        args.len() - 1
                    ));
                }
                Ok(())
            })?,
        )?;
    }
    {
        let calls = calls.clone();
        obj.set(
            "draw",
            lua.create_function(move |_, args: mlua::MultiValue| {
                calls.borrow_mut().push(format!("draw argc={}", args.len()));
                Ok(())
            })?,
        )?;
    }
    lua.globals().set("obj", obj)?;

    // Dialog variable `col` (from --dialog), figure name and effect name
    // are given to the script as raw SJIS byte strings, exactly like
    // ExEdit passes SJIS text through Lua 5.1 byte strings.
    lua.globals().set("col", 0xff0000i64)?;
    let sjis_en = [0x89u8, 0x7e]; // 円 in SJIS
    let sjis_effect = [0x90u8, 0x46, 0x92, 0xb2, 0x95, 0xe2, 0x90, 0xb3]; // 色調補正
    let sjis_param = [0x96u8, 0xbe, 0x82, 0xe9, 0x82, 0xb3]; // 明るさ
    lua.globals()
        .set("FIGURE_NAME", lua.create_string(sjis_en)?)?;
    lua.globals()
        .set("EFFECT_NAME", lua.create_string(sjis_effect)?)?;
    lua.globals()
        .set("PARAM_NAME", lua.create_string(sjis_param)?)?;

    lua.load(SCRIPT_BODY).exec()?;

    println!("--- recorded host calls (in order) ---");
    for c in calls.borrow().iter() {
        println!("{c}");
    }
    let b = buf.borrow();
    let checksum: u64 = b.iter().map(|v| *v as u64).sum();
    println!("buffer checksum after run: {checksum}");
    println!(
        "pixel(0,0) = ({},{},{},{})  (figure fill 0x11,0x22,0x33 inverted R by check0 -> expect (238,34,51,255))",
        b[0], b[1], b[2], b[3]
    );
    let lua_version: String = lua.globals().get::<Table>("_G")?.get("_VERSION")?;
    println!("Lua version: {lua_version}");
    Ok(())
}
