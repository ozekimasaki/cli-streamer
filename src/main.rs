//! 軽量 ffmpeg ラッパー配信 CLI（Linux X11 / Wayland）。

mod config;
mod doctor;
mod ffmpeg;
mod portal;
mod session;
mod sources;

use config::{Config, Destination};
use doctor::{
    preflight, require_linux, run_doctor, validate_config_values, validate_window_id,
};
use ffmpeg::{
    build_args, masked_command, spawn_ffmpeg, spawn_ffmpeg_with_video_stdin, wait_ffmpeg,
    wait_helper_and_ffmpeg, StreamRequest,
};
use session::DisplayServer;
use sources::{list_audio, list_windows, AudioSource, Window};
use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        return interactive();
    }

    let cmd = args.remove(0);
    match cmd.as_str() {
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        "version" | "-V" | "--version" => {
            println!("cli-streamer {VERSION}");
            Ok(())
        }
        "init" => cmd_init(),
        "doctor" => cmd_doctor(),
        "list-windows" => cmd_list_windows(),
        "list-audio" => {
            require_linux()?;
            let srcs = list_audio().map_err(|e| e.to_string())?;
            for (i, a) in srcs.iter().enumerate() {
                println!("{:>3}  {}  ({})", i + 1, a.name, a.description);
            }
            Ok(())
        }
        "start" => {
            let (req, dry_run) = parse_start_args(&args)?;
            start_stream(&req, dry_run)
        }
        other => Err(format!(
            "不明なコマンド: {other}\n使い方は `cli-streamer help` を参照"
        )),
    }
}

fn print_help() {
    println!(
        "\
cli-streamer {VERSION} — 軽量 ffmpeg 配信ラッパー (Linux X11 / Wayland)

使い方:
  cli-streamer                  対話モード
  cli-streamer init             設定テンプレートを作成
  cli-streamer doctor           依存・環境・設定を点検
  cli-streamer list-windows     ウィンドウ一覧（X11 のみ）
  cli-streamer list-audio       音声ソース一覧
  cli-streamer start [options]  配信開始
  cli-streamer version
  cli-streamer help

start オプション:
  --window <0xid>     X11: ウィンドウ ID（X11 では必須）
  --audio <name>      Pulse ソース名（省略で無音）
  --dest <list>       youtube,twitch,kick をカンマ区切り（必須）
  --dry-run           コマンドを表示するだけ（起動しない）

Wayland では --window は不要。OS の共有ダイアログでウィンドウを選びます。

設定: ~/.config/cli-streamer/config
  （環境変数 CLI_STREAMER_CONFIG で上書き可）

Install:
  cargo install --git https://github.com/ozekimasaki/cli-streamer --locked
"
    );
}

fn cmd_init() -> Result<(), String> {
    let path = Config::config_path();
    let existed = path.exists();
    let path = Config::write_template().map_err(|e| e.to_string())?;
    if existed {
        println!("既に存在します（上書きしません）: {}", path.display());
    } else {
        println!("設定ファイルを作成しました: {}", path.display());
    }
    println!("ストリームキーを編集してください。");
    #[cfg(unix)]
    println!("権限の確認: chmod 600 {}", path.display());
    #[cfg(not(unix))]
    println!("（Linux 上では chmod 600 を推奨します）");
    Ok(())
}

fn cmd_doctor() -> Result<(), String> {
    let need_rtmps = Config::load()
        .ok()
        .map(|c| c.available_destinations().contains(&Destination::Kick))
        .unwrap_or(true);
    let items = run_doctor(need_rtmps);
    let mut failed = 0usize;
    for item in &items {
        let mark = if item.ok { "OK" } else { "NG" };
        println!("[{mark}] {}", item.message);
        if !item.ok {
            failed += 1;
        }
    }
    if failed > 0 {
        Err(format!("doctor: {failed} 件の問題があります"))
    } else {
        println!("すべて OK です。");
        Ok(())
    }
}

fn cmd_list_windows() -> Result<(), String> {
    require_linux()?;
    match session::detect()? {
        DisplayServer::Wayland => {
            println!(
                "Wayland ではウィンドウ一覧は使えません。\
                 配信開始時に OS の共有ダイアログでウィンドウを選んでください。"
            );
            Ok(())
        }
        DisplayServer::X11 => {
            let wins = list_windows().map_err(|e| e.to_string())?;
            for (i, w) in wins.iter().enumerate() {
                println!("{:>3}  {}  {}", i + 1, w.id, w.title);
            }
            Ok(())
        }
    }
}

fn parse_start_args(args: &[String]) -> Result<(StreamRequest, bool), String> {
    let display = session::detect()?;
    let mut window_id: Option<String> = None;
    let mut audio: Option<String> = None;
    let mut dest_raw: Option<String> = None;
    let mut dry_run = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--window" => {
                i += 1;
                window_id = Some(
                    args.get(i)
                        .ok_or("--window の値がありません")?
                        .clone(),
                );
            }
            "--audio" => {
                i += 1;
                audio = Some(args.get(i).ok_or("--audio の値がありません")?.clone());
            }
            "--dest" => {
                i += 1;
                dest_raw = Some(args.get(i).ok_or("--dest の値がありません")?.clone());
            }
            "--dry-run" => {
                dry_run = true;
            }
            other => return Err(format!("不明な引数: {other}")),
        }
        i += 1;
    }

    let dest_raw = dest_raw.ok_or("--dest は必須です")?;
    let destinations = parse_dest_list(&dest_raw)?;

    let window_id = match display {
        DisplayServer::X11 => {
            let id = window_id.ok_or("--window は X11 では必須です")?;
            validate_window_id(&id)?;
            Some(id)
        }
        DisplayServer::Wayland => {
            if window_id.is_some() {
                eprintln!("warning: Wayland では --window は無視されます（OS ダイアログで選択）");
            }
            None
        }
    };

    Ok((
        StreamRequest {
            window_id,
            audio: audio.filter(|a| !a.is_empty() && a != "none"),
            destinations,
            display,
        },
        dry_run,
    ))
}

fn parse_dest_list(s: &str) -> Result<Vec<Destination>, String> {
    let mut out = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let d = Destination::parse(part)
            .ok_or_else(|| format!("不明な配信先: {part}（youtube / twitch / kick）"))?;
        if !out.contains(&d) {
            out.push(d);
        }
    }
    if out.is_empty() {
        return Err("配信先が空です".into());
    }
    Ok(out)
}

fn start_stream(req: &StreamRequest, dry_run: bool) -> Result<(), String> {
    preflight(&req.destinations)?;
    let cfg = Config::load().map_err(|e| e.to_string())?;
    validate_config_values(&cfg)?;
    for d in &req.destinations {
        let _ = cfg.ingest_url(*d)?;
    }
    let args = build_args(&cfg, req)?;
    eprintln!("# {}", masked_command(&cfg, &args));
    if req.display == DisplayServer::Wayland {
        eprintln!("# video: portal_helper.py | ffmpeg (yuv4mpegpipe)");
    }
    if dry_run {
        println!("dry-run: ffmpeg は起動しません。");
        return Ok(());
    }
    run_pipeline(req.display, &args)
}

fn run_pipeline(display: DisplayServer, args: &[String]) -> Result<(), String> {
    match display {
        DisplayServer::X11 => {
            let child = spawn_ffmpeg(args).map_err(|e| format!("ffmpeg 起動失敗: {e}"))?;
            let code = wait_ffmpeg(child).map_err(|e| e.to_string())?;
            if code != 0 {
                return Err(format!("ffmpeg が終了コード {code} で終了しました"));
            }
            Ok(())
        }
        DisplayServer::Wayland => {
            println!("共有ダイアログでウィンドウを選んでください…");
            let mut helper = portal::spawn_portal_helper()
                .map_err(|e| format!("portal helper 起動失敗: {e}"))?;
            let video = helper
                .stdout
                .take()
                .ok_or_else(|| "portal helper の stdout を取得できません".to_string())?;
            let ffmpeg = spawn_ffmpeg_with_video_stdin(args, video)
                .map_err(|e| format!("ffmpeg 起動失敗: {e}"))?;
            let code = wait_helper_and_ffmpeg(helper, ffmpeg).map_err(|e| e.to_string())?;
            if code != 0 {
                return Err(format!("配信プロセスが終了コード {code} で終了しました"));
            }
            Ok(())
        }
    }
}

fn interactive() -> Result<(), String> {
    require_linux()?;
    let display = session::detect()?;
    let cfg = Config::load().map_err(|e| e.to_string())?;
    validate_config_values(&cfg)?;
    let available = cfg.available_destinations();
    if available.is_empty() {
        let path = Config::config_path();
        return Err(format!(
            "配信キーが未設定です。`cli-streamer init` のあと {} を編集してください",
            path.display()
        ));
    }

    preflight(&available)?;

    let window_id = match display {
        DisplayServer::X11 => {
            println!("=== ウィンドウ ===");
            let windows = list_windows().map_err(|e| {
                format!("{e}\n（Linux X11 で wmctrl が必要です）")
            })?;
            if windows.is_empty() {
                return Err("ウィンドウが見つかりません".into());
            }
            print_windows(&windows);
            let wi = read_index("ウィンドウ番号", 1, windows.len())?;
            Some(windows[wi - 1].id.clone())
        }
        DisplayServer::Wayland => {
            println!("=== ウィンドウ ===");
            println!("Wayland: 開始時に OS の共有ダイアログでウィンドウを選びます（単体キャプチャ）。");
            None
        }
    };

    println!("\n=== 音声（0 = なし）===");
    let audios = list_audio().map_err(|e| format!("{e}\n（pactl が必要です）"))?;
    println!("  0  (無音)");
    print_audios(&audios);
    let ai = read_index("音声番号", 0, audios.len())?;
    let audio = if ai == 0 {
        None
    } else {
        Some(audios[ai - 1].name.clone())
    };

    println!("\n=== 配信先（カンマ区切り可）===");
    for (i, d) in available.iter().enumerate() {
        println!("  {}  {}", i + 1, d.name());
    }
    let dests = read_destinations(&available)?;
    preflight(&dests)?;

    let req = StreamRequest {
        window_id,
        audio,
        destinations: dests,
        display,
    };
    let args = build_args(&cfg, &req)?;
    println!("\nコマンド:");
    println!("{}", masked_command(&cfg, &args));
    if display == DisplayServer::Wayland {
        println!("# video: portal_helper.py | ffmpeg (yuv4mpegpipe)");
    }
    print!("Enter で開始 / Ctrl+C で中止 > ");
    let _ = io::stdout().flush();
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;

    println!("配信中…（Ctrl+C で停止）");
    run_pipeline(display, &args)
}

fn print_windows(windows: &[Window]) {
    for (i, w) in windows.iter().enumerate() {
        println!("  {}  {}  {}", i + 1, w.id, w.title);
    }
}

fn print_audios(audios: &[AudioSource]) {
    for (i, a) in audios.iter().enumerate() {
        println!("  {}  {}  ({})", i + 1, a.name, a.description);
    }
}

fn read_index(prompt: &str, min: usize, max: usize) -> Result<usize, String> {
    loop {
        print!("{prompt} [{min}-{max}]: ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        io::stdin()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match line.parse::<usize>() {
            Ok(n) if n >= min && n <= max => return Ok(n),
            _ => eprintln!("無効な番号です"),
        }
    }
}

fn read_destinations(available: &[Destination]) -> Result<Vec<Destination>, String> {
    loop {
        print!("配信先番号: ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        io::stdin()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut out = Vec::new();
        let mut ok = true;
        for part in line.split(',') {
            let part = part.trim();
            match part.parse::<usize>() {
                Ok(n) if n >= 1 && n <= available.len() => {
                    let d = available[n - 1];
                    if !out.contains(&d) {
                        out.push(d);
                    }
                }
                _ => {
                    eprintln!("無効: {part}");
                    ok = false;
                    break;
                }
            }
        }
        if ok && !out.is_empty() {
            return Ok(out);
        }
    }
}
