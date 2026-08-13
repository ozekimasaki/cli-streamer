# cli-streamer

Lightweight ffmpeg wrapper for live streaming to **YouTube / Twitch / Kick** on **Linux X11**.

This tool only lists windows/audio, reads config, and spawns `ffmpeg`. Encoding and upload are done by ffmpeg.

## Install

```bash
cargo install --git https://github.com/ozekimasaki/cli-streamer --locked
```

Ensure `~/.cargo/bin` is on your `PATH`.

Requires **Rust 1.74+**. Not published to crates.io (`publish = false`).

### Requirements (runtime)

- Linux **X11** session (Wayland not supported)
- `ffmpeg` with `x11grab`, `pulse`, `libx264`, `aac`, `rtmp` (and `rtmps` for Kick)
- `wmctrl`, `pactl`

```bash
cli-streamer doctor
```

### Limitations

- `x11grab` captures a screen rectangle; overlapping windows may appear
- Stream keys appear in process arguments (`ps`); keep the machine trusted
- No Wayland, NVENC/VAAPI auto, audio mix, overlays, or OAuth

---

## 日本語

Linux/X11 向けの**極薄** ffmpeg ラッパー。ウィンドウと音声を選び、YouTube / Twitch / Kick へ RTMP(S) 配信する。

### インストール

```bash
cargo install --git https://github.com/ozekimasaki/cli-streamer --locked
```

開発（[Devbox](https://www.jetify.com/devbox) 推奨）:

```bash
devbox shell
devbox run build   # target/release/cli-streamer
devbox run test
```

### 設定

```bash
cli-streamer init
# ~/.config/cli-streamer/config を編集
chmod 600 ~/.config/cli-streamer/config
```

```
youtube_key=YOUR_YT_KEY
twitch_key=YOUR_TWITCH_KEY
kick_url=rtmps://xxxxxxxx.global-contribute.live-video.net
kick_key=YOUR_KICK_KEY

bitrate=4500k
fps=30
```

- **YouTube:** Studio のストリームキー
- **Twitch:** 配信のプライマリキー（ingest は `live.twitch.tv`）
- **Kick:** ダッシュボードのアカウント固有 Stream URL と Key（ホストだけで可。`:443/app` は自動付与）

パスは `CLI_STREAMER_CONFIG` で上書き可能。**ストリームキーをリポジトリにコミットしないこと。**

画面上のコマンド表示ではキーをマスクする。ffmpeg のログは `-loglevel error` で URL を出しにくくしている。ただし同じマシンの `ps` では引数にキーが見える。

### 使い方

```bash
cli-streamer doctor
cli-streamer              # 対話（番号選択）
cli-streamer list-windows
cli-streamer list-audio
cli-streamer start --window 0x01234567 --audio alsa_input.usb-Mic --dest youtube,twitch
cli-streamer start --window 0x01234567 --dest kick --dry-run
```

配信中は ffmpeg の stderr（stats）を表示。停止は **Ctrl+C**。

### 動作の要点

- 映像: `x11grab` + `-window_id`
- 音声: Pulse ソースを **1つ**（省略時は `-an`）
- 映像: `libx264` veryfast CBR、キーフレーム 2 秒
- 音声: AAC 48 kHz stereo
- 複数先: エンコード 1 回 + ffmpeg `tee`

### ライセンス

MIT
