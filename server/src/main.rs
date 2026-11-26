//! pcrc-server: PC Remote Control MCP server.
//!
//! Linux, macOS and Windows. Always approves MCP connections (an optional
//! bearer token can be enabled); ships with a loopback admin API the nsh
//! desktop UI drives.

mod config;
mod http;
mod logging;
mod mcp;
mod platform;
mod state;
mod tools;
mod tunnel;
mod updates;

use state::AppState;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

const HELP: &str = "\
pcrc-server — PC Remote Control MCP server

USAGE:
    pcrc-server [OPTIONS]

OPTIONS:
    -c, --config <PATH>   Config file (default: ~/.config/pc-remote-control-mcp/config.json)
    -b, --bind <ADDR>     Bind address override (default from config: 127.0.0.1)
    -p, --port <PORT>     Port override (default from config: 8080)
        --stdio           Serve MCP over stdio instead of HTTP (for desktop MCP clients)
        --print-token     Print the configured bearer token and exit
        --setup-uinput    Print the /dev/uinput permissions setup (Linux) and exit
    -V, --version         Print the version
    -h, --help            Print this help
";

struct Args {
    config: Option<PathBuf>,
    bind: Option<String>,
    port: Option<u16>,
    stdio: bool,
    print_token: bool,
    setup_uinput: bool,
    help: bool,
    version: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        config: None,
        bind: None,
        port: None,
        stdio: false,
        print_token: false,
        setup_uinput: false,
        help: false,
        version: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let (key, inline) = match arg.split_once('=') {
            Some((k, v)) => (k.to_string(), Some(v.to_string())),
            None => (arg.clone(), None),
        };
        match key.as_str() {
            "-c" | "--config" => {
                let v = inline.or_else(|| it.next()).ok_or("--config needs a path")?;
                args.config = Some(PathBuf::from(v));
            }
            "-b" | "--bind" => {
                args.bind = Some(inline.or_else(|| it.next()).ok_or("--bind needs an address")?);
            }
            "-p" | "--port" => {
                let v = inline.or_else(|| it.next()).ok_or("--port needs a number")?;
                args.port = Some(v.parse().map_err(|_| format!("invalid port: {v}"))?);
            }
            "--stdio" => args.stdio = true,
            "--print-token" => args.print_token = true,
            "--setup-uinput" => args.setup_uinput = true,
            "-V" | "--version" => args.version = true,
            "-h" | "--help" => args.help = true,
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(args)
}

async fn run_stdio(state: Arc<AppState>) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    logging::info(format!(
        "pcrc-server {} serving MCP on stdio ({}/{})",
        state.version,
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let msg: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                let err = serde_json::json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("Parse error: {e}")}});
                let _ = stdout.write_all(format!("{err}\n").as_bytes()).await;
                let _ = stdout.flush().await;
                continue;
            }
        };
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("").to_string();
        state.stats.bump_requests("stdio");
        let reply = mcp::dispatch(&state, msg).await;
        logging::info(format!("mcp {method} <- stdio"));
        if let Some(resp) = reply {
            let _ = stdout.write_all(format!("{resp}\n").as_bytes()).await;
            let _ = stdout.flush().await;
        }
    }
}

#[tokio::main]
async fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}\n\n{HELP}");
            std::process::exit(2);
        }
    };
    if args.help {
        println!("{HELP}");
        return;
    }
    if args.version {
        println!(
            "pcrc-server {} ({}/{})",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        return;
    }
    if args.setup_uinput {
        #[cfg(target_os = "linux")]
        println!("{}", platform::linux::uinput::setup_instructions());
        #[cfg(not(target_os = "linux"))]
        println!("uinput is a Linux-only mechanism.");
        return;
    }

    let config_path = args.config.clone().unwrap_or_else(config::default_config_path);
    let handle = match config::ConfigHandle::load(config_path.clone()) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("error: cannot load config {}: {e}", config_path.display());
            std::process::exit(1);
        }
    };
    let cfg = handle.get();
    let bind = args.bind.clone().unwrap_or_else(|| cfg.bind.clone());
    let port = args.port.unwrap_or(cfg.port);

    if args.print_token {
        println!("{}", cfg.token);
        eprintln!(
            "config: {} (token auth: {})",
            handle.path().display(),
            if cfg.token_enabled { "enabled" } else { "disabled — always approve" }
        );
        return;
    }

    logging::init(cfg.log_lines, args.stdio);
    let state = AppState::new(handle.clone(), args.stdio, bind.clone(), port);

    if args.stdio {
        run_stdio(state).await;
        return;
    }

    let ip: IpAddr = match bind.parse() {
        Ok(ip) => ip,
        Err(_) => {
            logging::error(format!("invalid bind address: {bind}"));
            std::process::exit(1);
        }
    };
    let addr = SocketAddr::new(ip, port);
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            logging::error(format!("cannot bind {addr}: {e}"));
            std::process::exit(1);
        }
    };

    logging::info(format!(
        "pcrc-server {} on {addr} — MCP at http://{addr}/mcp, {} tools",
        state.version,
        tools::specs().len()
    ));
    if cfg.token_enabled {
        logging::info("bearer token auth enabled");
    } else {
        logging::info("always-approve mode: no token required");
        if !ip.is_loopback() {
            logging::warn(
                "bound to a non-loopback address without a token: any reachable MCP client is approved",
            );
        }
    }
    logging::info(format!("config: {}", handle.path().display()));

    if cfg.tunnel.auto_start {
        if !cfg.token_enabled {
            logging::warn("tunnel auto-start skipped: enable the bearer token first");
        } else {
            let st = state.clone();
            let kind = if cfg.tunnel.kind.trim().is_empty() { "auto".to_string() } else { cfg.tunnel.kind.clone() };
            let token = cfg.tunnel.ngrok_authtoken.clone();
            let socket = cfg.tunnel.tailscale_socket.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                logging::info(format!("starting automatic {kind} tunnel..."));
                match tunnel::start(&kind, st.port, &token, &socket).await {
                    Ok(s) => {
                        let tstate = s.get("state").and_then(|v| v.as_str()).unwrap_or("?");
                        match s.get("url").and_then(|u| u.as_str()) {
                            Some(url) => logging::info(format!("automatic tunnel ready ({tstate}): {url}")),
                            None => logging::warn(format!(
                                "automatic tunnel ended without a URL ({tstate}): {}",
                                s.get("error").and_then(|e| e.as_str()).unwrap_or("no error")
                            )),
                        }
                    }
                    Err(e) => logging::warn(format!("automatic tunnel failed: {e}")),
                }
            });
        }
    }

    let app = http::router(state.clone());
    let serve = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown({
        let shutdown_state = state.clone();
        async move {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {},
                _ = shutdown_state.shutdown.notified() => {},
            }
            logging::info("shutting down");
        }
    });
    if let Err(e) = serve.await {
        logging::error(format!("server error: {e}"));
        let _ = tunnel::stop(&cfg.tunnel.tailscale_socket).await;
        std::process::exit(1);
    }
    let _ = tunnel::stop(&cfg.tunnel.tailscale_socket).await;
}
