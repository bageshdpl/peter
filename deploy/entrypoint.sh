#!/bin/sh
# Starts Peter in a container. With BACKUP_S3_BUCKET set, the database is
# restored from the backup on start and copied there continuously while
# running, so nothing is lost when the host wipes its disk (free hosting).
set -e

export PETER_BIND="0.0.0.0:${PORT:-8080}"
# Render tells the service its public address.
if [ -z "$PETER_APP_URL" ] && [ -n "$RENDER_EXTERNAL_URL" ]; then
  export PETER_APP_URL="$RENDER_EXTERNAL_URL"
fi
export BACKUP_S3_PATH="${BACKUP_S3_PATH:-peter}"
export BACKUP_S3_REGION="${BACKUP_S3_REGION:-us-east-1}"
APP_DIR="${APP_DIR:-/app}"
export PETER_DB_PATH="${PETER_DB_PATH:-/data/peter.db}"
export PETER_DATABASE_URL="sqlite://$PETER_DB_PATH"
mkdir -p "$(dirname "$PETER_DB_PATH")"

if [ -n "$BACKUP_S3_BUCKET" ]; then
  echo "Peter: restoring the database from the backup, if there is one..."
  litestream restore -config "$APP_DIR/litestream.yml" -if-db-not-exists -if-replica-exists "$PETER_DB_PATH"
  echo "Peter: starting, with continuous backup to bucket $BACKUP_S3_BUCKET."
  exec litestream replicate -config "$APP_DIR/litestream.yml" -exec "$APP_DIR/peter-server"
else
  echo "Peter: WARNING: BACKUP_S3_BUCKET is not set. On hosts that wipe the disk on restart, all data is lost then."
  exec "$APP_DIR/peter-server"
fi
