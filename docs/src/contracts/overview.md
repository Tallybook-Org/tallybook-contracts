# Overview

## Dependency graph

`price_book` has no dependencies on the other contract. `statement_registry` depends
on `price_book`: `anchor()` makes two live cross-contract calls to
`price_book.version_at()` to check the period against the schedule in force. That
dependency is also a compile-time one — `statement_registry/src/price_book.rs` uses
`soroban_sdk::contractimport!` to pull in `price_book`'s compiled wasm, so
`price_book` has to build first. `price_book`'s own address is passed into
`statement_registry`'s constructor once, at deployment, and is immutable after that —
there is no setter, so a `statement_registry` instance can never be pointed at a
different price book after it goes live.

```text
price_book  ◀──────────────  statement_registry
 (no deps)      version_at()      (anchor)
```

## Multi-tenant by operator address

Neither contract has a notion of "the" operator. Every storage key that holds
operator data is keyed by the operator's `Address` — `Latest(Address)`,
`Version(Address, u32)`, and `Timeline(Address)` in `price_book`; `Seq(Address)`,
`Statement(Address, u64)`, `ConsumerIdx(Address, Address)`, and `Dispute(Address, u64)`
in `statement_registry`. One deployed instance of each contract serves every operator
who ever calls `publish()` or `anchor()` against it; each operator's history is fully
isolated from every other operator's by the address in the key. There's no
registration step and no allowlist — the first `publish()` call for a new operator
address is what creates that operator's history.

## What stays off-chain, and why

Metering — counting individual requests and pricing them — never touches either
contract. At $0.01 a request, that's a deliberate cost tradeoff, not a convenience
one: [the worked example's real measured fee](../protocol/worked-example.md#the-cost-of-anchoring)
for a single Soroban contract call on testnet was about $0.0362. Writing that once per
billing period, covering however many requests the period contains, is negligible
against revenue. Writing it once per request would have cost about $3,616 to bill
$1,000 of that same example month's usage — more than the revenue it would have
recorded. Everything that would require a per-request on-chain write — metering
itself, and the off-chain custody and sweeping of payment-channel commitments — stays
off-chain by design; see [How it works](../how-it-works.md) for which step runs
where.

## Deployed contracts

| Contract | Address | Wasm hash | Explorer |
|---|---|---|---|
| `price_book` | `CC6VXCEIXGGFIZXH2DGPVPUXYVVI3YVEQQ5M7XAWNEN7SADIYACTRFZ6` | `d03c8c9b193810b7bc0755d8dc8e517d4b1279cac29f4e1ce0e43d786ec08188` | [stellar.expert](https://stellar.expert/explorer/testnet/contract/CC6VXCEIXGGFIZXH2DGPVPUXYVVI3YVEQQ5M7XAWNEN7SADIYACTRFZ6) |
| `statement_registry` | `CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3` | `c309069607e59b48c7cf65ee4f353a8cefcfe361b73a6c3ae74eb984733defd9` | [stellar.expert](https://stellar.expert/explorer/testnet/contract/CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3) |

These contracts have no upgrade path by design, so a new build is a new deployment at
a new address rather than an in-place upgrade. The `v0.1.0` addresses are still on
chain and simply no longer current.

Both are testnet only, and neither has been audited — see
[SECURITY.md](https://github.com/Tallybook-Org/tallybook-contracts/blob/main/SECURITY.md).
The following pages document each contract's full interface, read from its source:
[`price_book`](price-book.md), [`statement_registry`](statement-registry.md),
[merkle verification](merkle.md), and [errors](errors.md).
