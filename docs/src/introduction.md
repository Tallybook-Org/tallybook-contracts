# Introduction

Tallybook is settlement bookkeeping for services that charge per HTTP request: APIs
paid by AI agents and other machines over Stellar. It lets a buyer take one line off an
invoice and check it against the chain, instead of trusting the seller's own records.

Tallybook is not a payment facilitator. It is not a router. It is not a payment SDK. It
holds no funds, moves no tokens, and takes no fee. Metering — counting individual
requests — stays off-chain: at $0.01 a request, an on-chain write for every request
would cost more than the request earns.

Neither contract in this repository has been audited. See
[SECURITY.md](https://github.com/Tallybook-Org/tallybook-contracts/blob/main/SECURITY.md)
before using them for anything beyond testnet.

The system has two on-chain contracts and one off-chain layer:

- **`price_book`** — an append-only, versioned record of what an operator charges and
  from when.
- **`statement_registry`** — anchors each billing period's statement against a merkle
  root of usage records, checks it against the price book, and lets a buyer dispute one
  publicly.
- **An off-chain collector, settler, and indexer** — meters requests, submits
  payment-channel commitments before they expire, and reconciles on-chain transfers
  against anchored statements. Implemented in a separate repository,
  [`Tallybook-Org/tallybook`](https://github.com/Tallybook-Org/tallybook), which
  consumes these two contracts' deployed interfaces and vendors none of their code.
  Not part of this repository, and not audited either.

Continue to [The problem](problem.md) for why a settlement layer needs this on top of
it.
