#!/bin/sh
# Starts RGS on Mac or Linux, installing a waiting update first.
#
# To update: put the new build next to the server as rgs-server.new, then schedule a restart in
# the admin panel. When the server stops, this script swaps the new build in (the old one is
# kept as rgs-server.old) and starts it again.
#
#   ./run-server.sh /opt/rgs          run once; systemd or launchd restarts it after it stops
#   ./run-server.sh /opt/rgs --loop   keep restarting it yourself (no service manager)
#
# The folder holds rgs-server and rgs.toml; the server runs from there.

set -u
DIR="${1:-$(pwd)}"
LOOP="${2:-}"
cd "$DIR" || exit 1

start() {
    if [ -f rgs-server.new ]; then
        echo "Installing the new build..."
        [ -f rgs-server ] && mv -f rgs-server rgs-server.old
        mv -f rgs-server.new rgs-server
        chmod +x rgs-server
    fi
    if [ ! -x rgs-server ]; then
        echo "No rgs-server in $DIR" >&2
        exit 1
    fi
    echo "Starting RGS ($(date))"
}

if [ "$LOOP" = "--loop" ]; then
    while true; do
        start
        ./rgs-server
        echo "RGS stopped with code $? ($(date)). Starting again in 3 seconds; Ctrl+C to quit."
        sleep 3
    done
else
    start
    exec ./rgs-server
fi
