# Installing RGS

RGS is one program plus a settings file. Download the build for your system from the
[releases page](https://github.com/SpiritEXEWasTaken/rgs/releases), or build it yourself with `cargo build --release` (needs [Rust](https://rustup.rs)).

| System | Download |
|---|---|
| Mac with Apple Silicon (Mac mini M1 and later) | `macos-arm64` |
| Intel Mac | `macos-x64` |
| Linux PC or server | `linux-x64` |
| Linux on ARM (Raspberry Pi 4/5, ARM VPS) | `linux-arm64` |
| Windows | `windows-x64` |

Each download has `rgs-server`, a run script, `rgs.example.toml`, and a service file for its
system. Copy `rgs.example.toml` to `rgs.toml` and change what you need.

The server keeps its database and `secret.key` in the `data` folder. **Back up `data`**,
especially `secret.key`: without it, everyone's 2FA stops working.

Then link it to the RGS website: at https://spiritexe.pages.dev/rgs/servers/
make an account (2FA required), press **Link server**, and add the two lines it shows to
`rgs.toml`. Restart RGS, then open the server's **Admin panel** from that page and create the
owner account (the first account on a server becomes its owner).

## Linux (systemd)

```sh
sudo useradd --system --home /opt/rgs --shell /usr/sbin/nologin rgs
sudo mkdir -p /opt/rgs
sudo tar xzf rgs-*-linux-x64.tar.gz --strip-components=1 -C /opt/rgs
sudo cp /opt/rgs/rgs.example.toml /opt/rgs/rgs.toml
sudo chown -R rgs:rgs /opt/rgs
sudo cp /opt/rgs/rgs.service /etc/systemd/system/
sudo systemctl enable --now rgs
journalctl -u rgs -f        # watch the log
```

systemd starts RGS at boot and brings it back after scheduled restarts.

## Mac (launchd), e.g. a Mac mini

```sh
mkdir -p ~/rgs
tar xzf rgs-*-macos-arm64.tar.gz --strip-components=1 -C ~/rgs
cp ~/rgs/rgs.example.toml ~/rgs/rgs.toml
xattr -dr com.apple.quarantine ~/rgs   # downloads aren't signed by Apple yet
```

Edit `~/rgs/dev.rgs.server.plist`: replace every `YOU` with your Mac username. Then:

```sh
sudo cp ~/rgs/dev.rgs.server.plist /Library/LaunchDaemons/
sudo launchctl bootstrap system /Library/LaunchDaemons/dev.rgs.server.plist
tail -f ~/rgs/rgs.log       # watch the log
```

launchd starts RGS at boot (even before anyone logs in) and brings it back after scheduled
restarts. To keep a Mac mini serving, turn off sleep in System Settings > Energy, and turn on
"Start up automatically after a power failure".

## Windows

Unzip somewhere like `C:\RGS`, copy `rgs.example.toml` to `rgs.toml`, then run:

```powershell
powershell -ExecutionPolicy Bypass -File .\run-server.ps1 -Dir C:\RGS
```

(Windows blocks scripts by default; `-ExecutionPolicy Bypass` allows just this one without
changing the setting for the whole PC.) It restarts RGS after scheduled restarts. To start it at boot, add that command to Task
Scheduler with "Run whether user is logged on or not".

## Updating

1. Put the new `rgs-server` (or `rgs-server.exe`) next to the old one, named
   `rgs-server.new` (`rgs-server.new.exe` on Windows).
2. In the admin panel, schedule a restart. Players get the countdown.
3. When the server stops, the run script installs the new build and starts it. The old one is
   kept as `rgs-server.old`.

## Putting it online (free)

Linking the server to the RGS website (above) is all it takes: the server connects out to the
website, and players, games and the admin panel reach it through the website's relay. Keep RGS
listening on `127.0.0.1`; no ports are opened on your router and your home IP stays hidden.
Games on the same home network can still use the server's local address directly.
