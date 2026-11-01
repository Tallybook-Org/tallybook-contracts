# Invoking the contracts

Every command below was run for real against the live testnet deployment —
`price_book` at `CC6VXCEIXGGFIZXH2DGPVPUXYVVI3YVEQQ5M7XAWNEN7SADIYACTRFZ6` and
`statement_registry` at `CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3` —
before this page was committed, using the `tb-deployer` identity as `operator` and a
`tb-consumer-demo` identity as `consumer`. Output shown is the actual output, not
retyped. `__constructor` isn't repeated here since it only runs once, at deploy — see
[Deploying](deploying.md).

Two statements exist on this deployment, anchored to write these pages. `seq: 1` is
left `Anchored` and is what [Verifying a charge](../guides/verifying-a-charge.md)
checks against; `seq: 2` is the one disputed and resolved below, so the dispute calls
have real output without disturbing the verification walkthrough.

## `price_book`

### `publish`

`publish()` requires the new `effective_ledger` to exceed both the current ledger and
the previous version's `effective_ledger`, so a literal historical number goes stale
as soon as either constraint moves past it. That is not hypothetical: writing this
page, the first attempt passed the current ledger as `effective_ledger` and the
transaction failed with `EffectiveInPast` (`#3`), because the network advanced a few
ledgers between reading the value and the transaction executing. Read both constraints
live, and leave margin:

```
LATEST=$(stellar contract invoke --id CC6VXCEIXGGFIZXH2DGPVPUXYVVI3YVEQQ5M7XAWNEN7SADIYACTRFZ6 --network testnet --source tb-deployer -- latest --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM)
PREV_EFF=$(stellar contract invoke --id CC6VXCEIXGGFIZXH2DGPVPUXYVVI3YVEQQ5M7XAWNEN7SADIYACTRFZ6 --network testnet --source tb-deployer -- get_version --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM --version "$LATEST" | python3 -c "import json,sys; print(json.load(sys.stdin)['effective_ledger'])")

stellar contract invoke \
  --id CC6VXCEIXGGFIZXH2DGPVPUXYVVI3YVEQQ5M7XAWNEN7SADIYACTRFZ6 \
  --network testnet \
  --source tb-deployer \
  --send=yes \
  -- publish \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --schedule_hash b9c42dee77fe7a69df68f21e6f8206ec3ae66b0c173b7bee821e271d9092b9cb \
  --uri https://example.com/tallybook-testnet-schedule-v1.json \
  --effective_ledger "$((PREV_EFF + 100000))"
```

On a fresh deployment there is no previous version to read, so the first publish just
needs margin over the current ledger. That is the call that produced version `1` here:

```
🔗 https://stellar.expert/explorer/testnet/tx/343c4362fbf5216fa927bd7523a452684c173c7bfc7c7a560acd1c9a35d9521e
📅 CC6VXCEIXGGFIZXH2DGPVPUXYVVI3YVEQQ5M7XAWNEN7SADIYACTRFZ6 - Success - Event: PublishEvent (price_book, publish), operator: "GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM", version: 1, schedule_hash: "b9c42dee77fe7a69df68f21e6f8206ec3ae66b0c173b7bee821e271d9092b9cb", effective_ledger: 5510354
1
```

The returned `1` is the new version number. It increments per operator, so the number
you get depends on how many times this has been run for that operator before you —
which is exactly why the commands above read the constraints live instead of assuming
a specific prior state.

### `get_version`

```
stellar contract invoke \
  --id CC6VXCEIXGGFIZXH2DGPVPUXYVVI3YVEQQ5M7XAWNEN7SADIYACTRFZ6 \
  --network testnet \
  --source tb-deployer \
  -- get_version \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --version 1
```

```json
{"effective_ledger":5510354,"operator":"GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM","published_ledger":5510336,"schedule_hash":"b9c42dee77fe7a69df68f21e6f8206ec3ae66b0c173b7bee821e271d9092b9cb","uri":"https://example.com/tallybook-testnet-schedule-v1.json","version":1}
```

### `latest`

```
stellar contract invoke \
  --id CC6VXCEIXGGFIZXH2DGPVPUXYVVI3YVEQQ5M7XAWNEN7SADIYACTRFZ6 \
  --network testnet --source tb-deployer -- latest \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM
```

```
1
```

(This grows every time `publish` above is re-run — see the note there. `latest`
itself only fails with `NotFound` for an operator that has never published.)

### `version_at`

```
stellar contract invoke \
  --id CC6VXCEIXGGFIZXH2DGPVPUXYVVI3YVEQQ5M7XAWNEN7SADIYACTRFZ6 \
  --network testnet --source tb-deployer -- version_at \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --ledger 5510360
```

```
1
```

Ledger `5,510,360` is the end of the period statement `seq: 1` covers, and version
`1` took effect at `5,510,354`, so version `1` is what applied. Asking about
`5,510,353` — one ledger before it took effect — returns `Error(Contract, #2)`,
`NotFound`, rather than guessing at a version that was not yet in force.

### `extend_version_ttl`

Run from the `tb-consumer-demo` identity deliberately — the consumer is not the
operator, and not a party to the price book at all, which is the point:

```
stellar contract invoke \
  --id CC6VXCEIXGGFIZXH2DGPVPUXYVVI3YVEQQ5M7XAWNEN7SADIYACTRFZ6 \
  --network testnet \
  --source tb-consumer-demo \
  --send=yes \
  -- extend_version_ttl \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --version 1 \
  --ledgers 6307200
```

```
🔗 https://stellar.expert/explorer/testnet/tx/b8f60c8d97317949fa38a42fdfc0b37ab8216f6c3c7c8351c4fee91b0fdf3e4a
null
```

`null` is `Ok(())`. Any identity, like `extend_statement_ttl` — and for a sharper reason. Keeping a
statement readable is pointless if the price version it was billed under archives, so
whoever pays rent for the statement needs to be able to pay it here too. `--ledgers`
is clamped to `BUMP_AMOUNT` (6,307,200 ledgers, roughly a year), and the version and
the operator's timeline are extended together.

## `statement_registry`

### `anchor`

```
stellar contract invoke \
  --id CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3 \
  --network testnet \
  --source tb-deployer \
  --send=yes \
  -- anchor \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --consumer GAJ46LDZSSYAB4YY6VMPM763P652ZROT7TBG3YYE3BZBAUZDXYDDK6OY \
  --period_start 5510355 \
  --period_end 5510360 \
  --usage_root 4156f949f4e7601dcbaaef2fe0536362c13bb40fd8e0bc57e1f2126ee75df860 \
  --request_count 1 \
  --token CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC \
  --amount_billed 100000 \
  --amount_settled 100000 \
  --price_book_version 1 \
  --protocol X402
```

```
🔗 https://stellar.expert/explorer/testnet/tx/e94dec82cdcab1490ece6ef351e6c09dd00f217a0d48944b718d390da70eb599
📅 CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3 - Success - Event: AnchorEvent (statement, anchor), operator: "GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM", consumer: "GAJ46LDZSSYAB4YY6VMPM763P652ZROT7TBG3YYE3BZBAUZDXYDDK6OY", seq: 1, usage_root: "4156f949f4e7601dcbaaef2fe0536362c13bb40fd8e0bc57e1f2126ee75df860", amount_billed: "100000", amount_settled: "100000", protocol: "X402"
1
```

`--channel` is omitted, since `Option<Address>` with no flag passed means `None` —
correct for `Protocol::X402`. The period must already have ended (`period_end` no later
than the current ledger) and must sit inside one price version's window, which is why
both bounds fall after version `1`'s `effective_ledger` of `5510354`.

`usage_root` here is a single record's leaf, since `request_count` is `1` — see
[`verify_charge`](#verify_charge) for the record it was derived from. `seq: 2` on this
deployment was anchored the same way with `request_count: 5`, and is the one disputed
below.

### `verify_usage`

```
stellar contract invoke \
  --id CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3 \
  --network testnet --source tb-deployer -- verify_usage \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --seq 1 \
  --leaf 4156f949f4e7601dcbaaef2fe0536362c13bb40fd8e0bc57e1f2126ee75df860 \
  --proof '[]'
```

```
true
```

Answers membership only: the leaf is 32 bytes with nothing readable in it, so a caller
who did not derive it themselves is trusting whoever did. `verify_charge` below is the
one to reach for when checking an invoice line. See
[Verifying a charge](../guides/verifying-a-charge.md) for the full walkthrough,
including the negative cases.

### `verify_charge`

The call a buyer checking an invoice line should use. It takes the record rather than a
leaf, derives the leaf itself, and binds the record to the statement before folding.

```
stellar contract invoke \
  --id CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3 \
  --network testnet \
  --source tb-deployer \
  -- verify_charge \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --seq 1 \
  --record '{"amount":"100000","consumer":"GAJ46LDZSSYAB4YY6VMPM763P652ZROT7TBG3YYE3BZBAUZDXYDDK6OY","endpoint":"ec71df17a899f521f884c5d16596fbb637ce8c83a267889cdfe5f581a1d8172d","ledger":5510357,"price_v":1,"reqid":"abababababababababababababababababababababababababababababababab","units":1}' \
  --proof '[]'
```

```
true
```

`--record` is the usage record itself, not a hash of it. `amount` is a string because
it is an `i128`, and `endpoint` is `sha256(method || " " || path_template)` —
`sha256("GET /v1/search")` here.

The four failure modes, all run against this same statement:

| Change to the record | Result |
|---|---|
| `amount` → `100001` | `false` — the derived leaf is not the one in the tree |
| `price_v` → `2` | `Error(Contract, #16)`, `RecordPriceMismatch` |
| a different `consumer` | `Error(Contract, #15)`, `RecordConsumerMismatch` |
| `ledger` → `5510354` | `Error(Contract, #17)`, `RecordOutsidePeriod` |

See [Verifying a charge](../guides/verifying-a-charge.md) for what each one tells a
buyer.

### `open_dispute`

Signed by the **consumer**, not the operator:

```
stellar contract invoke \
  --id CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3 \
  --network testnet \
  --source tb-consumer-demo \
  --send=yes \
  -- open_dispute \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --seq 2 \
  --consumer GAJ46LDZSSYAB4YY6VMPM763P652ZROT7TBG3YYE3BZBAUZDXYDDK6OY \
  --reason_hash 0101010101010101010101010101010101010101010101010101010101010101
```

```
🔗 https://stellar.expert/explorer/testnet/tx/f214d0707a7a12ff4dac1d5a5933c1fd2499bb862e24b87ff977add3e3c6896a
📅 CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3 - Success - Event: DisputeEvent (statement, dispute), operator: "GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM", consumer: "GAJ46LDZSSYAB4YY6VMPM763P652ZROT7TBG3YYE3BZBAUZDXYDDK6OY", seq: 2, reason_hash: "0101010101010101010101010101010101010101010101010101010101010101"
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
          "CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3",
          "resolve_dispute",
          [scval.to_address(operator_kp.public_key), scval.to_uint64(2),
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

That whole procedure is committed as
[`scripts/resolve-dispute.py`](https://github.com/Tallybook-Org/tallybook-contracts/blob/main/scripts/resolve-dispute.py),
so it does not have to be reconstructed from this page. It reads both secrets from the
local `stellar keys` store rather than taking them as arguments, and takes the
statement's sequence number:

```
python3 scripts/resolve-dispute.py 2
```

```
submitted, hash: 7922dc28a2fc8af21631cac58c610cf1d7e6dfe2d1a00ed72f692ba78a72ca49
status: GetTransactionStatus.SUCCESS
```

Confirmed after the fact with a plain read:

```
stellar contract invoke \
  --id CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3 \
  --network testnet --source tb-deployer -- get_statement \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM --seq 2
```

```json
{"amount_billed":"500000","amount_settled":"500000","anchored_ledger":5510389,"channel":null,"consumer":"GAJ46LDZSSYAB4YY6VMPM763P652ZROT7TBG3YYE3BZBAUZDXYDDK6OY","operator":"GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM","period_end":5510360,"period_start":5510355,"price_book_version":1,"protocol":"X402","request_count":5,"status":"Resolved","token":"CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC","usage_root":"bb48f8056bf24646e0e9eb0758ca0c3769ece76845023c4d320438182d9ee3fa"}
```

`status: "Resolved"`. Statement `seq: 1` is untouched and still `Anchored`, which is
what [Verifying a charge](../guides/verifying-a-charge.md) checks against.

### `get_statement`

Shown above. No auth required.

### `list_statements`

```
stellar contract invoke \
  --id CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3 \
  --network testnet --source tb-deployer -- list_statements \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --consumer GAJ46LDZSSYAB4YY6VMPM763P652ZROT7TBG3YYE3BZBAUZDXYDDK6OY
```

```
[1,2]
```

Oldest first. Both statements belong to the same (operator, consumer) pair.

### `get_dispute`

```
stellar contract invoke \
  --id CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3 \
  --network testnet --source tb-deployer -- get_dispute \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM --seq 2
```

```json
{"amount_credited":"0","consumer":"GAJ46LDZSSYAB4YY6VMPM763P652ZROT7TBG3YYE3BZBAUZDXYDDK6OY","opened_ledger":5510397,"reason_hash":"0101010101010101010101010101010101010101010101010101010101010101","resolution_hash":"cf6bfd7bf721a7151cdcc05a2033ea52ef77d35178a66a18599b92286dbce134","resolved_ledger":5510401}
```

`amount_credited: 0` with a `resolution_hash` set is a resolution that agreed no
credit was due — the dispute is closed, not that nothing happened.

### `extend_statement_ttl`

Run from the **consumer** identity deliberately — proving the "no auth, anyone may
pay rent" design actually holds, not just the operator extending their own record:

```
stellar contract invoke \
  --id CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3 \
  --network testnet \
  --source tb-consumer-demo \
  --send=yes \
  -- extend_statement_ttl \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --seq 1 \
  --ledgers 6307200
```

```
🔗 https://stellar.expert/explorer/testnet/tx/8be9c6652787730b9ed128e1f88060f2b9101f6ec9b570eab84f09d18af7f20b
null
```

`null` is `Ok(())` — success, no return value.
