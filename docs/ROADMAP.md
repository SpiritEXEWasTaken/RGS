# RGS roadmap

Each milestone is usable on its own. Lanternfall moves over as milestones land.

## M1: Accounts (done)
- [x] Rust workspace, config file, SQLite with migrations, health endpoint
- [x] Sign up with a username and password (Argon2id)
- [x] Optional TOTP 2FA (recommended), with recovery codes; required for staff in the admin panel
- [x] Log in with password, then 2FA if turned on; one revocable token per login (stored hashed); log out; revoke sessions
- [x] Device login codes (headsets)
- [x] Roles; the first account created becomes the owner
- [x] Rate limiting on auth routes
- [x] Admin panel skeleton: sign in, list accounts, change roles, audit log

## M1.5: Updates and maintenance (done)
- [x] Scheduled restarts and maintenance, with countdown announcements (60 to 1 minutes)
- [x] Players turned away with clear errors while it's down; staff can still sign in
- [x] Minimum game version per game (`update_required`)
- [x] Error codes list (docs/ERRORS.md)
- [x] Run script that restarts the server and installs a waiting build

## M2: Website (done)
- [x] RGS website on Cloudflare (free plan): what RGS is, operator accounts (2FA required)
- [x] Link servers to an account; only the owner sees and manages them; status every minute
- [x] Relay: servers connect out, so the website reaches them from anywhere (no tunnel or ports)
- [x] Admin panel and player pages moved from the server to the website

## M3: Saves and moderation (mostly done)
- [x] Cloud saves (named slots per game)
- [x] Reports from inside games, and a report queue in the admin panel
- [x] Staff badges games can show (and staff can hide)
- [x] Built-in account page: sign up, log in, approve a headset, 2FA
- [ ] Bans (account and hashed IP), mutes, kicks

## M4: Lobbies and rooms
- [ ] Public and private (code) matchmaking, version checks
- [ ] WebSocket rooms: state sync, events, relay
- [ ] Voice relay
- [ ] Unity SDK (C#), and moving Lanternfall off Photon

## M5: Scale and polish
- [ ] Postgres support, multiple nodes, room placement
- [ ] Metrics page, status endpoint (for a Discord status bot)
- [ ] Platform account linking (Meta, Steam)
- [ ] Currency, cosmetics and purchases handled by the server
