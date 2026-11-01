//! `price-book`: an append-only, versioned record of what an operator
//! charges and from when. See the workspace README for the full interface.
#![no_std]

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String};

mod error;
mod event;
mod storage;
mod test;
mod types;

use error::Error;
use event::PublishEvent;
use types::{PriceBookVersion, TimelineEntry};

#[contract]
pub struct PriceBook;

#[contractimpl]
impl PriceBook {
    /// Deploy-time only. Stores `admin` in instance storage and extends the
    /// instance TTL.
    ///
    /// `admin` holds no power over operator data — it cannot publish, edit,
    /// or remove a version. It exists only as a documented owner for future
    /// rent funding; there is no admin-gated function anywhere in this
    /// contract.
    ///
    /// Per CAP-0058, the host invokes a contract's constructor exactly once,
    /// at creation, and it is never callable again — so there is no
    /// double-initialization case to guard against here.
    pub fn __constructor(env: Env, admin: Address) {
        storage::set_admin(&env, &admin);
        storage::extend_instance_ttl(&env);
    }

    /// Publishes a new price schedule version for `operator`. Callable only
    /// by `operator`.
    ///
    /// Schedules must move strictly forward in time: `effective_ledger` may
    /// equal the current ledger (effective immediately) but not precede it,
    /// and must strictly exceed the previous version's `effective_ledger` if
    /// one exists. `uri` must be at most 200 bytes.
    ///
    /// Returns the new version number (the first published version is 1).
    ///
    /// Errors: `UriTooLong`, `EffectiveInPast`, `EffectiveNotAfter`,
    /// `TimelineFull`.
    pub fn publish(
        env: Env,
        operator: Address,
        schedule_hash: BytesN<32>,
        uri: String,
        effective_ledger: u32,
    ) -> Result<u32, Error> {
        operator.require_auth();

        if uri.len() > 200 {
            return Err(Error::UriTooLong);
        }

        let current_ledger = env.ledger().sequence();
        if effective_ledger < current_ledger {
            return Err(Error::EffectiveInPast);
        }

        let mut timeline = storage::get_timeline(&env, &operator);
        if let Some(previous) = timeline.last() {
            if effective_ledger <= previous.effective_ledger {
                return Err(Error::EffectiveNotAfter);
            }
        }
        if timeline.len() >= storage::TIMELINE_CAP {
            return Err(Error::TimelineFull);
        }

        // Safe: timeline.len() gains exactly one entry per publish and is
        // bounded above by TIMELINE_CAP (checked just above), so `latest`
        // here is at most TIMELINE_CAP and this addition cannot overflow.
        let version = storage::get_latest(&env, &operator).unwrap_or(0) + 1;
        let published_ledger = current_ledger;

        let price_book_version = PriceBookVersion {
            operator: operator.clone(),
            version,
            schedule_hash: schedule_hash.clone(),
            uri,
            effective_ledger,
            published_ledger,
        };
        storage::set_version(&env, &operator, version, &price_book_version);
        storage::set_latest(&env, &operator, version);
        timeline.push_back(TimelineEntry { effective_ledger, version });
        storage::set_timeline(&env, &operator, &timeline);
        storage::extend_instance_ttl(&env);

        PublishEvent { operator, version, schedule_hash, effective_ledger }.publish(&env);

        Ok(version)
    }

    /// Returns the published `PriceBookVersion` for `operator` at
    /// `version`. No auth — read-only.
    ///
    /// Errors: `NotFound` if no such version exists.
    pub fn get_version(
        env: Env,
        operator: Address,
        version: u32,
    ) -> Result<PriceBookVersion, Error> {
        storage::get_version(&env, &operator, version).ok_or(Error::NotFound)
    }

    /// Returns the latest version number `operator` has published. No
    /// auth — read-only.
    ///
    /// Errors: `NotFound` if `operator` has never published.
    pub fn latest(env: Env, operator: Address) -> Result<u32, Error> {
        storage::get_latest(&env, &operator).ok_or(Error::NotFound)
    }

    /// Extends the TTL of `operator`'s published `version` — and of the
    /// timeline that places it in time — out to `ledgers` from now,
    /// clamped to a sane maximum.
    ///
    /// No auth, intentionally, and for the same reason
    /// `statement_registry::extend_statement_ttl` has none: anyone may pay
    /// rent to keep an audit record alive. Here that reason is load
    /// bearing rather than merely convenient. A statement is only
    /// falsifiable while the price evidence it names is still readable —
    /// `Version` holds the schedule hash and URI the amounts were priced
    /// against, and `Timeline` is what `version_at()` searches to confirm
    /// independently which version was in force on the day. Before this
    /// function existed the only writer that touched either key was
    /// `publish()`, which requires the operator's auth; an operator who
    /// has stopped publishing never calls it again, so the registry's
    /// statements could be kept alive indefinitely by a stranger while the
    /// prices they depend on were left to archive. Do not "fix" this by
    /// adding auth.
    ///
    /// The timeline is extended alongside the version rather than on its
    /// own call: extending one without the other leaves an audit record
    /// that is half readable, which is not a state worth offering.
    ///
    /// Errors: `NotFound` if `operator` has no such published version.
    pub fn extend_version_ttl(
        env: Env,
        operator: Address,
        version: u32,
        ledgers: u32,
    ) -> Result<(), Error> {
        storage::get_version(&env, &operator, version).ok_or(Error::NotFound)?;

        storage::extend_version_ttl_to(&env, &operator, version, ledgers);
        storage::extend_timeline_ttl_to(&env, &operator, ledgers);

        Ok(())
    }

    /// Returns which version of `operator`'s price schedule was in force at
    /// `ledger`: the version with the highest `effective_ledger` not
    /// exceeding `ledger`. No auth — read-only. This is the function
    /// `statement_registry` calls, and the function a buyer calls to check
    /// which prices applied on the day they were billed.
    ///
    /// Binary searches `Timeline(operator)` rather than scanning every
    /// published version, so this stays cheap even near `TIMELINE_CAP`.
    ///
    /// Errors: `NotFound` if the timeline is empty or every entry's
    /// `effective_ledger` is after `ledger`.
    pub fn version_at(env: Env, operator: Address, ledger: u32) -> Result<u32, Error> {
        let timeline = storage::get_timeline(&env, &operator);

        // Upper-bound binary search: after the loop, `count` is the number
        // of entries with effective_ledger <= ledger. Timeline is ascending
        // by effective_ledger, so the entry at count - 1, if any, is the
        // one with the highest effective_ledger not exceeding `ledger`.
        let mut lo: u32 = 0;
        let mut hi: u32 = timeline.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            // Unreachable: mid is always < hi <= timeline.len() here, so
            // the entry always exists.
            let entry = timeline.get(mid).ok_or(Error::NotFound)?;
            if entry.effective_ledger <= ledger {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }

        if lo == 0 {
            return Err(Error::NotFound);
        }
        // Unreachable: lo - 1 < timeline.len() by construction above.
        let entry = timeline.get(lo - 1).ok_or(Error::NotFound)?;
        Ok(entry.version)
    }
}
