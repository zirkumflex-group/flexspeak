#!/bin/sh
# Takes a consistent copy of the collection server's database while it keeps running, puts
# it in a dated file, and deletes copies older than KEEP_DAYS days.
#
#   ./backup.sh                  copies into ./backups next to this script
#   ./backup.sh /path/to/folder  copies into that folder
#   KEEP_DAYS=30 ./backup.sh     keeps 30 days instead of 14
#
# Run it from anywhere; it works on the compose.yaml in its own folder. It needs Docker and
# the server running. DEPLOY.md says how to run it every night and how to restore a copy.

set -eu

# cron starts scripts with a short PATH that may not include Docker.
PATH=/usr/local/bin:/usr/bin:/bin:$PATH
cd "$(dirname "$0")"

dest_dir=${1:-$PWD/backups}
keep_days=${KEEP_DAYS:-14}
name="monster-$(date -u +%Y-%m-%d-%H%M%S).sqlite"

# Copies hold every account's email address: only the owner may read them.
umask 077
mkdir -p "$dest_dir"

# Whatever happens below, don't leave the temporary copy in the server's volume.
cleanup() {
    docker compose exec -T monster rm -f "/data/$name" >/dev/null 2>&1 || true
}
trap cleanup EXIT

# 1. The server writes the copy into its own volume. It uses SQLite's VACUUM INTO, which
#    reads one consistent snapshot, and checks that the copy opens.
docker compose exec -T monster flexspeak-monster backup "/data/$name"

# 2. Copy it out of the container to this machine.
docker compose cp "monster:/data/$name" "$dest_dir/$name"
chmod 600 "$dest_dir/$name"

# 3. Delete copies older than KEEP_DAYS days.
find "$dest_dir" -type f -name 'monster-*.sqlite' -mtime +"$keep_days" -delete

echo "Saved $dest_dir/$name"
