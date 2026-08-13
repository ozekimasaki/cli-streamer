# Changelog

## [0.2.0] - 2026-08-13

### Added

- Wayland: xdg-desktop-portal ScreenCast（WINDOW）でウィンドウ単体キャプチャ
- PyGObject + GStreamer `pipewiresrc` → Y4M → ffmpeg の既存ツール橋渡し
- 表示サーバ自動判定（X11 / Wayland）。Wayland では Xwayland にフォールバックしない

### Changed

- `doctor` / `start` / 対話がセッション種別で分岐
- Wayland では `--window` 不要（OS 共有ダイアログ）
- `list-windows` は Wayland では案内のみ

## [0.1.0] - 2026-08-13

### Added

- Linux/X11 向けの軽量 ffmpeg ラッパー CLI
- YouTube / Twitch / Kick への RTMP(S) 配信（エンコード1回 + tee）
- ウィンドウ選択（wmctrl）と音声ソース選択（pactl）
- 対話モードと非対話 `start` / `list-*` / `init` / `doctor`
- `cargo install --git` 向けのメタデータと GitHub Actions CI
