//! ffmpeg コマンド生成と spawn。

use crate::config::{mask_secrets, Config, Destination};
use crate::session::DisplayServer;
use std::io;
use std::process::{Child, Command, Stdio};

#[derive(Debug, Clone)]
pub struct StreamRequest {
    /// X11 のとき必須（0xid）。Wayland では None（ポータルで選択）。
    pub window_id: Option<String>,
    pub audio: Option<String>,
    pub destinations: Vec<Destination>,
    pub display: DisplayServer,
}

/// ffmpeg 引数リストを組み立てる（プログラム名は含めない）。
pub fn build_args(cfg: &Config, req: &StreamRequest) -> Result<Vec<String>, String> {
    if req.destinations.is_empty() {
        return Err("配信先が指定されていません".into());
    }

    let g = (cfg.fps * 2).to_string();
    let bitrate = cfg.bitrate.clone();

    let mut urls = Vec::new();
    for d in &req.destinations {
        urls.push(cfg.ingest_url(*d)?);
    }

    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-stats".into(),
    ];

    match req.display {
        DisplayServer::X11 => {
            let window_id = req
                .window_id
                .as_ref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "X11 では --window が必要です".to_string())?;
            let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
            args.extend([
                "-f".into(),
                "x11grab".into(),
                "-framerate".into(),
                cfg.fps.to_string(),
                "-window_id".into(),
                window_id.clone(),
                "-i".into(),
                display,
            ]);
        }
        DisplayServer::Wayland => {
            // portal_helper → Y4M on stdin（-r で設定 fps に合わせる）
            args.extend([
                "-f".into(),
                "yuv4mpegpipe".into(),
                "-r".into(),
                cfg.fps.to_string(),
                "-i".into(),
                "-".into(),
            ]);
        }
    }

    let has_audio = req.audio.as_ref().is_some_and(|a| !a.is_empty());
    if has_audio {
        args.extend([
            "-f".into(),
            "pulse".into(),
            "-i".into(),
            req.audio.clone().unwrap(),
        ]);
    }

    // マップ: 映像は常に 0、音声はあれば 1
    if has_audio {
        args.extend(["-map".into(), "0:v".into(), "-map".into(), "1:a".into()]);
    }

    args.extend([
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "veryfast".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-b:v".into(),
        bitrate.clone(),
        "-maxrate".into(),
        bitrate.clone(),
        "-bufsize".into(),
        format_bufsize(&bitrate),
        "-g".into(),
        g.clone(),
        "-keyint_min".into(),
        g,
        "-sc_threshold".into(),
        "0".into(),
    ]);

    if has_audio {
        args.extend([
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            "160k".into(),
            "-ar".into(),
            "48000".into(),
            "-ac".into(),
            "2".into(),
        ]);
    } else {
        args.push("-an".into());
    }

    if urls.len() == 1 {
        args.extend(["-f".into(), "flv".into(), urls[0].clone()]);
    } else {
        let tee = urls
            .iter()
            .map(|u| format!("[f=flv]{u}"))
            .collect::<Vec<_>>()
            .join("|");
        args.extend(["-f".into(), "tee".into(), tee]);
    }

    Ok(args)
}

fn format_bufsize(bitrate: &str) -> String {
    let s = bitrate.trim().to_ascii_lowercase();
    if let Some(num) = s.strip_suffix('k') {
        if let Ok(n) = num.parse::<u64>() {
            return format!("{}k", n * 2);
        }
    }
    if let Some(num) = s.strip_suffix('m') {
        if let Ok(n) = num.parse::<u64>() {
            return format!("{}k", n * 2000);
        }
    }
    "9000k".into()
}

pub fn secrets_from_config(cfg: &Config) -> Vec<String> {
    let mut s = Vec::new();
    if let Some(k) = &cfg.youtube_key {
        s.push(k.clone());
    }
    if let Some(k) = &cfg.twitch_key {
        s.push(k.clone());
    }
    if let Some(k) = &cfg.kick_key {
        s.push(k.clone());
    }
    s
}

pub fn format_command_line(args: &[String]) -> String {
    let mut parts = vec!["ffmpeg".to_string()];
    for a in args {
        if a.contains(' ') || a.contains('|') || a.contains('[') {
            parts.push(format!("'{a}'"));
        } else {
            parts.push(a.clone());
        }
    }
    parts.join(" ")
}

pub fn masked_command(cfg: &Config, args: &[String]) -> String {
    let line = format_command_line(args);
    let owned = secrets_from_config(cfg);
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    mask_secrets(&line, &refs)
}

pub fn spawn_ffmpeg(args: &[String]) -> io::Result<Child> {
    Command::new("ffmpeg")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
}

/// Wayland: helper(Y4M stdout) → ffmpeg stdin。両方を待ち、先に終わった方で相手を止める。
pub fn spawn_ffmpeg_with_video_stdin(
    args: &[String],
    video_stdout: impl Into<Stdio>,
) -> io::Result<Child> {
    Command::new("ffmpeg")
        .args(args)
        .stdin(video_stdout)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
}

pub fn wait_ffmpeg(mut child: Child) -> io::Result<i32> {
    let status = child.wait()?;
    Ok(status.code().unwrap_or(1))
}

/// SIGKILL せずに止める。helper が Session.Close と gst-launch 停止をできるようにする。
fn terminate_gracefully(child: &mut Child) {
    #[cfg(unix)]
    {
        let pid = child.id().to_string();
        let _ = Command::new("kill").args(["-INT", &pid]).status();
        for _ in 0..20 {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(100)),
                Err(_) => break,
            }
        }
        let _ = Command::new("kill").args(["-TERM", &pid]).status();
        for _ in 0..10 {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(100)),
                Err(_) => break,
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// helper と ffmpeg を待ち、終了コードを返す（ffmpeg 優先）。
pub fn wait_helper_and_ffmpeg(mut helper: Child, mut ffmpeg: Child) -> io::Result<i32> {
    loop {
        match ffmpeg.try_wait()? {
            Some(status) => {
                terminate_gracefully(&mut helper);
                return Ok(status.code().unwrap_or(1));
            }
            None => {}
        }
        match helper.try_wait()? {
            Some(_status) => {
                // 映像側が先に終わった → ffmpeg に EOF
                let status = ffmpeg.wait()?;
                return Ok(status.code().unwrap_or(1));
            }
            None => {}
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_cfg() -> Config {
        Config {
            youtube_key: Some("ytkey".into()),
            twitch_key: Some("twkey".into()),
            kick_url: Some("rtmps://fa723fc1b171.global-contribute.live-video.net".into()),
            kick_key: Some("kickkey".into()),
            bitrate: "4500k".into(),
            fps: 30,
        }
    }

    #[test]
    fn single_dest_flv_x11() {
        let cfg = sample_cfg();
        let args = build_args(
            &cfg,
            &StreamRequest {
                window_id: Some("0x123".into()),
                audio: Some("alsa_input.mic".into()),
                destinations: vec![Destination::Youtube],
                display: DisplayServer::X11,
            },
        )
        .unwrap();
        assert!(args.contains(&"x11grab".to_string()));
        assert!(args.contains(&"flv".to_string()));
        assert!(args.iter().any(|a| a.contains("ytkey")));
    }

    #[test]
    fn wayland_y4m_stdin() {
        let cfg = sample_cfg();
        let args = build_args(
            &cfg,
            &StreamRequest {
                window_id: None,
                audio: None,
                destinations: vec![Destination::Youtube],
                display: DisplayServer::Wayland,
            },
        )
        .unwrap();
        assert!(args.contains(&"yuv4mpegpipe".to_string()));
        assert!(!args.iter().any(|a| a == "x11grab"));
        assert!(args.contains(&"-an".to_string()));
        assert!(args.windows(2).any(|w| w[0] == "-r" && w[1] == "30"));
    }

    #[test]
    fn multi_dest_tee() {
        let cfg = sample_cfg();
        let args = build_args(
            &cfg,
            &StreamRequest {
                window_id: Some("0x123".into()),
                audio: None,
                destinations: vec![Destination::Youtube, Destination::Twitch],
                display: DisplayServer::X11,
            },
        )
        .unwrap();
        assert!(args.contains(&"tee".to_string()));
        let tee = args.last().unwrap();
        assert!(tee.contains("[f=flv]"));
        assert!(tee.contains('|'));
    }

    #[test]
    fn mask_in_command() {
        let cfg = sample_cfg();
        let args = build_args(
            &cfg,
            &StreamRequest {
                window_id: Some("0x1".into()),
                audio: None,
                destinations: vec![Destination::Youtube],
                display: DisplayServer::X11,
            },
        )
        .unwrap();
        let masked = masked_command(&cfg, &args);
        assert!(!masked.contains("ytkey"));
        assert!(masked.contains("***"));
    }
}
