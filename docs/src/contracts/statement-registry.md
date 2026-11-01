# `statement_registry`

Anchors a billing period's statement so a buyer can verify one charge against it,
check it against the price then in force, and contest it publicly if it's wrong.
Reads `price_book` via a cross-contract call; the `price_book` address is fixed at
construction. See [Statement lifecycle](../protocol/statement-lifecycle.md) for the
`Anchored → Disputed → Resolved` state machine this contract enforces.

## Storage

| Key | Tier | Value | Meaning |
|---|---|---|---|
| `Admin` | Instance | `Address` | Owns rent for this instance. No power over operator data. |
| `PriceBook` | Instance | `Address` | The `price_book` contract this registry checks prices against. Immutable — no setter exists. |
| `Seq(Address)` | Persistent | `u64` | The operator's last assigned sequence number. |
| `Statement(Address, u64)` | Persistent | `Statement` | One anchored statement, by `(operator, seq)`. |
| `ConsumerIdx(Address, Address)` | Persistent | `Vec<u64>` | Sequence numbers anchored for `(operator, consumer)`, oldest first, capped at `CONSUMER_IDX_CAP` (500). |
| `Dispute(Address, u64)` | Persistent | `Dispute` | The dispute against `(operator, seq)`, if one has been opened. |

`Statement` holds `operator`, `consumer` (the payer account or, for `MppSession`, the
channel contract address), `period_start`/`period_end` (ledgers, inclusive),
`usage_root: BytesN<32>`, `request_count: u64`, `token: Address` (which SEP-41 asset
the bill is denominated in — a record only, never transferred by this contract),
`amount_billed`/`amount_settled: i128`, `price_book_version: u32`, `protocol`,
`channel: Option<Address>` (`Some` iff `protocol == MppSession`), and
`anchored_ledger`/`status`, both set by the contract. `amount_settled` is the
operator's own unverifiable claim about what actually landed on-chain for the period —
this contract cannot check it and doesn't try to; an off-chain indexer independently
summing the real transfer and channel events is what makes it meaningful. `Dispute`
holds `consumer`, `reason_hash: BytesN<32>`, `opened_ledger`, and, once resolved,
`resolution_hash`, `amount_credited`, and `resolved_ledger`.

## Functions

### `__constructor(env: Env, admin: Address, price_book: Address)`

Deploy-time only, callable once. Stores both addresses in instance storage and
extends the instance TTL. `price_book` has no setter — a mutable price book address
would let an operator swap in a permissive registry and invalidate every historical
statement.

### `anchor(env, operator, consumer, period_start: u32, period_end: u32, usage_root: BytesN<32>, request_count: u64, token: Address, amount_billed: i128, amount_settled: i128, price_book_version: u32, protocol: Protocol, channel: Option<Address>) -> Result<u64, Error>`

Callable only by `operator`. Validates, in order:

1. `period_start >= period_end`, or `period_end > env.ledger().sequence()` →
   `BadPeriod`. A period can't be empty or unfinished.
2. `request_count == 0` → `EmptyStatement`.
3. `amount_billed < 0 || amount_settled < 0 || amount_settled > amount_billed` →
   `BadAmounts`.
4. `channel.is_some()` must equal `protocol == MppSession` → `ChannelMismatch`
   otherwise.
5. Two live calls to `price_book.version_at()` — one for `period_start`, one for
   `period_end`. Either call failing → `PriceVersionUnknown`. The two results
   disagreeing with each other → `PeriodSpansPriceChange` (see
   [Pricing](../protocol/pricing.md)). Only once they agree is that one version
   compared against `price_book_version`; a mismatch → `PriceVersionStale`.
6. `ConsumerIdx(operator, consumer).len() >= CONSUMER_IDX_CAP` → `IndexFull`.

On success: assigns `seq = Seq(operator).unwrap_or(0) + 1` (the first anchor is `1`),
writes the `Statement` with `anchored_ledger` set to the current ledger and
`status = Anchored`, writes `Seq`, appends `seq` to `ConsumerIdx`, extends the
instance TTL, publishes the `anchor` event, and returns `seq`.

### `verify_usage(env, operator: Address, seq: u64, leaf: BytesN<32>, proof: Vec<BytesN<32>>) -> Result<bool, Error>`

No auth — anyone can verify a charge, and that's the point. `proof.len() >
MAX_PROOF_NODES` (32) → `ProofTooLong`. Loads the statement (`NotFound` if it
doesn't exist), folds `leaf` up through `proof` (see [Merkle verification](merkle.md)),
and returns whether the result equals `usage_root` — `Ok(false)`, not an error, for a
well-formed proof that doesn't match.

### `verify_charge(env, operator: Address, seq: u64, record: UsageRecord, proof: Vec<BytesN<32>>) -> Result<bool, Error>`

No auth. **This is the call an invoice line is checked with**, and the one a buyer
should reach for; `verify_usage` above answers a narrower question.

`verify_usage` takes an opaque 32-byte leaf, so it can only say whether that hash is
in the tree. It cannot say what the hash means, which leaves the caller trusting
whoever computed it — including on how the amount and the price version were encoded.
`verify_charge` instead takes the record's own fields, derives the leaf itself (see
[Merkle verification](merkle.md)), and binds the record to the statement before
folding:

| Check | Failure |
|---|---|
| `record.consumer == statement.consumer` | `RecordConsumerMismatch` |
| `record.price_v == statement.price_book_version` | `RecordPriceMismatch` |
| `period_start <= record.ledger <= period_end`, both inclusive | `RecordOutsidePeriod` |

`price_book_version` is the version `anchor()` already proved was in force across the
whole period, so pinning a line to it is what ties an individual charge to the
schedule the statement claims to have been billed under. Together with `anchor()`'s
own price check that is the full chain: this request, at this price version, inside
this period, under a schedule the price book agrees was in force then.

Returns `Ok(false)` — not an error — for a record that is consistent with the
statement but whose proof doesn't reach `usage_root`: a well-formed claim that simply
isn't in the tree. The three record errors are for input that contradicts the
statement outright, where `false` would under-report what is wrong. `ProofTooLong`
and `NotFound` behave exactly as in `verify_usage`.

### `open_dispute(env, operator: Address, seq: u64, consumer: Address, reason_hash: BytesN<32>) -> Result<(), Error>`

Callable only by the statement's own `consumer` (`consumer.require_auth()`). If the
supplied `consumer` doesn't match the statement's `consumer` field, returns
`NotFound` — the same error as a missing statement, so a party unrelated to it can't
even learn whether it exists. `NotAnchored` if the statement isn't currently
`Anchored`. On success, sets `status = Disputed`, writes a `Dispute` with
`opened_ledger` set and `amount_credited = 0`, and publishes the `dispute` event.

### `resolve_dispute(env, operator: Address, seq: u64, resolution_hash: BytesN<32>, amount_credited: i128) -> Result<(), Error>`

Requires **both** `operator.require_auth()` and the statement's `consumer.require_auth()`,
operator first. `NotDisputed` if the statement isn't currently `Disputed`.
`amount_credited < 0 || amount_credited > statement.amount_billed` → `CreditTooLarge`.
On success, sets `status = Resolved`, fills in `resolution_hash`, `amount_credited`,
and `resolved_ledger` on the `Dispute`, and publishes the `resolve` event. There is no
arbiter, no admin override, and no timeout — if the two sides never agree, the
statement stays publicly `Disputed` forever.

### `get_statement(env, operator: Address, seq: u64) -> Result<Statement, Error>`

No auth, read-only. Errors: `NotFound`.

### `list_statements(env, operator: Address, consumer: Address) -> Result<Vec<u64>, Error>`

No auth, read-only. Returns `ConsumerIdx(operator, consumer)` as-is — sequence
numbers oldest first. An empty result means no statements exist for the pair; that's
not an error.

### `get_dispute(env, operator: Address, seq: u64) -> Result<Dispute, Error>`

No auth, read-only. Errors: `NotFound` if no dispute has been opened.

### `extend_statement_ttl(env, operator: Address, seq: u64, ledgers: u32) -> Result<(), Error>`

**No auth, intentionally.** Anyone may pay rent to keep an audit record alive — an
auditor checking a statement years later is not a party to it and could never
authenticate as one. Extends the TTL of the `Statement`, and its `Dispute` if one
exists, out to `ledgers` from now, each clamped to a maximum of `BUMP_AMOUNT`.
Errors: `NotFound` if no such statement exists.

## Errors

See the full table, both contracts, on the [Errors](errors.md) page.

## Events

| Event | Topics | Data (positional) |
|---|---|---|
| `anchor` | `("statement", "anchor")` | `(operator: Address, consumer: Address, seq: u64, usage_root: BytesN<32>, amount_billed: i128, amount_settled: i128, protocol: Protocol)` |
| `dispute` | `("statement", "dispute")` | `(operator: Address, consumer: Address, seq: u64, reason_hash: BytesN<32>)` |
| `resolve` | `("statement", "resolve")` | `(operator: Address, consumer: Address, seq: u64, resolution_hash: BytesN<32>, amount_credited: i128)` |
