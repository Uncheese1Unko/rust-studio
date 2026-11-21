use signalsmith_stretch::Stretch;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (src_dir, out_dir) = (&args[1], &args[2]);
    std::fs::create_dir_all(format!("{out_dir}/signalsmith_formant_preserved")).unwrap();
    std::fs::create_dir_all(format!("{out_dir}/signalsmith_formant_shifted")).unwrap();
    let mut files: Vec<_> = std::fs::read_dir(src_dir).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    for f in files {
        let mut r = hound::WavReader::open(&f).unwrap();
        let spec = r.spec();
        let input: Vec<f32> = r.samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect();
        let name = f.file_stem().unwrap().to_string_lossy().to_string();
        for st in [4i32, 7, 12, -4, -7] {
            for (dir, preserve) in [("signalsmith_formant_preserved", true), ("signalsmith_formant_shifted", false)] {
                let mut s = Stretch::preset_default(1, spec.sample_rate);
                s.set_transpose_factor_semitones(st as f32, None);
                if preserve { s.set_formant_factor(1.0, true); }
                let mut out = vec![0f32; input.len()];
                s.exact(&input, &mut out);
                let p = format!("{out_dir}/{dir}/{name}_{st:+}st.wav");
                let mut w = hound::WavWriter::create(Path::new(&p), spec).unwrap();
                for v in out { w.write_sample((v.clamp(-1.0, 1.0) * 32767.0) as i16).unwrap(); }
                w.finalize().unwrap();
            }
        }
    }
}
