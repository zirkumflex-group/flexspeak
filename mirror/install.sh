#!/bin/sh
# Installs or updates the Flexspeak update mirror on the Beelink. Run from this directory:
#   sudo ./install.sh
# Safe to run again. DNS (updates.zirkumflex.io, proxied) is set up separately; see README.md.
set -eu
cd "$(dirname "$0")"

python3 -B -m unittest -q test_sync

stamp=$(date -u +%Y%m%dT%H%M%SZ)
backup=/srv/backups/host-updates/flexspeak-mirror-install-$stamp
install -d -m 700 "$backup"
for f in /srv/flexspeak-updates/sync.py /etc/systemd/system/flexspeak-mirror.service \
         /etc/systemd/system/flexspeak-mirror.timer /etc/caddy/sites-enabled/flexspeak-updates.caddy; do
    if [ -e "$f" ]; then cp -p "$f" "$backup/"; fi
done

if ! id flexspeak-mirror >/dev/null 2>&1; then
    useradd --system --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin flexspeak-mirror
fi
install -d -o root -g root -m 755 /srv/flexspeak-updates
install -d -o flexspeak-mirror -g flexspeak-mirror -m 755 /srv/flexspeak-updates/public
# Root-owned: the service runs this code but can't change it.
install -o root -g root -m 644 sync.py /srv/flexspeak-updates/sync.py
install -o root -g root -m 644 README.md /srv/flexspeak-updates/OPERATIONS.md
install -o root -g root -m 644 flexspeak-mirror.service flexspeak-mirror.timer /etc/systemd/system/
systemctl daemon-reload
systemd-analyze verify /etc/systemd/system/flexspeak-mirror.service /etc/systemd/system/flexspeak-mirror.timer

install -o root -g root -m 644 flexspeak-updates.caddy /etc/caddy/sites-enabled/flexspeak-updates.caddy
if ! caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile >/dev/null 2>&1; then
    echo "Caddy rejects the configuration; restoring the previous site file." >&2
    if [ -e "$backup/flexspeak-updates.caddy" ]; then
        cp -p "$backup/flexspeak-updates.caddy" /etc/caddy/sites-enabled/
    else
        rm -f /etc/caddy/sites-enabled/flexspeak-updates.caddy
    fi
    caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile
    exit 1
fi
systemctl reload caddy
systemctl enable --now flexspeak-mirror.timer
echo "Installed. Previous files: $backup"
