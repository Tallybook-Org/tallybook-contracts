# Verifying a charge

A full walkthrough against the live testnet deployment, checking one specific
request against a real anchored statement. Every command below was run for real
against `CB2IEP4SQ2GC5747HFHNMXEYWEULC5Z5TTTLET2QA4CAA5SYCWAXFKAW` (`price_book`) and
`CB75TTWGP3TLKEDGA2WOLEVCNKLUX6X5KS47GMWGBHUAVES7J55LY25M` (`statement_registry`)
before this page was committed, and the output shown is the real output — not
retyped from source.

Statement `seq: 1` for operator
`GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM` was anchored specifically
to write this page: one usage record, so the merkle tree is the simplest possible
case — a single leaf, where the leaf and the root are the same value and the proof is
empty. [Merkle verification](../contracts/merkle.md) covers the multi-leaf case.

## 1. Get the statement

```
stellar contract invoke \
  --id CB75TTWGP3TLKEDGA2WOLEVCNKLUX6X5KS47GMWGBHUAVES7J55LY25M \
  --network testnet \
  --source tb-deployer \
  -- get_statement \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --seq 1
```

```json
{"amount_billed":"500000","amount_settled":"500000","anchored_ledger":4592019,"channel":null,"consumer":"GAJ46LDZSSYAB4YY6VMPM763P652ZROT7TBG3YYE3BZBAUZDXYDDK6OY","operator":"GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM","period_end":4592000,"period_start":4590000,"price_book_version":1,"protocol":"X402","request_count":5,"status":"Anchored","token":"CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC","usage_root":"bb48f8056bf24646e0e9eb0758ca0c3769ece76845023c4d320438182d9ee3fa"}
```

Any caller can run this — `get_statement` takes no auth. Note
`price_book_version: 1` and `usage_root`; both matter for the next two steps.

## 2. Get the price book version that applied

Confirm what `price_book` itself says was in force at the end of this period —
that's what `anchor()` already checked before this statement was allowed to exist,
but you don't have to take the statement's word for `price_book_version` either:

```
stellar contract invoke \
  --id CB2IEP4SQ2GC5747HFHNMXEYWEULC5Z5TTTLET2QA4CAA5SYCWAXFKAW \
  --network testnet \
  --source tb-deployer \
  -- version_at \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --ledger 4592000
```

```
1
```

Matches `price_book_version: 1` from the statement.

## 3. Build the leaf

The leaf format is fixed and documented in full in
[Merkle verification](../contracts/merkle.md) — an XDR-encoded record, double-hashed.
For this statement, the leaf (built the same way a real off-chain collector would)
is:

```
bb48f8056bf24646e0e9eb0758ca0c3769ece76845023c4d320438182d9ee3fa
```

Since this statement covers a single usage record, this leaf **is** the
`usage_root` — there's nothing to fold it up through, so the proof is empty.

## 4. Call `verify_usage`

```
stellar contract invoke \
  --id CB75TTWGP3TLKEDGA2WOLEVCNKLUX6X5KS47GMWGBHUAVES7J55LY25M \
  --network testnet \
  --source tb-deployer \
  -- verify_usage \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --seq 1 \
  --leaf bb48f8056bf24646e0e9eb0758ca0c3769ece76845023c4d320438182d9ee3fa \
  --proof '[]'
```

```
true
```

## 5. Read the result

`true` means this leaf really is what `usage_root` commits to. `verify_usage` also
takes no auth — anyone can run exactly this command against exactly this statement
and get the same answer, without asking the operator to confirm anything.

For comparison, the same call with a leaf that doesn't match returns `false`, not an
error — this was run for real too:

```
stellar contract invoke \
  --id CB75TTWGP3TLKEDGA2WOLEVCNKLUX6X5KS47GMWGBHUAVES7J55LY25M \
  --network testnet \
  --source tb-deployer \
  -- verify_usage \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --seq 1 \
  --leaf 0000000000000000000000000000000000000000000000000000000000000000 \
  --proof '[]'
```

```
false
```

## 6. The same check in one call, without trusting step 3

Step 3 is the weak link in the walkthrough above. The leaf was "built the same way a
real off-chain collector would" — which means that to accept the `true` in step 4,
you have to accept that the leaf was built from the record you were actually billed
for, at the amount and price version on your invoice. `verify_usage` cannot tell you
that. It sees 32 bytes and answers one question: is this hash in the tree.

`verify_charge` closes it. You pass the record's fields rather than a leaf, the
contract derives the leaf itself, and before folding anything it checks the record
against the statement: the consumer must be the statement's consumer, the price
version must be the statement's `price_book_version`, and the ledger must fall inside
the billing period. Step 2 confirmed that `price_book_version` really was the version
in force; this pins your individual line to it.

```
stellar contract invoke \
  --id <statement_registry> \
  --network testnet \
  --source <your-identity> \
  -- verify_charge \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --seq 1 \
  --record '{
    "amount": "100000",
    "consumer": "GAJ46LDZSSYAB4YY6VMPM763P652ZROT7TBG3YYE3BZBAUZDXYDDK6OY",
    "endpoint": "ec71df17a899f521f884c5d16596fbb637ce8c83a267889cdfe5f581a1d8172d",
    "ledger": 4591000,
    "price_v": 1,
    "reqid": "abababababababababababababababababababababababababababababababab",
    "units": 1
  }' \
  --proof '[]'
```

`endpoint` is `sha256(method || " " || path_template)` — `sha256("GET /v1/search")` in
this example. [Merkle verification](../contracts/merkle.md) gives the full record
format and the committed vectors it is tested against.

What the three failure modes mean to you, as distinct from `false`:

| Result | What it tells you |
|---|---|
| `true` | This exact record, at this amount and price version, is in what you were billed. |
| `false` | The record is consistent with the statement, but it isn't in the tree — you were not billed for this. |
| `RecordPriceMismatch` | The line was priced against a different schedule than the statement claims. |
| `RecordConsumerMismatch` | The line was billed to someone else. |
| `RecordOutsidePeriod` | The line belongs to a different billing period. |

Those three are errors rather than `false` on purpose: `false` would tell you the
record isn't in this statement while hiding the more useful fact that it could not
have been.

**Not yet live at the addresses at the top of this page.** `verify_charge` was added
after the `v0.1.0` deploy, so unlike every other command here its output is not
reproduced from a real testnet run — the invocation above shows the call shape, and
the behaviour it describes is what
[`contracts/statement-registry/src/test.rs`](https://github.com/Tallybook-Org/tallybook-contracts/blob/main/contracts/statement-registry/src/test.rs)
covers. This page will be rerun against the live contract, with real output, when the
next deploy lands.
