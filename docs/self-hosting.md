# Running your own SMCraft server

SMCraft never talks to Nintendo's network. Course sharing, accounts, leaderboards and online play
go through `mmk server`, which anyone can run. One process serves everything:

| Port (default) | Protocol | What |
|---|---|---|
| 8640 | HTTP (TCP) | course API: upload, search, download, accounts, likes, comments, leaderboards |
| 8640 | UDP | online-play relay for desktop players (rooms, NAT hole punching) |
| 8641 | WebSocket (TCP) | the same relay for browser players |

## Start it

```sh
cargo build --release -p mmk-cli
MMK_ADMIN_TOKEN=$(openssl rand -hex 32) ./core/target/release/mmk server \
    --dir /srv/smcraft --addr 0.0.0.0:8640
```

- `--dir`: where everything is kept (created if missing). Back this folder up; nothing else holds
  state.
- `--addr`: the HTTP port; the UDP relay uses the same port number, the WebSocket relay the next
  one (`--relay-ws` to choose another).
- `--admin-token` (or `$MMK_ADMIN_TOKEN`): enables moderation. Keep it secret; without it there is
  no admin.

Open the ports in your firewall: TCP 8640 and 8641, UDP 8640.

Other options: `--threads` (request threads, default 8), `--writes-per-minute` (uploads, votes,
comments and clears per client address, default 60), `--accounts-per-hour` (new accounts per
address, default 5), `--trust-proxy` (behind your own reverse proxy: take the client address from
`X-Forwarded-For`; never set it on a server reachable directly, or anyone can fake addresses).
Over a limit the server answers 429; requests with the admin token aren't limited, so you can
seed a new server with `mmk upload --token $MMK_ADMIN_TOKEN courses/*.zlib` (raw, zlib, or `.bcd`
with `--keys`). `GET /api/health` reports that the server is up, its course
and account counts, and the engine version its replays need.

### What's in the data folder

| File | Holds |
|---|---|
| `courses/<code>.bin` | uploaded course data (validated on upload) |
| `index.json` | course metadata and play statistics |
| `accounts.json` | accounts: name, maker ID, and a SHA-256 of each token (never the token) |
| `social.json` | votes, comments, courses each player has cleared |

Files are rewritten atomically (written to a temporary file, then renamed).

## Courses from TGRCode (optional)

`--upstream tgrcode` lets the Endless Challenge and 100 Mario runs draw courses from TGRCode's API
(`https://tgrcode.com/mm2`, a third-party service that reads Nintendo's Course World). When a run starts or its
difficulty runs short, the server fetches a batch for that difficulty, decrypts the courses and keeps them (with
their attempts and clears) next to your own. It needs the course key table from your own game: `--exe` with your
SMM2 `main` (the table is found in it with the first download) or `--keys` with a table file. Requests are slow on
purpose (it's someone else's free service) and back off when it's busy; when it's down, runs play what you hold.

    mmk server --dir /srv/smcraft --upstream tgrcode --exe ~/smm-work/extracted/smm2-v303-exefs/main

## HTTPS and browsers

The server speaks plain HTTP and WebSocket. A page served over HTTPS may only call `https://` and
`wss://` addresses, so for the web build put a TLS reverse proxy in front, for example Caddy:

```
smcraft.example.org {
    reverse_proxy /api/* 127.0.0.1:8640
    reverse_proxy /relay 127.0.0.1:8641
}
```

Browser players then use `wss://smcraft.example.org/relay` as the relay address. The UDP relay
can't go through an HTTP proxy; desktop players connect to UDP port 8640 directly.

## Accounts

An account is a display name (up to 10 characters) and a secret token, created with:

```sh
mmk register Ada --server https://smcraft.example.org
# maker ID NVV-TJH-X8G  (Ada)
# token 3f9c…            ← shown once; keep it
export MMK_TOKEN=3f9c…
```

The token is the only credential: send it as `Authorization: Bearer <token>`. Anyone holding it
acts as that player, so treat it like a password. The server stores only its SHA-256, so a copy of
`accounts.json` can't be used to sign in. There's no password reset: a lost token means a new
account.

Uploads without a token still work (shown under the `X-Maker` name, with no maker ID). Votes and
comments need an account.

## Verified records

The engine is deterministic, so a clear can be sent as a **replay**: the inputs of every frame.
`POST /api/courses/<code>/replay` makes the server play the replay on the course itself; the clear
time is the replay's, not the client's word, and a best time becomes a *verified* record whose run
anyone can fetch (`GET …/replay`) to race as a ghost. Start the server with `--verified-records`
to accept records only this way (typed-in times then count as clears only). Replays are
SMCraft's engine's (archived); the recomp's runs don't send them yet.

A replay is tied to the engine version that recorded it (engine updates that change how courses
play bump it); the server refuses replays from another version.

## Moderation

With an admin token set:

```sh
curl -X POST -H "Authorization: Bearer $MMK_ADMIN_TOKEN" http://localhost:8640/api/admin/ban/<account id>
curl -X POST -H "Authorization: Bearer $MMK_ADMIN_TOKEN" http://localhost:8640/api/admin/unban/<account id>
curl -X DELETE -H "Authorization: Bearer $MMK_ADMIN_TOKEN" http://localhost:8640/api/courses/<code>
```

Banned accounts can't upload, vote or comment, and drop out of the leaderboards. Players can
delete their own courses with their own token.

## Online play

The relay (UDP on the HTTP port, WebSocket on the next) puts up to 4 players in a named room and
forwards their inputs; SMCraft's players used it (see the `smcraft-archive` branch). The recomp
doesn't use it yet.

## API

The HTTP API is documented at the top of `core/crates/server/src/lib.rs`; the relay protocol at
the top of `core/crates/netplay/src/relay.rs`. The Rust client is `core/crates/online`.
