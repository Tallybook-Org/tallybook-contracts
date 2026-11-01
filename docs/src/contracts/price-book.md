# `price_book`

An append-only, versioned record of what an operator charges and from when. The full
price schedule — endpoints, units, per-unit prices — lives off-chain as canonical
JSON; only its `sha256` hash and its location go on-chain. See
[Pricing](../protocol/pricing.md) for the mechanics and a worked example.

## Storage

| Key | Tier | Value | Meaning |
|---|---|---|---|
| `Admin` | Instance | `Address` | Owns rent for this contract instance. No power over operator data — see `__constructor` below. |
| `Latest(Address)` | Persistent | `u32` | The operator's latest published version number. |
| `Version(Address, u32)` | Persistent | `PriceBookVersion` | One published version, by operator and version number. |
| `Timeline(Address)` | Persistent | `Vec<TimelineEntry>` | The operator's `(effective_ledger, version)` pairs, ascending, capped at `TIMELINE_CAP` (256). Binary-searched by `version_at`. |

`PriceBookVersion` holds `operator`, `version`, `schedule_hash: BytesN<32>`,
`uri: String` (the schedule's `https://` or `ipfs://` location, max 200 bytes),
`effective_ledger`, and `published_ledger` (set by the contract, never the caller).
`TimelineEntry` is just `{ effective_ledger, version }` — a denormalized index kept
separate from the full record so `version_at` doesn't have to load every
`PriceBookVersion` to answer "which one applied."

## Functions

### `__constructor(env: Env, admin: Address)`

Deploy-time only, callable once (per [CAP-0058](https://github.com/stellar/stellar-protocol/blob/master/core/cap-0058.md),
a constructor is never callable again after creation). Stores `admin` in instance
storage and extends the instance TTL. `admin` holds no power over operator data —
it cannot publish, edit, or remove a version for anyone. It exists only as a
documented owner for future rent funding; there is no admin-gated function anywhere
in this contract.

### `publish(env: Env, operator: Address, schedule_hash: BytesN<32>, uri: String, effective_ledger: u32) -> Result<u32, Error>`

Callable only by `operator` (`operator.require_auth()`, checked first). Validates, in
order:

1. `uri.len() > 200` → `UriTooLong`.
2. `effective_ledger < env.ledger().sequence()` → `EffectiveInPast`. Equal to the
   current ledger is allowed — a schedule can take effect immediately.
3. If `operator` has a previous version, `effective_ledger <= previous.effective_ledger`
   → `EffectiveNotAfter`. Versions move strictly forward in time.
4. `Timeline(operator).len() >= TIMELINE_CAP` → `TimelineFull`.

On success: assigns `version = Latest(operator).unwrap_or(0) + 1` (the first published
version is `1`), writes `Version(operator, version)` with `published_ledger` set to
the current ledger, writes `Latest(operator)`, appends a `TimelineEntry` to
`Timeline(operator)`, extends the instance TTL, publishes the `publish` event, and
returns `version`.

### `get_version(env: Env, operator: Address, version: u32) -> Result<PriceBookVersion, Error>`

No auth, read-only. Errors: `NotFound` if no such version exists.

### `latest(env: Env, operator: Address) -> Result<u32, Error>`

No auth, read-only. Errors: `NotFound` if `operator` has never published.

### `version_at(env: Env, operator: Address, ledger: u32) -> Result<u32, Error>`

No auth, read-only. Binary-searches `Timeline(operator)` for the entry with the
highest `effective_ledger` not exceeding `ledger`, and returns its version — the
version in force at that ledger. This is the function `statement_registry.anchor()`
calls (twice, per period — see [Pricing](../protocol/pricing.md)), and the function a
buyer calls directly to check which price applied when they were billed. Errors:
`NotFound` if the timeline is empty, or every entry's `effective_ledger` is after
`ledger`.

### `extend_version_ttl(env: Env, operator: Address, version: u32, ledgers: u32) -> Result<(), Error>`

**No auth, intentionally.** Anyone may pay rent to keep a published version readable,
for the same reason `statement_registry.extend_statement_ttl` takes no auth — except
that here the reason is load bearing rather than merely convenient.

A statement is only falsifiable while the price evidence it names is still readable.
`Version(operator, v)` holds the schedule hash and URI the amounts were priced
against; `Timeline(operator)` is what `version_at()` searches to confirm which
version was in force. Every other writer touching either key is `publish()`, which
requires the operator's auth — and an operator who has stopped publishing never calls
it again. Without this function a third party could keep a statement alive
indefinitely in `statement_registry` while the prices it depends on archived out from
under it, leaving a record that proves a buyer was billed but no longer proves the
rate was the one in force.

Extends the version and the timeline together; extending one without the other leaves
an audit record half readable. `ledgers` is clamped to a sane maximum (`BUMP_AMOUNT`,
365 days of ledgers), and the extension never shortens an existing TTL. Errors:
`NotFound` if `operator` has no such published version.

## Errors

| Discriminant | Variant | Raised by | Meaning |
|---|---|---|---|
| 1 | *(unused)* | — | Was `AlreadyInitialized`. Per [CAP-0058](https://github.com/stellar/stellar-protocol/blob/master/core/cap-0058.md), a constructor runs exactly once at creation and is never callable again, so this guard was unreachable on-chain. Left unused rather than renumbered — see [Errors](errors.md). |
| 2 | `NotFound` | `get_version`, `latest`, `version_at`, `extend_version_ttl` | No such version, or the operator has never published. |
| 3 | `EffectiveInPast` | `publish` | `effective_ledger` is before the current ledger. |
| 4 | `EffectiveNotAfter` | `publish` | `effective_ledger` doesn't strictly exceed the previous version's. |
| 5 | `TimelineFull` | `publish` | `TIMELINE_CAP` (256 entries) reached. |
| 6 | `UriTooLong` | `publish` | `uri` exceeds 200 bytes. |

## Events

| Event | Topics | Data (positional) |
|---|---|---|
| `publish` | `("price_book", "publish")` | `(operator: Address, version: u32, schedule_hash: BytesN<32>, effective_ledger: u32)` |
