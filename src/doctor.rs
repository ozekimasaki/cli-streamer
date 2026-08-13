//! 依存コマンド・環境・設定の事前チェック（doctor / start 共通）。

use crate::config::{Config, Destination};
use crate::session::{self, DisplayServer};
use std::env;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct CheckItem {
    pub ok: bool,
    pub message: String,
}

pub fn is_linux() -> bool {
    cfg!(target_os = "linux")
}

pub fn require_linux() -> Result<(), String> {
    if is_linux() {
        Ok(())
    } else {
        Err(format!(
            "cli-streamer は Linux（X11 または Wayland）専用です（現在の OS: {}）",
            env::consts::OS
        ))
    }
}

pub fn bin_on_path(name: &str) -> bool {
    which_via_type(name)
}

fn which_via_type(name: &str) -> bool {
    let Ok(path) = env::var("PATH") else {
        return false;
    };
    let ext = if cfg!(windows) { ".exe" } else { "" };
    for dir in env::split_paths(&path) {
        let candidate = dir.join(format!("{name}{ext}"));
        if candidate.is_file() {
            return true;
        }
    }
    false
}

fn ffmpeg_output(args: &[&str]) -> Option<String> {
    let output = Command::new("ffmpeg").args(args).output().ok()?;
    let mut s = String::from_utf8_lossy(&output.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&output.stderr));
    Some(s)
}

fn ffmpeg_has_protocol(name: &str) -> bool {
    ffmpeg_output(&["-hide_banner", "-protocols"])
        .map(|t| {
            t.lines()
                .any(|l| l.trim() == name || l.split_whitespace().any(|w| w == name))
        })
        .unwrap_or(false)
}

fn ffmpeg_has_encoder(name: &str) -> bool {
    ffmpeg_output(&["-hide_banner", "-encoders"])
        .map(|t| t.contains(name))
        .unwrap_or(false)
}

fn python_has_gio() -> bool {
    Command::new("python3")
        .args([
            "-c",
            "import gi; gi.require_version('Gio','2.0'); from gi.repository import Gio",
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn gst_has_pipewiresrc() -> bool {
    Command::new("gst-inspect-1.0")
        .arg("pipewiresrc")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn portal_desktop_on_bus() -> bool {
    // セッションバスにポータルがあるか（失敗しても doctor は続行）
    Command::new("gdbus")
        .args([
            "introspect",
            "--session",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or_else(|_| {
            Command::new("busctl")
                .args(["--user", "status", "org.freedesktop.portal.Desktop"])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        })
}

/// window_id は 0x + 16進。
pub fn validate_window_id(id: &str) -> Result<(), String> {
    let id = id.trim();
    let hex = id
        .strip_prefix("0x")
        .or_else(|| id.strip_prefix("0X"))
        .ok_or_else(|| format!("ウィンドウ ID は 0x で始まる必要があります: {id}"))?;
    if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("ウィンドウ ID の形式が不正です: {id}"));
    }
    Ok(())
}

pub fn validate_bitrate(s: &str) -> Result<(), String> {
    let s = s.trim();
    let lower = s.to_ascii_lowercase();
    let (num, _unit) = if let Some(n) = lower.strip_suffix('k') {
        (n, 'k')
    } else if let Some(n) = lower.strip_suffix('m') {
        (n, 'm')
    } else {
        return Err(format!(
            "bitrate は 4500k / 6m のような形式にしてください: {s}"
        ));
    };
    if num.is_empty() || !num.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("bitrate の数値が不正です: {s}"));
    }
    Ok(())
}

pub fn validate_fps(fps: u32) -> Result<(), String> {
    if (1..=120).contains(&fps) {
        Ok(())
    } else {
        Err(format!("fps は 1〜120 にしてください: {fps}"))
    }
}

pub fn validate_config_values(cfg: &Config) -> Result<(), String> {
    validate_fps(cfg.fps)?;
    validate_bitrate(&cfg.bitrate)?;
    Ok(())
}

#[cfg(unix)]
fn config_permissions_ok(path: &std::path::Path) -> Option<bool> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    let meta = fs::metadata(path).ok()?;
    let mode = meta.permissions().mode();
    Some(mode & 0o077 == 0)
}

#[cfg(not(unix))]
fn config_permissions_ok(_path: &std::path::Path) -> Option<bool> {
    None
}

fn push_common_ffmpeg(items: &mut Vec<CheckItem>, need_rtmps: bool) {
    for bin in ["ffmpeg", "pactl"] {
        let ok = bin_on_path(bin);
        items.push(CheckItem {
            ok,
            message: if ok {
                format!("{bin}: 見つかりました")
            } else {
                format!("{bin}: PATH にありません")
            },
        });
    }

    if bin_on_path("ffmpeg") {
        let rtmp = ffmpeg_has_protocol("rtmp");
        items.push(CheckItem {
            ok: rtmp,
            message: if rtmp {
                "ffmpeg protocol: rtmp".into()
            } else {
                "ffmpeg に rtmp プロトコルがありません".into()
            },
        });
        if need_rtmps {
            let rtmps = ffmpeg_has_protocol("rtmps");
            items.push(CheckItem {
                ok: rtmps,
                message: if rtmps {
                    "ffmpeg protocol: rtmps".into()
                } else {
                    "ffmpeg に rtmps がありません（Kick 配信に必要）".into()
                },
            });
        }
        let x264 = ffmpeg_has_encoder("libx264");
        items.push(CheckItem {
            ok: x264,
            message: if x264 {
                "ffmpeg encoder: libx264".into()
            } else {
                "ffmpeg に libx264 がありません".into()
            },
        });
        let aac = ffmpeg_has_encoder("aac");
        items.push(CheckItem {
            ok: aac,
            message: if aac {
                "ffmpeg encoder: aac".into()
            } else {
                "ffmpeg に aac エンコーダがありません".into()
            },
        });
    }
}

fn push_x11_deps(items: &mut Vec<CheckItem>) {
    let display = env::var("DISPLAY");
    items.push(CheckItem {
        ok: display.as_ref().map(|d| !d.is_empty()).unwrap_or(false),
        message: match display {
            Ok(d) if !d.is_empty() => format!("DISPLAY={d}"),
            _ => "DISPLAY が未設定（X11 に必要）".into(),
        },
    });
    let ok = bin_on_path("wmctrl");
    items.push(CheckItem {
        ok,
        message: if ok {
            "wmctrl: 見つかりました".into()
        } else {
            "wmctrl: PATH にありません（X11 のウィンドウ一覧に必要）".into()
        },
    });
}

fn push_wayland_deps(items: &mut Vec<CheckItem>) {
    let py = bin_on_path("python3");
    items.push(CheckItem {
        ok: py,
        message: if py {
            "python3: 見つかりました".into()
        } else {
            "python3: PATH にありません（Wayland ポータルに必要）".into()
        },
    });
    if py {
        let gio = python_has_gio();
        items.push(CheckItem {
            ok: gio,
            message: if gio {
                "PyGObject Gio: OK".into()
            } else {
                "PyGObject Gio がありません（python3-gi / PyGObject を入れてください）".into()
            },
        });
    }
    let gst = bin_on_path("gst-launch-1.0");
    items.push(CheckItem {
        ok: gst,
        message: if gst {
            "gst-launch-1.0: 見つかりました".into()
        } else {
            "gst-launch-1.0: PATH にありません".into()
        },
    });
    if bin_on_path("gst-inspect-1.0") {
        let pw = gst_has_pipewiresrc();
        items.push(CheckItem {
            ok: pw,
            message: if pw {
                "gstreamer pipewiresrc: OK".into()
            } else {
                "gstreamer に pipewiresrc がありません（gstreamer1.0-pipewire 等）".into()
            },
        });
    } else {
        items.push(CheckItem {
            ok: false,
            message: "gst-inspect-1.0: PATH にありません".into(),
        });
    }
    let portal = portal_desktop_on_bus();
    items.push(CheckItem {
        ok: portal,
        message: if portal {
            "xdg-desktop-portal: セッションバス上にあります".into()
        } else {
            "org.freedesktop.portal.Desktop が見つかりません（xdg-desktop-portal を入れてください）"
                .into()
        },
    });
}

fn push_config(items: &mut Vec<CheckItem>) {
    let path = Config::config_path();
    if path.exists() {
        items.push(CheckItem {
            ok: true,
            message: format!("設定ファイル: {}", path.display()),
        });
        match config_permissions_ok(&path) {
            Some(true) => items.push(CheckItem {
                ok: true,
                message: "設定ファイルの権限: 所有者のみ（OK）".into(),
            }),
            Some(false) => items.push(CheckItem {
                ok: false,
                message: format!(
                    "設定ファイルの権限が緩いです。`chmod 600 {}` を実行してください",
                    path.display()
                ),
            }),
            None => {}
        }
        match Config::load() {
            Ok(cfg) => {
                match validate_config_values(&cfg) {
                    Ok(()) => items.push(CheckItem {
                        ok: true,
                        message: format!("bitrate={} fps={}", cfg.bitrate, cfg.fps),
                    }),
                    Err(e) => items.push(CheckItem {
                        ok: false,
                        message: e,
                    }),
                }
                let avail = cfg.available_destinations();
                if avail.is_empty() {
                    items.push(CheckItem {
                        ok: false,
                        message: "配信キーが未設定です（youtube_key / twitch_key / kick_*）".into(),
                    });
                } else {
                    let names: Vec<&str> = avail.iter().map(|d| d.name()).collect();
                    items.push(CheckItem {
                        ok: true,
                        message: format!("配信先キー: {}", names.join(", ")),
                    });
                }
            }
            Err(e) => items.push(CheckItem {
                ok: false,
                message: format!("設定の読み込み失敗: {e}"),
            }),
        }
    } else {
        items.push(CheckItem {
            ok: false,
            message: format!(
                "設定ファイルがありません。`cli-streamer init` で作成: {}",
                path.display()
            ),
        });
    }
}

/// doctor 用の点検項目を返す。
pub fn run_doctor(need_rtmps: bool) -> Vec<CheckItem> {
    let mut items = Vec::new();

    items.push(CheckItem {
        ok: is_linux(),
        message: if is_linux() {
            "OS: Linux".into()
        } else {
            format!("OS: {}（Linux が必要）", env::consts::OS)
        },
    });

    match session::detect() {
        Ok(ds) => {
            items.push(CheckItem {
                ok: true,
                message: format!("表示サーバ: {}", ds.name()),
            });
            match ds {
                DisplayServer::X11 => push_x11_deps(&mut items),
                DisplayServer::Wayland => push_wayland_deps(&mut items),
            }
        }
        Err(e) => items.push(CheckItem {
            ok: false,
            message: e,
        }),
    }

    push_common_ffmpeg(&mut items, need_rtmps);
    push_config(&mut items);
    items
}

fn preflight_common(destinations: &[Destination]) -> Result<(), String> {
    require_linux()?;
    if !bin_on_path("ffmpeg") {
        return Err("ffmpeg が PATH にありません".into());
    }
    if !bin_on_path("pactl") {
        return Err("pactl が PATH にありません".into());
    }
    if !ffmpeg_has_protocol("rtmp") {
        return Err("ffmpeg に rtmp プロトコルがありません".into());
    }
    if destinations.contains(&Destination::Kick) && !ffmpeg_has_protocol("rtmps") {
        return Err("Kick には rtmps が必要です".into());
    }
    if !ffmpeg_has_encoder("libx264") {
        return Err("ffmpeg に libx264 がありません".into());
    }
    if !ffmpeg_has_encoder("aac") {
        return Err("ffmpeg に aac エンコーダがありません".into());
    }
    let cfg = Config::load().map_err(|e| e.to_string())?;
    validate_config_values(&cfg)?;
    #[cfg(unix)]
    {
        let path = Config::config_path();
        if path.exists() {
            if let Some(false) = config_permissions_ok(&path) {
                return Err(format!(
                    "設定ファイルの権限が緩いです。`chmod 600 {}`",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

/// start / 対話の直前に必須条件だけ検査する。
pub fn preflight(destinations: &[Destination]) -> Result<(), String> {
    preflight_common(destinations)?;
    let ds = session::detect()?;
    match ds {
        DisplayServer::X11 => {
            if env::var("DISPLAY").map(|d| d.is_empty()).unwrap_or(true) {
                return Err("DISPLAY が未設定です。X11 セッションで実行してください".into());
            }
            if !bin_on_path("wmctrl") {
                return Err("wmctrl が PATH にありません".into());
            }
        }
        DisplayServer::Wayland => {
            if !bin_on_path("python3") {
                return Err("python3 が PATH にありません（Wayland に必要）".into());
            }
            if !python_has_gio() {
                return Err("PyGObject Gio がありません（python3-gi）".into());
            }
            if !bin_on_path("gst-launch-1.0") {
                return Err("gst-launch-1.0 が PATH にありません".into());
            }
            if !gst_has_pipewiresrc() {
                return Err("gstreamer に pipewiresrc がありません".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_id_ok() {
        assert!(validate_window_id("0x12ab").is_ok());
        assert!(validate_window_id("0XFF").is_ok());
    }

    #[test]
    fn window_id_bad() {
        assert!(validate_window_id("123").is_err());
        assert!(validate_window_id("0x").is_err());
        assert!(validate_window_id("0xzz").is_err());
    }

    #[test]
    fn bitrate_ok() {
        assert!(validate_bitrate("4500k").is_ok());
        assert!(validate_bitrate("6M").is_ok());
    }

    #[test]
    fn bitrate_bad() {
        assert!(validate_bitrate("fast").is_err());
        assert!(validate_bitrate("4500").is_err());
    }

    #[test]
    fn fps_range() {
        assert!(validate_fps(30).is_ok());
        assert!(validate_fps(0).is_err());
        assert!(validate_fps(121).is_err());
    }
}
