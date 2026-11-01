use soroban_sdk::contracterror;

/// Discriminants are explicit and never renumbered once committed — an
/// indexer in another repo matches on these numbers.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    // 1 is deliberately unused. It was AlreadyInitialized, guarding
    // __constructor against a second call — but per CAP-0058, a contract's
    // constructor is only ever invoked once at creation and is never
    // callable again, so that error was unreachable on-chain. Removed
    // rather than renumbered, per the no-renumbering rule above. Same
    // reasoning and same fix as price_book's own Error enum.
    NotFound = 2,
    /// `period_start >= period_end`, or `period_end > current ledger`.
    BadPeriod = 3,
    /// A negative amount, or `amount_settled > amount_billed`.
    BadAmounts = 4,
    /// `request_count == 0`.
    EmptyStatement = 5,
    /// `price_book` has no such version for this operator.
    PriceVersionUnknown = 6,
    /// The period sits inside one version's window, but that version is
    /// not the `price_book_version` the caller claimed.
    PriceVersionStale = 7,
    /// `channel` set without `MppSession`, or absent with it.
    ChannelMismatch = 8,
    /// `CONSUMER_IDX_CAP` reached.
    IndexFull = 9,
    /// A dispute was opened on a statement that is not `Anchored`.
    NotAnchored = 10,
    /// `resolve_dispute` called on a statement that is not `Disputed`.
    NotDisputed = 11,
    /// `amount_credited` is negative, or exceeds `amount_billed`.
    CreditTooLarge = 12,
    /// A merkle proof longer than `MAX_PROOF_NODES`.
    ProofTooLong = 13,
    /// `version_at(period_start) != version_at(period_end)`: the schedule
    /// changed partway through the period, so no single
    /// `price_book_version` honestly covers the whole thing. Distinct from
    /// `PriceVersionStale`, which is about the claimed version not
    /// matching what was in force — this is about the period itself
    /// straddling a change, independent of what was claimed.
    PeriodSpansPriceChange = 14,
}
