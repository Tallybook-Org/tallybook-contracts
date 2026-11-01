# Verifying a charge

A full walkthrough against the live testnet deployment, checking one specific request
against a real anchored statement. Every command below was run for real against
`CC6VXCEIXGGFIZXH2DGPVPUXYVVI3YVEQQ5M7XAWNEN7SADIYACTRFZ6` (`price_book`) and
`CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3` (`statement_registry`)
before this page was committed, and the output shown is the real output — not retyped
from source.

Statement `seq: 1` for operator
`GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM` was anchored specifically
to write this page: one usage record, so the merkle tree is the simplest possible case
— a single leaf, where the leaf and the root are the same value and the proof is empty.
[Merkle verification](../contracts/merkle.md) covers the multi-leaf case.

## 1. Get the statement

```
stellar contract invoke \
  --id CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3 \
  --network testnet \
  --source tb-deployer \
  -- get_statement \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --seq 1
```

```json
{"amount_billed":"100000","amount_settled":"100000","anchored_ledger":5510370,"channel":null,"consumer":"GAJ46LDZSSYAB4YY6VMPM763P652ZROT7TBG3YYE3BZBAUZDXYDDK6OY","operator":"GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM","period_end":5510360,"period_start":5510355,"price_book_version":1,"protocol":"X402","request_count":1,"status":"Anchored","token":"CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC","usage_root":"4156f949f4e7601dcbaaef2fe0536362c13bb40fd8e0bc57e1f2126ee75df860"}
```

Any caller can run this — `get_statement` takes no auth. Note `price_book_version: 1`,
the period `5510355`–`5510360`, and `usage_root`; all three matter below.

`amount_billed` is `100000`, which is 0.01 USDC in USDC's seven-decimal smallest unit —
one request at the published rate.

## 2. Get the price book version that applied

Confirm what `price_book` itself says was in force at the end of this period. That's
what `anchor()` already checked before this statement was allowed to exist, but you
don't have to take the statement's word for `price_book_version` either:

```
stellar contract invoke \
  --id CC6VXCEIXGGFIZXH2DGPVPUXYVVI3YVEQQ5M7XAWNEN7SADIYACTRFZ6 \
  --network testnet \
  --source tb-deployer \
  -- version_at \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --ledger 5510360
```

```
1
```

Matches `price_book_version: 1` from the statement. And the version itself:

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

`effective_ledger: 5510354` is before the period's start, and the operator published it
at ledger `5510336` — *before* it took effect. That ordering is the point of the whole
contract: the price was committed publicly before the requests it priced were served.

`schedule_hash` is the `sha256` of the schedule document itself. This one is the hash of
exactly these bytes:

```json
{"endpoints":[{"method":"GET","path_template":"/v1/search","price_per_unit":"100000","unit":"request"}],"token":"CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC","version":1}
```

Hash that file yourself and you get `b9c42dee77fe7a69df68f21e6f8206ec3ae66b0c173b7bee821e271d9092b9cb`.
That's how you check a schedule you fetched from `uri` is the one that was committed —
the document stays off-chain, only its hash is anchored.

## 3. Check your line with `verify_charge`

This is the call that does the real work. You pass the usage record's own fields, and
the contract derives the leaf itself and checks the record against the statement before
folding anything:

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

`endpoint` is `sha256(method || " " || path_template)` —
`sha256("GET /v1/search")` here, which is why the path template stays off-chain while
the endpoint billed is still committed. `amount` is a string because it is an `i128`.
`proof` is empty because this statement has a single record, so its leaf *is* the
`usage_root`.

`true` means all of this holds at once: this record, at this amount, priced against
version `1`, inside this billing period, is part of what `usage_root` commits to. No
auth was needed — anyone can run exactly this command and get the same answer, without
asking the operator to confirm anything.

## 4. What the failures look like

Each of these was run for real against the same statement.

**An altered amount returns `false`, not an error.** Change `amount` to `100001` and
the leaf the contract derives is no longer the leaf in the tree:

```
false
```

This is the case that matters most, and the one a plain merkle check can't catch. A
caller who accepted a leaf computed by someone else would be verifying that *some* hash
is in the tree while reading a different amount off their invoice.

**A record priced against another schedule is rejected outright.** `price_v: 2`:

```
Error(Contract, #16)
```

`#16` is `RecordPriceMismatch`. The statement claims version `1`, and step 2 confirmed
version `1` is what the price book says was in force, so a line priced under any other
version cannot honestly belong to this bill.

**A record billed to someone else:** a different `consumer`:

```
Error(Contract, #15)
```

`#15` is `RecordConsumerMismatch`.

**A record from another period:** `ledger: 5510354`, one before `period_start`:

```
Error(Contract, #17)
```

`#17` is `RecordOutsidePeriod`. `period_start` and `period_end` are both inclusive, so
this is genuinely outside.

Those three are errors rather than `false` on purpose. `false` would tell you the record
isn't in this statement while hiding the more useful fact that it could not have been.

## 5. The narrower check, if you already hold a leaf

`verify_usage` takes a leaf rather than a record and only answers membership. It is the
right call when something upstream already derived the leaf and you want nothing but the
tree check:

```
stellar contract invoke \
  --id CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3 \
  --network testnet \
  --source tb-deployer \
  -- verify_usage \
  --operator GDOLCHAOYP63BEHGAUJJS5IVQNUXLPBWCHO2HZRBTZRU52XBMW2TRJLM \
  --seq 1 \
  --leaf 4156f949f4e7601dcbaaef2fe0536362c13bb40fd8e0bc57e1f2126ee75df860 \
  --proof '[]'
```

```
true
```

Same statement, same answer — but notice what you had to supply. The leaf is 32 bytes
with nothing in it you can read. Deriving it from the record is what connects it to a
line on an invoice, which is why step 3 is the one to reach for.

## 6. Keeping the record readable

Persistent entries archive when their TTL lapses, and an audit record is worthless if it
cannot be read in three years. Either party — or neither — can pay that rent. This was
run from `tb-consumer-demo`, who is not the operator:

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

Do the same for the price version the statement depends on, or step 2 stops working
long before step 3 does:

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

`null` is `Ok(())` — success, no return value. Neither call needed the operator's
authorization, and neither caller was a party to the statement.

## If the charge is wrong

Nothing above changes the bill; it only tells you whether the bill matches the chain. If
a charge is wrong, `open_dispute` marks the statement publicly, and it stays marked
unless both sides agree to resolve it. [Invoking the
contracts](../developers/invoking.md) has a real dispute and resolution, run end to end
on statement `seq: 2` of this same deployment.
