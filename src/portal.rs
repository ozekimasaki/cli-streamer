//! Wayland ScreenCast ポータルヘルパーの展開と起動。

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

const HELPER_SRC: &str = include_str!("portal_helper.py");

/// ヘルパーをランタイムディレクトリへ書き、パスを返す。
pub fn materialize_helper() -> io::Result<PathBuf> {
    let dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    let path = dir.join("cli-streamer-portal-helper.py");
    let mut f = fs::File::create(&path)?;
    f.write_all(HELPER_SRC.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o700));
    }
    Ok(path)
}

/// python3 ヘルパーを起動（stdout = Y4M）。
pub fn spawn_portal_helper(fps: u32) -> io::Result<Child> {
    let script = materialize_helper()?;
    Command::new("python3")
        .arg(&script)
        .env("CLI_STREAMER_FPS", fps.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
}
