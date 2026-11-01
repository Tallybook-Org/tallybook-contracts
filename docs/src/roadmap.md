# Roadmap

## What exists

Both contracts — `price_book` and `statement_registry` — are complete, tested, and
deployed to testnet. `price_book.extend_version_ttl` and
`statement_registry.verify_charge` were added after the `v0.1.0` deploy and are not
live at the published addresses yet. See [Contracts](contracts/overview.md) for the
addresses, and [How it works](how-it-works.md) for which of the five settlement steps
these two contracts cover (anchoring and verification) and where the other three run.

## The off-chain layer, in a separate repository

None of this is in this repository, and none of it is planned *for* this repository.
It lives in [`Tallybook-Org/tallybook`](https://github.com/Tallybook-Org/tallybook),
which consumes these two contracts' deployed interfaces — contract IDs by environment
variable — and vendors or forks none of their code. Three services sharing one
Postgres database:

- **Collector** (`cmd/collector`, `internal/meter`, `internal/httpapi`,
  `internal/custody`). An HTTP metering layer that wraps a paid API, prices each
  request against the operator's published schedule, builds the merkle tree `anchor()`
  commits to (`internal/merkle`), and holds custody of payment-channel commitments.
- **Settler** (`cmd/settler`, `internal/settle`). Watches open channels and sweeps
  earned revenue on chain before the payer can reclaim it — the step that closes the
  gap described in [The problem](problem.md).
- **Indexer** (`cmd/indexer`, `internal/indexer`). Ingests Soroban events into
  Postgres so metered usage can be reconciled against what actually settled, which is
  the independent check on a statement's `amount_settled` claim that this contract
  deliberately doesn't attempt on-chain.

The merkle tree builder there and `leaf.rs` here have to agree byte for byte. Both
repositories commit the same `fixtures/merkle/` files and test against them; see
[Merkle verification](contracts/merkle.md) for what each file pins.

Still genuinely unbuilt: a **dashboard** over all of the above, so an operator or a
buyer can read a period's statement, its dispute status and its reconciliation state
without running any commands.

## Planned: consumer index bucketing

**Status: settled design, not yet implemented.** `ConsumerIdx(operator, consumer)`
currently stores every sequence number ever anchored for a pair in one persistent
`Vec<u64>` — every `anchor()` call for that pair reads, appends to, and rewrites the
whole thing, and `IndexFull` (`CONSUMER_IDX_CAP` = 500) permanently blocks a pair
once its lifetime history fills it.

The settled replacement, recorded in full in `CLAUDE.md` §6:

- The storage key becomes `ConsumerIdx(operator, consumer, bucket)`, where
  `bucket = period_end / BUCKET_LEDGERS`.
- `BUCKET_LEDGERS = 518_400` — 30 × `DAY_IN_LEDGERS` (17,280), the same unit
  `BUMP_THRESHOLD` already uses elsewhere in this contract. Each bucket spans roughly
  30 days.
- `list_statements` changes signature to
  `list_statements(env, operator, consumer, start_bucket: u32, limit: u32) -> Result<Vec<u64>, Error>`,
  returning ascending sequence numbers from `start_bucket` forward, `limit` clamped
  to 100.
- `IndexFull` stays at discriminant 9 but is redefined: it means a single bucket
  exceeded `BUCKET_CAP = 500`, not that the pair's lifetime history is full — lifetime
  capacity per pair becomes unbounded, since a pair just accumulates more buckets
  over time instead of hitting a hard ceiling.

This is a public API change (the `list_statements` signature), not only a storage
change, which is why it's specified here and in `CLAUDE.md` before any code changes
land — see the tracked issue for current implementation status.

## Deliberately out of scope

- A facilitator. Tallybook doesn't settle payments — see
  [The problem](problem.md) for what OpenZeppelin already does here.
- A router. Tallybook doesn't execute or route requests — RouteDock already does.
- A payment channel contract. Tallybook consumes
  `stellar-experimental/one-way-channel`; it doesn't fork, wrap, or reimplement it.
- A protocol fee. Tallybook takes no cut of anything, and there's no fee logic
  anywhere in either contract.
