//! 依存コマンド・環境・設定の事前チェック（doctor / start 共通）。

use crate::config::{Config, Destination};
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
            "cli-streamer は Linux/X11 専用です（現在の OS: {}）",
            env::consts::OS
        ))
    }
}

fn command_exists(name: &str) -> bool {
    Command::new(name)
        .arg("-h")
        .output()
        .map(|o| o.status.success() || !o.stdout.is_empty() || !o.stderr.is_empty())
        .unwrap_or_else(|_| {
            // -h が失敗してもバイナリが存在すれば OK（一部は exit != 0）
            which_via_type(name)
        })
}

fn which_via_type(name: &str) -> bool {
    // PATH 走査（外部 which に依存しない）
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

fn bin_on_path(name: &str) -> bool {
    which_via_type(name) || command_exists(name)
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

/// bitrate は 数字 + k/m（大文字可）。
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
    // group/other に read/write/exec が付いていないこと（所有者のみ）
    Some(mode & 0o077 == 0)
}

#[cfg(not(unix))]
fn config_permissions_ok(_path: &std::path::Path) -> Option<bool> {
    None
}

/// doctor 用の点検項目を返す。失敗があっても全項目を列挙する。
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

    let display = env::var("DISPLAY");
    items.push(CheckItem {
        ok: display.as_ref().map(|d| !d.is_empty()).unwrap_or(false),
        message: match display {
            Ok(d) if !d.is_empty() => format!("DISPLAY={d}"),
            _ => "DISPLAY が未設定（X11 セッションが必要）".into(),
        },
    });

    for bin in ["ffmpeg", "wmctrl", "pactl"] {
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
                    "ffmpeg に rtmps がありません（Kick 配信に必要。OpenSSL 付きビルドを入れてください）"
                        .into()
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

    items
}

/// start / 対話の直前に必須条件だけ検査する。
pub fn preflight(destinations: &[Destination]) -> Result<(), String> {
    require_linux()?;

    if env::var("DISPLAY").map(|d| d.is_empty()).unwrap_or(true) {
        return Err("DISPLAY が未設定です。X11 セッションで実行してください".into());
    }

    for bin in ["ffmpeg", "wmctrl", "pactl"] {
        if !bin_on_path(bin) {
            return Err(format!(
                "{bin} が PATH にありません。`cli-streamer doctor` で詳細を確認してください"
            ));
        }
    }

    if !ffmpeg_has_protocol("rtmp") {
        return Err("ffmpeg に rtmp プロトコルがありません".into());
    }
    if destinations.contains(&Destination::Kick) && !ffmpeg_has_protocol("rtmps") {
        return Err(
            "Kick には rtmps が必要です。OpenSSL 付き ffmpeg を入れてください".into(),
        );
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
                    "設定ファイルの権限が緩いです。`chmod 600 {}` を実行してください",
                    path.display()
                ));
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
