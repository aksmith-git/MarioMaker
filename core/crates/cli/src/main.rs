//! `mmk` — the course server's tool: run the open-source course server (the recomp's 100 Mario
//! mod and Course World talk to it), create accounts and upload courses. SMCraft's full tool
//! (course dumps, imports, the engine's headless runs) is on the `smcraft-archive` branch.

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use mmk_formats::smm2::{self, crypto};

#[derive(Parser)]
#[command(
    name = "mmk",
    version,
    about = "Open-source course server and its tools"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the open-source course server (courses + index in DIR) and the online-play relay
    /// (UDP, same port).
    Server {
        #[arg(long, default_value = "mmk-server-data")]
        dir: PathBuf,
        #[arg(long, default_value = "0.0.0.0:8640")]
        addr: String,
        /// Moderation token (ban accounts, delete courses); default $MMK_ADMIN_TOKEN.
        #[arg(long)]
        admin_token: Option<String>,
        /// The relay's WebSocket address for browsers (default: the HTTP port + 1).
        #[arg(long)]
        relay_ws: Option<String>,
        /// World records only from replays the server re-simulates (typed-in times count as
        /// clears only).
        #[arg(long)]
        verified_records: bool,
        /// Request threads.
        #[arg(long, default_value_t = 8)]
        threads: usize,
        /// Writes (uploads, votes, comments, clears) per client address per minute.
        #[arg(long, default_value_t = 60)]
        writes_per_minute: u32,
        /// New accounts per client address per hour.
        #[arg(long, default_value_t = 5)]
        accounts_per_hour: u32,
        /// Behind your own reverse proxy: take client addresses from X-Forwarded-For.
        #[arg(long)]
        trust_proxy: bool,
        /// Endless Challenge / 100 Mario courses also from an upstream source: `tgrcode`
        /// (tgrcode.com/mm2, a third-party service reading Nintendo's Course World). Needs the
        /// course key table: --keys, or --exe to find it in your game.
        #[arg(long)]
        upstream: Option<String>,
        /// The course key table (smmfmt.smm2.keyfind's output) for --upstream.
        #[arg(long)]
        keys: Option<PathBuf>,
        /// Your own SMM2 executable (the `main` NSO) to find the course key table in, for
        /// --upstream.
        #[arg(long)]
        exe: Option<PathBuf>,
    },
    /// Upload courses (raw, zlib, or .bcd with --keys); several at once seed a new server.
    Upload {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Course key table for encrypted .bcd files (an asset cache's keys/course_table.bin).
        #[arg(long)]
        keys: Option<PathBuf>,
        #[arg(long, default_value = "http://localhost:8640")]
        server: String,
        #[arg(long, default_value = "anonymous")]
        maker: String,
        /// Account token (default $MMK_TOKEN): uploads under that account.
        #[arg(long)]
        token: Option<String>,
    },
    /// Create an account on a course server; prints the maker ID and the token (keep it secret,
    /// pass it with --token or $MMK_TOKEN).
    Register {
        name: String,
        #[arg(long, default_value = "http://localhost:8640")]
        server: String,
    },
}

fn main() -> ExitCode {
    match Cli::parse().cmd {
        Cmd::Server {
            dir,
            addr,
            admin_token,
            relay_ws,
            verified_records,
            threads,
            writes_per_minute,
            accounts_per_hour,
            trust_proxy,
            upstream,
            keys,
            exe,
        } => {
            let admin = admin_token.or_else(|| std::env::var("MMK_ADMIN_TOKEN").ok());
            let store = match mmk_server::Store::open(&dir) {
                Ok(s) => s.with_admin(admin).with_verified_records(verified_records),
                Err(e) => {
                    eprintln!("{}: {e:?}", dir.display());
                    return ExitCode::FAILURE;
                }
            };
            // the online-play relay: UDP on the same port number, WebSocket on the next
            let ws = relay_ws.unwrap_or_else(|| match addr.parse::<std::net::SocketAddr>() {
                Ok(mut a) => {
                    a.set_port(a.port().wrapping_add(1));
                    a.to_string()
                }
                Err(_) => "0.0.0.0:8641".into(),
            });
            match mmk_server::start_relay(&addr, &ws) {
                Ok(_) => eprintln!("mmk-server: online-play relay on udp://{addr} and ws://{ws}"),
                Err(e) => eprintln!("mmk-server: no relay ({e})"),
            }
            eprintln!("mmk-server: {} on http://{addr}", dir.display());
            let opts = mmk_server::ServeOptions {
                threads,
                limits: mmk_server::limits::Limits {
                    writes_per_minute,
                    accounts_per_hour,
                },
                trust_proxy,
            };
            let store = std::sync::Arc::new(store);
            if let Some(up) = upstream {
                if up != "tgrcode" {
                    eprintln!("server: --upstream {up}: the source is tgrcode");
                    return ExitCode::FAILURE;
                }
                let keys = match (read_key_table(keys), exe) {
                    (Some(t), _) => mmk_server::upstream::Keys::Table(Box::new(t)),
                    (None, Some(p)) => {
                        match std::fs::read(&p).map(|b| mmk_formats::nso::Nso::parse(&b)) {
                            Ok(Ok(nso)) => mmk_server::upstream::Keys::Find(
                                [
                                    mmk_formats::nso::SegmentKind::Ro,
                                    mmk_formats::nso::SegmentKind::Data,
                                ]
                                .into_iter()
                                .filter_map(|k| nso.segment(k).map(<[u8]>::to_vec))
                                .collect(),
                            ),
                            _ => {
                                eprintln!("server: {}: not an NSO", p.display());
                                return ExitCode::FAILURE;
                            }
                        }
                    }
                    (None, None) => {
                        eprintln!(
                            "server: --upstream needs --keys or --exe (the course key table)"
                        );
                        return ExitCode::FAILURE;
                    }
                };
                mmk_server::upstream::start(
                    store.clone(),
                    Box::new(mmk_server::upstream::TgrCode::new()),
                    keys,
                    Default::default(),
                );
                eprintln!("mmk-server: Endless / 100 Mario courses also from tgrcode.com/mm2");
            }
            match mmk_server::serve_shared(store, &addr, opts) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("server: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Cmd::Upload {
            files,
            keys,
            server,
            maker,
            token,
        } => {
            let table = read_key_table(keys);
            let client = online(server, token);
            let (mut ok, mut failed) = (0usize, 0usize);
            for file in &files {
                let Some(data) = std::fs::read(file)
                    .ok()
                    .and_then(|raw| course_data(raw, table.as_ref()))
                else {
                    eprintln!(
                        "{}: not course data (raw, zlib or .bcd with --keys)",
                        file.display()
                    );
                    failed += 1;
                    continue;
                };
                match client.upload(&data, &maker) {
                    Ok(l) => {
                        println!("{} {}", l.code, l.name);
                        ok += 1;
                    }
                    Err(e) => {
                        eprintln!("{}: {e}", file.display());
                        failed += 1;
                    }
                }
            }
            if files.len() > 1 {
                eprintln!("uploaded {ok}, failed {failed}");
            }
            if failed == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Cmd::Register { name, server } => match mmk_online::Client::new(server).register(&name) {
            Ok((a, token)) => {
                println!("maker ID {}  ({})", a.maker_id, a.name);
                println!("token {token}");
                ExitCode::SUCCESS
            }
            Err(e) => fail("register", e),
        },
    }
}

fn course_data(raw: Vec<u8>, table: Option<&smm2::KeyTable>) -> Option<Vec<u8>> {
    if raw.len() == smm2::COURSE_SIZE {
        Some(raw)
    } else if raw.len() == smm2::BCD_SIZE {
        crypto::decrypt_bcd(&raw, table?, false)
            .ok()
            .map(|b| b.body)
    } else {
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(&raw[..])
            .read_to_end(&mut out)
            .ok()?;
        Some(out)
    }
}

fn read_key_table(path: Option<PathBuf>) -> Option<smm2::KeyTable> {
    let b = std::fs::read(path?).ok()?;
    let words: Vec<u32> = b
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    words.try_into().ok()
}

fn online(server: String, token: Option<String>) -> mmk_online::Client {
    let token = token.or_else(|| std::env::var("MMK_TOKEN").ok().filter(|t| !t.is_empty()));
    mmk_online::Client::new(server).with_token(token)
}

fn fail(what: &str, e: impl std::fmt::Display) -> ExitCode {
    eprintln!("{what}: {e}");
    ExitCode::FAILURE
}
