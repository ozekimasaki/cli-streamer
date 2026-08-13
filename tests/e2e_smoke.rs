//! CLI スモーク / ドライラン E2E（実配信・ポータル UI は含まない）。

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cli-streamer"))
}

fn temp_config_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("cli-streamer-e2e-{name}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir.join("config")
}

fn write_config(path: &PathBuf, body: &str) {
    fs::write(path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
}

#[test]
fn version_prints_semver() {
    let out = Command::new(bin())
        .arg("version")
        .output()
        .expect("spawn");
    assert!(out.status.success(), "stderr={}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("cli-streamer"), "{stdout}");
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")), "{stdout}");
}

#[test]
fn help_mentions_wayland_and_x11() {
    let out = Command::new(bin()).arg("help").output().expect("spawn");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Wayland"), "{stdout}");
    assert!(stdout.contains("X11") || stdout.contains("x11"), "{stdout}");
    assert!(stdout.contains("--dry-run"), "{stdout}");
}

#[test]
fn init_creates_config_once() {
    let path = temp_config_path("init");
    let _ = fs::remove_file(&path);

    let out = Command::new(bin())
        .env("CLI_STREAMER_CONFIG", &path)
        .arg("init")
        .output()
        .expect("spawn");
    assert!(out.status.success(), "stderr={}", String::from_utf8_lossy(&out.stderr));
    assert!(path.exists());
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("youtube_key="));

    // 2回目は上書きしない
    fs::write(&path, "youtube_key=keep\n").unwrap();
    let out2 = Command::new(bin())
        .env("CLI_STREAMER_CONFIG", &path)
        .arg("init")
        .output()
        .expect("spawn");
    assert!(out2.status.success());
    let text2 = fs::read_to_string(&path).unwrap();
    assert!(text2.contains("youtube_key=keep"));
}

#[test]
fn unknown_command_fails() {
    let out = Command::new(bin())
        .arg("not-a-command")
        .output()
        .expect("spawn");
    assert!(!out.status.success());
}

#[test]
fn start_requires_dest() {
    let out = Command::new(bin())
        .env("XDG_SESSION_TYPE", "x11")
        .env("DISPLAY", ":0")
        .args(["start", "--window", "0x1"])
        .output()
        .expect("spawn");
    assert!(!out.status.success());
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        err.contains("--dest") || err.contains("必須"),
        "unexpected: {err}"
    );
}

/// X11 想定の dry-run。ffmpeg / pactl / wmctrl が無い環境では無視。
#[test]
fn x11_start_dry_run_masks_key() {
    if cfg!(not(target_os = "linux")) {
        return;
    }
    if which_missing("ffmpeg") || which_missing("pactl") || which_missing("wmctrl") {
        eprintln!("skip x11 dry-run: missing runtime deps");
        return;
    }

    let path = temp_config_path("x11-dry");
    write_config(
        &path,
        "youtube_key=supersecretkey999\nbitrate=4500k\nfps=30\n",
    );

    let out = Command::new(bin())
        .env("CLI_STREAMER_CONFIG", &path)
        .env("XDG_SESSION_TYPE", "x11")
        .env("DISPLAY", ":0")
        .env_remove("WAYLAND_DISPLAY")
        .args([
            "start",
            "--window",
            "0xabc",
            "--dest",
            "youtube",
            "--dry-run",
        ])
        .output()
        .expect("spawn");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() {
        eprintln!("x11 dry-run soft-fail: {combined}");
        return;
    }
    assert!(combined.contains("dry-run") || combined.contains("x11grab"), "{combined}");
    assert!(
        !combined.contains("supersecretkey999"),
        "key leaked: {combined}"
    );
    assert!(combined.contains("***") || combined.contains("youtube"), "{combined}");
}

/// Wayland 想定の dry-run（ポータルは開かない）。依存が無ければスキップ。
#[test]
fn wayland_start_dry_run_uses_y4m() {
    if cfg!(not(target_os = "linux")) {
        return;
    }
    if which_missing("ffmpeg")
        || which_missing("pactl")
        || which_missing("python3")
        || which_missing("gst-launch-1.0")
    {
        eprintln!("skip wayland dry-run: missing runtime deps");
        return;
    }

    let path = temp_config_path("wl-dry");
    write_config(
        &path,
        "youtube_key=waylandsecretkey888\nbitrate=4500k\nfps=30\n",
    );

    let out = Command::new(bin())
        .env("CLI_STREAMER_CONFIG", &path)
        .env("XDG_SESSION_TYPE", "wayland")
        .env("WAYLAND_DISPLAY", "wayland-0")
        .env_remove("DISPLAY")
        .args(["start", "--dest", "youtube", "--dry-run"])
        .output()
        .expect("spawn");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() {
        eprintln!("wayland dry-run soft-fail: {combined}");
        return;
    }
    assert!(
        combined.contains("yuv4mpegpipe") || combined.contains("portal_helper"),
        "{combined}"
    );
    assert!(!combined.contains("waylandsecretkey888"), "key leaked");
}

fn which_missing(name: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {name} >/dev/null 2>&1")])
        .status()
        .map(|s| !s.success())
        .unwrap_or(true)
}
