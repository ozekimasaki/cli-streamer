//! ffmpeg コマンド生成と spawn。

use crate::config::{mask_secrets, Config, Destination};
use std::io;
use std::process::{Child, Command, Stdio};

#[derive(Debug, Clone)]
pub struct StreamRequest {
    pub window_id: String,
    pub audio: Option<String>,
    pub destinations: Vec<Destination>,
}

/// ffmpeg 引数リストを組み立てる（プログラム名は含めない）。
pub fn build_args(cfg: &Config, req: &StreamRequest) -> Result<Vec<String>, String> {
    if req.destinations.is_empty() {
        return Err("配信先が指定されていません".into());
    }

    let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
    let fps = cfg.fps.to_string();
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
        "-f".into(),
        "x11grab".into(),
        "-framerate".into(),
        fps,
        "-window_id".into(),
        req.window_id.clone(),
        "-i".into(),
        display,
    ];

    let has_audio = req.audio.as_ref().is_some_and(|a| !a.is_empty());
    if has_audio {
        args.extend([
            "-f".into(),
            "pulse".into(),
            "-i".into(),
            req.audio.clone().unwrap(),
        ]);
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

/// ffmpeg を前面で待ち、終了コードを返す。
/// 端末の Ctrl+C は同一プロセスグループへ届くため、ffmpeg にも SIGINT が送られる。
pub fn wait_ffmpeg(mut child: Child) -> io::Result<i32> {
    let status = child.wait()?;
    Ok(status.code().unwrap_or(1))
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
    fn single_dest_flv() {
        let cfg = sample_cfg();
        let args = build_args(
            &cfg,
            &StreamRequest {
                window_id: "0x123".into(),
                audio: Some("alsa_input.mic".into()),
                destinations: vec![Destination::Youtube],
            },
        )
        .unwrap();
        assert!(args.iter().any(|a| a == "error"));
        assert!(args.contains(&"-stats".to_string()));
        assert!(args.contains(&"flv".to_string()));
        assert!(args.iter().any(|a| a.contains("ytkey")));
        assert!(!args.iter().any(|a| a == "tee"));
    }

    #[test]
    fn multi_dest_tee() {
        let cfg = sample_cfg();
        let args = build_args(
            &cfg,
            &StreamRequest {
                window_id: "0x123".into(),
                audio: None,
                destinations: vec![Destination::Youtube, Destination::Twitch],
            },
        )
        .unwrap();
        assert!(args.contains(&"tee".to_string()));
        assert!(args.contains(&"-an".to_string()));
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
                window_id: "0x1".into(),
                audio: None,
                destinations: vec![Destination::Youtube],
            },
        )
        .unwrap();
        let masked = masked_command(&cfg, &args);
        assert!(!masked.contains("ytkey"));
        assert!(masked.contains("***"));
    }
}
