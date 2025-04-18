#!/usr/bin/env bash
set -euo pipefail

if [ $# -ne 3 ]; then
  echo "Usage: $0 <txid> <vout> <creation_height>"
  exit 1
fi

TXID="$1"
VOUT="$2"
HEIGHT="$3"

: "${BITCOIN_CORE_COOKIE_FILE:?set this to your rpc cookie file}"

DATADIR=$(dirname "$BITCOIN_CORE_COOKIE_FILE")
CLI="bitcoin-cli -datadir=${DATADIR}"

# 1. map height → blockhash
BLOCK_HASH=$($CLI getblockhash "${HEIGHT}")

# 2. fetch the raw transaction *from that block*
RAW=$( $CLI getrawtransaction "${TXID}" true "${BLOCK_HASH}" )

# 3. extract exactly the five fields
AMOUNT=$(jq -r ".vout[${VOUT}].value * 100000000 | floor" <<<"$RAW")
SCRIPT_HEX=$(jq -r ".vout[${VOUT}].scriptPubKey.hex" <<<"$RAW")

cat <<EOF
txid:   ${TXID}
vout:   ${VOUT}
amount: ${AMOUNT}
height: ${HEIGHT}
script: ${SCRIPT_HEX}
EOF
