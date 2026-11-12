# Flexspeak Monster server

The server behind the Monster collection page in Flexspeak. It holds accounts, rolls each can's card with a secret only it knows, stores collections and answers the leaderboard. It is one Rust process with one SQLite file.

To host it, see [`DEPLOY.md`](DEPLOY.md): a step-by-step guide for a server owner, with the production setup in [`deploy/`](deploy/). This file is for working on the server.

Every call the app makes is specified in [`crates/monster-core/src/api.rs`](../crates/monster-core/src/api.rs); the rules both sides share (flavours, rarities, the roll, the rankings) are in [`crates/monster-core/src/lib.rs`](../crates/monster-core/src/lib.rs).

## Running it locally

```sh
cd server
DEV_CODE=123456 PASSKEYS=off PUBLIC_URL=http://localhost:8787 DATA_DIR=./data cargo run
```

With `DEV_CODE` every sign-in code is `123456` and no email is sent; with `PASSKEYS=off` the code alone signs in. Then, for example:

```sh
curl -i -X POST localhost:8787/v1/login/start -H 'content-type: application/json' -d '{"email":"sam@example.com"}'
curl -s -X POST localhost:8787/v1/login/finish -H 'content-type: application/json' -d '{"email":"sam@example.com","code":"123456"}'
# Take "session" from the answer:
curl -s -X POST localhost:8787/v1/cans -H "authorization: Bearer $SESSION" -H 'content-type: application/json' -d '{"flavour":"mango-loco","key":"first"}'
curl -s localhost:8787/v1/leaderboard -H "authorization: Bearer $SESSION"
```

Checks, all from this folder: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.

## Environment

| Variable | Default | What it does |
| --- | --- | --- |
| `PORT` | `8787` | Port to listen on, on all interfaces. |
| `DATA_DIR` | `/data` | Folder for `monster.sqlite`; created if missing. |
| `PUBLIC_URL` | `http://localhost:$PORT` | The server's own origin as people's browsers reach it, such as `https://monster.example.com`. Passkeys are tied to its host name, so changing it later makes existing passkeys stop working. |
| `PASSKEYS` | `required` | `required`: a sign-in code leads to creating a passkey, which then signs in. `off`: a code alone signs in, and passkey sign-in is turned off. |
| `RESEND_API_KEY` | | Sends sign-in codes through [Resend](https://resend.com). Needs `MAIL_FROM`. |
| `MAIL_FROM` | | The sender, such as `Flexspeak <monster@example.com>`, on a domain verified with Resend. |
| `DEV_CODE` | | Six digits that every sign-in code is, with no email sent. For local use only: anyone can sign in as anyone. |
| `RUST_LOG` | `info` | Log filter. |

Exactly one of `RESEND_API_KEY` and `DEV_CODE` must be set; the server refuses to start otherwise.

Behind a proxy (as on most hosts), the server reads the caller's address from the last entry of `X-Forwarded-For` for its per-address limits. Don't expose it directly to the internet without one, or that header can be made up.

## Docker

The image is built from the repository root, since it needs the shared crate:

```sh
docker build -f server/Dockerfile -t flexspeak-monster .
docker run -p 8787:8787 -v monster-data:/data \
  -e PUBLIC_URL=http://localhost:8787 -e DEV_CODE=123456 -e PASSKEYS=off \
  flexspeak-monster
```

It runs as an unprivileged user (uid 10001) and keeps everything in the `/data` volume. A host that mounts a volume owned by root must make it writable for that user, or run the container as root. The health check runs `flexspeak-monster healthcheck`, which asks the server for `/health`.

[`compose.yaml`](compose.yaml) is an example for trying it out locally, with a named volume: `docker compose up --build` from this folder. The production setup (the published image, Caddy for HTTPS, Watchtower for updates) is in [`deploy/`](deploy/).

## Publishing

On every push to `main` that touches `server/`, `crates/monster-core/` or the workflow, [`server.yml`](../.github/workflows/server.yml) lints, tests and test-builds the image, then publishes it as `ghcr.io/<owner>/flexspeak-monster:latest` and `:<commit sha>`. Pull requests never publish. Hosts running [`deploy/compose.yaml`](deploy/compose.yaml) pick up `:latest` within minutes.

## Backups

`flexspeak-monster backup <file>` writes a consistent copy of the database in `DATA_DIR` to `<file>` while the server keeps running (SQLite's `VACUUM INTO`, which includes what is still in the write-ahead log), refuses to overwrite a file, and checks that the copy opens. The image has no `sqlite3` tool, so this is how [`deploy/backup.sh`](deploy/backup.sh) takes its nightly copy.

## What it stores

Everything is in `monster.sqlite`:

| Table | Holds |
| --- | --- |
| `users` | Email address, display name, when the account was made, whether it is hidden, whether local cans were imported, the account's secret roll key, its passkey user handle, and the last can number handed out. |
| `login_codes` | For each address with a pending code: the code's SHA-256 hash, when it runs out (10 minutes), and the tries left (5). |
| `hits` | One row each time a rate-limited thing happens (a code sent to an address, a code or passkey sign-in asked for from a network address, a settings save by an account), kept for an hour. |
| `sessions` | The SHA-256 of each session token, whose it is, and when it runs out (30 days after it was last used). |
| `tickets` | The SHA-256 of each passkey ticket, its kind, whose it is once known, and whether the passkey page finished it. 15 minutes, used once. |
| `passkeys` | Each passkey's credential id and public key, as `webauthn-rs` stores it, with when it was made and last used. |
| `challenges` | Identity-linking challenges: whose, and when they run out (5 minutes, used once). |
| `identities` | Linked TeamSpeak identities: uid, public key, where it was found, when it was linked. Never a private key. |
| `cans` | Every can: its number in the collection, flavour, time, rarity and seed, whether it was imported (`legacy`), when it was removed, and the app's key for retrying an add. Removed cans stay, so their numbers are never reused, but are never listed or counted. |
| `settings` | One row per account: the app's settings as the JSON text it sent (an object, at most 256 KB), the version (counts saves from 1) and when it was last saved. The server never looks inside or logs it; only the app knows what is in it. |

Sign-in codes, session tokens and tickets are only stored as hashes, and none of them, nor email bodies, nor settings, are logged. `DELETE /v1/me` removes the account's row and, with it, everything above that belongs to it.

Backups are copies of this file; see [Backups](#backups) above and [`DEPLOY.md`](DEPLOY.md).
