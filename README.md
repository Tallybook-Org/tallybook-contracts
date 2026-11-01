# Tallybook

Settlement bookkeeping for services paid per request by machines.

[![License: Apache 2.0](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Stellar](https://img.shields.io/badge/Stellar-network-7D00FF)](https://stellar.org)
[![Soroban](https://img.shields.io/badge/Soroban-smart%20contracts-000000)](https://soroban.stellar.org)
[![CI](https://github.com/Tallybook-Org/tallybook-contracts/actions/workflows/ci.yml/badge.svg)](https://github.com/Tallybook-Org/tallybook-contracts/actions/workflows/ci.yml)
[![docs](https://github.com/Tallybook-Org/tallybook-contracts/actions/workflows/docs.yml/badge.svg)](https://tallybook-org.github.io/tallybook-contracts/)

**[Read the docs](https://tallybook-org.github.io/tallybook-contracts/)** for the
full protocol explanation, contract reference, and guides — this README covers the
essentials; the docs site covers everything.

## Live on testnet

| Contract | Address | Wasm hash | Explorer |
|---|---|---|---|
| `price_book` | `CC6VXCEIXGGFIZXH2DGPVPUXYVVI3YVEQQ5M7XAWNEN7SADIYACTRFZ6` | `d03c8c9b193810b7bc0755d8dc8e517d4b1279cac29f4e1ce0e43d786ec08188` | [stellar.expert](https://stellar.expert/explorer/testnet/contract/CC6VXCEIXGGFIZXH2DGPVPUXYVVI3YVEQQ5M7XAWNEN7SADIYACTRFZ6) |
| `statement_registry` | `CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3` | `c309069607e59b48c7cf65ee4f353a8cefcfe361b73a6c3ae74eb984733defd9` | [stellar.expert](https://stellar.expert/explorer/testnet/contract/CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3) |

**Neither contract has been audited, and both are deployed to testnet only. Do not use
them with real funds — see [SECURITY.md](SECURITY.md).**

Every function documented below is live at those addresses, including
`price_book::extend_version_ttl` and `statement_registry::verify_charge`. These
contracts have no upgrade path by design, so a new build means a new address — the
`v0.1.0` deployment is still on chain at its own addresses and is simply no longer
current.

## The problem

Machine-paid APIs settle three different ways today, and none of the three leaves the
buyer with something they can check.

x402 settles per request against a signed authorization entry. MPP charge mode settles
per request as a direct SAC transfer. MPP session mode settles a whole channel at once,
against commitments the payer and payee exchange off-chain as the channel progresses.
In the first two, the receipt is just "a transfer happened" — nothing on-chain says what
it was for or whether the price was right. In session mode it's worse: the seller's
revenue is a signature sitting in their own database until someone submits it.

That has a sharp failure mode. A payment channel has a funder-controlled refund window.
If it expires before the latest commitment is swept into the channel contract, the
funder's refund claims back the entire remaining balance — including amounts the
recipient already earned by serving requests, if nobody got a signature on-chain in
time. Money the recipient billed for and delivered on can revert to the payer through no
fault on either side.

Two other projects sit near this problem, and it's worth naming them rather than
blurring what each one does. OpenZeppelin's facilitator handles settlement — moving the
money for x402 payments. RouteDock handles execution and routing — getting a request to
the right paid endpoint. Tallybook does neither. It does the books: an on-chain,
merkle-rooted record of what was billed and at what price, so a buyer can check one line
item against it without trusting the seller's own database.

## How it works

The full Tallybook system has off-chain and on-chain parts. This repository is the
on-chain part — two contracts, nothing else.

1. **Meter.** An off-chain collector counts requests and prices them against the
   operator's published schedule. This stays off-chain: at $0.01 a request, a
   per-request ledger write costs more than the request earns.
2. **Custody commitments.** For MPP session mode, an off-chain custodian holds the
   signed channel commitments as they arrive.
3. **Sweep before the refund window.** The custodian submits the latest commitment to
   the payment channel contract before the funder's refund window closes, so accrued
   revenue is claimed instead of reverting in a refund. Tallybook consumes the existing
   `stellar-experimental/one-way-channel` contract for this — it isn't forked, vendored,
   or reimplemented here.
4. **Anchor statements.** When a billing period closes, the collector merkle-roots the
   period's usage records and anchors the statement in `statement_registry`, tied to the
   exact `price_book` version in force for that period. This is where this repo starts.
5. **Verify.** A buyer takes one line off an invoice and checks it against the anchored
   root with `verify_usage` — no need to trust the seller's off-chain books. If a charge
   looks wrong, `open_dispute` puts that in public view too.

## The rest of Tallybook

This repository is the on-chain half. The off-chain half — metering, custody,
sweeping, indexing — is a separate repository,
[`Tallybook-Org/tallybook`](https://github.com/Tallybook-Org/tallybook), which
consumes these two contracts' deployed interfaces by contract ID and vendors or forks
none of their code. Three services over one Postgres database: a **collector** that
wraps a paid API and prices each request, a **settler** that sweeps channel revenue
before a payer can reclaim it, and an **indexer** that reconciles anchored statements
against what actually settled on chain.

The two repositories share one hard constraint. The collector's merkle tree builder
and this repository's `leaf.rs` must agree byte for byte, or every proof the collector
produces is invalid on chain. Both commit the same `fixtures/merkle/` files and test
against them — see [The usage record leaf format](#the-usage-record-leaf-format).

Neither repository has been audited.

## Contracts

The two crates in this repo are named `price-book` and `statement-registry`; the
contracts they compile to are `price_book` and `statement_registry`. Crate names appear
only in build commands below (`make build`, `cargo`/`stellar` `--package` flags) —
everywhere else, including the section headers immediately below, refers to the
contracts by their deployed names.

### `price_book`

An append-only, versioned record of what an operator charges and from when. Without it,
the amounts in a statement are unfalsifiable — a buyer cannot distinguish an honest bill
from a retroactively raised price. The full price schedule (endpoints, units, per-unit
prices) lives off-chain as canonical JSON; only its `sha256` hash and its location go
on-chain.

#### Functions

- **`__constructor(admin: Address)`** — deploy-time only. Stores `admin` in instance
  storage. `admin` holds no power over operator data — it cannot publish, edit, or remove
  a version. It exists only as a documented owner for future rent funding; there is no
  admin-gated function anywhere in this contract.

- **`publish(operator: Address, schedule_hash: BytesN<32>, uri: String, effective_ledger: u32) -> Result<u32, Error>`**
  — callable only by `operator`. Publishes a new price schedule version. Schedules must
  move strictly forward in time: `effective_ledger` may equal the current ledger
  (effective immediately) but not precede it, and must strictly exceed the previous
  version's `effective_ledger` if one exists. `uri` must be at most 200 bytes. Returns
  the new version number (the first published version is `1`).
  Errors: `UriTooLong`, `EffectiveInPast`, `EffectiveNotAfter`, `TimelineFull`.

- **`get_version(operator: Address, version: u32) -> Result<PriceBookVersion, Error>`** —
  no auth, read-only. Errors: `NotFound`.

- **`latest(operator: Address) -> Result<u32, Error>`** — no auth, read-only. The latest
  version number `operator` has published. Errors: `NotFound` if `operator` has never
  published.

- **`extend_version_ttl(operator: Address, version: u32, ledgers: u32) -> Result<(), Error>`**
  — **no auth, intentionally**, and for the same reason
  `statement_registry`'s `extend_statement_ttl` has none: anyone may pay rent to keep an
  audit record alive. Here that reason is load bearing rather than merely convenient. A
  statement is only falsifiable while the price evidence it names is still readable —
  `Version` holds the schedule hash and URI the amounts were priced against, and
  `Timeline` is what `version_at()` searches to confirm which version was in force. The
  only other writer touching either key is `publish()`, which requires the operator's
  auth, and an operator who has stopped publishing never calls it again; without this
  function a stranger could keep a statement alive indefinitely while the prices it
  depends on archived out from under it. Extends the version and the timeline together —
  extending one without the other leaves an audit record half readable. `ledgers` is
  clamped to a sane maximum. Errors: `NotFound`.

- **`version_at(operator: Address, ledger: u32) -> Result<u32, Error>`** — no auth,
  read-only. Returns which version of `operator`'s schedule was in force at `ledger`: the
  version with the highest `effective_ledger` not exceeding `ledger`. This is the
  function `statement_registry` calls, and the function a buyer calls to check which
  prices applied on the day they were billed. Binary searches the operator's timeline
  rather than scanning every published version. Errors: `NotFound` if the timeline is
  empty or every entry's `effective_ledger` is after `ledger`.

#### Errors

| Discriminant | Variant | Meaning |
|---|---|---|
| 1 | *(unused)* | Was `AlreadyInitialized`. Per [CAP-0058](https://github.com/stellar/stellar-protocol/blob/master/core/cap-0058.md), a contract's constructor is invoked exactly once, at creation, and is never callable again — the guard this occupied was unreachable on-chain, and the discriminant is left unused rather than renumbered. |
| 2 | `NotFound` | No such version, or operator has never published. |
| 3 | `EffectiveInPast` | `effective_ledger` is before the current ledger. |
| 4 | `EffectiveNotAfter` | `effective_ledger` does not strictly exceed the previous version's. |
| 5 | `TimelineFull` | `TIMELINE_CAP` (256) reached — see Known limitations. |
| 6 | `UriTooLong` | `uri` exceeds 200 bytes. |

#### Events

`publish` — topics `("price_book", "publish")`, data `(operator: Address, version: u32, schedule_hash: BytesN<32>, effective_ledger: u32)`.

### `statement_registry`

Anchors a billing period's statement so a buyer can verify one charge against it, check
it against the price then in force, and contest it publicly if it is wrong. Reads
`price_book` via a cross-contract call; the `price_book` address is set at construction
and is immutable.

#### Functions

- **`__constructor(admin: Address, price_book: Address)`** — deploy-time only. Stores
  both in instance storage. `admin` holds no power over operator data. `price_book` is
  immutable after construction — there is no setter. A mutable price book address would
  let an operator swap in a permissive registry and invalidate every historical
  statement; if the price book must change, a new `statement_registry` is deployed.

- **`anchor(operator, consumer, period_start: u32, period_end: u32, usage_root: BytesN<32>, request_count: u64, token: Address, amount_billed: i128, amount_settled: i128, price_book_version: u32, protocol: Protocol, channel: Option<Address>) -> Result<u64, Error>`**
  — callable only by `operator`. Validates, in order: the period is non-empty and
  already finished (`period_start < period_end <= current ledger`); `request_count` is
  non-zero; amounts are non-negative and `amount_settled <= amount_billed`; `channel` is
  present iff `protocol == MppSession`; and — the most important checks in this contract
  — that the price book agrees on **one** version for the whole period. This last check
  is two live calls to `price_book`, not one: `version_at(period_start)` and
  `version_at(period_end)` must agree with each other before either is compared against
  `price_book_version`. If the schedule changed partway through the period, the two
  calls disagree and the period is rejected outright with `PeriodSpansPriceChange` — no
  single version honestly covers a statement whose window straddles a price change, no
  matter what the caller claims. Only once the period is confirmed to sit inside one
  version's window is that version compared against `price_book_version`; a mismatch
  there is `PriceVersionStale`. Returns the new sequence number for `operator` (the
  first anchor is `1`).
  Errors: `BadPeriod`, `EmptyStatement`, `BadAmounts`, `ChannelMismatch`,
  `PriceVersionUnknown`, `PeriodSpansPriceChange`, `PriceVersionStale`, `IndexFull`.

- **`verify_usage(operator, seq: u64, leaf: BytesN<32>, proof: Vec<BytesN<32>>) -> Result<bool, Error>`**
  — no auth; anyone can verify a charge against a statement, and that is the whole
  point. Folds `leaf` up through `proof` via sorted-pair merkle hashing and compares the
  result against the statement's `usage_root`. Returns `Ok(false)` — not an error — for
  a valid-shaped proof that does not reach the root; errors are reserved for malformed
  input.
  Errors: `ProofTooLong` if `proof` has more than `MAX_PROOF_NODES` (32) entries,
  `NotFound` if no such statement exists.

- **`verify_charge(operator, seq: u64, record: UsageRecord, proof: Vec<BytesN<32>>) -> Result<bool, Error>`**
  — no auth. **This is the function an invoice line is checked with.** Where
  `verify_usage` takes an opaque 32-byte leaf and answers only "is this hash in the
  tree" — leaving the caller trusting whoever computed the leaf, including on how the
  amount and price version were encoded into it — `verify_charge` takes the record's own
  fields, derives the leaf itself, and binds the record to the statement before folding:
  `record.consumer` must be the statement's `consumer`, `record.price_v` must be the
  statement's `price_book_version` (the version `anchor()` already proved was in force
  across the whole period), and `record.ledger` must fall inside
  `[period_start, period_end]`, both inclusive. Together with `anchor()`'s price check
  that is the full chain a buyer needs: this request, at this price version, inside this
  period, under a schedule the price book agrees was in force then. Returns `Ok(false)`
  for a record that is consistent with the statement but whose proof does not reach
  `usage_root`; the record errors are for input that contradicts the statement outright,
  where `false` would under-report what is wrong.
  Errors: `ProofTooLong`, `NotFound`, `RecordConsumerMismatch`, `RecordPriceMismatch`,
  `RecordOutsidePeriod`.

- **`open_dispute(operator, seq: u64, consumer: Address, reason_hash: BytesN<32>) -> Result<(), Error>`**
  — callable only by the statement's `consumer`. `consumer` must match the statement's
  own `consumer` field — a mismatch returns `NotFound`, the same error as a missing
  statement, so a party unrelated to a statement cannot learn whether it exists. Sets
  the statement's status to `Disputed` and records the dispute.
  Errors: `NotFound`, `NotAnchored` if the statement is not currently `Anchored`.

- **`resolve_dispute(operator, seq: u64, resolution_hash: BytesN<32>, amount_credited: i128) -> Result<(), Error>`**
  — requires **both** parties' auth, `operator` first, then the statement's `consumer`.
  There is deliberately no arbiter, no admin override, and no timeout that
  auto-resolves in the operator's favour — if the two sides never agree, the statement
  stays publicly `Disputed` forever, and that public mark is the entire enforcement
  mechanism.
  Errors: `NotFound`, `NotDisputed` if the statement is not currently `Disputed`,
  `CreditTooLarge` if `amount_credited` is negative or exceeds the statement's
  `amount_billed`.

- **`get_statement(operator, seq: u64) -> Result<Statement, Error>`** — no auth,
  read-only. Errors: `NotFound`.

- **`list_statements(operator, consumer) -> Result<Vec<u64>, Error>`** — no auth,
  read-only. Sequence numbers anchored for the pair, oldest first, newest last. An empty
  result means no statements exist for the pair; that is not an error.

- **`get_dispute(operator, seq: u64) -> Result<Dispute, Error>`** — no auth, read-only.
  Errors: `NotFound` if no dispute has been opened.

- **`extend_statement_ttl(operator, seq: u64, ledgers: u32) -> Result<(), Error>`** —
  **no auth, intentionally.** Anyone may pay rent to keep an audit record alive — an
  auditor checking a three-year-old statement is not a party to it and was never going
  to be able to authenticate as one; requiring auth here would just make old records
  unrecoverable once their original parties are unreachable. Extends the TTL of the
  statement, and its dispute if one exists, out to `ledgers` from now, clamped to a sane
  maximum. Errors: `NotFound` if no such statement exists.

#### Errors

| Discriminant | Variant | Meaning |
|---|---|---|
| 1 | *(unused)* | Was `AlreadyInitialized`, for the same reason and with the same fix as `price_book`'s — see above. |
| 2 | `NotFound` | No such statement/dispute, or (for `open_dispute`) a real statement but the wrong consumer. |
| 3 | `BadPeriod` | `period_start >= period_end`, or `period_end > current ledger`. |
| 4 | `BadAmounts` | A negative amount, or `amount_settled > amount_billed`. |
| 5 | `EmptyStatement` | `request_count == 0`. |
| 6 | `PriceVersionUnknown` | `price_book` has no such version for this operator (or the cross-contract call itself failed). |
| 7 | `PriceVersionStale` | `version_at(period_start)` and `version_at(period_end)` agree with each other, but not with the claimed `price_book_version`. |
| 8 | `ChannelMismatch` | `channel` set without `MppSession`, or absent with it. |
| 9 | `IndexFull` | `CONSUMER_IDX_CAP` (500) reached — see Known limitations. |
| 10 | `NotAnchored` | A dispute was opened on a statement that is not `Anchored`. |
| 11 | `NotDisputed` | `resolve_dispute` called on a statement that is not `Disputed`. |
| 12 | `CreditTooLarge` | `amount_credited` is negative or exceeds `amount_billed`. |
| 13 | `ProofTooLong` | A merkle proof longer than `MAX_PROOF_NODES` (32). |
| 14 | `PeriodSpansPriceChange` | `version_at(period_start) != version_at(period_end)`: the schedule changed partway through the period, so no single `price_book_version` honestly covers the whole thing. Distinct from `PriceVersionStale`, which is about the claimed version not matching what's in force — this is about the period itself straddling a change, independent of what was claimed. |
| 15 | `RecordConsumerMismatch` | `verify_charge`: the record's `consumer` is not the statement's, so it was never part of this bill whatever the proof shows. |
| 16 | `RecordPriceMismatch` | `verify_charge`: the record's `price_v` is not the statement's `price_book_version`. |
| 17 | `RecordOutsidePeriod` | `verify_charge`: the record's `ledger` falls outside `[period_start, period_end]`. |

#### Events

- `anchor` — topics `("statement", "anchor")`, data `(operator: Address, consumer: Address, seq: u64, usage_root: BytesN<32>, amount_billed: i128, amount_settled: i128, protocol: Protocol)`.
- `dispute` — topics `("statement", "dispute")`, data `(operator: Address, consumer: Address, seq: u64, reason_hash: BytesN<32>)`.
- `resolve` — topics `("statement", "resolve")`, data `(operator: Address, consumer: Address, seq: u64, resolution_hash: BytesN<32>, amount_credited: i128)`.

## The usage record leaf format

A usage tree leaf is derived from one billed request. The derivation is fixed and is
implemented twice — here, in `contracts/statement-registry/src/leaf.rs`, and in the
off-chain collector's `internal/merkle/leaf.go` in the
[`tallybook`](https://github.com/Tallybook-Org/tallybook) repo. The two must agree byte
for byte or every proof the collector produces is invalid on chain.

```
record_bytes = XDR(ScVal::Map{           // symbol keys, sorted alphabetically
    "amount":   I128(charged_amount),
    "consumer": Address(consumer),       // G… account, or C… channel contract
    "endpoint": Bytes(sha256(method || " " || path_template)),
    "ledger":   U32(settlement_or_observation_ledger),
    "price_v":  U32(price_book_version),
    "reqid":    Bytes(request_id),
    "units":    U64(unit_count),
})
leaf = sha256(sha256(record_bytes))
```

On this side that is not a hand-rolled encoder: `UsageRecord` is a `#[contracttype]`
struct, and the macro encodes a named-field struct as exactly that `ScVal::Map`, keyed by
field names as symbols, sorted alphabetically. The type *is* the format.

The hash is applied twice on purpose. Sorted-pair folding gives internal nodes no domain
tag of their own, so a single hash would leave a leaf and an internal node
indistinguishable and an internal node could be presented as a leaf. Hashing the record
twice puts every leaf one hash further from the folding domain.

### Cross-repo fixtures

`fixtures/merkle/` is the contract test between the two implementations. Both repos
commit the same files and test against them.

- `single-leaf.json`, `four-leaves.json`, `seven-leaves.json` (unbalanced) pin how
  `fold` combines nodes. They start from leaves that are already hashes, so they say
  nothing about how a request becomes a leaf.
- `records.json` pins that other half: six records with their `record_bytes` and leaf.
  Each vector carries `record_bytes` as well as the leaf so a mismatch localises itself —
  `record_bytes` differing means the encoding drifted, only the leaf differing means the
  hashing did. The vectors are chosen for where two implementations actually diverge: a
  negative amount (the two's-complement path), both ends of the `i128` range with the
  `u32` and `u64` ceilings, and an MPP session record whose `consumer` is the channel
  contract (`C…`) rather than an account (`G…`).

`records.json` is generated from this repo's own implementation, not written by hand.
Regenerate it with:

```sh
TALLYBOOK_WRITE_FIXTURES=1 cargo test -p statement-registry record_vectors
```

Without that variable the same test only compares, so the committed file cannot drift
from the code that produced it without CI failing.

## Known limitations

`Timeline(operator)` — the sorted vector `price_book`'s `version_at()` binary-searches —
is capped at **256 entries** (`TIMELINE_CAP`). `publish()` returns `TimelineFull` once an
operator's 256th version would be appended.

`ConsumerIdx(operator, consumer)` — the vector `statement_registry`'s `list_statements()`
returns — is capped at **500 entries** (`CONSUMER_IDX_CAP`). `anchor()` returns
`IndexFull` once a given (operator, consumer) pair's 500th statement would be appended.

A merkle proof passed to `verify_usage()` is capped at **32 nodes**
(`MAX_PROOF_NODES`), which covers over 4 billion leaves; longer proofs are rejected
with `ProofTooLong` rather than accepted unbounded.

All three are deliberate, documented limits, not hidden ones.

`amount_settled` is the operator's unverifiable claim. The contract cannot check it and
must not pretend to. What makes it meaningful is an off-chain indexer independently
summing SAC transfer and channel events for the period and flagging mismatches.

## Quick start

1. Clone the repo: `git clone https://github.com/Tallybook-Org/tallybook-contracts.git && cd tallybook-contracts`.
2. Install Rust via [rustup](https://rustup.rs).
3. Add the wasm target: `rustup target add wasm32v1-none`. This repo's own
   `rust-toolchain.toml` already lists it and installs it automatically the first time
   you run a cargo command in this directory, but you need it on any toolchain you drive
   by hand. Use `wasm32v1-none` throughout — never `wasm32-unknown-unknown`, which the
   Soroban runtime rejects and which isn't even supported for these contracts past
   Rust 1.82.
4. Install the Stellar CLI: `cargo install --locked stellar-cli` (`28.0.0` or newer).
5. Pick a network: `stellar network ls` shows the built-ins, in this order — `local`,
   `futurenet`, `mainnet`, `testnet`. `testnet` has a default public RPC URL; `mainnet`
   needs your own (see `scripts/deploy-mainnet.sh`).
6. Set up an identity: `stellar keys generate <name> --fund --network testnet` creates
   and funds one on testnet in a single step; `stellar keys add <name>` imports an
   existing secret key instead. Never pass a secret key as a command-line argument
   beyond this local setup — this repo's own deploy scripts only ever read one from the
   environment or the keys store.
7. Build: `make build` — builds `price-book`, then `statement-registry`, via
   `stellar contract build`. `statement-registry` contractimports `price-book`'s
   compiled wasm at compile time, so it cannot compile until that wasm exists on disk;
   `make build` (and CI) encode that ordering, but a bare `cargo test` or `cargo clippy`
   on a clean checkout needs `make build` (or at least
   `stellar contract build --package price-book`) run first.
8. Test: `make test`. Format and lint: `make fmt` / `make clippy`.

## Maintainers

| Name | Role | GitHub | Telegram |
|---|---|---|---|
| Cisco | Maintainer | [@ciscokwiz](https://github.com/ciscokwiz) | [@cisco_i](https://t.me/cisco_i) |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for build instructions, coding standards, and
testing conventions. Issues labeled
[`good first issue`](https://github.com/Tallybook-Org/tallybook-contracts/labels/good%20first%20issue)
are a reasonable place to start.

## Contributors

[![Contributors](https://contrib.rocks/image?repo=Tallybook-Org/tallybook-contracts)](https://github.com/Tallybook-Org/tallybook-contracts/graphs/contributors)
