//! 素の key=value 設定と配信先 URL 組み立て。

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub youtube_key: Option<String>,
    pub twitch_key: Option<String>,
    pub kick_url: Option<String>,
    pub kick_key: Option<String>,
    pub bitrate: String,
    pub fps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    Youtube,
    Twitch,
    Kick,
}

impl Destination {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "youtube" | "yt" => Some(Self::Youtube),
            "twitch" | "tw" => Some(Self::Twitch),
            "kick" => Some(Self::Kick),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Youtube => "youtube",
            Self::Twitch => "twitch",
            Self::Kick => "kick",
        }
    }
}

impl Config {
    pub fn config_path() -> PathBuf {
        if let Ok(p) = env::var("CLI_STREAMER_CONFIG") {
            return PathBuf::from(p);
        }
        let home = env::var_os("HOME")
            .or_else(|| env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        home.join(".config").join("cli-streamer").join("config")
    }

    pub fn load() -> io::Result<Self> {
        let path = Self::config_path();
        if !path.exists() {
            return Ok(Self::default_values());
        }
        let text = fs::read_to_string(&path)?;
        Ok(Self::parse(&text))
    }

    pub fn default_values() -> Self {
        Self {
            youtube_key: None,
            twitch_key: None,
            kick_url: None,
            kick_key: None,
            bitrate: "4500k".into(),
            fps: 30,
        }
    }

    pub fn parse(text: &str) -> Self {
        let mut cfg = Self::default_values();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let key = k.trim();
            let val = v.trim().to_string();
            if val.is_empty() {
                continue;
            }
            match key {
                "youtube_key" => cfg.youtube_key = Some(val),
                "twitch_key" => cfg.twitch_key = Some(val),
                "kick_url" => cfg.kick_url = Some(val),
                "kick_key" => cfg.kick_key = Some(val),
                "bitrate" => cfg.bitrate = val,
                "fps" => {
                    if let Ok(n) = val.parse::<u32>() {
                        if n > 0 {
                            cfg.fps = n;
                        }
                    }
                }
                _ => {}
            }
        }
        cfg
    }

    pub fn template() -> &'static str {
        "\
# cli-streamer 設定（権限は 0600 推奨）
# 使わない配信先の行は空のままか削除してよい

youtube_key=
twitch_key=
kick_url=rtmps://xxxxxxxx.global-contribute.live-video.net
kick_key=

bitrate=4500k
fps=30
"
    }

    pub fn write_template() -> io::Result<PathBuf> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if path.exists() {
            return Ok(path);
        }
        let mut f = fs::File::create(&path)?;
        f.write_all(Self::template().as_bytes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(path)
    }

    /// キーが揃っている配信先だけ返す。
    pub fn available_destinations(&self) -> Vec<Destination> {
        let mut out = Vec::new();
        if self.youtube_key.as_ref().is_some_and(|k| !k.is_empty()) {
            out.push(Destination::Youtube);
        }
        if self.twitch_key.as_ref().is_some_and(|k| !k.is_empty()) {
            out.push(Destination::Twitch);
        }
        if self.kick_key.as_ref().is_some_and(|k| !k.is_empty())
            && self.kick_url.as_ref().is_some_and(|u| !u.is_empty())
        {
            out.push(Destination::Kick);
        }
        out
    }

    pub fn ingest_url(&self, dest: Destination) -> Result<String, String> {
        match dest {
            Destination::Youtube => {
                let key = self
                    .youtube_key
                    .as_ref()
                    .filter(|k| !k.is_empty())
                    .ok_or_else(|| "youtube_key が未設定です".to_string())?;
                Ok(format!("rtmp://a.rtmp.youtube.com/live2/{key}"))
            }
            Destination::Twitch => {
                let key = self
                    .twitch_key
                    .as_ref()
                    .filter(|k| !k.is_empty())
                    .ok_or_else(|| "twitch_key が未設定です".to_string())?;
                Ok(format!("rtmp://live.twitch.tv/app/{key}"))
            }
            Destination::Kick => {
                let key = self
                    .kick_key
                    .as_ref()
                    .filter(|k| !k.is_empty())
                    .ok_or_else(|| "kick_key が未設定です".to_string())?;
                let url = self
                    .kick_url
                    .as_ref()
                    .filter(|u| !u.is_empty())
                    .ok_or_else(|| "kick_url が未設定です".to_string())?;
                Ok(normalize_kick_url(url, key))
            }
        }
    }
}

/// Kick の ingest を `rtmps://host:443/app/{key}` に正規化する。
pub fn normalize_kick_url(base: &str, key: &str) -> String {
    let mut s = base.trim().to_string();
    while s.ends_with('/') {
        s.pop();
    }

    // 既にキーが末尾についている場合はそのまま返す
    if s.ends_with(key) && (s.ends_with(&format!("/{key}")) || s.ends_with(&format!("app/{key}"))) {
        if !s.starts_with("rtmp") {
            s = format!("rtmps://{}", s.trim_start_matches("//"));
        }
        return s;
    }

    if !s.starts_with("rtmp://") && !s.starts_with("rtmps://") {
        s = format!("rtmps://{s}");
    }

    // rtmps 推奨（Kick は RTMPS）
    if s.starts_with("rtmp://") && !s.starts_with("rtmps://") {
        s = format!("rtmps://{}", s.trim_start_matches("rtmp://"));
    }

    // ホスト部分を取り出し :443/app を保証
    let after_scheme = s
        .strip_prefix("rtmps://")
        .or_else(|| s.strip_prefix("rtmp://"))
        .unwrap_or(&s);

    let host_and_rest = after_scheme;
    let host = host_and_rest
        .split('/')
        .next()
        .unwrap_or(host_and_rest)
        .split(':')
        .next()
        .unwrap_or(host_and_rest);

    format!("rtmps://{host}:443/app/{key}")
}

/// ログ用にストリームキー部分をマスクする。
pub fn mask_secrets(cmd: &str, secrets: &[&str]) -> String {
    let mut out = cmd.to_string();
    for secret in secrets {
        if secret.is_empty() {
            continue;
        }
        out = out.replace(secret, "***");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kick_bare_host() {
        let u = normalize_kick_url(
            "rtmps://fa723fc1b171.global-contribute.live-video.net",
            "sk_test",
        );
        assert_eq!(
            u,
            "rtmps://fa723fc1b171.global-contribute.live-video.net:443/app/sk_test"
        );
    }

    #[test]
    fn kick_with_port_app() {
        let u = normalize_kick_url(
            "rtmps://fa723fc1b171.global-contribute.live-video.net:443/app",
            "sk_test",
        );
        assert_eq!(
            u,
            "rtmps://fa723fc1b171.global-contribute.live-video.net:443/app/sk_test"
        );
    }

    #[test]
    fn kick_rtmp_upgraded() {
        let u = normalize_kick_url(
            "rtmp://fa723fc1b171.global-contribute.live-video.net",
            "sk_test",
        );
        assert!(u.starts_with("rtmps://"));
        assert!(u.ends_with("/app/sk_test"));
    }

    #[test]
    fn parse_config() {
        let cfg = Config::parse(
            "youtube_key=abc\nbitrate=6000k\nfps=60\n# comment\nunknown=1\n",
        );
        assert_eq!(cfg.youtube_key.as_deref(), Some("abc"));
        assert_eq!(cfg.bitrate, "6000k");
        assert_eq!(cfg.fps, 60);
    }

    #[test]
    fn mask_key() {
        let s = mask_secrets("rtmp://x/live2/secretkey", &["secretkey"]);
        assert_eq!(s, "rtmp://x/live2/***");
    }
}
