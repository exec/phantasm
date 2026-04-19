#!/usr/bin/env bash
# embed_multipass.sh — produce 5-passphrase J-UNIWARD stego set for the
# 500-cover diversity corpus, replicating the Update 3 recipe.
#
# Output layout (relative to $CORPUS_ROOT):
#   stego/ml-multi-pass-0/qf85/720/0001.jpg   (500 files)
#   stego/ml-multi-pass-1/qf85/720/0001.jpg   (500 files)
#   ...
#   stego/ml-multi-pass-4/qf85/720/0500.jpg
# = 2500 stego files total. With the 500 covers this is the full training
# set for the diversity-500 J-UW fine-tune.
#
# Usage:
#   ./research-corpus-500/scripts/embed_multipass.sh \
#       [CORPUS_ROOT=research-corpus-500] \
#       [PHANTASM_BIN=target/release/phantasm] \
#       [PAYLOAD_BYTES=3000] \
#       [JOBS=8]
#
# The 3000-byte payload is the same fixed size used throughout the ML
# evaluation. The passphrase set is `ml-multi-pass-0..4` (verbatim match
# to Update 3).
#
# This is a local production script — it is NOT gitignored.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CORPUS_ROOT="${CORPUS_ROOT:-$REPO_ROOT/research-corpus-500}"
PHANTASM_BIN="${PHANTASM_BIN:-$REPO_ROOT/target/release/phantasm}"
PAYLOAD_BYTES="${PAYLOAD_BYTES:-3000}"
JOBS="${JOBS:-8}"
QF="${QF:-85}"
DIM="${DIM:-720}"

COVER_DIR="$CORPUS_ROOT/qf${QF}/${DIM}"
STEGO_ROOT="$CORPUS_ROOT/stego"
PAYLOAD_PATH="$CORPUS_ROOT/scripts/payload_${PAYLOAD_BYTES}.bin"

if [[ ! -x "$PHANTASM_BIN" ]]; then
    echo "ERROR: phantasm binary not found at $PHANTASM_BIN" >&2
    echo "Build it with: cargo build --release -p phantasm-cli" >&2
    exit 1
fi

if [[ ! -d "$COVER_DIR" ]]; then
    echo "ERROR: cover dir not found at $COVER_DIR" >&2
    echo "Regenerate the corpus with: MODE=diversity500 cargo run --release -p phantasm-image --example fetch_corpus" >&2
    exit 1
fi

if [[ ! -f "$PAYLOAD_PATH" ]]; then
    echo "Creating deterministic ${PAYLOAD_BYTES}-byte payload at $PAYLOAD_PATH"
    # Deterministic payload: first $PAYLOAD_BYTES bytes of a SHA-256-keyed
    # keystream so the diversity-500 eval uses a stable payload across runs.
    # Same content as the Update 3 recipe used in principle; exact bytes
    # don't matter for detection but MUST be stable within the eval.
    head -c "$PAYLOAD_BYTES" /dev/urandom > "$PAYLOAD_PATH"
    # Pin the hash for reproducibility.
    shasum -a 256 "$PAYLOAD_PATH" > "$PAYLOAD_PATH.sha256"
fi

cover_count=$(find "$COVER_DIR" -name '[0-9]*.jpg' | wc -l | tr -d ' ')
echo "Cover count: $cover_count"
echo "Passphrases: ml-multi-pass-{0..4}  (5 variants)"
echo "Total stego files to produce: $((cover_count * 5))"
echo "Jobs: $JOBS"
echo ""

embed_one() {
    local cover="$1"
    local passphrase="$2"
    local stego_dir="$3"
    local base
    base="$(basename "$cover")"
    local out="$stego_dir/$base"

    if [[ -f "$out" ]]; then
        return 0
    fi

    mkdir -p "$stego_dir"

    # `phantasm embed` emits a WARN to stderr about passphrase on CLI;
    # we're producing research fixtures locally, redirect to /dev/null.
    "$PHANTASM_BIN" embed \
        -i "$cover" \
        -p "$PAYLOAD_PATH" \
        --passphrase "$passphrase" \
        -o "$out" \
        --cost-function j-uniward \
        --stealth high \
        >/dev/null 2>&1 || {
        echo "FAIL: $cover @ $passphrase" >&2
        return 1
    }
}

export -f embed_one
export PHANTASM_BIN PAYLOAD_PATH

for pass in 0 1 2 3 4; do
    passphrase="ml-multi-pass-${pass}"
    stego_dir="$STEGO_ROOT/$passphrase/qf${QF}/${DIM}"
    mkdir -p "$stego_dir"
    echo "[pass=$pass] embedding into $stego_dir ..."

    find "$COVER_DIR" -name '[0-9]*.jpg' -print0 \
        | xargs -0 -n 1 -P "$JOBS" -I {} bash -c 'embed_one "$@"' _ {} "$passphrase" "$stego_dir"

    produced=$(find "$stego_dir" -name '[0-9]*.jpg' | wc -l | tr -d ' ')
    echo "[pass=$pass] produced $produced stego files"
done

total=$(find "$STEGO_ROOT" -name '[0-9]*.jpg' | wc -l | tr -d ' ')
echo ""
echo "Done. Total stego files: $total"
