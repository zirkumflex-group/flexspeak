# Monster collection: server-side plan

Date: **2026-11-12**. The design the server in `server/` was built from. The shape: a Rust server in a container, one SQLite file on a volume, sign-in by email without a password, then a passkey, then optionally linking TeamSpeak identities. A can is added by picking the flavour just drunk; nothing is scanned. A leaderboard ranks collections, and anyone on it can open another person's board. Choices marked **Assumed** were assumptions and can change.

## Status

**Built:** the server (`server/`, see its README), the rules shared with the app (`crates/monster-core`, whose `api` module is the contract and wins where this document differs), and the app's side: a collection server address built into the app (`FLEXSPEAK_COLLECTION_SERVER` in `.cargo/config.toml`), sign-in by email code, a passkey made on a page the server hosts while the app waits, linking Flexspeak's own identity, the collection, the one-time upload of cans kept on the computer, the leaderboard and other people's boards.

**Not built yet:** the sign-in link in the email (the code only), invites, review flags and the events table, the offline queue and "unopened" cans, finding TeamSpeak 3 and 6 identities, removing a passkey or unlinking an identity from the page, the nightly backup, and a deployment. Passkeys go through the browser on every system; the system dialogs described under Passkey are not used.

**Never run:** sending a real email, a passkey in a real browser, and the Windows build of the app's side.

## Why a server

Today everything is local (`monster.sqlite`), with the roll key in the same file. So anyone can read the key, or edit the database. A server fixes both, and makes shared features possible: the leaderboard and other people's boards.

## The first version, in one paragraph

One small Rust server process with one SQLite file on a persistent volume. To sign in the first time, a person types their email address in the app, receives a six-digit code and a link by email, and types the code or opens the link. They then create a passkey, which is what signs them in from then on. After that they can link any or all of the TeamSpeak identities found on the computer, or skip it, and the page opens. Adding a can sends the flavour; the server rolls its rarity with a secret key, stores it with the time and answers, and the app plays the reveal.

## What the server owns

| Moves to the server | Stays in the app |
| --- | --- |
| Accounts, sessions, passkeys and linked identities | The flavour picker, search and favourites |
| Rolling rarity, with a secret key | The reveal animation and sound |
| The collection (cans, flavour, when, by whom) | The 3D can and card rendering |
| The leaderboard and who is hidden | Board statistics, computed from the cans it's given |
| Rate limits and review flags | A local cache, so the page opens offline |

## Stack

| Part | Choice | Why |
| --- | --- | --- |
| Language | Rust | Same as the app, so both use one shared crate for the rules they must agree on |
| HTTP | axum on tokio | The app already runs tokio |
| Database | One SQLite file through `rusqlite`, WAL mode | The app already bundles `rusqlite` |
| Shared crate | `monster-core`: flavours, the odds table, the roll, the reveal ladder, the leaderboard's rankings | One definition of each, not two |
| Email | An email service's HTTP API, called with `reqwest` | **Assumed: Resend.** Needs a sending domain you control. The server can't deliver mail itself: Railway disables outbound SMTP below its Pro plan, and mail sent straight from a cloud server's address is mostly treated as spam |
| Host | One Railway service with a volume mounted at `/data` | **Assumed.** Any host with a persistent disk works |
| Code | A `server/` crate in this repository | **Assumed.** It needs the shared crate. The release workflow must then ignore server-only changes, because a push to `main` publishes an app release today |

There is no ready-made sign-in library here as Better Auth would have been for TypeScript, so sign-in is a small amount of our own code. Passwordless email keeps it small: there are no passwords to hash, store or reset.

What one SQLite file costs:

- **One server instance only.** Two processes on two machines can't share the file. Fine at this size; moving to Postgres later is a data copy.
- **The volume is the only copy** until there's a backup. Plan: a nightly `VACUUM INTO` copy to object storage, and a restore that has been tried once.
- Deploys restart the one process, so there are a few seconds of downtime each time. The app's offline handling (below) covers it.

## Where sign-in applies

**Only the Monster page needs an account.** Everything else in Flexspeak (servers, voice, chat, settings) works as today, with no account and no call to this server.

- Signed out, the Monster page shows the sign-in form in place of the board. Nothing else on the page is reachable.
- Signed in, a saved session opens the page straight away, also without a network connection, from the local cache.
- Only the first sign-in on a device needs the network.
- The app never contacts the server until the person opens the Monster page.

Because the page always has an account behind it, the app needs only one kind of store (the server, with a local cache), not a local-only mode beside it.

## What has to be set up

Beyond the Rust server itself:

| Thing | What for | State |
| --- | --- | --- |
| A host with a persistent disk | Runs the server, holds the SQLite file | **Assumed: Railway.** Not created |
| A public HTTPS address | The app calls it, for example `api.<domain>` | Needs a domain for Flexspeak |
| An email service account | Sends the codes | A Resend account of its own, not one shared with another project. The free plan is enough: about 3,000 emails a month, 100 a day |
| A sending domain with DNS records (SPF, DKIM, DMARC) | Without them the codes land in spam | **Open: which domain?** |
| Three secrets in the host's environment | The email service key, the rarity secret, the key that hashes codes and tokens | Not created |
| A backup target | A nightly copy of the SQLite file | Not created |
| A short privacy note and a delete-account call | The server now stores email addresses | Not written |

Link tracking must be off for the sending domain, so the email service doesn't rewrite the sign-in link when that is added later.

## Signing in

Four steps the first time, shown as a strip above the form. Later sign-ins are the passkey alone.

| Step | The person | Can it be skipped? |
| --- | --- | --- |
| 1. Email | Types their address | No |
| 2. Code | Types the six-digit code from the email, or opens the link in it | No |
| 3. Passkey | Creates one with Touch ID, Windows Hello or a password manager | No |
| 4. Identities | Links any or all TeamSpeak identities found on the computer | Yes |

### Email: a code, and a link

1. The app sends the address: `POST /v1/login/start`.
2. The server makes a six-digit code and a link token, stores only their hashes, and emails both. They last ten minutes and one use.
3. The person types the code (`POST /v1/login/finish`), or opens the link on the computer Flexspeak runs on.
4. The server answers with a short **setup ticket**, not a session: it only allows creating a passkey.

Rules that keep the code safe:

- Five wrong tries cancel a code. A new one can be asked for once a minute, and a few times an hour for each address and IP.
- The answer to step 1 is the same whether or not the address has an account, so the form can't be used to find out who has one.
- A first sign-in creates the account. **Assumed:** it also needs an invite code, so one person can't farm accounts with throwaway addresses.

**What the link needs that the code doesn't.** Flexspeak registered as the handler for a `flexspeak://` address on each operating system, the app handling being opened that way, and a web page on the server in between, because many mail apps won't make a `flexspeak://` link clickable. The link also only works on the computer Flexspeak is on; the code works when the mail is read on a phone. So the code ships first and the link follows.

### Passkey

A passkey is a key pair made by the device or a password manager. The private half stays there, the server stores the public half, and using it takes a fingerprint, face or PIN.

1. With the setup ticket the app asks for registration options: `POST /v1/passkeys/start`.
2. The device makes the passkey, and the app sends the result: `POST /v1/passkeys/finish`.
3. The server stores the public key and answers with the **session token**: random, stored hashed, valid 30 days and renewed while in use. The app keeps it in the operating system's credential store (Keychain on macOS, Credential Manager on Windows).

Later sign-ins, when the session has run out or on another device where the passkey is available: `POST /v1/login/passkey/start` and `/finish`, no email. The email code is then only for a device without the passkey, or for recovery after losing it; it leads to creating a new passkey.

On the server this is the `webauthn-rs` crate. In the app it differs by operating system, and this is the expensive part of the plan:

| System | How the passkey dialog is shown | State |
| --- | --- | --- |
| Windows | The system's own WebAuthn dialog (Windows Hello), called from the app | The released platform; do this first |
| macOS | The system's dialog needs the app signed with a developer account and tied to the server's domain | **To check:** how the macOS build is signed. Until then, through the browser |
| Linux | No system dialog exists | Through the browser, or a hardware security key |

"Through the browser" means the app opens a page on the server, the passkey is made there, and the app waits for the server to say it's done.

**To check before building:** that each of these behaves as described, with a small test on each system, before any of the rest is built on it.

## Linking TeamSpeak identities

Optional, offered as the last step and later from the page's settings. The step lists **every TeamSpeak identity found on the computer**, each with its own Link button, and a Link All button. Linking saves each identity's **public key and uid** to the account. Private keys never leave the computer and are not backed up by the server.

Where identities are found:

| Source | Where | State |
| --- | --- | --- |
| Flexspeak | `identity.txt` (`client::load_identity`), one for each install | Readable today |
| TeamSpeak 6 | Its `settings.db`, which `ts6_import` already opens for other settings | **To check:** where and how it stores identities. Not documented |
| TeamSpeak 3 | Its `settings.db` | **To check:** same |

TeamSpeak's files are opened read-only, and only when Link is pressed. A private key is read only to sign the challenge below, and is never copied, saved or logged, as `ts6_import` does for login data today. Identities on another computer are offered when the person signs in there.

For each identity being linked:

1. The app asks for a challenge: `POST /v1/identities/challenge`.
2. It signs `"flexspeak-monster-link-v1:" + challenge` with that identity's key (`EccKeyPrivP256::sign`) and sends the signature, public key and where it was found: `POST /v1/identities`.
3. The server verifies the signature (`EccKeyPubP256::verify`) and stores the public key and uid on the account. The signature is what stops someone claiming an identity they don't hold.

What the links are for: a TeamSpeak server shows each person's identity uid, so Flexspeak could later show a channel member's board next to their name, whichever client they connect with. That is opt-in and not in the first version.

Rules:

- An account can have any number of identities, from any computer. An identity belongs to one account; linking one that's already linked elsewhere is refused.
- An identity can be unlinked from the page's settings.
- **No identity signs anyone in.** The passkey does that, so a wrongly linked identity could at worst attach a name to a board, not open an account.
- **To check before building:** that a TeamSpeak server can't get a client to sign our challenge text during a normal connection. The fixed prefix is there for that.

## Data model

```
users       id, email, name, created_at, flagged, hidden  -- hidden: off the leaderboard
login_codes email, code_hash, link_hash, expires_at, tries
passkeys    credential_id, user_id, public_key, counter, name, created_at, last_used_at
sessions    token_hash, user_id, expires_at, last_used_at
identities  public_key, uid, user_id, source, linked_at  -- one row for each linked identity
invites     code, made_by, used_by
user_keys   user_id, key                              -- the roll key, made at sign-up
cans        id, user_id, flavour, at, rarity, seed, legacy, removed_at
events      id, user_id, kind, detail, at             -- adds, removals, refusals, for review
```

Rarity and seed are **stored** on the can, not recomputed. That lets the formula or the secret change later without changing anyone's cards.

## API

All JSON over HTTPS. Everything after sign-in needs the bearer token.

| Call | Does |
| --- | --- |
| `POST /v1/login/start`, `POST /v1/login/finish` | Email code or link; answers with a setup ticket |
| `POST /v1/passkeys/start`, `POST /v1/passkeys/finish` | Create a passkey; answers with a session |
| `POST /v1/login/passkey/start`, `POST /v1/login/passkey/finish` | Sign in with a passkey |
| `DELETE /v1/passkeys/{id}` | Remove a passkey |
| `POST /v1/identities/challenge`, `POST /v1/identities`, `DELETE /v1/identities/{uid}` | Link and unlink identities |
| `DELETE /v1/session` | Sign out |
| `POST /v1/cans` | Adds a can of a flavour, drunk now. Returns it with its number and rarity |
| `GET /v1/cans?since=` | The collection, or what changed since a moment |
| `DELETE /v1/cans/{id}` | Removes a can |
| `GET /v1/leaderboard?by=collection\|legendary\|flavours` | Everyone not hidden, ranked, with their totals |
| `GET /v1/users/{id}/board`, `GET /v1/users/{id}/cans` | Someone's board and cards, if they aren't hidden |
| `PATCH /v1/me` | Name, and whether to be hidden |
| `GET /health` | For the host |

`POST /v1/cans` takes an idempotency key, so a retry after a lost reply can't add a can twice.

## Rolling rarity

`HMAC-SHA256(user_key, flavour | server_time | can_number)`, mapped through the same odds table as today (60 / 25 / 10 / 4 / 1). The app does the same today with a key in its own database (`monster::draw`).

- `user_key` is made at random when the account is created and lives only on the server. Without it nobody can predict a roll.
- The time is the server's, so a client can't choose the moment; the can's number makes two cans in one second differ.
- The result is stored on the can and never recomputed, so a card can't change later.
- **Removing doesn't reroll.** A removed can is kept, marked removed, and doesn't count; adding a can afterwards is a new can with a new number, not a second try at the old one. So removing and re-adding only gives another ordinary roll, the same as adding one more can.

## Stopping cheating

Nothing proves a can was drunk: a person can tap a flavour without one. What the server can do is make cheating pointless rather than impossible.

1. **Secret roll on the server.** Nobody can predict or choose a card.
2. **Server-held collection.** Ends editing the local database.
3. **Rate limit**: a few adds a minute for each user and IP, against scripts. No daily cap and nothing based on how much someone drinks.
4. **The leaderboard ranks collections, not counts.** Tapping a hundred times mostly adds Commons of flavours already held, so padding a count barely moves a place.
5. **Review flags.** Bursts of adds, or totals far above everyone else's, are flagged for a look, never refused automatically.

## Offline and the reveal

The roll needs the server, so an offline add can't show a card.

- Offline, or while the server restarts, the can is saved locally as **unopened**: it counts in the board straight away and shows as the plain aluminium can the reveal already uses for a card's back.
- When the server answers, the reveal plays.
- Online, the request goes out when a flavour is tapped and the reveal's first 160 ms cover the wait. If the answer is slow, the back keeps trembling until it arrives.

## Moving existing cans

On first sign-in the app uploads the local collection once.

- **Assumed: keep their rarity.** The server stores what the app rolled and marks those cans `legacy`. Rerolling would change cards people have already seen. Cans from the scanning version were already moved into the same shape on the computer (schema 2), keeping their cards.
- The local file stays as a cache and a backup.

## Privacy

The server learns: an email address, a display name, passkey public keys, linked TeamSpeak identity keys, and which flavour was drunk when. That is a record of what someone drinks and when, tied to an identity other TeamSpeak users can see.

- **Assumed:** a new account shows on the leaderboard, and Hide Me takes it off at once. A hidden person's place, board and cards can't be fetched by anyone else. Showing by TeamSpeak identity is a separate opt-in.
- A board shows what someone drinks and when (the heatmap), so the leaderboard page says plainly that others can open it.
- The email address is used for sign-in codes only.
- An export and a delete-everything call from day one.

## Changes in the app

1. Done: `Can` stores its rarity and seed, rolled once when it's added.
2. `Tracker` talks to the server through one store, which keeps the existing local SQLite file as its cache.
3. The shared rules move into `monster-core`.
4. The Monster page is gated: signed out, it shows only the sign-in steps. Their screens exist already as preview scenes (`ui::monster::sign_in`, `monster::Gate`); they need wiring to the server. The token goes in the credential store. A sign-out button on the page.
5. The passkey dialog for each operating system, and optional identity linking.
6. A new "unopened" state for cans waiting on the server.
7. A setting for the server address, so it can be self-hosted or switched off.

## Order of work

| Step | Ships | Proves |
| --- | --- | --- |
| 0 | In the app: the shared crate; the store interface. No server yet (rarity is already stored on the can) | The app still works exactly as now |
| 1 | Server: email code sign-in, SQLite on the volume, add, list, remove, secret roll. App signs in and uses it from the WIP page. The code alone gives a session in this step | The main cheat is closed |
| 2 | Passkeys: Windows first, the browser route for macOS and Linux. From here the code only leads to creating a passkey. Optional identity linking | Nobody types a code twice on one device |
| 3 | Upload of existing cans; nightly backup; leaderboard and other people's boards | The data survives a lost volume, and collections are shared |
| 4 | Rate limits and review flags; offline queue and unopened cans | Abuse is visible, and it works on a train |
| 5 | Boards by TeamSpeak identity; the sign-in link that opens the app | The GitHub-profile idea, shared |

Steps 0 and 1 are the smallest useful slice.

## Adding a can

Decided on 2026-11-12: **no scanning.** A person picks the flavour they just drank, and the can is saved with that moment. This replaced every earlier version (barcode and base code, scanned or typed), and the camera, text recognition and Open Food Facts code are gone from the app.

- The Add panel lists every flavour as a button. The six most drunk lead under Your Favourites; the rest follow, most drunk first, then the ones never drunk in their usual order.
- A search field keeps the flavours whose name holds every word typed; Enter adds the first match.
- One tap adds the can and plays the reveal. The large card's bin removes a can added by mistake.
- A tap while a card is still opening is ignored, so a double tap doesn't add two cans.
- **Open:** a way to set an earlier time, for a can drunk away from the computer. Today a can is always saved as drunk now.

## Leaderboard and other people's boards

A third tab on the page, beside Overview and Flavours. Its screens exist as preview scenes (`monster-leaderboard`, `-legendary`, `-hidden`, `monster-person`), with made-up people until the server exists.

- **Ranked by collection, never by cans drunk.** Three rankings: different cards (each flavour at each rarity, out of 5 per flavour), Legendary cards, and different flavours. A hundred cans of one flavour don't beat three different flavours. This keeps the page from rewarding drinking more, in line with "Not in this plan" below.
- Ties share a place. The top three show in gold, silver and bronze.
- **Clicking a person opens their board:** the same stats, heatmap, flavour mix and card collection as your own Overview, read-only. Their cards can't be featured, replayed or removed. Back returns to the leaderboard.
- Your own row is marked You and opens your Overview.
- **Hide Me** takes you off the leaderboard and stops others opening your board; Show Me puts you back.
- The server answers the leaderboard from stored totals, recomputed when a can is added or removed, so it doesn't scan every can on each request.

## Not in this plan

- Passwords.
- Backing up a TeamSpeak identity's private key on the server.
- Trading, gifting or selling cards.
- Leaderboards that rank by cans drunk.
- Anything that changes with how much someone drinks in a day.
- Hosting Monster's artwork.
