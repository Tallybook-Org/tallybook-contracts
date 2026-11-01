# Running an operator

This guide is for the person running the API — publishing prices and anchoring
statements for buyers to check. It assumes you know HTTP. It doesn't assume you've
used Stellar before.

## What you need first

Everything in Tallybook is tied to a Stellar identity: a keypair, represented by an
address starting with `G`. You sign transactions with the secret half; the public
half — the `G...` address — is what `price_book` and `statement_registry` record as
`operator`. You don't need an account with a balance to have an identity, but you do
need one funded with a little XLM to pay transaction fees, since every call that
changes state (`publish`, `anchor`, and so on) costs a small fee.

Install the [Stellar CLI](https://developers.stellar.org/docs/tools/developer-tools/cli/stellar-cli), then
create and fund an identity on testnet in one step:

```
stellar keys generate my-operator --fund --network testnet
```

This creates a keypair, saves it locally under the alias `my-operator`, and funds it
with test XLM. Use `stellar keys address my-operator` any time you need to see the
resulting `G...` address.

## Publishing a schedule

Your price schedule — every endpoint, its unit, its price — lives off-chain, as
canonical JSON you host yourself. What goes on-chain is a commitment to it: the
`sha256` hash of that exact JSON, plus the URL where it can be fetched. Hash your
schedule file, then call `publish`:

```
stellar contract invoke \
  --id CC6VXCEIXGGFIZXH2DGPVPUXYVVI3YVEQQ5M7XAWNEN7SADIYACTRFZ6 \
  --network testnet \
  --source my-operator \
  --send=yes \
  -- publish \
  --operator <your G... address> \
  --schedule_hash <sha256 of your schedule JSON> \
  --uri https://your-domain.example/schedule.json \
  --effective_ledger <a ledger number now or in the future>
```

`effective_ledger` can be the current ledger (the price applies immediately) or any
ledger after it. It can't be in the past, and it can't be at or before your previous
version's `effective_ledger` if you've published before — prices only ever move
forward in time. `publish` returns your new version number; the first one you ever
publish is `1`.

## Changing prices

Changing a price means publishing a new version — there's no "edit" function, by
design: an editable price book would let you claim a lower price applied in the past
than actually did. Call `publish` again with a new hash, a new URI (or the same one,
if you keep one canonical file and just change its contents — though a fixed URL
whose content changes defeats the point of hashing it, so a new URI per version is the
honest choice), and an `effective_ledger` strictly after your last version's.

## Closing a period and anchoring a statement

Once a billing period ends, you (or your own tooling) merkle-root the period's usage
records and call `anchor`. There's one rule to plan around: **a period can't span a
price change.** If you changed prices partway through what would have been one
billing period, close it at the price change instead — anchor one statement running
up to just before the new version took effect, and start the next period from there.
See [Pricing](../protocol/pricing.md) for why, and
[Verifying a charge](verifying-a-charge.md) for a full worked `anchor` call.

`anchor` needs both a `price_book_version` you're claiming and the actual period
ledgers. If the version you claim doesn't match what `price_book` says was in force
for the whole period, the call fails — that check exists specifically so you can't
anchor against a schedule more favorable to you than the one that actually applied.

## When a buyer disputes

A buyer can open a dispute against any statement you've anchored — you'll see it via
the `dispute` event, or by calling `get_dispute` yourself. There's no admin and no
arbiter here: the statement just stays publicly marked `Disputed` until you and the
buyer agree on an outcome and both sign `resolve_dispute` together. There's nothing
you can do unilaterally to make a dispute go away except reach an actual agreement —
that's deliberate. See [Buying from an operator](buyer.md) for what the process looks
like from the other side.
