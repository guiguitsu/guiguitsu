#!/bin/bash

set -e

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR"/..

cargo build --release

cp target/release/guiguitsu /pub_data/installation/bin
