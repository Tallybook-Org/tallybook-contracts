# Errors

Both contracts declare one `#[contracterror]` enum each, `#[repr(u32)]`, with
explicit discriminants that are never renumbered once committed. Every fallible
public function returns `Result<T, Error>` — neither contract panics to signal a
business error.

## Discriminant 1 is deliberately unused, in both contracts

Both enums start at `2`. Discriminant `1` was originally `AlreadyInitialized`,
guarding each contract's constructor against being called a second time. Per
[CAP-0058](https://github.com/stellar/stellar-protocol/blob/master/core/cap-0058.md),
the Soroban host invokes a contract's constructor exactly once, at creation, and
never again — there is no code path that can call `__constructor` twice, so that
guard checked for a condition that could never occur. It was removed rather than
kept as dead code, and rather than renumbering every error after it — discriminants
are a wire contract another repository parses positionally, and this document's own
rule is that they're never renumbered once committed. `1` stays permanently unused in
both enums as a record of that decision.

## `price_book`

| Discriminant | Variant | Raised by | Meaning |
|---|---|---|---|
| 1 | *(unused)* | — | See above. |
| 2 | `NotFound` | `get_version`, `latest`, `version_at`, `extend_version_ttl` | No such version, or the operator has never published. |
| 3 | `EffectiveInPast` | `publish` | `effective_ledger` is before the current ledger. |
| 4 | `EffectiveNotAfter` | `publish` | `effective_ledger` doesn't strictly exceed the previous version's `effective_ledger`. |
| 5 | `TimelineFull` | `publish` | `TIMELINE_CAP` (256 entries) reached for this operator. |
| 6 | `UriTooLong` | `publish` | `uri` exceeds 200 bytes. |

## `statement_registry`

| Discriminant | Variant | Raised by | Meaning |
|---|---|---|---|
| 1 | *(unused)* | — | See above. |
| 2 | `NotFound` | `get_statement`, `get_dispute`, `open_dispute`, `resolve_dispute`, `verify_usage`, `verify_charge`, `extend_statement_ttl` | No such statement or dispute exists, or (for `open_dispute`) a real statement exists but the caller isn't its `consumer`. |
| 3 | `BadPeriod` | `anchor` | `period_start >= period_end`, or `period_end` is after the current ledger. |
| 4 | `BadAmounts` | `anchor` | A negative `amount_billed`/`amount_settled`, or `amount_settled > amount_billed`. |
| 5 | `EmptyStatement` | `anchor` | `request_count == 0`. |
| 6 | `PriceVersionUnknown` | `anchor` | `price_book` has no such version for this operator, or the cross-contract call itself failed. |
| 7 | `PriceVersionStale` | `anchor` | `version_at(period_start)` and `version_at(period_end)` agree with each other, but not with the claimed `price_book_version`. |
| 8 | `ChannelMismatch` | `anchor` | `channel` is set without `protocol == MppSession`, or absent with it. |
| 9 | `IndexFull` | `anchor` | `CONSUMER_IDX_CAP` (500 entries) reached for this `(operator, consumer)` pair. |
| 10 | `NotAnchored` | `open_dispute` | The statement isn't currently `Anchored`. |
| 11 | `NotDisputed` | `resolve_dispute` | The statement isn't currently `Disputed`. |
| 12 | `CreditTooLarge` | `resolve_dispute` | `amount_credited` is negative or exceeds the statement's `amount_billed`. |
| 13 | `ProofTooLong` | `verify_usage`, `verify_charge` | The proof has more than `MAX_PROOF_NODES` (32) entries. |
| 14 | `PeriodSpansPriceChange` | `anchor` | `version_at(period_start) != version_at(period_end)` — the schedule changed partway through the period, so no single `price_book_version` honestly covers the whole statement. Distinct from `PriceVersionStale`, which is about the claimed version not matching what was in force; this is about the period itself straddling a change, independent of what was claimed. See [Pricing](../protocol/pricing.md). |
| 15 | `RecordConsumerMismatch` | `verify_charge` | The record's `consumer` isn't the statement's, so the record was never part of this bill whatever the proof shows. |
| 16 | `RecordPriceMismatch` | `verify_charge` | The record's `price_v` isn't the statement's `price_book_version` — the line was priced against a schedule the statement doesn't claim to have been billed under. |
| 17 | `RecordOutsidePeriod` | `verify_charge` | The record's `ledger` falls outside `[period_start, period_end]`, so it belongs to a different billing period. |

`list_statements` returns `Result<Vec<u64>, Error>` for interface consistency with
the rest of the contract, but it can't actually fail — an unknown `(operator,
consumer)` pair just returns an empty vector, which isn't an error.
