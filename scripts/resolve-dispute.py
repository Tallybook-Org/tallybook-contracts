#!/usr/bin/env python3
"""Resolves a dispute on statement_registry against the live testnet
deployment, dual-signed by both the operator and the consumer.

This exists because there is no way to do this with `stellar contract
invoke` alone. resolve_dispute() requires both parties' Soroban
authorization in one call. As of stellar-cli 28.0.0, tested directly
against this deployment:

  - `stellar contract invoke --source <one account> ...` only signs
    for that one account.
  - `--sign-with-key <other account>` (as an identity, and as a raw
    secret key) does not fill in a second party's authorization entry
    -- it fails with "Missing signing key for account ...", the same
    error whether or not the flag is passed at all.
  - `--build-only` never simulates, so its output transaction always
    has an empty auth list -- it never even records that a second
    signature is needed.
  - `stellar tx sign --auto-sign`, despite its own help text
    mentioning "non-root Soroban auth entries", only appends a
    transaction-envelope signature in practice. Decoding its output
    afterward shows the second party's actual authorization entry
    still marked "void".

What works: simulating for real (which correctly records one
`SOROBAN_CREDENTIALS_SOURCE_ACCOUNT` entry for the operator, needing
no separate signature, and one address-credentialed entry for the
consumer, which does), then signing that consumer entry's specific
authorization preimage with the Stellar SDK's own `authorize_entry`,
separately from signing the transaction envelope.

Requires the `stellar-sdk` Python package: `pip install stellar-sdk`.

Usage:
    python3 scripts/resolve-dispute.py <seq>

Reads both parties' secrets from the local `stellar keys` store (via
`stellar keys secret <alias>`) -- never accepts one as a CLI argument
or prints one, per this repo's own security rule against that.
Assumes two identities named `tb-deployer` (operator) and
`tb-consumer-demo` (consumer) already exist in that store.
"""
import os
import re
import subprocess
import sys

from stellar_sdk import Keypair, Network, SorobanServer, TransactionBuilder, scval, xdr
from stellar_sdk.auth import authorize_entry
from stellar_sdk.exceptions import PrepareTransactionException
from stellar_sdk.soroban_rpc import GetTransactionStatus

RPC = os.environ.get("STELLAR_RPC_URL", "https://soroban-testnet.stellar.org")
NETWORK = Network.TESTNET_NETWORK_PASSPHRASE
# Overridable because a redeploy changes it: these contracts have no upgrade
# path on purpose, so every rebuild of the wasm means a new address. The
# variable name matches the one the off-chain collector in the tallybook repo
# already reads, so a single exported value drives both.
CONTRACT = os.environ.get(
    "TB_STATEMENT_REGISTRY_ID",
    "CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3",
)

# statement_registry's Error enum (contracts/statement-registry/src/error.rs).
# Discriminant 1 is deliberately unused there -- see errors.md -- so it's
# omitted here too, not a gap in this table.
ERROR_NAMES = {
    2: "NotFound",
    3: "BadPeriod",
    4: "BadAmounts",
    5: "EmptyStatement",
    6: "PriceVersionUnknown",
    7: "PriceVersionStale",
    8: "ChannelMismatch",
    9: "IndexFull",
    10: "NotAnchored",
    11: "NotDisputed",
    12: "CreditTooLarge",
    13: "ProofTooLong",
    14: "PeriodSpansPriceChange",
}


def secret_for(alias: str) -> str:
    return subprocess.check_output(["stellar", "keys", "secret", alias], text=True).strip()


def describe_simulation_error(exc: PrepareTransactionException) -> str:
    """Turns a PrepareTransactionException into a readable line naming
    the actual contract error variant, instead of a bare exception repr."""
    raw = exc.simulate_transaction_response.error or str(exc)
    match = re.search(r"Error\(Contract,\s*#(\d+)\)", raw)
    if match:
        code = int(match.group(1))
        name = ERROR_NAMES.get(code, "unknown variant")
        return f"resolve_dispute failed: {name} (contract error #{code})\n\nFull simulation error:\n{raw}"
    return f"resolve_dispute failed (could not identify a contract error code)\n\nFull simulation error:\n{raw}"


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <seq>", file=sys.stderr)
        return 2
    seq = int(sys.argv[1])

    operator_kp = Keypair.from_secret(secret_for("tb-deployer"))
    consumer_kp = Keypair.from_secret(secret_for("tb-consumer-demo"))

    server = SorobanServer(RPC)
    source_account = server.load_account(operator_kp.public_key)

    args = [
        scval.to_address(operator_kp.public_key),
        scval.to_uint64(seq),
        scval.to_bytes(bytes.fromhex(
            "cf6bfd7bf721a7151cdcc05a2033ea52ef77d35178a66a18599b92286dbce134"
        )),
        scval.to_int128(0),
    ]

    tx = (
        TransactionBuilder(source_account, network_passphrase=NETWORK, base_fee=100)
        .append_invoke_contract_function_op(CONTRACT, "resolve_dispute", args)
        .set_timeout(300)
        .build()
    )

    try:
        tx = server.prepare_transaction(tx)
    except PrepareTransactionException as exc:
        print(describe_simulation_error(exc), file=sys.stderr)
        return 1

    valid_until = server.get_latest_ledger().sequence + 200

    op = tx.transaction.operations[0]
    for i, entry in enumerate(op.auth):
        if entry.credentials.type != xdr.SorobanCredentialsType.SOROBAN_CREDENTIALS_SOURCE_ACCOUNT:
            op.auth[i] = authorize_entry(entry, consumer_kp, valid_until, NETWORK)

    tx.sign(operator_kp)

    resp = server.send_transaction(tx)
    print("submitted, hash:", resp.hash)

    # send_transaction only confirms submission, not inclusion -- polling
    # here (rather than exiting immediately after "PENDING") is what
    # makes a read run right after this script reliably see "Resolved"
    # instead of racing ahead of confirmation and still seeing "Disputed".
    result = server.poll_transaction(resp.hash)
    print("status:", result.status)
    if result.status != GetTransactionStatus.SUCCESS:
        print(f"resolve_dispute transaction did not succeed: {result}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
