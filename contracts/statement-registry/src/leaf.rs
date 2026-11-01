use soroban_sdk::{contracttype, xdr::ToXdr, Address, Bytes, BytesN, Env};

/// One billed request, in the exact shape a usage tree leaf is derived
/// from.
///
/// The field list is the leaf format itself, not a convenience wrapper
/// around it. `#[contracttype]` encodes a named-field struct as an
/// `ScVal::Map` keyed by the field names as symbols, sorted alphabetically
/// — which is byte for byte the `record_bytes` the off-chain collector
/// builds before hashing. Fields are therefore declared in alphabetical
/// order deliberately: the macro sorts them regardless, so the order here
/// changes nothing about the encoding, but reading the struct in map order
/// is how a reviewer checks it against the collector's own encoder in the
/// `tallybook` repo (`internal/merkle/leaf.go`).
///
/// Renaming a field, changing a field's type, or adding one changes every
/// leaf this contract derives and invalidates every proof the collector
/// has already produced. Treat the field list as frozen.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct UsageRecord {
    /// Charged amount, in the statement token's smallest unit.
    pub amount: i128,
    /// The payer this request was billed to: an account (`G…`) for x402
    /// and MPP charge mode, or the channel contract (`C…`) for session
    /// mode.
    pub consumer: Address,
    /// `sha256(method || " " || path_template)` — the endpoint billed,
    /// committed as a hash so the path template itself stays off-chain.
    pub endpoint: BytesN<32>,
    /// The settlement ledger for this request, or the ledger it was
    /// observed at where settlement is deferred.
    pub ledger: u32,
    /// The price book version the amount was priced against.
    pub price_v: u32,
    /// The collector's request id for this request.
    pub reqid: BytesN<32>,
    /// Billed units — requests, tokens, or whatever the schedule prices.
    pub units: u64,
}

/// Derives `record`'s usage tree leaf: `sha256(sha256(record_bytes))`.
///
/// The hash is applied twice on purpose. A single hash would make a leaf
/// and an internal node of the tree indistinguishable, because sorted-pair
/// folding gives internal nodes no domain tag of their own — so an internal
/// node could be presented as a leaf and verified against a partial path.
/// Hashing the record twice puts every leaf one hash further from the
/// folding domain, and nothing an attacker can construct from tree nodes
/// lands on a double-hashed preimage.
pub fn leaf_hash(env: &Env, record: &UsageRecord) -> BytesN<32> {
    let record_bytes = record.clone().to_xdr(env);
    let inner = env.crypto().sha256(&record_bytes).to_bytes();
    env.crypto().sha256(&Bytes::from(inner)).to_bytes()
}
