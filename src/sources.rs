//! wmctrl / pactl の出力パース。

use std::io::{self, ErrorKind};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct Window {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct AudioSource {
    pub name: String,
    pub description: String,
}

/// `wmctrl -l` の1行をパースする。
/// 形式: `0x01234567  0 hostname Title words...`
pub fn parse_wmctrl_line(line: &str) -> Option<Window> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let mut parts = line.split_whitespace();
    let id = parts.next()?.to_string();
    if !id.starts_with("0x") && !id.starts_with("0X") {
        return None;
    }
    let _desktop = parts.next()?;
    let _host = parts.next()?;
    let title = parts.collect::<Vec<_>>().join(" ");
    Some(Window { id, title })
}

pub fn parse_wmctrl_output(text: &str) -> Vec<Window> {
    text.lines().filter_map(parse_wmctrl_line).collect()
}

/// `pactl list sources short` の1行。
/// 形式: `index\tname\tmodule\tsample\tstate`
pub fn parse_pactl_short_line(line: &str) -> Option<AudioSource> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let cols: Vec<&str> = if line.contains('\t') {
        line.split('\t').collect()
    } else {
        line.split_whitespace().collect()
    };
    if cols.len() < 2 {
        return None;
    }
    let name = cols[1].to_string();
    let description = if cols.len() > 2 {
        cols[2..].join(" ")
    } else {
        name.clone()
    };
    Some(AudioSource { name, description })
}

pub fn parse_pactl_short_output(text: &str) -> Vec<AudioSource> {
    text.lines().filter_map(parse_pactl_short_line).collect()
}

fn run_capture(cmd: &str, args: &[&str]) -> io::Result<String> {
    let output = Command::new(cmd).args(args).output()?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::new(
            ErrorKind::Other,
            format!(
                "{cmd} が失敗しました (exit {:?}): {err}",
                output.status.code()
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn list_windows() -> io::Result<Vec<Window>> {
    let text = run_capture("wmctrl", &["-l"])?;
    Ok(parse_wmctrl_output(&text))
}

pub fn list_audio() -> io::Result<Vec<AudioSource>> {
    let text = run_capture("pactl", &["list", "sources", "short"])?;
    Ok(parse_pactl_short_output(&text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wmctrl_line() {
        let w = parse_wmctrl_line("0x02a00007  0 myhost Firefox — Example").unwrap();
        assert_eq!(w.id, "0x02a00007");
        assert_eq!(w.title, "Firefox — Example");
    }

    #[test]
    fn pactl_line_tab() {
        let a = parse_pactl_short_line(
            "1\talsa_input.usb-Mic\tmodule-alsa-card.c\ts16le 2ch 48000Hz\tSUSPENDED",
        )
        .unwrap();
        assert_eq!(a.name, "alsa_input.usb-Mic");
    }

    #[test]
    fn pactl_line_spaces() {
        let a = parse_pactl_short_line(
            "42 alsa_output.pci.monitor module-alsa-card.c s16le 2ch 48000Hz RUNNING",
        )
        .unwrap();
        assert_eq!(a.name, "alsa_output.pci.monitor");
    }
}
