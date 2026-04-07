#!/bin/bash

set -e

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR"/..

DEBUG=0
for arg in "$@"; do
    case "$arg" in
        --debug) DEBUG=1 ;;
    esac
done

if [[ $DEBUG -eq 1 ]]; then
    cargo build
    cp target/debug/guiguitsu target/debug/gg /pub_data/installation/bin
else
    cargo build --release
    cp target/release/guiguitsu target/release/gg /pub_data/installation/bin
fi
