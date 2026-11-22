# Issue #52 試作の再現手順（使い捨て。本体コードではない）

作業ディレクトリ /tmp/proto52 で実行した実際のコマンド。素材はデモ素材と
同じ生成条件（testsrc2）で、音声は2つの正弦波をミックスしたもの。

## 1. 素材の生成

```sh
ffmpeg -y -loglevel error -f lavfi -i testsrc2=duration=4:size=1280x720:rate=30 -c:v libx264 -pix_fmt yuv420p src-a.mp4
ffmpeg -y -loglevel error -f lavfi -i testsrc2=duration=6:size=1280x720:rate=30 -vf hue=h=90 -c:v libx264 -pix_fmt yuv420p src-b.mp4
ffmpeg -y -loglevel error -f lavfi -i sine=frequency=440:duration=8 -ar 48000 bgm.wav
ffmpeg -y -loglevel error -f lavfi -i sine=frequency=660:duration=8 -ar 48000 se.wav
```

## 2. 音声ミックス（2トラック相当。440Hz を 0.8、660Hz を 0.4、ステレオ、5秒）

```sh
ffmpeg -y -loglevel error -i bgm.wav -i se.wav -filter_complex "[0:a]atrim=0:5,volume=0.8,aformat=channel_layouts=stereo[a0];[1:a]atrim=0:5,volume=0.4,aformat=channel_layouts=stereo[a1];[a0][a1]amix=inputs=2:duration=first:normalize=0[mix]" -map "[mix]" mix-stereo.wav
```

（初回はモノラルで生成し、レビュー指摘を受けてステレオで作り直した。
最終的な export.mp4 はこのステレオ版を使う。）

## 3. 素材フレームの展開（30fps）

```sh
ffmpeg -y -loglevel error -i src-a.mp4 -vf fps=30 fa/f%03d.png   # 120枚
ffmpeg -y -loglevel error -i src-b.mp4 -vf fps=30 fb/f%03d.png   # 180枚
```

## 4. タイムラインのフレーム列レンダリング（150枚）

```sh
python3 render_frames.py   # 同ディレクトリに置いた証拠スクリプト
```

タイムライン: A = src-a を 0–2000ms、B = src-b を 2000–5000ms、
B の先頭 1000ms がクロスディゾルブ窓（ADR-0014 の提案方式）。

## 5. エンコード

```sh
ffmpeg -y -loglevel error -framerate 30 -i frames/f%03d.png -i mix-stereo.wav -c:v libx264 -pix_fmt yuv420p -r 30 -c:a aac -b:a 192k -ar 48000 -movflags +faststart -shortest export.mp4
```

## 6. 検証

```sh
ffprobe -v error -show_entries stream=codec_type,codec_name,width,height,avg_frame_rate,sample_rate,channels -show_entries format=duration,size -of default=noprint_wrappers=1 export.mp4
ffmpeg -y -loglevel error -ss 2.5 -i export.mp4 -frames:v 1 check-mid.png
ffmpeg -y -loglevel error -ss 1.0 -i export.mp4 -frames:v 1 check-a.png
```

結果は ffprobe-export.txt のとおり（H.264 1280×720 30fps、AAC 48kHz
ステレオ、5.000 秒）。check-mid.png の画素 (8,8) は (28,18,0) で、
α=0.5 合成フレーム composited-2500ms.png の (30,20,0) と符号化誤差の
範囲で一致する。
