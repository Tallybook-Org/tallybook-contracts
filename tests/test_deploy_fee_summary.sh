#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT
mkdir -p "$WORKDIR/bin"

cat > "$WORKDIR/bin/stellar" <<'STELLAR'
#!/usr/bin/env bash
set -euo pipefail
# `contract deploy` passes constructor arguments after a `--` separator, so
# anything the script appends past that point reaches the contract's
# __constructor instead of the CLI. The real binary rejects it outright;
# reject it here too, or the test cannot tell the two positions apart -- and
# that is exactly the bug that made every real deploy fail while this test
# passed.
seen_separator=0
for arg in "$@"; do
  if [ "$arg" = "--" ]; then
    seen_separator=1
    continue
  fi
  if [ "$arg" = "--cost" ] && [ "$seen_separator" = 1 ]; then
    echo "error: unexpected argument '--cost' found" >&2
    exit 1
  fi
done
if [ "$1 $2 $3" = "contract build --package" ]; then
  mkdir -p target/wasm32v1-none/release
  printf wasm > "target/wasm32v1-none/release/${4//-/_}.wasm"
  exit 0
fi
if [ "$1 $2 $3" = "contract upload --wasm" ]; then
  case "$4" in
    *price_book*) fee=11; echo price-wasm ;;
    *) fee=17; echo registry-wasm ;;
  esac
elif [ "$1 $2 $3" = "contract deploy --wasm-hash" ]; then
  case "$4" in
    price-wasm) fee=13; echo PRICEBOOK ;;
    registry-wasm) fee=19; echo REGISTRY ;;
  esac
else
  echo "unexpected stellar arguments: $*" >&2
  exit 1
fi
if [ "${SKIP_INSTALL:-}" = 1 ] && [ "$1 $2 $3" = "contract upload --wasm" ]; then
  printf 'Skipping install because wasm already installed\n' >&2
  exit 0
fi
# The shape stellar-cli 28 actually prints. A bare "Fee Charged: N" line --
# what this stub used to emit -- is why this test passed while a real
# deployment could not get past its first upload: three different rows here
# end in "Fee Charged", and the inclusion row is printed twice.
cat >&2 <<TABLE
├────────────────────────────┤
│ Inclusion Fee Charged: 100 │ Non-Refundable Charged: 7 │ Refundable Charged: 11 │ Refunded: 3 │
├────────────────────────────┤
│ Inclusion Fee Charged: 100 │ Resource Fee Charged: 18 │ Refunded: 3 │
├────────────────────────────┤
│ Fee Charged: ${fee} │ Refunded: 3 │
└────────────────────────────┘
TABLE
if [ "${EXTRA_FEE:-}" = 1 ]; then
  printf '│ Fee Charged: 1 │\n' >&2
fi
STELLAR
chmod +x "$WORKDIR/bin/stellar"

if grep -q 'mapfile' "$REPO_ROOT/scripts/deploy-testnet.sh" "$REPO_ROOT/scripts/deploy-mainnet.sh"; then
  echo 'deployment scripts must remain compatible with Bash 3.2 (no mapfile)' >&2
  exit 1
fi

# Run copies of the scripts from inside the sandbox rather than the originals.
# Both scripts resolve their own REPO_ROOT from $BASH_SOURCE and cd to it,
# which undoes any `cd "$WORKDIR"` the caller does -- so invoking the
# originals pointed the stub `stellar contract build` at the real
# target/wasm32v1-none/release/ and overwrote both compiled contracts with a
# four-byte stub. That broke `cargo test` afterwards, because
# statement-registry contractimports price_book.wasm at compile time. Copied
# verbatim, so what runs is still exactly the committed script; only the
# directory it resolves as its repo root changes.
mkdir -p "$WORKDIR/scripts"
cp "$REPO_ROOT/scripts/deploy-testnet.sh" "$REPO_ROOT/scripts/deploy-mainnet.sh" "$WORKDIR/scripts/"

for script in deploy-testnet.sh deploy-mainnet.sh; do
  output=$(PATH="$WORKDIR/bin:$PATH" STELLAR_ACCOUNT=source ADMIN_ADDRESS=GADMIN \
    STELLAR_RPC_URL=https://rpc.example CONFIRM=yes "$WORKDIR/scripts/$script")
  grep -F 'price_book WASM upload fee (stroops): 11' <<<"$output"
  grep -F 'price_book deploy fee (stroops):      13' <<<"$output"
  grep -F 'statement_registry WASM upload fee (stroops): 17' <<<"$output"
  grep -F 'statement_registry deploy fee (stroops):      19' <<<"$output"
  grep -F 'total fees charged (stroops):         60' <<<"$output"
done

if (
  PATH="$WORKDIR/bin:$PATH" EXTRA_FEE=1 STELLAR_ACCOUNT=source ADMIN_ADDRESS=GADMIN \
    "$WORKDIR/scripts/deploy-testnet.sh"
); then
  echo 'expected duplicate fee lines to fail' >&2
  exit 1
fi

# A re-run is the normal case, not an edge case: `contract upload` skips the
# install once that wasm is on the network and prints no cost table at all.
# Zero fee lines used to be treated as a failure, which made the scripts work
# at most once per wasm.
skip_output=$(SKIP_INSTALL=1 PATH="$WORKDIR/bin:$PATH" STELLAR_ACCOUNT=source \
  ADMIN_ADDRESS=GADMIN STELLAR_RPC_URL=https://rpc.example CONFIRM=yes \
  "$WORKDIR/scripts/deploy-testnet.sh")
grep -F 'price_book WASM upload fee (stroops): 0' <<<"$skip_output"
grep -F 'statement_registry WASM upload fee (stroops): 0' <<<"$skip_output"
grep -F 'total fees charged (stroops):         32' <<<"$skip_output"

echo 'deploy fee summary tests passed'
