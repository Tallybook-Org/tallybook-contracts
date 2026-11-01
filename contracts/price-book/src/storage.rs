use soroban_sdk::{contracttype, vec, Address, Env, Vec};

use crate::types::{PriceBookVersion, TimelineEntry};

/// Ledgers in a day, used to derive the TTL constants below.
const DAY_IN_LEDGERS: u32 = 17_280;
/// Extend TTL once the remaining TTL drops below this many ledgers.
const BUMP_THRESHOLD: u32 = 30 * DAY_IN_LEDGERS;
/// ...and extend it out to this many ledgers of headroom. A year of
/// headroom on a one-year bump is deliberate: these are audit records, and
/// the cost of a rent bump is trivial next to an unreadable statement.
// pub: extend_version_ttl()'s clamp test needs the exact value to assert
// against, rather than duplicating the literal independently in test.rs.
pub const BUMP_AMOUNT: u32 = 365 * DAY_IN_LEDGERS;

/// `version_at()` binary-searches `Timeline(operator)` rather than scanning
/// every published `Version` key, so the vector is capped instead of left to
/// grow without limit. `publish()` errors with `TimelineFull` once reached.
/// Documented as a known limitation in the README.
pub const TIMELINE_CAP: u32 = 256;

#[contracttype]
pub enum DataKey {
    /// instance: the address that owns rent for this contract. Holds no
    /// power over operator data — see the constructor's doc comment.
    Admin,
    /// persistent: operator -> latest published version number.
    Latest(Address),
    /// persistent: (operator, version) -> the published PriceBookVersion.
    Version(Address, u32),
    /// persistent: operator -> Vec<TimelineEntry>, ascending by effective_ledger.
    Timeline(Address),
}

/// Extends the instance's own TTL. Called from the constructor and from
/// every state-changing call, per the workspace-wide TTL policy.
pub fn extend_instance_ttl(env: &Env) {
    env.storage().instance().extend_ttl(BUMP_THRESHOLD, BUMP_AMOUNT);
}

/// Whether `Admin` has been set. Test-only: production code has no reason
/// to check this — per CAP-0058 the constructor runs exactly once and is
/// never callable again — but the constructor test still wants a way to
/// prove the write landed without a public getter for Admin.
#[cfg(test)]
pub fn has_admin(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Admin)
}

/// Sets `Admin`. Constructor-only; there is no setter after construction.
pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

/// The latest version number an operator has published, if any.
pub fn get_latest(env: &Env, operator: &Address) -> Option<u32> {
    env.storage().persistent().get(&DataKey::Latest(operator.clone()))
}

/// Sets and extends the TTL of `Latest(operator)`.
pub fn set_latest(env: &Env, operator: &Address, version: u32) {
    let key = DataKey::Latest(operator.clone());
    env.storage().persistent().set(&key, &version);
    env.storage().persistent().extend_ttl(&key, BUMP_THRESHOLD, BUMP_AMOUNT);
}

/// A single published version, if it exists.
pub fn get_version(env: &Env, operator: &Address, version: u32) -> Option<PriceBookVersion> {
    env.storage().persistent().get(&DataKey::Version(operator.clone(), version))
}

/// Sets and extends the TTL of `Version(operator, version)`.
pub fn set_version(env: &Env, operator: &Address, version: u32, pbv: &PriceBookVersion) {
    let key = DataKey::Version(operator.clone(), version);
    env.storage().persistent().set(&key, pbv);
    env.storage().persistent().extend_ttl(&key, BUMP_THRESHOLD, BUMP_AMOUNT);
}

/// An operator's timeline, or an empty vector if they have never published.
pub fn get_timeline(env: &Env, operator: &Address) -> Vec<TimelineEntry> {
    env.storage().persistent().get(&DataKey::Timeline(operator.clone())).unwrap_or(vec![env])
}

/// Sets and extends the TTL of `Timeline(operator)`.
pub fn set_timeline(env: &Env, operator: &Address, timeline: &Vec<TimelineEntry>) {
    let key = DataKey::Timeline(operator.clone());
    env.storage().persistent().set(&key, timeline);
    env.storage().persistent().extend_ttl(&key, BUMP_THRESHOLD, BUMP_AMOUNT);
}

/// Extends `Version(operator, version)`'s TTL to `ledgers` from now
/// (clamped to `BUMP_AMOUNT`, the same ceiling every other TTL bump in this
/// contract uses), unless it is already extended at least that far —
/// `extend_ttl`'s threshold and target are the same value on purpose, so
/// this only ever extends, never shortens.
pub fn extend_version_ttl_to(env: &Env, operator: &Address, version: u32, ledgers: u32) {
    let extend_to = ledgers.min(BUMP_AMOUNT);
    let key = DataKey::Version(operator.clone(), version);
    env.storage().persistent().extend_ttl(&key, extend_to, extend_to);
}

/// Extends `Timeline(operator)`'s TTL the same way. A published version is
/// only evidence while the timeline that places it in time is still
/// readable: `version_at()` answers "which schedule applied at ledger N"
/// by binary-searching this vector, and with it archived a stored
/// `PriceBookVersion` proves a schedule existed but not that it was the one
/// in force. The two are extended together for that reason, never one
/// alone.
pub fn extend_timeline_ttl_to(env: &Env, operator: &Address, ledgers: u32) {
    let key = DataKey::Timeline(operator.clone());
    if env.storage().persistent().has(&key) {
        let extend_to = ledgers.min(BUMP_AMOUNT);
        env.storage().persistent().extend_ttl(&key, extend_to, extend_to);
    }
}
