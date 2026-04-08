#!/bin/bash
# Record duru demo as GIF (fully automated)
set -e
cd "$(dirname "$0")"

CAST_FILE="demo.cast"
GIF_FILE="demo.gif"

cargo build --release 2>/dev/null

# DURU_NO_ALT_SCREEN lets asciinema capture the TUI output
export DURU_NO_ALT_SCREEN=1

asciinema rec "$CAST_FILE" --cols 120 --rows 30 --overwrite --command "
expect -c '
    spawn ./target/release/duru --demo
    sleep 2

    # Navigate projects
    send \"\033\[B\"; sleep 0.6
    send \"\033\[B\"; sleep 0.6
    send \"\033\[B\"; sleep 0.6

    # Enter files pane
    send \"\033\[C\"; sleep 0.8

    # Browse files
    send \"\033\[B\"; sleep 0.6
    send \"\033\[B\"; sleep 0.6

    # Enter preview
    send \"\033\[C\"; sleep 1.2

    # Scroll
    send \"\033\[B\"; sleep 0.3
    send \"\033\[B\"; sleep 0.3
    send \"\033\[B\"; sleep 0.3
    send \"\033\[B\"; sleep 0.3
    sleep 1.5

    # Back to projects
    send \"\033\[D\"; sleep 0.4
    send \"\033\[D\"; sleep 0.6

    # Another project
    send \"\033\[A\"; sleep 0.4
    send \"\033\[A\"; sleep 0.4
    send \"\033\[C\"; sleep 0.6
    send \"\033\[C\"; sleep 1.5

    # Quit
    send \"q\"
    expect eof
'
"

echo "Converting to GIF..."
agg "$CAST_FILE" "$GIF_FILE" --font-size 14
rm -f "$CAST_FILE"
echo "Done! → $GIF_FILE"
