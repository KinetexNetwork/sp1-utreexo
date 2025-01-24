#!/bin/bash

COMMAND="BITCOIN_CORE_RPC_URL=http://127.0.0.1:28332 BITCOIN_CORE_RPC_USER=user BITCOIN_CORE_RPC_PASSWORD=pass ./target/release/bridge"
BACKUP_BASE=~/bridge-backup

mkdir -p "$BACKUP_BASE"

while true; do
    echo "Starting command: $COMMAND"
    $COMMAND &
    CMD_PID=$!
    sleep 3600
    echo "Stopping command with PID $CMD_PID"
    kill -INT $CMD_PID
    wait $CMD_PID 2>/dev/null
    TIMESTAMP=$(date +%Y%m%d_%H%M%S)
    BACKUP_DIR="$BACKUP_BASE/$TIMESTAMP"
    mkdir -p "$BACKUP_DIR"
    echo "Backing up ~/.bridge to $BACKUP_DIR using rsync"
    rsync -a ~/.bridge/ "$BACKUP_DIR/"
    echo "Pausing for 5 seconds before restarting..."
    sleep 5
done
