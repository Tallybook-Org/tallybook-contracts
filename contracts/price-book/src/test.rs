#![cfg(test)]

// The crate is #![no_std]; tests need std for the standard test harness and
// for building throwaway data like an over-length URI below.
extern crate std;

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::{Error, PriceBook};

/// A distinct 32-byte value per `seed`, standing in for a real sha256
/// schedule hash — publish() never inspects its contents.
fn schedule_hash(env: &Env, seed: u8) -> soroban_sdk::BytesN<32> {
    soroban_sdk::BytesN::from_array(env, &[seed; 32])
}

/// Registers a fresh `PriceBook` with a generated admin. Auth is not
/// mocked here — individual tests opt into `env.mock_all_auths()` or
/// explicit `mock_auths` themselves, since which is correct differs per
/// test (happy path vs. negative auth path).
fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    let admin = Address::generate(&env);
    let contract_id = env.register(PriceBook, (admin.clone(),));
    (env, contract_id, admin)
}

mod constructor {
    use super::*;

    #[test]
    fn happy_path_sets_admin() {
        // There is no public getter for Admin — by design, it holds no
        // power and nothing reads it back — so reach into storage.rs
        // directly (crate-internal) to prove the write actually landed.
        let (env, contract_id, _admin) = setup();
        let admin_is_set = env.as_contract(&contract_id, || crate::storage::has_admin(&env));
        assert!(admin_is_set);
    }
}

mod publish {
    use soroban_sdk::{testutils::Events as _, Event as _, IntoVal, String};

    use crate::{event::PublishEvent, PriceBookClient};

    use super::*;

    #[test]
    fn happy_path_returns_incrementing_versions() {
        let (env, contract_id, _admin) = setup();
        env.mock_all_auths();
        let client = PriceBookClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let uri = String::from_str(&env, "https://example.com/schedule.json");
        let current_ledger = env.ledger().sequence();

        let v1 = client.publish(&operator, &schedule_hash(&env, 1), &uri, &current_ledger);
        assert_eq!(v1, 1);

        let v2 = client.publish(&operator, &schedule_hash(&env, 2), &uri, &(current_ledger + 1));
        assert_eq!(v2, 2);
    }

    #[test]
    fn happy_path_emits_publish_event() {
        let (env, contract_id, _admin) = setup();
        env.mock_all_auths();
        let client = PriceBookClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let hash = schedule_hash(&env, 7);
        let uri = String::from_str(&env, "https://example.com/schedule.json");
        let current_ledger = env.ledger().sequence();

        let version = client.publish(&operator, &hash, &uri, &current_ledger);

        let expected = PublishEvent {
            operator,
            version,
            schedule_hash: hash,
            effective_ledger: current_ledger,
        }
        .to_xdr(&env, &contract_id);
        assert_eq!(env.events().all(), std::vec![expected]);
    }

    #[test]
    fn uri_too_long_is_rejected() {
        let (env, contract_id, _admin) = setup();
        env.mock_all_auths();
        let client = PriceBookClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let long_uri = "a".repeat(201);
        let uri = String::from_str(&env, &long_uri);
        let current_ledger = env.ledger().sequence();

        let result = client.try_publish(&operator, &schedule_hash(&env, 1), &uri, &current_ledger);
        assert_eq!(result, Err(Ok(Error::UriTooLong)));
    }

    #[test]
    fn effective_in_past_is_rejected() {
        use soroban_sdk::testutils::Ledger as _;

        let (env, contract_id, _admin) = setup();
        env.mock_all_auths();
        env.ledger().set_sequence_number(1_000);
        let client = PriceBookClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let uri = String::from_str(&env, "https://example.com/schedule.json");

        let result = client.try_publish(&operator, &schedule_hash(&env, 1), &uri, &999);
        assert_eq!(result, Err(Ok(Error::EffectiveInPast)));
    }

    #[test]
    fn effective_not_after_previous_is_rejected() {
        let (env, contract_id, _admin) = setup();
        env.mock_all_auths();
        let client = PriceBookClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let uri = String::from_str(&env, "https://example.com/schedule.json");
        let current_ledger = env.ledger().sequence();

        client.publish(&operator, &schedule_hash(&env, 1), &uri, &current_ledger);

        // Not strictly after the first version's effective_ledger: equal is
        // rejected, same as anything lower would be.
        let result = client.try_publish(&operator, &schedule_hash(&env, 2), &uri, &current_ledger);
        assert_eq!(result, Err(Ok(Error::EffectiveNotAfter)));
    }

    #[test]
    fn timeline_full_is_rejected_once_cap_is_reached() {
        let (env, contract_id, _admin) = setup();
        env.mock_all_auths();
        let client = PriceBookClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let uri = String::from_str(&env, "https://example.com/schedule.json");
        let current_ledger = env.ledger().sequence();

        for i in 0..crate::storage::TIMELINE_CAP {
            client.publish(&operator, &schedule_hash(&env, 1), &uri, &(current_ledger + i));
        }

        let result = client.try_publish(
            &operator,
            &schedule_hash(&env, 1),
            &uri,
            &(current_ledger + crate::storage::TIMELINE_CAP),
        );
        assert_eq!(result, Err(Ok(Error::TimelineFull)));
    }

    #[test]
    fn unauthorized_caller_is_rejected() {
        use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};

        let (env, contract_id, _admin) = setup();
        let client = PriceBookClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let attacker = Address::generate(&env);
        let hash = schedule_hash(&env, 1);
        let uri = String::from_str(&env, "https://example.com/schedule.json");
        let current_ledger = env.ledger().sequence();

        // Authorize attacker, not operator — publish() requires
        // operator.require_auth(), so this must fail even though *some*
        // valid auth entry is present.
        env.mock_auths(&[MockAuth {
            address: &attacker,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "publish",
                args: (operator.clone(), hash.clone(), uri.clone(), current_ledger).into_val(&env),
                sub_invokes: &[],
            },
        }]);

        let result = client.try_publish(&operator, &hash, &uri, &current_ledger);
        assert!(result.is_err());
    }
}

mod get_version {
    use soroban_sdk::String;

    use crate::PriceBookClient;

    use super::*;

    #[test]
    fn happy_path_returns_the_published_version() {
        let (env, contract_id, _admin) = setup();
        env.mock_all_auths();
        let client = PriceBookClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let hash = schedule_hash(&env, 1);
        let uri = String::from_str(&env, "https://example.com/schedule.json");
        let current_ledger = env.ledger().sequence();

        let version = client.publish(&operator, &hash, &uri, &current_ledger);
        let published = client.get_version(&operator, &version);

        assert_eq!(published.operator, operator);
        assert_eq!(published.version, version);
        assert_eq!(published.schedule_hash, hash);
        assert_eq!(published.uri, uri);
        assert_eq!(published.effective_ledger, current_ledger);
        assert_eq!(published.published_ledger, current_ledger);
    }

    #[test]
    fn not_found_for_unpublished_version() {
        let (env, contract_id, _admin) = setup();
        let client = PriceBookClient::new(&env, &contract_id);
        let operator = Address::generate(&env);

        let result = client.try_get_version(&operator, &1);
        assert_eq!(result, Err(Ok(Error::NotFound)));
    }
}

mod latest {
    use soroban_sdk::String;

    use crate::PriceBookClient;

    use super::*;

    #[test]
    fn happy_path_returns_the_highest_published_version() {
        let (env, contract_id, _admin) = setup();
        env.mock_all_auths();
        let client = PriceBookClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let uri = String::from_str(&env, "https://example.com/schedule.json");
        let current_ledger = env.ledger().sequence();

        client.publish(&operator, &schedule_hash(&env, 1), &uri, &current_ledger);
        client.publish(&operator, &schedule_hash(&env, 2), &uri, &(current_ledger + 1));

        assert_eq!(client.latest(&operator), 2);
    }

    #[test]
    fn not_found_for_an_operator_that_never_published() {
        let (env, contract_id, _admin) = setup();
        let client = PriceBookClient::new(&env, &contract_id);
        let operator = Address::generate(&env);

        let result = client.try_latest(&operator);
        assert_eq!(result, Err(Ok(Error::NotFound)));
    }
}

mod version_at {
    use soroban_sdk::String;

    use crate::PriceBookClient;

    use super::*;

    #[test]
    fn empty_timeline_is_not_found() {
        let (env, contract_id, _admin) = setup();
        let client = PriceBookClient::new(&env, &contract_id);
        let operator = Address::generate(&env);

        let result = client.try_version_at(&operator, &0);
        assert_eq!(result, Err(Ok(Error::NotFound)));
    }

    #[test]
    fn before_first_version_is_not_found() {
        let (env, contract_id, _admin) = setup();
        env.mock_all_auths();
        let client = PriceBookClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let uri = String::from_str(&env, "https://example.com/schedule.json");
        let current_ledger = env.ledger().sequence();
        let first_effective = current_ledger + 10;

        client.publish(&operator, &schedule_hash(&env, 1), &uri, &first_effective);

        let result = client.try_version_at(&operator, &(first_effective - 1));
        assert_eq!(result, Err(Ok(Error::NotFound)));
    }

    #[test]
    fn exactly_equal_ledger_returns_that_version() {
        let (env, contract_id, _admin) = setup();
        env.mock_all_auths();
        let client = PriceBookClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let uri = String::from_str(&env, "https://example.com/schedule.json");
        let current_ledger = env.ledger().sequence();

        let version = client.publish(&operator, &schedule_hash(&env, 1), &uri, &current_ledger);

        assert_eq!(client.version_at(&operator, &current_ledger), version);
    }

    #[test]
    fn after_last_version_returns_the_latest_version() {
        let (env, contract_id, _admin) = setup();
        env.mock_all_auths();
        let client = PriceBookClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let uri = String::from_str(&env, "https://example.com/schedule.json");
        let current_ledger = env.ledger().sequence();

        client.publish(&operator, &schedule_hash(&env, 1), &uri, &current_ledger);
        let v2 = client.publish(&operator, &schedule_hash(&env, 2), &uri, &(current_ledger + 5));

        assert_eq!(client.version_at(&operator, &(current_ledger + 1_000)), v2);
    }
}

mod extend_version_ttl {
    use soroban_sdk::testutils::storage::Persistent as _;
    use soroban_sdk::testutils::Ledger as _;
    use soroban_sdk::String;

    use crate::storage::DataKey;
    use crate::PriceBookClient;

    use super::*;

    /// Publishes one version for a generated operator and returns
    /// (env, contract_id, operator, version).
    fn published() -> (Env, Address, Address, u32) {
        let (env, contract_id, _admin) = setup();
        env.mock_all_auths();
        let client = PriceBookClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let uri = String::from_str(&env, "https://example.com/schedule.json");
        let effective = env.ledger().sequence();
        let version = client.publish(&operator, &schedule_hash(&env, 1), &uri, &effective);
        (env, contract_id, operator, version)
    }

    // DataKey is not Clone, so each reading builds its own key rather
    // than holding one across calls.
    fn version_ttl(env: &Env, contract_id: &Address, operator: &Address, version: u32) -> u32 {
        let key = DataKey::Version(operator.clone(), version);
        env.as_contract(contract_id, || env.storage().persistent().get_ttl(&key))
    }

    fn timeline_ttl(env: &Env, contract_id: &Address, operator: &Address) -> u32 {
        let key = DataKey::Timeline(operator.clone());
        env.as_contract(contract_id, || env.storage().persistent().get_ttl(&key))
    }

    #[test]
    fn no_auth_required_and_extends_version_and_timeline() {
        let (env, contract_id, operator, version) = published();
        let client = PriceBookClient::new(&env, &contract_id);

        // publish() already extended both keys out to the BUMP_AMOUNT
        // ceiling, so without advancing the ledger first any extension
        // would be a correct no-op rather than a real test of growth.
        env.ledger().set_sequence_number(env.ledger().sequence() + 1_000);
        let version_before = version_ttl(&env, &contract_id, &operator, version);
        let timeline_before = timeline_ttl(&env, &contract_id, &operator);

        // Strict enforcement with zero valid auth entries — the strongest
        // proof that no auth is required, stronger than simply never
        // having called mock_all_auths (which published() left active).
        env.mock_auths(&[]);
        client.extend_version_ttl(&operator, &version, &crate::storage::BUMP_AMOUNT);

        assert!(version_ttl(&env, &contract_id, &operator, version) > version_before);
        assert!(timeline_ttl(&env, &contract_id, &operator) > timeline_before);
    }

    #[test]
    fn clamped_to_a_sane_maximum() {
        let (env, contract_id, operator, version) = published();
        let client = PriceBookClient::new(&env, &contract_id);
        env.ledger().set_sequence_number(env.ledger().sequence() + 1_000);

        client.extend_version_ttl(&operator, &version, &u32::MAX);

        let ttl = version_ttl(&env, &contract_id, &operator, version);
        assert!(ttl <= crate::storage::BUMP_AMOUNT);
    }

    #[test]
    fn not_found_for_unpublished_version() {
        let (env, contract_id, operator, version) = published();
        let client = PriceBookClient::new(&env, &contract_id);
        assert_eq!(
            client.try_extend_version_ttl(&operator, &(version + 1), &1_000),
            Err(Ok(Error::NotFound))
        );
    }

    #[test]
    fn not_found_for_an_operator_that_never_published() {
        let (env, contract_id, _operator, _version) = published();
        let client = PriceBookClient::new(&env, &contract_id);
        let stranger = Address::generate(&env);
        assert_eq!(client.try_extend_version_ttl(&stranger, &1, &1_000), Err(Ok(Error::NotFound)));
    }
}

/// Properties of `version_at` that the fixed boundary tests cannot express:
/// that it is monotonic over arbitrary timelines, and that it agrees with a
/// linear scan over every published version.
///
/// Randomised with a fixed-seed LCG rather than a property-testing crate, so
/// the sequence is identical on every machine and in CI and the workspace
/// gains no dependency -- this repo pins soroban-sdk to an exact patch
/// version on purpose, and a test-only crate would still land in Cargo.lock
/// and in the supply-chain audit's scope. The generator is deliberately
/// duplicated in statement-registry's own property tests rather than shared:
/// the two contracts are separate crates, and a third crate to hold eight
/// lines would cost more than it saves.
mod version_at_properties {
    use soroban_sdk::testutils::Ledger as _;
    use soroban_sdk::String;

    use crate::PriceBookClient;

    use super::*;

    struct Lcg(u32);

    impl Lcg {
        fn next_u32(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            self.0
        }

        fn below(&mut self, bound: u32) -> u32 {
            self.next_u32() % bound
        }
    }

    /// Publishes `effective_ledgers` in order for one operator and returns
    /// the client and that operator.
    fn publish_all(env: &Env, contract_id: &Address, effective: &[u32]) -> Address {
        let client = PriceBookClient::new(env, contract_id);
        let operator = Address::generate(env);
        let uri = String::from_str(env, "https://example.com/schedule.json");
        for (i, ledger) in effective.iter().enumerate() {
            let version = client.publish(&operator, &schedule_hash(env, i as u8), &uri, ledger);
            assert_eq!(version, i as u32 + 1, "publish must number versions from 1");
        }
        operator
    }

    /// Which version a linear scan says was in force at `ledger`: the last
    /// entry whose effective_ledger does not exceed it. The oracle
    /// version_at's binary search is checked against.
    fn scan(effective: &[u32], ledger: u32) -> Option<u32> {
        let mut found = None;
        for (i, candidate) in effective.iter().enumerate() {
            if *candidate <= ledger {
                found = Some(i as u32 + 1);
            }
        }
        found
    }

    #[test]
    fn agrees_with_a_linear_scan_over_arbitrary_timelines() {
        for seed in 0..48u32 {
            let (env, contract_id, _admin) = setup();
            env.mock_all_auths();
            // Publishing requires effective_ledger >= the current ledger, so
            // the ledger stays low while the timeline is built.
            env.ledger().set_sequence_number(1);

            let mut rng = Lcg(seed.wrapping_mul(2_654_435_761).wrapping_add(1));
            let count = rng.below(10) + 1;
            let mut effective = std::vec::Vec::new();
            let mut ledger = 1u32;
            for _ in 0..count {
                // Strictly ascending, with gaps, so queries land both on
                // entries and between them.
                ledger += rng.below(50) + 1;
                effective.push(ledger);
            }

            let operator = publish_all(&env, &contract_id, &effective);
            let client = PriceBookClient::new(&env, &contract_id);
            let highest = *effective.last().unwrap();

            for _ in 0..40 {
                let query = rng.below(highest + 60);
                let expected = scan(&effective, query);
                let actual = client.try_version_at(&operator, &query).ok().map(|r| r.unwrap());
                assert_eq!(
                    actual, expected,
                    "seed {seed}: version_at({query}) over {effective:?} disagreed with a linear scan"
                );
            }
        }
    }

    #[test]
    fn is_monotonic_and_exact_on_every_boundary() {
        for seed in 0..48u32 {
            let (env, contract_id, _admin) = setup();
            env.mock_all_auths();
            env.ledger().set_sequence_number(1);

            let mut rng = Lcg(seed.wrapping_mul(40_503).wrapping_add(7));
            let count = rng.below(10) + 1;
            let mut effective = std::vec::Vec::new();
            // Start above 1 so effective[0] - 1 is always a legal query.
            let mut ledger = 10u32;
            for _ in 0..count {
                ledger += rng.below(50) + 2;
                effective.push(ledger);
            }

            let operator = publish_all(&env, &contract_id, &effective);
            let client = PriceBookClient::new(&env, &contract_id);

            for (i, boundary) in effective.iter().enumerate() {
                // A schedule applies from its effective ledger onward, so
                // the boundary itself belongs to the new version and the
                // ledger before it to the old one.
                assert_eq!(client.version_at(&operator, boundary), i as u32 + 1);

                let before = client.try_version_at(&operator, &(boundary - 1));
                if i == 0 {
                    assert_eq!(before, Err(Ok(Error::NotFound)));
                } else {
                    assert_eq!(before, Ok(Ok(i as u32)));
                }
            }

            // Monotonic: a later query never resolves to an earlier version.
            let highest = *effective.last().unwrap();
            let mut previous = 0u32;
            for query in 0..=highest + 20 {
                if let Ok(Ok(version)) = client.try_version_at(&operator, &query) {
                    assert!(
                        version >= previous,
                        "seed {seed}: version_at went backwards at ledger {query}"
                    );
                    previous = version;
                }
            }
        }
    }
}
