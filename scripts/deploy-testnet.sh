#!/usr/bin/env bash
# Deploys price_book, then statement_registry (pointed at price_book's
# address), to Stellar testnet. Builds both contracts first.
#
# Required environment:
#   STELLAR_ACCOUNT  Signing/source account: an identity name from the
#                     `stellar keys` store (recommended — see `stellar keys
#                     add`/`stellar keys generate`), or a secret key / seed
#                     phrase you export into the environment yourself.
#                     NEVER pass a secret key as a script argument or
#                     hardcode one here — this script only ever reads it
#                     from the environment or the keys store.
#   ADMIN_ADDRESS     Public key (G...) to pass as `admin` to both
#                     constructors. Holds no power over operator data in
#                     either contract; it only exists as a documented rent
#                     payer. Can be the same address as STELLAR_ACCOUNT's,
#                     but is deliberately a separate variable since the
#                     signer and the admin need not be the same account.
#
# Usage:
#   STELLAR_ACCOUNT=alice ADMIN_ADDRESS=GABC... ./scripts/deploy-testnet.sh
set -euo pipefail

: "${STELLAR_ACCOUNT:?Set STELLAR_ACCOUNT to a stellar keys identity (see \`stellar keys add\`), a secret key, or a seed phrase, via the environment. Never pass it as a command-line argument.}"
: "${ADMIN_ADDRESS:?Set ADMIN_ADDRESS to the public key (G...) to use as admin for both contracts.}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

NETWORK="testnet"
PRICE_BOOK_WASM="target/wasm32v1-none/release/price_book.wasm"
STATEMENT_REGISTRY_WASM="target/wasm32v1-none/release/statement_registry.wasm"

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    sha256sum "$1" | awk '{print $1}'
  fi
}

# stellar-cli v28 writes the final, charged fee only to the --cost stderr table.
# Keep the command's stdout intact (it is the wasm hash or contract ID), while
# requiring exactly one final fee for each transaction submitted here.
LAST_COMMAND_OUTPUT=""
LAST_FEE_CHARGED=""
run_stellar_with_fee() {
  local label="$1"
  shift
  local cost_output fee_lines
  cost_output="$(mktemp)"
  trap 'rm -f "$cost_output"' RETURN

  if ! LAST_COMMAND_OUTPUT=$("$@" --cost 2> "$cost_output"); then
    cat "$cost_output" >&2
    echo "Failed while submitting $label." >&2
    return 1
  fi
  cat "$cost_output" >&2

  fee_lines=()
  while IFS= read -r line; do
    fee_lines+=("$line")
  done < <(grep -Eo 'Fee Charged: *[0-9]+' "$cost_output" | sed -E 's/.*: *//')
  if [ "${#fee_lines[@]}" -ne 1 ]; then
    echo "Could not determine one final fee charged for $label." >&2
    return 1
  fi
  LAST_FEE_CHARGED="${fee_lines[0]}"
  rm -f "$cost_output"
  trap - RETURN
}

echo "==> Building price-book" >&2
stellar contract build --package price-book

echo "==> Building statement-registry" >&2
stellar contract build --package statement-registry

echo "==> Uploading price_book WASM to $NETWORK" >&2
run_stellar_with_fee "price_book WASM upload" \
  stellar contract upload --wasm "$PRICE_BOOK_WASM" \
  --source-account "$STELLAR_ACCOUNT" --network "$NETWORK"
PRICE_BOOK_WASM_ID="$LAST_COMMAND_OUTPUT"
PRICE_BOOK_WASM_UPLOAD_FEE="$LAST_FEE_CHARGED"

echo "==> Deploying price_book to $NETWORK" >&2
run_stellar_with_fee "price_book deploy" \
  stellar contract deploy --wasm-hash "$PRICE_BOOK_WASM_ID" \
  --source-account "$STELLAR_ACCOUNT" --network "$NETWORK" \
  -- --admin "$ADMIN_ADDRESS"
PRICE_BOOK_ID="$LAST_COMMAND_OUTPUT"
PRICE_BOOK_DEPLOY_FEE="$LAST_FEE_CHARGED"

echo "==> Uploading statement_registry WASM to $NETWORK" >&2
run_stellar_with_fee "statement_registry WASM upload" \
  stellar contract upload --wasm "$STATEMENT_REGISTRY_WASM" \
  --source-account "$STELLAR_ACCOUNT" --network "$NETWORK"
STATEMENT_REGISTRY_WASM_ID="$LAST_COMMAND_OUTPUT"
STATEMENT_REGISTRY_WASM_UPLOAD_FEE="$LAST_FEE_CHARGED"

echo "==> Deploying statement_registry to $NETWORK (price_book = $PRICE_BOOK_ID)" >&2
run_stellar_with_fee "statement_registry deploy" \
  stellar contract deploy --wasm-hash "$STATEMENT_REGISTRY_WASM_ID" \
  --source-account "$STELLAR_ACCOUNT" --network "$NETWORK" \
  -- --admin "$ADMIN_ADDRESS" --price_book "$PRICE_BOOK_ID"
STATEMENT_REGISTRY_ID="$LAST_COMMAND_OUTPUT"
STATEMENT_REGISTRY_DEPLOY_FEE="$LAST_FEE_CHARGED"

PRICE_BOOK_WASM_HASH=$(sha256_of "$PRICE_BOOK_WASM")
STATEMENT_REGISTRY_WASM_HASH=$(sha256_of "$STATEMENT_REGISTRY_WASM")
TOTAL_FEE_CHARGED=$((
  PRICE_BOOK_WASM_UPLOAD_FEE + PRICE_BOOK_DEPLOY_FEE +
  STATEMENT_REGISTRY_WASM_UPLOAD_FEE + STATEMENT_REGISTRY_DEPLOY_FEE
))

echo
echo "network:                              $NETWORK"
echo "price_book address:                   $PRICE_BOOK_ID"
echo "price_book wasm hash:                 $PRICE_BOOK_WASM_HASH"
echo "price_book WASM upload fee (stroops): $PRICE_BOOK_WASM_UPLOAD_FEE"
echo "price_book deploy fee (stroops):      $PRICE_BOOK_DEPLOY_FEE"
echo "statement_registry address:           $STATEMENT_REGISTRY_ID"
echo "statement_registry wasm hash:         $STATEMENT_REGISTRY_WASM_HASH"
echo "statement_registry WASM upload fee (stroops): $STATEMENT_REGISTRY_WASM_UPLOAD_FEE"
echo "statement_registry deploy fee (stroops):      $STATEMENT_REGISTRY_DEPLOY_FEE"
echo "total fees charged (stroops):         $TOTAL_FEE_CHARGED"
