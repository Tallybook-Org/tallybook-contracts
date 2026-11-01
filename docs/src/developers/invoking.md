# Invoking the contracts

Every command below was run for real against the live testnet deployment —
`price_book` at `CB2IEP4SQ2GC5747HFHNMXEYWEULC5Z5TTTLET2QA4CAA5SYCWAXFKAW` and
`statement_registry` at `CB75TTWGP3TLKEDGA2WOLEVCNKLUX6X5KS47GMWGBHUAVES7J55LY25M` —
before this page was committed, using the `tb-deployer` identity as `operator` and a
freshly generated `tb-consumer-demo` identity as `consumer`. Output shown is the
actual output, not retyped. `__constructor` isn't repeated here since it only runs
once, at deploy — see [Deploying](deploying.md).

## `price_book`

### `publish`

This operator has published several times since this page was first written —
`publish()` requires the new `effective_ledger` to exceed both the current
ledger and the previous version's `effective_ledger`, so a literal historical
number goes stale as soon as either constraint moves past it (confirmed
directly: the value originally shown here, `4600000`, now fails with
`EffectiveInPast`). Read both constraints live instead of hardcoding either
one:

```
LATEST=$(stellar contract invoke --id CB2IEP4SQ2GC5747HFHNMXEYWEULC5Z5TTTLET2QA4CAA5SYCWAXFKAW --network testnet --source tb-deployer -- latest --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM)
PREV_EFF=$(stellar contract invoke --id CB2IEP4SQ2GC5747HFHNMXEYWEULC5Z5TTTLET2QA4CAA5SYCWAXFKAW --network testnet --source tb-deployer -- get_version --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM --version "$LATEST" | python3 -c "import json,sys; print(json.load(sys.stdin)['effective_ledger'])")

stellar contract invoke \
  --id CB2IEP4SQ2GC5747HFHNMXEYWEULC5Z5TTTLET2QA4CAA5SYCWAXFKAW \
  --network testnet \
  --source tb-deployer \
  --send=yes \
  -- publish \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --schedule_hash e759c25665bbca00d8f9efc4cfeb5a642613205085fca0bf75f805fdfc7525b7 \
  --uri https://example.com/tallybook-testnet-schedule-v2.json \
  --effective_ledger "$((PREV_EFF + 100000))"
```

```
🔗 https://stellar.expert/explorer/testnet/tx/a577f6e7cdb27247f7f9181ec09415b6de6341485be1994650401ba0196b5cf7
📅 ... Event: PublishEvent (price_book, publish), operator: "GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM", version: 11, schedule_hash: "e759c25665bbca00d8f9efc4cfeb5a642613205085fca0bf75f805fdfc7525b7", effective_ledger: 4893867
11
```

This is version `11` for an operator whose history has grown through repeated
testing of this page and of [Verifying a charge](../guides/verifying-a-charge.md)
— the exact version number you get depends on how many times this has run
before you, which is exactly why the commands above read the constraints live
instead of assuming a specific prior state.

### `get_version`

```
stellar contract invoke \
  --id CB2IEP4SQ2GC5747HFHNMXEYWEULC5Z5TTTLET2QA4CAA5SYCWAXFKAW \
  --network testnet \
  --source tb-deployer \
  -- get_version \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --version 1
```

```json
{"effective_ledger":4587955,"operator":"GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM","published_ledger":4587953,"schedule_hash":"d7f49405bb09c04a84b2d1dd710a1098b94c54fb6c7e5803053d05a417b96f69","uri":"https://example.com/tallybook-testnet-schedule.json","version":1}
```

### `latest`

```
stellar contract invoke \
  --id CB2IEP4SQ2GC5747HFHNMXEYWEULC5Z5TTTLET2QA4CAA5SYCWAXFKAW \
  --network testnet --source tb-deployer -- latest \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM
```

```
11
```

(This grows every time `publish` above is re-run — see the note there. `latest`
itself always succeeds; only the number changes.)

### `version_at`

```
stellar contract invoke \
  --id CB2IEP4SQ2GC5747HFHNMXEYWEULC5Z5TTTLET2QA4CAA5SYCWAXFKAW \
  --network testnet --source tb-deployer -- version_at \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --ledger 4592300
```

```
1
```

Ledger `4,592,300` is before version `2`'s `effective_ledger` (`4,600,000`), so this
correctly still returns `1` even though a newer version exists.

### `extend_version_ttl`

Not live at the address above: this was added after the `v0.1.0` deploy, so unlike
every other call on this page there is no real run to quote. The shape is:

```
stellar contract invoke \
  --id <price_book> \
  --network testnet \
  --source <any-identity> \
  --send=yes \
  -- extend_version_ttl \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --version 1 \
  --ledgers 6307200
```

Any identity, like `extend_statement_ttl` — and for a sharper reason. Keeping a
statement readable is pointless if the price version it was billed under archives, so
whoever pays rent for the statement needs to be able to pay it here too. `--ledgers`
is clamped to `BUMP_AMOUNT` (6,307,200 ledgers, roughly a year), and the version and
the operator's timeline are extended together.

## `statement_registry`

### `anchor`

```
stellar contract invoke \
  --id CB75TTWGP3TLKEDGA2WOLEVCNKLUX6X5KS47GMWGBHUAVES7J55LY25M \
  --network testnet \
  --source tb-deployer \
  --send=yes \
  -- anchor \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --consumer GAJ46LDZSSYAB4YY6VMPM763P652ZROT7TBG3YYE3BZBAUZDXYDDK6OY \
  --period_start 4590000 \
  --period_end 4592000 \
  --usage_root bb48f8056bf24646e0e9eb0758ca0c3769ece76845023c4d320438182d9ee3fa \
  --request_count 5 \
  --token CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC \
  --amount_billed 500000 \
  --amount_settled 500000 \
  --price_book_version 1 \
  --protocol X402
```

```
🔗 https://stellar.expert/explorer/testnet/tx/54da8ffd78e0061c7f1d2ace7139ca7628ec3e7f70807b8493ec87b57be72d48
📅 ... Event: AnchorEvent (statement, anchor), operator: "GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM", consumer: "GAJ46LDZSSYAB4YY6VMPM763P652ZROT7TBG3YYE3BZBAUZDXYDDK6OY", seq: 1, usage_root: "bb48f8056bf24646e0e9eb0758ca0c3769ece76845023c4d320438182d9ee3fa", amount_billed: "500000", amount_settled: "500000", protocol: "X402"
1
```

`--channel` is omitted, since `Option<Address>` with no flag passed means `None` —
correct for `Protocol::X402`. Fee charged: `2,726,864` stroops
(≈ $0.0502 — see [the worked example](../protocol/worked-example.md#the-cost-of-anchoring)).

### `verify_usage`

```
stellar contract invoke \
  --id CB75TTWGP3TLKEDGA2WOLEVCNKLUX6X5KS47GMWGBHUAVES7J55LY25M \
  --network testnet --source tb-deployer -- verify_usage \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --seq 1 \
  --leaf bb48f8056bf24646e0e9eb0758ca0c3769ece76845023c4d320438182d9ee3fa \
  --proof '[]'
```

```
true
```

See [Verifying a charge](../guides/verifying-a-charge.md) for the full walkthrough,
including the matching negative case.

### `verify_charge`

Also added after `v0.1.0`, so also not live at the address above. This is the call a
buyer checking an invoice line should use; `verify_usage` above only answers whether
a leaf you already hold is in the tree.

```
stellar contract invoke \
  --id <statement_registry> \
  --network testnet \
  --source <any-identity> \
  -- verify_charge \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --seq 1 \
  --record '{"amount":"100000","consumer":"GAJ46LDZSSYAB4YY6VMPM763P652ZROT7TBG3YYE3BZBAUZDXYDDK6OY","endpoint":"ec71df17a899f521f884c5d16596fbb637ce8c83a267889cdfe5f581a1d8172d","ledger":4591000,"price_v":1,"reqid":"abababababababababababababababababababababababababababababababab","units":1}' \
  --proof '[]'
```

`--record` is the usage record itself, not a hash of it: the contract derives the
leaf from these fields and then checks them against the statement before folding the
proof. `amount` is a string because it is an `i128`. See
[Verifying a charge](../guides/verifying-a-charge.md) for what each failure means.

### `open_dispute`

Signed by the **consumer**, not the operator:

```
stellar contract invoke \
  --id CB75TTWGP3TLKEDGA2WOLEVCNKLUX6X5KS47GMWGBHUAVES7J55LY25M \
  --network testnet \
  --source tb-consumer-demo \
  --send=yes \
  -- open_dispute \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --seq 1 \
  --consumer GAJ46LDZSSYAB4YY6VMPM763P652ZROT7TBG3YYE3BZBAUZDXYDDK6OY \
  --reason_hash 0101010101010101010101010101010101010101010101010101010101010101
```

```
🔗 https://stellar.expert/explorer/testnet/tx/32a75e2f9264e5bc518fefa474a33cfc3202760afb2e0c3b4348d02b78340af6
📅 ... Event: DisputeEvent (statement, dispute), operator: "GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM", consumer: "GAJ46LDZSSYAB4YY6VMPM763P652ZROT7TBG3YYE3BZBAUZDXYDDK6OY", seq: 1, reason_hash: "0101010101010101010101010101010101010101010101010101010101010101"
null
```

### `resolve_dispute` — the two-signature case

`resolve_dispute` needs **both** the operator's and the consumer's authorization in
one call. `stellar contract invoke --source <one account>` only signs for one
address, and — tested directly, on this exact call — `stellar-cli 28.0.0`'s
`--sign-with-key` doesn't fill in a second party's Soroban authorization entry
either; passing it (as an identity, and as a raw secret key) produced the same
`Missing signing key for account ...` error as leaving it off; a `--build-only` XDR
comes back with `"auth": []` entirely, since `--build-only` skips simulation and
never records what authorization is even needed. What worked: simulate for real to
record both required auth entries (`stellar tx simulate`, piping a built-only XDR
through it), which surfaces one `source_account` credential (the operator's — no
separate signature needed, since the operator is the transaction's source account)
and one `address` credential (the consumer's — genuinely needs its own signature).
Signing *that* consumer entry isn't something `stellar tx sign` does either, despite
its `--auto-sign` flag's description mentioning "non-root Soroban auth entries" —
in practice it only appends transaction-envelope signatures. The reliable path ended
up being the Stellar SDK's own `authorize_entry` helper (Python `stellar-sdk`),
which signs the specific authorization-entry preimage the consumer's address needs
to sign, separate from signing the transaction itself:

```python
from stellar_sdk import SorobanServer, TransactionBuilder, Network, Keypair, scval, xdr
from stellar_sdk.auth import authorize_entry

server = SorobanServer("https://soroban-testnet.stellar.org")
source_account = server.load_account(operator_kp.public_key)

tx = (TransactionBuilder(source_account, network_passphrase=Network.TESTNET_NETWORK_PASSPHRASE, base_fee=100)
      .append_invoke_contract_function_op(
          "CB75TTWGP3TLKEDGA2WOLEVCNKLUX6X5KS47GMWGBHUAVES7J55LY25M",
          "resolve_dispute",
          [scval.to_address(operator_kp.public_key), scval.to_uint64(1),
           scval.to_bytes(bytes.fromhex("02" * 32)), scval.to_int128(0)],
      )
      .set_timeout(300).build())

tx = server.prepare_transaction(tx)  # simulates; fills in both auth entries

valid_until = server.get_latest_ledger().sequence + 200
op = tx.transaction.operations[0]
for i, entry in enumerate(op.auth):
    if entry.credentials.type != xdr.SorobanCredentialsType.SOROBAN_CREDENTIALS_SOURCE_ACCOUNT:
        op.auth[i] = authorize_entry(entry, consumer_kp, valid_until, Network.TESTNET_NETWORK_PASSPHRASE)

tx.sign(operator_kp)          # the transaction envelope itself
resp = server.send_transaction(tx)
```

One real bug worth naming, since it cost real debugging time: the first attempt
checked `entry.credentials.address is not None` to decide which entries needed
signing, and every entry silently failed that check. The cause was not the network —
this network's own default for this call is classic `SOROBAN_CREDENTIALS_ADDRESS`,
confirmed by reading the raw `simulateTransaction` response directly, with no SDK in
the path. The cause was `prepare_transaction()` itself: the Python `stellar_sdk`
defaults `use_upgraded_auth=True`, which requests CAP-71 `SOROBAN_CREDENTIALS_ADDRESS_V2`
credentials from the RPC regardless of what the network would otherwise hand back —
so `entry.credentials.address` was `None` because the SDK had asked for a format the
entry wasn't in, not because the network issues that format. Confirmed directly:
re-running the identical call with `use_upgraded_auth=False` makes the classic
`.address` field populate instead, matching the raw RPC response exactly. Checking
`entry.credentials.type != SOROBAN_CREDENTIALS_SOURCE_ACCOUNT` instead (as above)
doesn't care which credential format is in play, and is what actually worked —
correct regardless of which default the SDK or the RPC happens to be using.

Result:

```
🔗 https://stellar.expert/explorer/testnet/tx/2fe06a897192352c8b73ffa571c44446581046e4c8b4cb36a78e90702cacad43
status: SUCCESS
fee_charged: 122638 stroops
```

Confirmed after the fact with a plain read:

```
stellar contract invoke \
  --id CB75TTWGP3TLKEDGA2WOLEVCNKLUX6X5KS47GMWGBHUAVES7J55LY25M \
  --network testnet --source tb-deployer -- get_statement \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM --seq 1
```

```json
{"amount_billed":"500000","amount_settled":"500000","anchored_ledger":4592019,"channel":null,"consumer":"GAJ46LDZSSYAB4YY6VMPM763P652ZROT7TBG3YYE3BZBAUZDXYDDK6OY","operator":"GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM","period_end":4592000,"period_start":4590000,"price_book_version":1,"protocol":"X402","request_count":5,"status":"Resolved","token":"CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC","usage_root":"bb48f8056bf24646e0e9eb0758ca0c3769ece76845023c4d320438182d9ee3fa"}
```

`status: "Resolved"`.

### `get_statement`

Shown above. No auth required.

### `list_statements`

```
stellar contract invoke \
  --id CB75TTWGP3TLKEDGA2WOLEVCNKLUX6X5KS47GMWGBHUAVES7J55LY25M \
  --network testnet --source tb-deployer -- list_statements \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --consumer GAJ46LDZSSYAB4YY6VMPM763P652ZROT7TBG3YYE3BZBAUZDXYDDK6OY
```

```
[1]
```

### `get_dispute`

```
stellar contract invoke \
  --id CB75TTWGP3TLKEDGA2WOLEVCNKLUX6X5KS47GMWGBHUAVES7J55LY25M \
  --network testnet --source tb-deployer -- get_dispute \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM --seq 1
```

```json
{"amount_credited":"0","consumer":"GAJ46LDZSSYAB4YY6VMPM763P652ZROT7TBG3YYE3BZBAUZDXYDDK6OY","opened_ledger":4592152,"reason_hash":"0101010101010101010101010101010101010101010101010101010101010101","resolution_hash":"0202020202020202020202020202020202020202020202020202020202020202","resolved_ledger":4592262}
```

### `extend_statement_ttl`

Run from the **consumer** identity deliberately — proving the "no auth, anyone may
pay rent" design actually holds, not just the operator extending their own record:

```
stellar contract invoke \
  --id CB75TTWGP3TLKEDGA2WOLEVCNKLUX6X5KS47GMWGBHUAVES7J55LY25M \
  --network testnet \
  --source tb-consumer-demo \
  --send=yes \
  -- extend_statement_ttl \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --seq 1 \
  --ledgers 100000
```

```
🔗 https://stellar.expert/explorer/testnet/tx/dfdc3cf625dd5cdc10182b091446767d2f07be79b3707a4b075a1a199d66b78a
null
```

`null` is `Ok(())` — success, no return value.
