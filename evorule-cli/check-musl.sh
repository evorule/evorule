#!/bin/bash
BIN=/mnt/d/evorule/.build/rust/x86_64-unknown-linux-musl/release/evorule
ls -la $BIN
echo "---"
file $BIN
echo "---"
ldd $BIN 2>&1
echo "---"
$BIN --version
echo "---"
$BIN validate /tmp 2>&1 | head -3

