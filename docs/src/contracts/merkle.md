# Merkle verification

`statement_registry` checks one usage record against an anchored `usage_root` by
folding a leaf up through a proof. Two calls do it:
`verify_charge` takes the record's fields and derives the leaf itself, and
`verify_usage` takes a leaf already computed and only folds it. A buyer checking an
invoice line wants `verify_charge`; `verify_usage` exists for callers that already
hold a leaf and want nothing but the membership answer.

This page is precise enough to implement a compatible tree builder in another
language — that's its job, since the off-chain collector that builds these trees does
exactly that, in Go.

## Leaf format

The format is implemented twice: here, in
[`contracts/statement-registry/src/leaf.rs`](https://github.com/Tallybook-Org/tallybook-contracts/blob/main/contracts/statement-registry/src/leaf.rs),
and in the off-chain collector's `internal/merkle/leaf.go` in the
[`tallybook`](https://github.com/Tallybook-Org/tallybook) repo. The two must agree
byte for byte or every proof the collector produces is invalid on chain, which is
what the fixtures at the bottom of this page exist to prevent.

```
record_bytes = XDR(ScVal::Map{           // keys sorted alphabetically
    Symbol("amount"):   I128(charged_amount),
    Symbol("consumer"): Address(consumer),
    Symbol("endpoint"): BytesN<32>(sha256(method || " " || path_template)),
    Symbol("ledger"):   U32(settlement_or_observation_ledger),
    Symbol("price_v"):  U32(price_book_version),
    Symbol("reqid"):    BytesN<32>(request_id),
    Symbol("units"):    U64(unit_count),
})
leaf = sha256(sha256(record_bytes))
```

`ScVal::Map` keys are sorted alphabetically — `amount`, `consumer`, `endpoint`,
`ledger`, `price_v`, `reqid`, `units` — which matches the convention
`stellar-experimental/one-way-channel` uses for its own commitments; that consistency
is deliberate, not incidental. `endpoint` is a hash rather than the raw method and
path so leaves stay a fixed size regardless of route length, and so the path template
itself stays off-chain.

On the contract side none of that is hand-rolled. `UsageRecord` is a
`#[contracttype]` struct whose fields are named and typed exactly as above, and the
macro encodes a named-field struct as precisely that `ScVal::Map`, keyed by field
names as symbols, sorted alphabetically. The type *is* the format — there is no
separate encoder to drift from it.

### Why the hash is applied twice

Sorted-pair folding gives internal nodes no domain tag of their own, so leaves and
internal nodes have to be separated some other way. Double-hashing does it by
preimage length: an internal node is `sha256` of 64 bytes (two concatenated
children), while a leaf is `sha256` of exactly 32 bytes (the first hash of the
record). No record, whatever its length, produces a leaf whose preimage is a
concatenated pair.

Without that, a single-hashed 64-byte record would be indistinguishable from an
internal node, and an internal node could be presented as a leaf and verified against
the path above it. `verify_charge` removes the question entirely for its callers,
since it derives the leaf from a record rather than accepting one.

## Sorted-pair folding

```rust
pub fn fold(env: &Env, leaf: BytesN<32>, proof: &Vec<BytesN<32>>) -> BytesN<32> {
    let mut node = leaf;
    for sibling in proof.iter() {
        let (lo, hi) = if node <= sibling { (node, sibling) } else { (sibling, node) };
        let mut concatenated = Bytes::from(lo);
        concatenated.append(&Bytes::from(hi));
        node = env.crypto().sha256(&concatenated).to_bytes();
    }
    node
}
```

At each step, the current node and the next proof entry are ordered by byte
comparison before concatenating — `sha256(min(a, b) || max(a, b))` — rather than
concatenated in a fixed left/right order. This is what makes a leaf index
unnecessary: a verifier doesn't need to know whether its leaf was originally the left
or right child at each level, because the fold puts the smaller value first
regardless. There's no leaf-index parameter anywhere in this contract's interface, so
an off-by-one in that index — a whole class of bug in index-based merkle proofs — is
structurally impossible here.

## Worked verification, from a committed fixture

[`fixtures/merkle/four-leaves.json`](https://github.com/Tallybook-Org/tallybook-contracts/blob/main/fixtures/merkle/four-leaves.json)
commits four 32-byte leaves and their root:

```
leaf[0] = 0101010101010101010101010101010101010101010101010101010101010101
leaf[1] = 0202020202020202020202020202020202020202020202020202020202020202
leaf[2] = 0303030303030303030303030303030303030303030303030303030303030303
leaf[3] = 0404040404040404040404040404040404040404040404040404040404040404
root    = 1d0cafe12ca55e5e8d0903a1847cffae908539b86fee1c52418b2cd453479e7c
```

The tree is balanced: two pairs folded once each, then those two results folded
together.

```
n01 = sha256(leaf[0] || leaf[1])   -- leaf[0] < leaf[1], no swap
    = f818afd37a6dc3bc92fb44731011277006db4efa6e9023cd7468c02335d22a4d
n23 = sha256(leaf[2] || leaf[3])   -- leaf[2] < leaf[3], no swap
    = 505a9c6ac70bdffa46248e2025483f9fe997a0e31ed25559e448b73b7e02b9bd
root = sha256(n01 || n23)          -- n01 < n23, no swap
     = 1d0cafe12ca55e5e8d0903a1847cffae908539b86fee1c52418b2cd453479e7c
```

The fixture's committed proof for `leaf[0]` is `[leaf[1], n23]` — which checks out:
folding `leaf[0]` with `leaf[1]` first reproduces `n01`, and folding that with `n23`
reproduces `root`, exactly as computed above. All four leaves' committed proofs were
independently re-derived and matched against the fixture before this page was
committed.

**Negative case:** flip one bit in `leaf[1]` before folding `leaf[0]`'s proof against
it, and the intermediate result no longer equals `n01`, so the final fold doesn't
equal `root`. `verify_usage` returns `Ok(false)` for this — a corrupted or wrong
sibling is a well-formed 32-byte value, not malformed input, so it's not an error.

## Fixtures

`fixtures/merkle/` is the contract test between the two implementations. Both repos
commit the same files and test against them, and the two halves of the format are
covered by different files.

**Tree shape** — `single-leaf.json` (a degenerate one-node tree, empty proof),
`four-leaves.json` (above), and `seven-leaves.json`, an unbalanced tree, deliberately
included because unbalanced trees are where sorted-pair implementations most often
diverge from each other. These pin how `fold` combines nodes. They start from leaves
that are already hashes, so they say nothing about how a request becomes a leaf.

**Leaf derivation** — `records.json` pins that other half: six records, each with its
`record_bytes` and resulting leaf. Carrying `record_bytes` as well as the leaf makes a
mismatch localise itself — `record_bytes` differing means the encoding drifted, only
the leaf differing means the hashing did. The six are chosen for where two
implementations actually diverge rather than for coverage of the happy path:

| Vector | What it catches |
|---|---|
| minimal | The floor case: one unit, one stroop. |
| typical x402 request at 0.01 USDC | The shape nearly every real record has. |
| credit line, negative amount | An `i128` two's-complement bug, the likeliest silent divergence. |
| MPP session, channel contract consumer | An encoder that only handles account (`G…`) strkeys and not a channel contract (`C…`). |
| `i128` max and field ceilings | The Hi/Lo split of an `i128`, plus the `u32` and `u64` ceilings. |
| `i128` min and field floors | The other end of the same range. |

`records.json` is generated from this repo's implementation rather than written by
hand:

```sh
TALLYBOOK_WRITE_FIXTURES=1 cargo test -p statement-registry record_vectors
```

Without that environment variable the same test only compares, so the committed file
can't drift from the code that produced it without CI failing.
