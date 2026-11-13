# RGS: Rust Game Server

A self-hostable backend for multiplayer games: accounts with optional 2FA (required for admins), cloud saves,
lobbies, real-time rooms, moderation, and an admin panel. It's an alternative to hosted
services like Photon and PlayFab that you run yourself, from a home PC up to a small cluster.

Made by [SpiritEXE](https://github.com/SpiritEXEWasTaken). **Status:** early development. Nothing is ready for production yet.

- Players never see each other's IP addresses: all traffic goes through the server.
- Everything is encrypted in transit; passwords use Argon2id; 2FA secrets are encrypted at rest.
- One binary with SQLite to start; Postgres and more nodes when you grow.

See [docs/DESIGN.md](docs/DESIGN.md) for how it works and [docs/ROADMAP.md](docs/ROADMAP.md) for
what's done and what's next.

## License

MIT. See [LICENSE](LICENSE).

## Try it

You need [Rust](https://rustup.rs) (on Windows, also the Visual Studio C++ Build Tools).

```
cargo run
```

The server only serves its API. Its admin panel and player pages live on the RGS website
(https://spiritexe.pages.dev/rgs/): make an account there, link your server, and
put the two lines it gives you (`hub_url`, `hub_key`) in `rgs.toml`. The server then connects out to
the website, so the panel, player sign-in and games reach it from anywhere, with no ports opened.
Settings go in `rgs.toml` (see `rgs.example.toml`).

To run it as a service on Linux, a Mac (like a Mac mini) or Windows, see
[docs/INSTALL.md](docs/INSTALL.md). Builds for all three come from GitHub Actions on each release.

To check everything works, run `./scripts/smoke-test.ps1` against a server with an empty
database.
