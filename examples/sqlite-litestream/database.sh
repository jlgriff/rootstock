#!/bin/sh
set -eu
mode=${1:-prepare}
case "$mode" in
  prepare|initialize|restore) ;;
  *) echo 'Choose prepare, initialize, or restore.' >&2; exit 2 ;;
esac
case "${SQLITE_DATABASE:?set SQLITE_DATABASE}" in
  .|..|*[!a-zA-Z0-9._-]*) echo 'SQLITE_DATABASE must be a plain filename.' >&2; exit 2 ;;
esac
database="/data/$SQLITE_DATABASE"
if [ "$mode" = prepare ] && [ -f "$database" ]; then
  exit 0
fi
for path in "$database" "$database-wal" "$database-shm" "$database-journal"; do
  if [ -e "$path" ]; then
    echo 'Database state already exists; use a fresh volume for recovery.' >&2
    exit 1
  fi
done
chown "$SQLITE_UID:$SQLITE_GID" /data
chmod 700 /data
if [ "$mode" = initialize ]; then
  (umask 077; set -C; : > "$database")
  chown "$SQLITE_UID:$SQLITE_GID" "$database"
else
  temporary=$(mktemp -d /data/.restore.XXXXXX)
  trap 'rm -rf "$temporary"' EXIT
  trap 'exit 1' HUP INT TERM
  litestream restore -config /etc/litestream.yml -integrity-check full \
    -o "$temporary/database" "$database"
  chmod 600 "$temporary/database"
  chown "$SQLITE_UID:$SQLITE_GID" "$temporary/database"
  ln "$temporary/database" "$database"
fi
