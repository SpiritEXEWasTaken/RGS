# RGS design

RGS (Rust Game Server) is a self-hostable backend for multiplayer games: accounts, saves,
lobbies, real-time rooms, moderation and an admin panel. It's meant to replace hosted services
like Photon and PlayFab for small studios that would rather run their own box. One machine to
start; more machines when a game grows.

The first game using it is Lanternfall (a co-op VR game for Quest), but nothing in RGS is
specific to that game.

## Goals

- **One binary to self-host.** `rgs-server` runs on a home PC or a cheap VPS, with SQLite and no
  other services required.
- **Players never see each other's IP addresses.** Every client talks only to the server.
  Game traffic between players is relayed, never peer-to-peer.
- **Encrypted everywhere.** All connections use TLS. Passwords and 2FA secrets are never stored
  in plain form.
- **Accounts with optional 2FA** (recommended to everyone), and **2FA required for staff** using
  the admin panel.
- **Easy device login for headsets and consoles.** Nobody types a password in VR: the game shows
  a short code, and you approve it on the website.
- **Moderation built in:** roles, reports, bans, audit log, and an admin panel.
- **Scales out later.** The same binary can run as several nodes sharing one database.

## Non-goals (for now)

- Matching every Photon or PlayFab feature. RGS covers what most small games need.
- Hosting the website. The public site is static (Cloudflare Pages) and calls the RGS API.

## A note on "end-to-end encryption"

True end-to-end encryption (where even the server can't read game traffic) would stop the
server from doing anything useful with it: no server-side checks, so no anticheat. RGS instead
does:

- **Encryption in transit** between every client and the server (TLS 1.3).
- **The server as the only party that sees traffic.** Other players never see your IP or talk
  to you directly.
- **IP privacy on the server too.** IPs are kept only briefly in memory for rate limiting. For
  bans, only a salted hash is stored, never the raw address.
- **Optional end-to-end encrypted messages** (for example private chat) can be added later as a
  separate feature where the server only relays sealed blobs.

## Architecture

```
  Quest / PC game                Website (Cloudflare Pages)        Admin panel
  (Unity + RGS SDK)              static HTML/JS                     (served by rgs-server)
        |  WSS + HTTPS                  |  HTTPS                           |
        +---------------+---------------+-----------------+----------------+
                        |                                 |
                 RGS website relay (the server connects out; hides its IP, gives TLS)
                        |
                 +-------------+        +-------------+
                 | rgs-server  |  ...   | rgs-server  |   (more nodes later)
                 |  node A     |        |  node B     |
                 +------+------+        +------+------+
                        |                      |
                        +----------+-----------+
                                   |
                         SQLite (single node) or Postgres (cluster)
```

- **The RGS website's relay** (see below): the home server connects out to it, so no ports are
  opened at home and the home IP is never public. Cloudflare provides the TLS certificate.
- **rgs-server** is one Rust binary (tokio and axum) with these parts:
  - **HTTP API** (`/api/v1/...`): accounts, auth, saves, lobbies, reports, admin.
  - **Realtime** (`/rt`, WebSocket): rooms where game state and events are relayed and checked.
  - **No web pages of its own.** The admin panel and player pages are on the RGS website, which
    reaches the server through the relay below.
- **Database**: SQLite by default; Postgres when running more than one node.

## Accounts and login

- Usernames are unique. An email is optional but needed for account recovery.
- Passwords are hashed with **Argon2id**.
- **2FA (TOTP) is optional for players, but recommended.** The website nudges you to turn it on
  after signing up. 2FA secrets are encrypted at rest with a server key.
- **Staff must use 2FA.** Moderators, admins and owners can't open the admin panel, or use any
  admin API, without 2FA turned on, and every admin session is signed in with a 2FA code.
- Recovery codes are shown once at setup and stored hashed.
- **Sessions:** one random bearer token per login, stored only as a hash, and revocable from
  the account page. Website logins last 30 days, headset logins 180 days, and admin panel logins
  12 hours (all configurable).
- **Device login** (headsets): the game asks for a code, shows something like `KX7-P2M`, and the
  player approves it on the website after logging in (with 2FA, if they use it). The device then gets
  its own refresh token, which can be revoked from the website.
- **Platform linking** (later): Meta and Steam account IDs attached to an RGS account, so saves
  and cosmetics follow the player across stores.

## Roles

`player` < `moderator` < `admin` < `owner`. Each role can do everything the roles below it can.

| Role | Can |
|---|---|
| player | Play, save, report |
| moderator | Review reports, mute, kick, temporary bans |
| admin | Permanent bans, edit accounts and roles below admin, server settings |
| owner | Everything, including making admins |

A player's role is shown in-game as a badge, which the player can hide.

## Data (first version)

- `accounts`: id, username, email, password hash, 2FA secret (encrypted), role, created and
  last-seen times, ban state.
- `recovery_codes`, `sessions`, `device_codes`.
- `saves`: account, game, slot name, JSON blob, version, updated time.
- `reports`: reporter, target, room, reason, details, status, handled by.
- `bans`: account and/or hashed IP, reason, expiry, issued by.
- `audit_log`: who did what in the admin panel, and when.
- `nodes` (cluster): id, address, load, last heartbeat.
- `rooms`: id, game, code, visibility, max players, node, player count.

## Realtime rooms

- Clients connect over WebSocket with their access token.
- **Joining:** by code (private rooms), or "best public room" (fullest open room first, with
  `max_players`), the same matchmaking Lanternfall uses today.
- **Messages:** binary frames (MessagePack): object state updates, events (RPCs) and voice
  packets.
- **Room authority:** the server keeps the current state of shared objects, and relays and
  checks updates. Owner-only fields, rate limits and plausibility checks (speed, reach) are the
  base layer of anticheat.
- **Voice:** relayed through the server, so voice doesn't leak IPs either.
- **Version check:** a room only accepts clients with the same game version.

## Relay (reaching a server from anywhere)

A server linked to an RGS website account keeps one WebSocket open *out* to the website
(`hub_url` + `hub_key`). Requests to `<website>/rgs/s/<server id>/api/v1/...` are passed down
that socket, answered by the server's own API, and sent back. So the admin panel, the player
page and games reach a home server from anywhere, with nothing opened on its network and its
address never shown. The website only relays; accounts, saves and everything else stay on the
server. Only the account that linked a server sees it in its server list.

## Updates, maintenance and errors

- Admins schedule a restart (for an update) or maintenance in the admin panel. Players get
  in-game announcements at 60, 45, 30, 15, 10, 5 and 1 minutes ("Servers restarting in 45
  minutes"), read from `GET /status`.
- When it starts, everyone but staff is turned away with `maintenance` ("RGS is rebooting right
  now"). A restart stops the server after a short grace period; `scripts/run-server.ps1` starts
  it again, installing a new build first if one is waiting.
- Each game has a `latest_version` and `min_version`. Older clients get `update_required`
  ("Update Lanternfall to keep playing").
- Every error has a stable code and a message written for players. See [ERRORS.md](ERRORS.md).

## Anticheat (layers, built up over time)

1. Server is the source of truth for saves, currency, cosmetics and unlocks. Clients ask; the
   server decides.
2. Rate limits and sanity checks on realtime messages.
3. Room-level authority for shared objects, with ownership rules.
4. Reports plus moderation tools; bans by account, plus a hashed IP as a backstop.

## Clustering (later)

- All nodes share one Postgres database.
- Nodes register themselves and send a heartbeat every few seconds.
- The node that receives a request to create a room picks the least-loaded node. The client is
  told which node to connect to, and that room stays on that node.
- If a node dies, its rooms end and clients rejoin through matchmaking.

## Website (Cloudflare Pages)

A plain static site (HTML, CSS and a little JS) in `web/`:

- Sign up, log in, set up 2FA (optional, recommended), recovery codes.
- Approve a device code.
- Account page: devices, linked platforms, delete account.
- Talks to the RGS API through the tunnel's hostname. CORS is allowed only for the site's
  domain.

## Unity SDK (later)

`sdk/unity` holds a C# package that games add to talk to RGS: login with a device code, saves,
lobbies and rooms. Lanternfall moves from Photon to it step by step:

1. Accounts and saves.
2. Lobbies.
3. Rooms and voice.
