//! `statement-registry`: anchors a billing period's statement so a buyer
//! can verify one charge against it, check it against the price then in
//! force, and contest it publicly if it is wrong. See the workspace README
//! for the full interface.
#![no_std]

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, Vec};

mod error;
mod event;
mod leaf;
mod merkle;
mod price_book;
mod storage;
mod test;
mod types;

use error::Error;
use event::{AnchorEvent, DisputeEvent, ResolveEvent};
use leaf::UsageRecord;
use types::{Dispute, Protocol, Statement, Status};

#[contract]
pub struct StatementRegistry;

#[contractimpl]
impl StatementRegistry {
    /// Deploy-time only. Stores `admin` and `price_book` in instance
    /// storage and extends the instance TTL.
    ///
    /// `admin` holds no power over operator data. `price_book` is
    /// immutable after construction — there is no setter. A mutable price
    /// book address would let an operator swap in a permissive registry
    /// and invalidate every historical statement; if the price book must
    /// change, a new `statement_registry` is deployed.
    ///
    /// Per CAP-0058, the host invokes a contract's constructor exactly
    /// once, at creation, and it is never callable again — so there is no
    /// double-initialization case to guard against here.
    pub fn __constructor(env: Env, admin: Address, price_book: Address) {
        storage::set_admin(&env, &admin);
        storage::set_price_book(&env, &price_book);
        storage::extend_instance_ttl(&env);
    }

    /// Anchors a billing period's statement. Callable only by `operator`.
    ///
    /// Validates, in order: the period is non-empty and already finished
    /// (`period_start < period_end <= current ledger`); `request_count` is
    /// non-zero; amounts are non-negative and `amount_settled <=
    /// amount_billed`; `channel` is present iff `protocol == MppSession`;
    /// and — the most important checks in this contract — that the price
    /// book agrees on one version for the whole period, and that
    /// `price_book_version` is that version.
    ///
    /// The price check is two live calls to the price_book contract, not
    /// one: `version_at(period_start)` and `version_at(period_end)` must
    /// agree with each other before either is compared against
    /// `price_book_version`. If the schedule changed partway through the
    /// period, the two calls disagree and the period is rejected outright
    /// — no single version honestly covers a statement whose window
    /// straddles a price change, no matter what the caller claims. Only
    /// once the period is confirmed to sit inside one version's window is
    /// that version compared against `price_book_version`, which is what
    /// stops an operator anchoring against a favourable old (or claiming a
    /// not-yet-effective new) schedule.
    ///
    /// Returns the new sequence number for `operator` (the first anchor is
    /// 1).
    ///
    /// Errors: `BadPeriod`, `EmptyStatement`, `BadAmounts`,
    /// `ChannelMismatch`, `PriceVersionUnknown`, `PeriodSpansPriceChange`,
    /// `PriceVersionStale`, `IndexFull`.
    // Argument count is fixed by the spec this contract is built against;
    // splitting it into a struct would change the public function
    // signature, which this repo has no authority to do.
    #[allow(clippy::too_many_arguments)]
    pub fn anchor(
        env: Env,
        operator: Address,
        consumer: Address,
        period_start: u32,
        period_end: u32,
        usage_root: BytesN<32>,
        request_count: u64,
        token: Address,
        amount_billed: i128,
        amount_settled: i128,
        price_book_version: u32,
        protocol: Protocol,
        channel: Option<Address>,
    ) -> Result<u64, Error> {
        operator.require_auth();

        let current_ledger = env.ledger().sequence();
        if period_start >= period_end || period_end > current_ledger {
            return Err(Error::BadPeriod);
        }
        if request_count == 0 {
            return Err(Error::EmptyStatement);
        }
        if amount_billed < 0 || amount_settled < 0 || amount_settled > amount_billed {
            return Err(Error::BadAmounts);
        }
        let channel_expected = matches!(protocol, Protocol::MppSession);
        if channel.is_some() != channel_expected {
            return Err(Error::ChannelMismatch);
        }

        // get_price_book() is None only for an unconstructed instance,
        // which cannot be executing this call — the host never invokes a
        // non-constructor function before construction. Kept as an error
        // rather than an unwrap because the invariant is enforced
        // elsewhere, not by the type system.
        let price_book_id = storage::get_price_book(&env).ok_or(Error::PriceVersionUnknown)?;
        let price_book_client = price_book::Client::new(&env, &price_book_id);
        let version_at_start = price_book_client
            .try_version_at(&operator, &period_start)
            .map_err(|_| Error::PriceVersionUnknown)?
            .map_err(|_| Error::PriceVersionUnknown)?;
        let version_at_end = price_book_client
            .try_version_at(&operator, &period_end)
            .map_err(|_| Error::PriceVersionUnknown)?
            .map_err(|_| Error::PriceVersionUnknown)?;
        if version_at_start != version_at_end {
            return Err(Error::PeriodSpansPriceChange);
        }
        if version_at_end != price_book_version {
            return Err(Error::PriceVersionStale);
        }

        let mut consumer_idx = storage::get_consumer_idx(&env, &operator, &consumer);
        if consumer_idx.len() >= storage::CONSUMER_IDX_CAP {
            return Err(Error::IndexFull);
        }

        // Unchecked +1 is deliberate: Seq(operator) has no cap in this
        // contract's error surface (unlike ConsumerIdx), and reaching
        // u64::MAX would require on the order of 10^19 anchor() calls —
        // not a case worth inventing an unspecified error variant for.
        let seq = storage::get_seq(&env, &operator).unwrap_or(0) + 1;

        let statement = Statement {
            operator: operator.clone(),
            consumer: consumer.clone(),
            period_start,
            period_end,
            usage_root: usage_root.clone(),
            request_count,
            token,
            amount_billed,
            amount_settled,
            price_book_version,
            protocol,
            channel,
            anchored_ledger: current_ledger,
            status: Status::Anchored,
        };
        storage::set_statement(&env, &operator, seq, &statement);
        storage::set_seq(&env, &operator, seq);
        consumer_idx.push_back(seq);
        storage::set_consumer_idx(&env, &operator, &consumer, &consumer_idx);
        storage::extend_instance_ttl(&env);

        AnchorEvent {
            operator,
            consumer,
            seq,
            usage_root,
            amount_billed,
            amount_settled,
            protocol,
        }
        .publish(&env);

        Ok(seq)
    }

    /// Verifies that `leaf` was part of the merkle tree anchored as
    /// `usage_root` for (`operator`, `seq`), given `proof`. No auth —
    /// anyone can verify a charge against a statement, and that is the
    /// whole point.
    ///
    /// The leaf format is fixed by the off-chain collector and is not
    /// computed here; this only folds whatever leaf it is given.
    ///
    /// Returns `Ok(false)` — not an error — for a valid-shaped proof that
    /// does not reach `usage_root`. Errors are reserved for malformed
    /// input.
    ///
    /// Errors: `ProofTooLong` if `proof` has more than `MAX_PROOF_NODES`
    /// entries. `NotFound` if no such statement exists.
    pub fn verify_usage(
        env: Env,
        operator: Address,
        seq: u64,
        leaf: BytesN<32>,
        proof: Vec<BytesN<32>>,
    ) -> Result<bool, Error> {
        if proof.len() > storage::MAX_PROOF_NODES {
            return Err(Error::ProofTooLong);
        }
        let statement = storage::get_statement(&env, &operator, seq).ok_or(Error::NotFound)?;
        let root = merkle::fold(&env, leaf, &proof);
        Ok(root == statement.usage_root)
    }

    /// Verifies a single billed request against the statement anchored at
    /// (`operator`, `seq`), deriving the leaf from `record` itself rather
    /// than taking the collector's word for it. No auth — anyone can check
    /// a charge, and that is the whole point.
    ///
    /// This is the function an invoice line is checked with.
    /// `verify_usage` answers a narrower question: it takes an opaque
    /// 32-byte leaf and only says whether that leaf is in the tree, which
    /// leaves the caller trusting whoever computed the leaf — including on
    /// how the amount and the price version were encoded into it. Here the
    /// contract computes the leaf from the record's own fields and then
    /// binds the record to the statement it is claimed to belong to:
    ///
    /// - `record.consumer` must be the statement's `consumer`, so a line
    ///   billed to someone else cannot be passed off as part of this bill;
    /// - `record.price_v` must be the statement's `price_book_version`,
    ///   which is the version `anchor()` already proved was the one in
    ///   force across the whole period — so a line priced against any
    ///   other schedule is rejected here rather than silently accepted;
    /// - `record.ledger` must fall inside `[period_start, period_end]`, so
    ///   a request from a different period cannot be counted in this one.
    ///
    /// Together with `anchor()`'s price check, that is the full chain a
    /// buyer needs: this request, at this price version, inside this
    /// period, under a schedule the price book agrees was in force then.
    ///
    /// Returns `Ok(false)` — not an error — when the record is consistent
    /// with the statement but its proof does not reach `usage_root`: that
    /// is a well-formed claim that simply is not in the tree. Errors are
    /// reserved for input that contradicts the statement outright, where
    /// `false` would under-report what is wrong.
    ///
    /// Errors: `ProofTooLong`, `NotFound`, `RecordConsumerMismatch`,
    /// `RecordPriceMismatch`, `RecordOutsidePeriod`.
    pub fn verify_charge(
        env: Env,
        operator: Address,
        seq: u64,
        record: UsageRecord,
        proof: Vec<BytesN<32>>,
    ) -> Result<bool, Error> {
        if proof.len() > storage::MAX_PROOF_NODES {
            return Err(Error::ProofTooLong);
        }
        let statement = storage::get_statement(&env, &operator, seq).ok_or(Error::NotFound)?;

        if record.consumer != statement.consumer {
            return Err(Error::RecordConsumerMismatch);
        }
        if record.price_v != statement.price_book_version {
            return Err(Error::RecordPriceMismatch);
        }
        if record.ledger < statement.period_start || record.ledger > statement.period_end {
            return Err(Error::RecordOutsidePeriod);
        }

        let leaf = leaf::leaf_hash(&env, &record);
        let root = merkle::fold(&env, leaf, &proof);
        Ok(root == statement.usage_root)
    }

    /// Opens a public dispute against (`operator`, `seq`). Callable only by
    /// the statement's `consumer`.
    ///
    /// `consumer` must match the statement's own `consumer` field — a
    /// mismatch returns `NotFound`, the same error as a missing statement,
    /// so a party unrelated to the statement cannot learn whether it
    /// exists.
    ///
    /// Errors: `NotFound`, `NotAnchored` if the statement is not currently
    /// `Anchored`.
    pub fn open_dispute(
        env: Env,
        operator: Address,
        seq: u64,
        consumer: Address,
        reason_hash: BytesN<32>,
    ) -> Result<(), Error> {
        consumer.require_auth();

        let mut statement = storage::get_statement(&env, &operator, seq).ok_or(Error::NotFound)?;
        if statement.consumer != consumer {
            return Err(Error::NotFound);
        }
        if statement.status != Status::Anchored {
            return Err(Error::NotAnchored);
        }

        statement.status = Status::Disputed;
        storage::set_statement(&env, &operator, seq, &statement);

        let dispute = Dispute {
            consumer: consumer.clone(),
            reason_hash: reason_hash.clone(),
            opened_ledger: env.ledger().sequence(),
            resolution_hash: None,
            amount_credited: 0,
            resolved_ledger: None,
        };
        storage::set_dispute(&env, &operator, seq, &dispute);
        storage::extend_instance_ttl(&env);

        DisputeEvent { operator, consumer, seq, reason_hash }.publish(&env);

        Ok(())
    }

    /// Resolves the dispute against (`operator`, `seq`). Requires **both**
    /// parties' auth — `operator` first, then the statement's `consumer` —
    /// per the two-signer design: there is deliberately no arbiter, no
    /// admin override, and no timeout that auto-resolves in the operator's
    /// favour. If the two sides never agree, the statement stays publicly
    /// `Disputed` forever, and that public mark is the entire enforcement
    /// mechanism.
    ///
    /// Errors: `NotFound`, `NotDisputed` if the statement is not currently
    /// `Disputed`, `CreditTooLarge` if `amount_credited` is negative or
    /// exceeds the statement's `amount_billed`.
    pub fn resolve_dispute(
        env: Env,
        operator: Address,
        seq: u64,
        resolution_hash: BytesN<32>,
        amount_credited: i128,
    ) -> Result<(), Error> {
        operator.require_auth();

        let mut statement = storage::get_statement(&env, &operator, seq).ok_or(Error::NotFound)?;
        statement.consumer.require_auth();

        if statement.status != Status::Disputed {
            return Err(Error::NotDisputed);
        }
        if amount_credited < 0 || amount_credited > statement.amount_billed {
            return Err(Error::CreditTooLarge);
        }

        statement.status = Status::Resolved;
        storage::set_statement(&env, &operator, seq, &statement);

        // A Dispute always exists alongside a Disputed statement —
        // open_dispute() writes both together and nothing else changes
        // status to Disputed — but this is a storage read, not a type-level
        // guarantee, so it stays an error rather than an unwrap.
        let mut dispute = storage::get_dispute(&env, &operator, seq).ok_or(Error::NotFound)?;
        dispute.resolution_hash = Some(resolution_hash.clone());
        dispute.amount_credited = amount_credited;
        dispute.resolved_ledger = Some(env.ledger().sequence());
        storage::set_dispute(&env, &operator, seq, &dispute);
        storage::extend_instance_ttl(&env);

        ResolveEvent {
            operator,
            consumer: statement.consumer.clone(),
            seq,
            resolution_hash,
            amount_credited,
        }
        .publish(&env);

        Ok(())
    }

    /// Returns the anchored statement at (`operator`, `seq`). No auth —
    /// read-only.
    ///
    /// Errors: `NotFound` if no such statement exists.
    pub fn get_statement(env: Env, operator: Address, seq: u64) -> Result<Statement, Error> {
        storage::get_statement(&env, &operator, seq).ok_or(Error::NotFound)
    }

    /// Returns the sequence numbers anchored for (`operator`, `consumer`),
    /// oldest first, newest last. No auth — read-only. An empty result
    /// means no statements exist for the pair; that is not an error.
    pub fn list_statements(
        env: Env,
        operator: Address,
        consumer: Address,
    ) -> Result<Vec<u64>, Error> {
        Ok(storage::get_consumer_idx(&env, &operator, &consumer))
    }

    /// Returns the dispute against (`operator`, `seq`). No auth —
    /// read-only.
    ///
    /// Errors: `NotFound` if no dispute has been opened.
    pub fn get_dispute(env: Env, operator: Address, seq: u64) -> Result<Dispute, Error> {
        storage::get_dispute(&env, &operator, seq).ok_or(Error::NotFound)
    }

    /// Extends the TTL of the statement at (`operator`, `seq`) — and its
    /// dispute, if one exists — out to `ledgers` from now, clamped to a
    /// sane maximum.
    ///
    /// No auth, intentionally: anyone may pay rent to keep an audit record
    /// alive. An auditor checking a three-year-old statement is not a
    /// party to it and was never going to be able to authenticate as one —
    /// requiring auth here would just make old records unrecoverable once
    /// their original parties are unreachable. Do not "fix" this by adding
    /// auth.
    ///
    /// Errors: `NotFound` if no such statement exists.
    pub fn extend_statement_ttl(
        env: Env,
        operator: Address,
        seq: u64,
        ledgers: u32,
    ) -> Result<(), Error> {
        storage::get_statement(&env, &operator, seq).ok_or(Error::NotFound)?;

        storage::extend_statement_ttl_to(&env, &operator, seq, ledgers);
        storage::extend_dispute_ttl_to(&env, &operator, seq, ledgers);

        Ok(())
    }
}
