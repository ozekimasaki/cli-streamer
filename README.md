# cli-streamer

Lightweight ffmpeg wrapper for live streaming to **YouTube / Twitch / Kick** on **Linux**.

Supports **X11** (window list + `x11grab`) and **Wayland** (OS ScreenCast portal → window-only capture via PipeWire / GStreamer).

Encoding and upload are done by ffmpeg. This tool lists sources, reads config, and spawns helpers.

## Install

```bash
cargo install --git https://github.com/ozekimasaki/cli-streamer --locked
```

Ensure `~/.cargo/bin` is on your `PATH`. Requires **Rust 1.74+**. Not on crates.io (`publish = false`).

### Requirements (runtime)

**Common**

- Linux
- `ffmpeg` (`libx264`, `aac`, `rtmp`; `rtmps` for Kick)
- `pactl` (Pulse / PipeWire)

**X11**

- X11 session, `wmctrl`

**Wayland** (window-only, no overlapping windows)

- Wayland session + `xdg-desktop-portal` (+ DE portal, e.g. gnome/kde/wlr)
- `python3` with **PyGObject** (`Gio`)
- `gst-launch-1.0` and GStreamer **pipewiresrc** (`gstreamer1.0-pipewire` etc.)

```bash
cli-streamer doctor
```

### Limitations

- **X11:** `x11grab` captures a screen rectangle; overlapping windows may appear
- **Wayland:** window is chosen in the OS share dialog (no CLI window ID); portal session must stay alive while streaming
- Stream keys appear in process arguments (`ps`); keep the machine trusted
- No NVENC/VAAPI auto, audio mix, overlays, or OAuth

---

## 日本語

Linux 向けの**極薄** ffmpeg ラッパー。YouTube / Twitch / Kick へ RTMP(S) 配信する。

| セッション | ウィンドウ指定 | キャプチャ |
|-----------|----------------|------------|
| **X11** | `wmctrl` 番号 / `--window 0xid` | `x11grab`（矩形。重なりが映ることがある） |
| **Wayland** | OS の共有ダイアログ | ポータル → PipeWire → GStreamer → Y4M → ffmpeg（**ウィンドウ単体**） |

### インストール

```bash
cargo install --git https://github.com/ozekimasaki/cli-streamer --locked
```

開発（[Devbox](https://www.jetify.com/devbox)）:

```bash
devbox shell
devbox run build
devbox run test
```

Wayland の実キャプチャはホストの portal / PyGObject が必要です（Devbox だけでは不足することがあります）。

### 設定

```bash
cli-streamer init
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

**ストリームキーをリポジトリにコミットしないこと。**

### 使い方

```bash
cli-streamer doctor
cli-streamer              # 対話

# X11
cli-streamer list-windows
cli-streamer start --window 0x01234567 --audio alsa_input.usb-Mic --dest youtube,twitch

# Wayland（共有ダイアログが開く）
cli-streamer start --dest kick
cli-streamer start --dest youtube --dry-run
```

停止は **Ctrl+C**。

### ライセンス

MIT
