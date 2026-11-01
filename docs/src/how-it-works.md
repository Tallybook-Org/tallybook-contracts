# How it works

The full Tallybook system has five steps. Two of them are the contracts in this
repository. The other three are off-chain and live in a separate repository,
[`Tallybook-Org/tallybook`](https://github.com/Tallybook-Org/tallybook) — nothing in
this repository performs them, and this page says which is which rather than
describing one system that runs end to end.

| Step | What happens | Where |
|---|---|---|
| 1. Meter | An off-chain collector counts requests and prices them against the operator's published schedule. | `tallybook`, `internal/meter` + `internal/httpapi` |
| 2. Custody commitments | For MPP session mode, an off-chain custodian holds the signed channel commitments as they arrive. | `tallybook`, `internal/custody` |
| 3. Sweep before the refund window | The custodian submits the latest commitment to the channel contract's `settle` before the funder's refund window closes. | `tallybook`, `internal/settle`, calling `stellar-experimental/one-way-channel` |
| 4. Anchor statements | When a billing period closes, the collector merkle-roots the period's usage records and calls `anchor` on `statement_registry`, tied to the exact `price_book` version in force. | `statement_registry.anchor()` |
| 5. Verify | A buyer checks one line item against the anchored root with `verify_charge`. A wrong charge goes to `open_dispute`. | `statement_registry.verify_charge()`, `verify_usage()`, `open_dispute()`, `resolve_dispute()` |

Steps 4 and 5 are the two contracts here, deployed to testnet with a full test suite
behind them — see [Contracts](contracts/overview.md). Steps 1 through 3 are
implemented in `tallybook` as three services sharing one Postgres database, and are
out of scope for this repository by design: see [the roadmap](roadmap.md) for what
each one covers.

Two things that follow from the split, and are easy to read past. Neither repository
is audited. And nothing *this* repository ships mitigates the refund risk described in
[The problem](problem.md) — step 3 is what does that, it runs elsewhere, and an
operator relying on MPP session mode needs to satisfy themselves that it is actually
running for them.
