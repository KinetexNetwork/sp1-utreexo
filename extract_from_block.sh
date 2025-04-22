#!/usr/bin/env bash
#
# extract_from_block.sh
#
# Purpose:
#   Generate or update test data for unit tests by extracting a specific
#   transaction output’s details from a block in Bitcoin Core.
#
# Usage:
#   ./extract_from_block.sh <TXID> <VOUT> <HEIGHT>
#
# Example:
#   export BITCOIN_CORE_COOKIE_FILE="$HOME/.bitcoin/.cookie"
#   ./extract_from_block.sh \
#     e3c1f9a2...b4d0 0 680000 \
#     > testdata/utxo_output.txt
#
# Environment:
#   BITCOIN_CORE_COOKIE_FILE  Absolute path to Bitcoin Core RPC cookie file
#                             (e.g. "$HOME/.bitcoin/.cookie")
#
# Dependencies:
#   - bitcoin-cli  (needs txindex=1, server=1 in bitcoin.conf)
#   - jq           (for JSON parsing)
#
# Steps:
#   1) Map block height → block hash via `getblockhash`
#   2) Fetch raw tx from that block with `getrawtransaction … true`
#   3) Extract:
#        • amount in satoshis (value * 1e8, floored)
#        • scriptPubKey hex
#   4) Emit formatted key: value pairs for use in test fixtures
#
# Output:
#   txid:   <TXID>
#   vout:   <VOUT>
#   amount: <satoshis>
#   height: <HEIGHT>
#   script: <scriptPubKey hex>
#

set -euo pipefail

if [ $# -ne 3 ]; then
  cat <<EOF
Usage:
  $0 <TXID> <VOUT> <HEIGHT>

Generate test data by extracting a specific tx output from a block.

Arguments:
  TXID    Transaction ID to query
  VOUT    Output index (0-based)
  HEIGHT  Block height containing the tx

Requires:
  BITCOIN_CORE_COOKIE_FILE (env) – path to your RPC cookie file

Example:
  export BITCOIN_CORE_COOKIE_FILE="\$HOME/.bitcoin/.cookie"
  $0 e3c1f9a2...b4d0 0 680000 > out.txt
EOF
  exit 1
fi

TXID="$1"
VOUT="$2"
HEIGHT="$3"

: "${BITCOIN_CORE_COOKIE_FILE:?Environment variable BITCOIN_CORE_COOKIE_FILE must be set}"

DATADIR=$(dirname "$BITCOIN_CORE_COOKIE_FILE")
CLI="bitcoin-cli -datadir=${DATADIR}"

# 1) Block hash from height
BLOCK_HASH=$($CLI getblockhash "$HEIGHT")

# 2) Raw transaction JSON from that block
RAW=$($CLI getrawtransaction "$TXID" true "$BLOCK_HASH")

# 3) Extract amount (satoshis) & scriptPubKey hex
AMOUNT=$(jq -r ".vout[$VOUT].value * 100000000 | floor" <<<"$RAW")
SCRIPT_HEX=$(jq -r ".vout[$VOUT].scriptPubKey.hex" <<<"$RAW")

# 4) Emit test fixture
cat <<EOF
txid:   ${TXID}
vout:   ${VOUT}
amount: ${AMOUNT}
height: ${HEIGHT}
script: ${SCRIPT_HEX}
EOF
