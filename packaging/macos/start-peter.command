#!/bin/bash
# Starts Peter from this folder. No installation or admin password needed.
cd "$(dirname "$0")" || exit 1

# Files downloaded from the internet are marked "quarantined", and macOS will
# not run unsigned programs that carry the mark. These are your own files, so
# removing the mark needs no password.
xattr -dr com.apple.quarantine . 2>/dev/null
chmod +x ./peter-server 2>/dev/null

export PETER_BIND="127.0.0.1:5173"
export PETER_APP_URL="http://localhost:5173"
export PETER_WEB_DIR="$PWD/web"

# Your AI keys, if you made my-keys.sh (see my-keys.example.sh).
[ -f ./my-keys.sh ] && source ./my-keys.sh

# Open the browser a few seconds after the server starts.
( sleep 3; open "http://localhost:5173" ) &

echo
echo "  Peter is starting. Your browser will open at http://localhost:5173"
echo "  Keep this window open while you use Peter. Close it (or press Ctrl+C) to stop Peter."
echo
./peter-server
echo
echo "  Peter has stopped. You can close this window."
