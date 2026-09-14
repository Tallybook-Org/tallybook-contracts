#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT
mkdir -p "$WORKDIR/bin"

cat > "$WORKDIR/bin/stellar" <<'STELLAR'
#!/usr/bin/env bash
set -euo pipefail
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
printf 'Fee Charged: %s\n' "$fee" >&2
if [ "${EXTRA_FEE:-}" = 1 ]; then
  printf 'Fee Charged: 1\n' >&2
fi
STELLAR
chmod +x "$WORKDIR/bin/stellar"

if grep -q 'mapfile' "$REPO_ROOT/scripts/deploy-testnet.sh" "$REPO_ROOT/scripts/deploy-mainnet.sh"; then
  echo 'deployment scripts must remain compatible with Bash 3.2 (no mapfile)' >&2
  exit 1
fi

for script in deploy-testnet.sh deploy-mainnet.sh; do
  output=$(cd "$WORKDIR" && \
    PATH="$WORKDIR/bin:$PATH" STELLAR_ACCOUNT=source ADMIN_ADDRESS=GADMIN \
    STELLAR_RPC_URL=https://rpc.example CONFIRM=yes "$REPO_ROOT/scripts/$script")
  grep -F 'price_book WASM upload fee (stroops): 11' <<<"$output"
  grep -F 'price_book deploy fee (stroops):      13' <<<"$output"
  grep -F 'statement_registry WASM upload fee (stroops): 17' <<<"$output"
  grep -F 'statement_registry deploy fee (stroops):      19' <<<"$output"
  grep -F 'total fees charged (stroops):         60' <<<"$output"
done

if (
  cd "$WORKDIR"
  PATH="$WORKDIR/bin:$PATH" EXTRA_FEE=1 STELLAR_ACCOUNT=source ADMIN_ADDRESS=GADMIN \
    "$REPO_ROOT/scripts/deploy-testnet.sh"
); then
  echo 'expected duplicate fee lines to fail' >&2
  exit 1
fi

echo 'deploy fee summary tests passed'
