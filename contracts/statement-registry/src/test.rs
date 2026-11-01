#![cfg(test)]

// The crate is #![no_std]; tests need std for the standard test harness and
// for building throwaway data like the leaf array below.
extern crate std;

use soroban_sdk::{testutils::Address as _, xdr::ToXdr as _, Address, Bytes, BytesN, Env, String};

use crate::{Error, StatementRegistry};

/// sha256(min(a, b) || max(a, b)) — a reference implementation independent
/// of merkle::fold, so tests assert against a computation that doesn't
/// share fold's own logic.
fn hash_pair(env: &Env, a: &BytesN<32>, b: &BytesN<32>) -> BytesN<32> {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let mut concatenated = Bytes::from(lo.clone());
    concatenated.append(&Bytes::from(hi.clone()));
    env.crypto().sha256(&concatenated).to_bytes()
}

/// Registers a fresh, real price_book contract (never mocked — the whole
/// point of §6's PriceVersionStale check is to test against real
/// on-chain price-book behaviour) and a fresh StatementRegistry pointed at
/// it. Auth is not mocked here; individual tests opt into
/// `env.mock_all_auths()` or explicit `mock_auths` themselves.
fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    let admin = Address::generate(&env);
    let price_book_admin = Address::generate(&env);
    let price_book_id = env.register(crate::price_book::WASM, (price_book_admin,));
    let contract_id = env.register(StatementRegistry, (admin.clone(), price_book_id.clone()));
    (env, contract_id, admin, price_book_id)
}

/// Publishes a price schedule for `operator`, effective immediately at
/// `effective_ledger`, against the real price_book at `price_book_id`, and
/// returns the resulting version number. Caller must have already
/// authorized `operator` (e.g. via `env.mock_all_auths()`).
fn publish_schedule(
    env: &Env,
    price_book_id: &Address,
    operator: &Address,
    effective_ledger: u32,
) -> u32 {
    let client = crate::price_book::Client::new(env, price_book_id);
    let schedule_hash = BytesN::from_array(env, &[1u8; 32]);
    let uri = String::from_str(env, "https://example.com/schedule.json");
    client.publish(operator, &schedule_hash, &uri, &effective_ledger)
}

mod constructor {
    use super::*;

    #[test]
    fn happy_path_sets_admin_and_price_book() {
        // No public getters for Admin or PriceBook by design (admin holds
        // no power; price_book is only ever read internally by anchor()) —
        // reach into storage.rs directly (crate-internal) to prove the
        // writes landed.
        let (env, contract_id, _admin, price_book_id) = setup();
        let (admin_is_set, stored_price_book) = env.as_contract(&contract_id, || {
            (crate::storage::has_admin(&env), crate::storage::get_price_book(&env))
        });
        assert!(admin_is_set);
        assert_eq!(stored_price_book, Some(price_book_id));
    }
}

mod anchor {
    use soroban_sdk::testutils::{Events as _, Ledger as _};
    use soroban_sdk::Event as _;

    use crate::event::AnchorEvent;
    use crate::types::Protocol;
    use crate::StatementRegistryClient;

    use super::*;

    #[test]
    fn happy_path_returns_incrementing_seq() {
        let (env, contract_id, _admin, price_book_id) = setup();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);
        let usage_root = BytesN::from_array(&env, &[42u8; 32]);

        let base = env.ledger().sequence();
        let version = publish_schedule(&env, &price_book_id, &operator, base);
        env.ledger().set_sequence_number(base + 100);

        let seq1 = client.anchor(
            &operator,
            &consumer,
            &base,
            &(base + 50),
            &usage_root,
            &10,
            &token,
            &1000i128,
            &900i128,
            &version,
            &Protocol::X402,
            &None,
        );
        assert_eq!(seq1, 1);

        let seq2 = client.anchor(
            &operator,
            &consumer,
            &(base + 50),
            &(base + 90),
            &usage_root,
            &10,
            &token,
            &1000i128,
            &900i128,
            &version,
            &Protocol::X402,
            &None,
        );
        assert_eq!(seq2, 2);
    }

    #[test]
    fn happy_path_emits_anchor_event() {
        let (env, contract_id, _admin, price_book_id) = setup();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);
        let usage_root = BytesN::from_array(&env, &[42u8; 32]);

        let base = env.ledger().sequence();
        let version = publish_schedule(&env, &price_book_id, &operator, base);
        env.ledger().set_sequence_number(base + 100);

        let seq = client.anchor(
            &operator,
            &consumer,
            &base,
            &(base + 50),
            &usage_root,
            &10,
            &token,
            &1000i128,
            &900i128,
            &version,
            &Protocol::X402,
            &None,
        );

        let expected = AnchorEvent {
            operator,
            consumer,
            seq,
            usage_root,
            amount_billed: 1000,
            amount_settled: 900,
            protocol: Protocol::X402,
        }
        .to_xdr(&env, &contract_id);
        assert_eq!(env.events().all(), std::vec![expected]);
    }

    // BadPeriod, EmptyStatement, and BadAmounts are all checked before the
    // price_book cross-contract call, so these tests pass an arbitrary
    // price_book_version (0) — it's never reached.

    #[test]
    fn bad_period_start_not_before_end_is_rejected() {
        let (env, contract_id, _admin, _price_book_id) = setup();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);
        let usage_root = BytesN::from_array(&env, &[42u8; 32]);
        let base = env.ledger().sequence();

        let result = client.try_anchor(
            &operator,
            &consumer,
            &base,
            &base, // period_start == period_end
            &usage_root,
            &10,
            &token,
            &1000i128,
            &900i128,
            &0,
            &Protocol::X402,
            &None,
        );
        assert_eq!(result, Err(Ok(Error::BadPeriod)));
    }

    #[test]
    fn bad_period_end_after_current_ledger_is_rejected() {
        let (env, contract_id, _admin, _price_book_id) = setup();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);
        let usage_root = BytesN::from_array(&env, &[42u8; 32]);
        let base = env.ledger().sequence();

        let result = client.try_anchor(
            &operator,
            &consumer,
            &base,
            &(base + 1), // period_end is in the future — the period hasn't finished
            &usage_root,
            &10,
            &token,
            &1000i128,
            &900i128,
            &0,
            &Protocol::X402,
            &None,
        );
        assert_eq!(result, Err(Ok(Error::BadPeriod)));
    }

    #[test]
    fn empty_statement_is_rejected() {
        let (env, contract_id, _admin, _price_book_id) = setup();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);
        let usage_root = BytesN::from_array(&env, &[42u8; 32]);
        let base = env.ledger().sequence();
        env.ledger().set_sequence_number(base + 100);

        let result = client.try_anchor(
            &operator,
            &consumer,
            &base,
            &(base + 1),
            &usage_root,
            &0, // request_count
            &token,
            &1000i128,
            &900i128,
            &0,
            &Protocol::X402,
            &None,
        );
        assert_eq!(result, Err(Ok(Error::EmptyStatement)));
    }

    #[test]
    fn bad_amounts_negative_billed_is_rejected() {
        let (env, contract_id, _admin, _price_book_id) = setup();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);
        let usage_root = BytesN::from_array(&env, &[42u8; 32]);
        let base = env.ledger().sequence();
        env.ledger().set_sequence_number(base + 100);

        let result = client.try_anchor(
            &operator,
            &consumer,
            &base,
            &(base + 1),
            &usage_root,
            &10,
            &token,
            &(-1i128),
            &0i128,
            &0,
            &Protocol::X402,
            &None,
        );
        assert_eq!(result, Err(Ok(Error::BadAmounts)));
    }

    #[test]
    fn bad_amounts_negative_settled_is_rejected() {
        let (env, contract_id, _admin, _price_book_id) = setup();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);
        let usage_root = BytesN::from_array(&env, &[42u8; 32]);
        let base = env.ledger().sequence();
        env.ledger().set_sequence_number(base + 100);

        let result = client.try_anchor(
            &operator,
            &consumer,
            &base,
            &(base + 1),
            &usage_root,
            &10,
            &token,
            &1000i128,
            &(-1i128),
            &0,
            &Protocol::X402,
            &None,
        );
        assert_eq!(result, Err(Ok(Error::BadAmounts)));
    }

    #[test]
    fn bad_amounts_settled_exceeds_billed_is_rejected() {
        let (env, contract_id, _admin, _price_book_id) = setup();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);
        let usage_root = BytesN::from_array(&env, &[42u8; 32]);
        let base = env.ledger().sequence();
        env.ledger().set_sequence_number(base + 100);

        let result = client.try_anchor(
            &operator,
            &consumer,
            &base,
            &(base + 1),
            &usage_root,
            &10,
            &token,
            &100i128,
            &101i128, // settled > billed
            &0,
            &Protocol::X402,
            &None,
        );
        assert_eq!(result, Err(Ok(Error::BadAmounts)));
    }

    // ChannelMismatch is also checked before the price_book call, so these
    // tests pass an arbitrary price_book_version (0) too.

    #[test]
    fn channel_missing_for_mpp_session_is_rejected() {
        let (env, contract_id, _admin, _price_book_id) = setup();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);
        let usage_root = BytesN::from_array(&env, &[42u8; 32]);
        let base = env.ledger().sequence();
        env.ledger().set_sequence_number(base + 100);

        let result = client.try_anchor(
            &operator,
            &consumer,
            &base,
            &(base + 50),
            &usage_root,
            &10,
            &token,
            &1000i128,
            &900i128,
            &0,
            &Protocol::MppSession,
            &None, // channel required for MppSession
        );
        assert_eq!(result, Err(Ok(Error::ChannelMismatch)));
    }

    #[test]
    fn channel_present_for_non_session_protocol_is_rejected() {
        let (env, contract_id, _admin, _price_book_id) = setup();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);
        let channel = Address::generate(&env);
        let usage_root = BytesN::from_array(&env, &[42u8; 32]);
        let base = env.ledger().sequence();
        env.ledger().set_sequence_number(base + 100);

        let result = client.try_anchor(
            &operator,
            &consumer,
            &base,
            &(base + 50),
            &usage_root,
            &10,
            &token,
            &1000i128,
            &900i128,
            &0,
            &Protocol::X402,
            &Some(channel), // channel must be absent outside MppSession
        );
        assert_eq!(result, Err(Ok(Error::ChannelMismatch)));
    }

    // The price_book check itself: anchored against a REAL deployed
    // price_book (never mocked — see setup()'s own doc comment), because
    // this is the check the whole contract exists for. PriceVersionUnknown
    // isn't named in its own build-sequence step, so it's covered here
    // alongside PriceVersionStale and PeriodSpansPriceChange as the other
    // halves of the same cross-contract validation block.

    #[test]
    fn period_straddling_price_change_is_rejected() {
        let (env, contract_id, _admin, price_book_id) = setup();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);
        let usage_root = BytesN::from_array(&env, &[42u8; 32]);

        let base = env.ledger().sequence();
        // v1, effective immediately.
        let v1 = publish_schedule(&env, &price_book_id, &operator, base);
        // v2 supersedes v1 partway through the eventual statement period.
        let v2 = publish_schedule(&env, &price_book_id, &operator, base + 20);
        assert_eq!(v2, v1 + 1);
        env.ledger().set_sequence_number(base + 200);

        // period_start (base) is under v1; period_end (base + 100) is
        // after v2's effective_ledger (base + 20), so it's under v2. No
        // single price_book_version honestly covers this period —
        // rejected regardless of which version is claimed.
        let result = client.try_anchor(
            &operator,
            &consumer,
            &base,
            &(base + 100),
            &usage_root,
            &10,
            &token,
            &1000i128,
            &900i128,
            &v1,
            &Protocol::X402,
            &None,
        );
        assert_eq!(result, Err(Ok(Error::PeriodSpansPriceChange)));
    }

    #[test]
    fn price_version_stale_is_rejected() {
        let (env, contract_id, _admin, price_book_id) = setup();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);
        let usage_root = BytesN::from_array(&env, &[42u8; 32]);

        let base = env.ledger().sequence();
        let v1 = publish_schedule(&env, &price_book_id, &operator, base);
        let v2 = publish_schedule(&env, &price_book_id, &operator, base + 20);
        env.ledger().set_sequence_number(base + 200);

        // Both period_start and period_end (base + 50, base + 100) are
        // after v2's effective_ledger, so the period sits entirely inside
        // v2's window — no spanning. The claim of v1 is simply wrong.
        let result = client.try_anchor(
            &operator,
            &consumer,
            &(base + 50),
            &(base + 100),
            &usage_root,
            &10,
            &token,
            &1000i128,
            &900i128,
            &v1,
            &Protocol::X402,
            &None,
        );
        assert_eq!(result, Err(Ok(Error::PriceVersionStale)));
        // Sanity: v2 is what version_at would actually return here.
        assert_eq!(v2, v1 + 1);
    }

    #[test]
    fn claimed_version_newer_than_in_force_is_rejected() {
        // Inverse of price_version_stale_is_rejected: only one version
        // exists, the period sits entirely inside its window (no
        // spanning), but the claim names a version that hasn't been
        // published — newer than what's actually in force, not older.
        let (env, contract_id, _admin, price_book_id) = setup();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);
        let usage_root = BytesN::from_array(&env, &[42u8; 32]);

        let base = env.ledger().sequence();
        let v1 = publish_schedule(&env, &price_book_id, &operator, base);
        env.ledger().set_sequence_number(base + 200);

        let result = client.try_anchor(
            &operator,
            &consumer,
            &base,
            &(base + 100),
            &usage_root,
            &10,
            &token,
            &1000i128,
            &900i128,
            &(v1 + 1), // claims a version that was never published
            &Protocol::X402,
            &None,
        );
        assert_eq!(result, Err(Ok(Error::PriceVersionStale)));
    }

    #[test]
    fn honest_period_fully_inside_one_version_window_succeeds() {
        // Multiple versions exist over the contract's lifetime, but the
        // chosen period sits entirely before the second one takes effect —
        // the mere existence of a later version elsewhere in time must not
        // false-positive as PeriodSpansPriceChange.
        let (env, contract_id, _admin, price_book_id) = setup();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);
        let usage_root = BytesN::from_array(&env, &[42u8; 32]);

        let base = env.ledger().sequence();
        let v1 = publish_schedule(&env, &price_book_id, &operator, base);
        publish_schedule(&env, &price_book_id, &operator, base + 50);
        env.ledger().set_sequence_number(base + 200);

        let seq = client.anchor(
            &operator,
            &consumer,
            &(base + 10),
            &(base + 40), // fully before v2's effective_ledger (base + 50)
            &usage_root,
            &10,
            &token,
            &1000i128,
            &900i128,
            &v1,
            &Protocol::X402,
            &None,
        );
        assert_eq!(seq, 1);
    }

    #[test]
    fn price_version_unknown_is_rejected() {
        let (env, contract_id, _admin, _price_book_id) = setup();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);
        let usage_root = BytesN::from_array(&env, &[42u8; 32]);
        let base = env.ledger().sequence();
        env.ledger().set_sequence_number(base + 100);

        // operator has never published anything on the real price_book —
        // version_at() itself errors, distinct from a version mismatch.
        let result = client.try_anchor(
            &operator,
            &consumer,
            &base,
            &(base + 50),
            &usage_root,
            &10,
            &token,
            &1000i128,
            &900i128,
            &1,
            &Protocol::X402,
            &None,
        );
        assert_eq!(result, Err(Ok(Error::PriceVersionUnknown)));
    }

    #[test]
    fn unauthorized_caller_is_rejected() {
        use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
        use soroban_sdk::IntoVal;

        let (env, contract_id, _admin, _price_book_id) = setup();
        let operator = Address::generate(&env);
        let attacker = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);
        let usage_root = BytesN::from_array(&env, &[42u8; 32]);
        let base = env.ledger().sequence();
        env.ledger().set_sequence_number(base + 100);

        let client = StatementRegistryClient::new(&env, &contract_id);

        // Authorize attacker, not operator — anchor() requires
        // operator.require_auth(), so this must fail even though *some*
        // valid auth entry is present.
        env.mock_auths(&[MockAuth {
            address: &attacker,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "anchor",
                args: (
                    operator.clone(),
                    consumer.clone(),
                    base,
                    base + 50,
                    usage_root.clone(),
                    10u64,
                    token.clone(),
                    1000i128,
                    900i128,
                    1u32,
                    Protocol::X402,
                    Option::<Address>::None,
                )
                    .into_val(&env),
                sub_invokes: &[],
            },
        }]);

        let result = client.try_anchor(
            &operator,
            &consumer,
            &base,
            &(base + 50),
            &usage_root,
            &10,
            &token,
            &1000i128,
            &900i128,
            &1,
            &Protocol::X402,
            &None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn index_full_is_rejected_once_cap_is_reached() {
        let (env, contract_id, _admin, price_book_id) = setup();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);
        let usage_root = BytesN::from_array(&env, &[42u8; 32]);

        let base = env.ledger().sequence();
        let version = publish_schedule(&env, &price_book_id, &operator, base);
        env.ledger().set_sequence_number(base + crate::storage::CONSUMER_IDX_CAP + 100);

        for i in 0..crate::storage::CONSUMER_IDX_CAP {
            client.anchor(
                &operator,
                &consumer,
                &(base + i),
                &(base + i + 1),
                &usage_root,
                &10,
                &token,
                &1000i128,
                &900i128,
                &version,
                &Protocol::X402,
                &None,
            );
        }

        let cap = crate::storage::CONSUMER_IDX_CAP;
        let result = client.try_anchor(
            &operator,
            &consumer,
            &(base + cap),
            &(base + cap + 1),
            &usage_root,
            &10,
            &token,
            &1000i128,
            &900i128,
            &version,
            &Protocol::X402,
            &None,
        );
        assert_eq!(result, Err(Ok(Error::IndexFull)));
    }
}

mod verify_usage {
    use serde_json::Value;
    use soroban_sdk::testutils::Ledger as _;
    use soroban_sdk::vec;

    use crate::types::Protocol;
    use crate::StatementRegistryClient;

    use super::*;

    fn hex_to_bytesn(env: &Env, hex: &str) -> BytesN<32> {
        let mut bytes = [0u8; 32];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        BytesN::from_array(env, &bytes)
    }

    /// Anchors a minimal, otherwise-valid statement with `usage_root` and
    /// returns (Env, contract_id, operator, seq) for verify_usage calls
    /// against it.
    fn anchor_with_usage_root(usage_root: &BytesN<32>) -> (Env, Address, Address, u64) {
        let env = usage_root.env().clone();
        let admin = Address::generate(&env);
        let price_book_admin = Address::generate(&env);
        let price_book_id = env.register(crate::price_book::WASM, (price_book_admin,));
        let contract_id = env.register(StatementRegistry, (admin, price_book_id.clone()));
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);

        let base = env.ledger().sequence();
        let version = publish_schedule(&env, &price_book_id, &operator, base);
        env.ledger().set_sequence_number(base + 100);

        let seq = client.anchor(
            &operator,
            &consumer,
            &base,
            &(base + 50),
            usage_root,
            &10,
            &token,
            &1000i128,
            &900i128,
            &version,
            &Protocol::X402,
            &None,
        );
        (env, contract_id, operator, seq)
    }

    fn assert_fixture_verifies(json: &str) {
        // hex_to_bytesn needs an Env before anchor_with_usage_root's own
        // setup exists, so build one just to parse the root — BytesN
        // values built against it are then handed into
        // anchor_with_usage_root, which reuses that same Env throughout.
        let env = Env::default();
        let parsed: Value = serde_json::from_str(json).unwrap();
        let usage_root = hex_to_bytesn(&env, parsed["root"].as_str().unwrap());

        let (env, contract_id, operator, seq) = anchor_with_usage_root(&usage_root);
        let client = StatementRegistryClient::new(&env, &contract_id);

        for entry in parsed["proofs"].as_array().unwrap() {
            let leaf = hex_to_bytesn(&env, entry["leaf"].as_str().unwrap());
            let mut proof = vec![&env];
            for node in entry["proof"].as_array().unwrap() {
                proof.push_back(hex_to_bytesn(&env, node.as_str().unwrap()));
            }
            assert!(client.verify_usage(&operator, &seq, &leaf, &proof));
        }
    }

    #[test]
    fn single_leaf_fixture_verifies() {
        assert_fixture_verifies(include_str!("../../../fixtures/merkle/single-leaf.json"));
    }

    #[test]
    fn four_leaves_fixture_verifies() {
        assert_fixture_verifies(include_str!("../../../fixtures/merkle/four-leaves.json"));
    }

    #[test]
    fn seven_leaves_fixture_verifies() {
        assert_fixture_verifies(include_str!("../../../fixtures/merkle/seven-leaves.json"));
    }

    #[test]
    fn corrupted_proof_returns_false_not_error() {
        let env = Env::default();
        let parsed: Value =
            serde_json::from_str(include_str!("../../../fixtures/merkle/four-leaves.json"))
                .unwrap();
        let usage_root = hex_to_bytesn(&env, parsed["root"].as_str().unwrap());

        let (env, contract_id, operator, seq) = anchor_with_usage_root(&usage_root);
        let client = StatementRegistryClient::new(&env, &contract_id);

        let entry = &parsed["proofs"][0];
        let leaf = hex_to_bytesn(&env, entry["leaf"].as_str().unwrap());
        let mut proof = vec![&env];
        for (i, node) in entry["proof"].as_array().unwrap().iter().enumerate() {
            let mut node_bytes = hex_to_bytesn(&env, node.as_str().unwrap()).to_array();
            if i == 0 {
                node_bytes[0] ^= 0xff;
            }
            proof.push_back(BytesN::from_array(&env, &node_bytes));
        }

        assert!(!client.verify_usage(&operator, &seq, &leaf, &proof));
    }

    #[test]
    fn not_found_for_missing_statement() {
        let (env, contract_id, _admin, _price_book_id) = setup();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let leaf = BytesN::from_array(&env, &[1u8; 32]);

        let result = client.try_verify_usage(&operator, &1, &leaf, &vec![&env]);
        assert_eq!(result, Err(Ok(Error::NotFound)));
    }

    #[test]
    fn proof_too_long_is_rejected() {
        let (env, contract_id, _admin, _price_book_id) = setup();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let leaf = BytesN::from_array(&env, &[1u8; 32]);

        let mut proof = vec![&env];
        for i in 0..(crate::storage::MAX_PROOF_NODES + 1) {
            proof.push_back(BytesN::from_array(&env, &[i as u8; 32]));
        }

        // No statement needs to exist: ProofTooLong is checked before the
        // storage lookup.
        let result = client.try_verify_usage(&operator, &1, &leaf, &proof);
        assert_eq!(result, Err(Ok(Error::ProofTooLong)));
    }
}

mod open_dispute {
    use soroban_sdk::testutils::{Events as _, Ledger as _};
    use soroban_sdk::Event as _;

    use crate::event::DisputeEvent;
    use crate::types::{Protocol, Status};
    use crate::StatementRegistryClient;

    use super::*;

    /// Anchors a minimal, otherwise-valid statement and returns
    /// (Env, contract_id, operator, consumer, seq).
    fn anchor_statement() -> (Env, Address, Address, Address, u64) {
        let (env, contract_id, _admin, price_book_id) = setup();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);
        let usage_root = BytesN::from_array(&env, &[42u8; 32]);

        let base = env.ledger().sequence();
        let version = publish_schedule(&env, &price_book_id, &operator, base);
        env.ledger().set_sequence_number(base + 100);

        let seq = client.anchor(
            &operator,
            &consumer,
            &base,
            &(base + 50),
            &usage_root,
            &10,
            &token,
            &1000i128,
            &900i128,
            &version,
            &Protocol::X402,
            &None,
        );
        (env, contract_id, operator, consumer, seq)
    }

    #[test]
    fn happy_path_sets_status_disputed_and_writes_dispute() {
        let (env, contract_id, operator, consumer, seq) = anchor_statement();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let reason_hash = BytesN::from_array(&env, &[7u8; 32]);

        client.open_dispute(&operator, &seq, &consumer, &reason_hash);

        // No public getters yet (they land in a later commit) — reach into
        // storage.rs directly to confirm the transition.
        let (status, dispute) = env.as_contract(&contract_id, || {
            (
                crate::storage::get_statement(&env, &operator, seq).unwrap().status,
                crate::storage::get_dispute(&env, &operator, seq).unwrap(),
            )
        });
        assert_eq!(status, Status::Disputed);
        assert_eq!(dispute.consumer, consumer);
        assert_eq!(dispute.reason_hash, reason_hash);
        assert_eq!(dispute.amount_credited, 0);
        assert_eq!(dispute.resolution_hash, None);
        assert_eq!(dispute.resolved_ledger, None);
    }

    #[test]
    fn happy_path_emits_dispute_event() {
        let (env, contract_id, operator, consumer, seq) = anchor_statement();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let reason_hash = BytesN::from_array(&env, &[7u8; 32]);

        client.open_dispute(&operator, &seq, &consumer, &reason_hash);

        let expected =
            DisputeEvent { operator, consumer, seq, reason_hash }.to_xdr(&env, &contract_id);
        assert_eq!(env.events().all(), std::vec![expected]);
    }

    #[test]
    fn not_found_for_missing_statement() {
        let (env, contract_id, _admin, _price_book_id) = setup();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let reason_hash = BytesN::from_array(&env, &[7u8; 32]);

        let result = client.try_open_dispute(&operator, &1, &consumer, &reason_hash);
        assert_eq!(result, Err(Ok(Error::NotFound)));
    }

    #[test]
    fn not_found_for_wrong_consumer_does_not_leak_existence() {
        let (env, contract_id, operator, _real_consumer, seq) = anchor_statement();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let impostor = Address::generate(&env);
        let reason_hash = BytesN::from_array(&env, &[7u8; 32]);

        // A statement genuinely exists at (operator, seq), but not for
        // this consumer — must come back identical to a missing statement.
        let result = client.try_open_dispute(&operator, &seq, &impostor, &reason_hash);
        assert_eq!(result, Err(Ok(Error::NotFound)));
    }

    #[test]
    fn not_anchored_when_already_disputed() {
        let (env, contract_id, operator, consumer, seq) = anchor_statement();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let reason_hash = BytesN::from_array(&env, &[7u8; 32]);

        client.open_dispute(&operator, &seq, &consumer, &reason_hash);

        let result = client.try_open_dispute(&operator, &seq, &consumer, &reason_hash);
        assert_eq!(result, Err(Ok(Error::NotAnchored)));
    }

    #[test]
    fn unauthorized_caller_is_rejected() {
        use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
        use soroban_sdk::IntoVal;

        let (env, contract_id, operator, consumer, seq) = anchor_statement();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let attacker = Address::generate(&env);
        let reason_hash = BytesN::from_array(&env, &[7u8; 32]);

        // Authorize attacker, not consumer — open_dispute() requires
        // consumer.require_auth(). Confirmed separately that mock_auths
        // here correctly switches auth enforcement back to strict, even
        // though anchor_statement() used mock_all_auths for the setup call.
        env.mock_auths(&[MockAuth {
            address: &attacker,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "open_dispute",
                args: (operator.clone(), seq, consumer.clone(), reason_hash.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }]);

        let result = client.try_open_dispute(&operator, &seq, &consumer, &reason_hash);
        assert!(result.is_err());
    }
}

mod resolve_dispute {
    use soroban_sdk::testutils::{Events as _, Ledger as _};
    use soroban_sdk::Event as _;

    use crate::event::ResolveEvent;
    use crate::types::{Protocol, Status};
    use crate::StatementRegistryClient;

    use super::*;

    /// Anchors a statement and opens a dispute against it. Returns
    /// (Env, contract_id, operator, consumer, seq), with amount_billed
    /// fixed at 1000 so tests can reason about CreditTooLarge's boundary.
    fn anchor_and_dispute() -> (Env, Address, Address, Address, u64) {
        let (env, contract_id, _admin, price_book_id) = setup();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);
        let usage_root = BytesN::from_array(&env, &[42u8; 32]);

        let base = env.ledger().sequence();
        let version = publish_schedule(&env, &price_book_id, &operator, base);
        env.ledger().set_sequence_number(base + 100);

        let seq = client.anchor(
            &operator,
            &consumer,
            &base,
            &(base + 50),
            &usage_root,
            &10,
            &token,
            &1000i128,
            &900i128,
            &version,
            &Protocol::X402,
            &None,
        );
        let reason_hash = BytesN::from_array(&env, &[7u8; 32]);
        client.open_dispute(&operator, &seq, &consumer, &reason_hash);

        (env, contract_id, operator, consumer, seq)
    }

    #[test]
    fn happy_path_sets_status_resolved_and_writes_dispute() {
        let (env, contract_id, operator, consumer, seq) = anchor_and_dispute();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let resolution_hash = BytesN::from_array(&env, &[9u8; 32]);

        client.resolve_dispute(&operator, &seq, &resolution_hash, &500i128);

        let (status, dispute) = env.as_contract(&contract_id, || {
            (
                crate::storage::get_statement(&env, &operator, seq).unwrap().status,
                crate::storage::get_dispute(&env, &operator, seq).unwrap(),
            )
        });
        assert_eq!(status, Status::Resolved);
        assert_eq!(dispute.consumer, consumer);
        assert_eq!(dispute.resolution_hash, Some(resolution_hash));
        assert_eq!(dispute.amount_credited, 500);
        assert!(dispute.resolved_ledger.is_some());
    }

    #[test]
    fn happy_path_emits_resolve_event() {
        let (env, contract_id, operator, consumer, seq) = anchor_and_dispute();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let resolution_hash = BytesN::from_array(&env, &[9u8; 32]);

        client.resolve_dispute(&operator, &seq, &resolution_hash, &500i128);

        let expected =
            ResolveEvent { operator, consumer, seq, resolution_hash, amount_credited: 500 }
                .to_xdr(&env, &contract_id);
        assert_eq!(env.events().all(), std::vec![expected]);
    }

    #[test]
    fn not_disputed_when_still_anchored() {
        let (env, contract_id, _admin, price_book_id) = setup();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);
        let usage_root = BytesN::from_array(&env, &[42u8; 32]);

        let base = env.ledger().sequence();
        let version = publish_schedule(&env, &price_book_id, &operator, base);
        env.ledger().set_sequence_number(base + 100);

        // Anchored, never disputed.
        let seq = client.anchor(
            &operator,
            &consumer,
            &base,
            &(base + 50),
            &usage_root,
            &10,
            &token,
            &1000i128,
            &900i128,
            &version,
            &Protocol::X402,
            &None,
        );
        let resolution_hash = BytesN::from_array(&env, &[9u8; 32]);

        let result = client.try_resolve_dispute(&operator, &seq, &resolution_hash, &500i128);
        assert_eq!(result, Err(Ok(Error::NotDisputed)));
    }

    #[test]
    fn credit_too_large_negative_is_rejected() {
        let (env, contract_id, operator, _consumer, seq) = anchor_and_dispute();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let resolution_hash = BytesN::from_array(&env, &[9u8; 32]);

        let result = client.try_resolve_dispute(&operator, &seq, &resolution_hash, &(-1i128));
        assert_eq!(result, Err(Ok(Error::CreditTooLarge)));
    }

    #[test]
    fn credit_too_large_exceeds_billed_is_rejected() {
        let (env, contract_id, operator, _consumer, seq) = anchor_and_dispute();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let resolution_hash = BytesN::from_array(&env, &[9u8; 32]);

        // amount_billed was 1000 in anchor_and_dispute().
        let result = client.try_resolve_dispute(&operator, &seq, &resolution_hash, &1001i128);
        assert_eq!(result, Err(Ok(Error::CreditTooLarge)));
    }

    #[test]
    fn single_signer_operator_only_is_rejected() {
        use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
        use soroban_sdk::IntoVal;

        let (env, contract_id, operator, _consumer, seq) = anchor_and_dispute();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let resolution_hash = BytesN::from_array(&env, &[9u8; 32]);

        // Only operator authorized — consumer's own require_auth() has no
        // matching entry.
        env.mock_auths(&[MockAuth {
            address: &operator,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "resolve_dispute",
                args: (operator.clone(), seq, resolution_hash.clone(), 500i128).into_val(&env),
                sub_invokes: &[],
            },
        }]);

        let result = client.try_resolve_dispute(&operator, &seq, &resolution_hash, &500i128);
        assert!(result.is_err());
    }

    #[test]
    fn single_signer_consumer_only_is_rejected() {
        use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
        use soroban_sdk::IntoVal;

        let (env, contract_id, operator, consumer, seq) = anchor_and_dispute();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let resolution_hash = BytesN::from_array(&env, &[9u8; 32]);

        // Only consumer authorized — operator.require_auth() is the very
        // first check in resolve_dispute(), so this fails before the
        // statement is even loaded.
        env.mock_auths(&[MockAuth {
            address: &consumer,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "resolve_dispute",
                args: (operator.clone(), seq, resolution_hash.clone(), 500i128).into_val(&env),
                sub_invokes: &[],
            },
        }]);

        let result = client.try_resolve_dispute(&operator, &seq, &resolution_hash, &500i128);
        assert!(result.is_err());
    }
}

mod extend_statement_ttl {
    use soroban_sdk::testutils::storage::Persistent as _;
    use soroban_sdk::testutils::Ledger as _;

    use crate::storage::DataKey;
    use crate::types::Protocol;
    use crate::StatementRegistryClient;

    use super::*;

    /// Anchors a minimal, otherwise-valid statement and returns
    /// (Env, contract_id, operator, consumer, seq).
    fn anchor_statement() -> (Env, Address, Address, Address, u64) {
        let (env, contract_id, _admin, price_book_id) = setup();
        env.mock_all_auths();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);
        let usage_root = BytesN::from_array(&env, &[42u8; 32]);

        let base = env.ledger().sequence();
        let version = publish_schedule(&env, &price_book_id, &operator, base);
        env.ledger().set_sequence_number(base + 100);

        let seq = client.anchor(
            &operator,
            &consumer,
            &base,
            &(base + 50),
            &usage_root,
            &10,
            &token,
            &1000i128,
            &900i128,
            &version,
            &Protocol::X402,
            &None,
        );
        (env, contract_id, operator, consumer, seq)
    }

    fn statement_ttl(env: &Env, contract_id: &Address, operator: &Address, seq: u64) -> u32 {
        let key = DataKey::Statement(operator.clone(), seq);
        env.as_contract(contract_id, || env.storage().persistent().get_ttl(&key))
    }

    #[test]
    fn no_auth_required_and_extends_statement_ttl() {
        let (env, contract_id, operator, _consumer, seq) = anchor_statement();
        let client = StatementRegistryClient::new(&env, &contract_id);

        // anchor() already extends Statement's TTL out to the fixed
        // BUMP_AMOUNT ceiling, so extend_statement_ttl() with any smaller
        // target would be a correct no-op, not a useful test of growth.
        // Advance the ledger first so the remaining TTL erodes, giving a
        // real gap for the extension to close.
        env.ledger().set_sequence_number(env.ledger().sequence() + 1_000);
        let ttl_before = statement_ttl(&env, &contract_id, &operator, seq);

        // Switch to strict enforcement with zero valid auth entries — the
        // strongest proof that no auth is required, stronger than simply
        // never having called mock_all_auths (which anchor_statement()
        // already used for its own setup and left active).
        env.mock_auths(&[]);
        client.extend_statement_ttl(&operator, &seq, &crate::storage::BUMP_AMOUNT);

        let ttl_after = statement_ttl(&env, &contract_id, &operator, seq);
        assert!(ttl_after > ttl_before);
    }

    #[test]
    fn also_extends_dispute_ttl_when_one_exists() {
        let (env, contract_id, operator, consumer, seq) = anchor_statement();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let reason_hash = BytesN::from_array(&env, &[7u8; 32]);
        env.mock_all_auths();
        client.open_dispute(&operator, &seq, &consumer, &reason_hash);

        env.ledger().set_sequence_number(env.ledger().sequence() + 1_000);
        let dispute_key = DataKey::Dispute(operator.clone(), seq);
        let ttl_before =
            env.as_contract(&contract_id, || env.storage().persistent().get_ttl(&dispute_key));

        env.mock_auths(&[]);
        client.extend_statement_ttl(&operator, &seq, &crate::storage::BUMP_AMOUNT);

        let ttl_after =
            env.as_contract(&contract_id, || env.storage().persistent().get_ttl(&dispute_key));
        assert!(ttl_after > ttl_before);
    }

    #[test]
    fn does_not_error_when_no_dispute_exists() {
        // A statement that was never disputed has no Dispute key at all —
        // extend_dispute_ttl_to() must skip it rather than erroring.
        let (env, contract_id, operator, _consumer, seq) = anchor_statement();
        let client = StatementRegistryClient::new(&env, &contract_id);

        env.mock_auths(&[]);
        client.extend_statement_ttl(&operator, &seq, &1_000_000);
    }

    #[test]
    fn clamped_to_a_sane_maximum() {
        let (env, contract_id, operator, _consumer, seq) = anchor_statement();
        let client = StatementRegistryClient::new(&env, &contract_id);

        env.mock_auths(&[]);
        client.extend_statement_ttl(&operator, &seq, &u32::MAX);

        let ttl_after = statement_ttl(&env, &contract_id, &operator, seq);
        assert!(ttl_after <= crate::storage::BUMP_AMOUNT);
    }

    #[test]
    fn not_found_for_missing_statement() {
        let (env, contract_id, _admin, _price_book_id) = setup();
        let client = StatementRegistryClient::new(&env, &contract_id);
        let operator = Address::generate(&env);

        let result = client.try_extend_statement_ttl(&operator, &1, &1_000_000);
        assert_eq!(result, Err(Ok(Error::NotFound)));
    }
}

mod merkle {
    use soroban_sdk::vec;

    use crate::merkle::fold;

    use super::*;

    #[test]
    fn single_leaf_with_empty_proof_returns_the_leaf_unchanged() {
        let env = Env::default();
        let leaf = BytesN::from_array(&env, &[7u8; 32]);

        let root = fold(&env, leaf.clone(), &vec![&env]);

        assert_eq!(root, leaf);
    }

    #[test]
    fn two_leaf_tree_matches_a_hand_computed_root() {
        let env = Env::default();
        let leaf0 = BytesN::from_array(&env, &[1u8; 32]);
        let leaf1 = BytesN::from_array(&env, &[2u8; 32]);
        let expected_root = hash_pair(&env, &leaf0, &leaf1);

        // Sorted-pair hashing means the root doesn't depend on which leaf
        // the proof starts from.
        assert_eq!(fold(&env, leaf0.clone(), &vec![&env, leaf1.clone()]), expected_root);
        assert_eq!(fold(&env, leaf1, &vec![&env, leaf0]), expected_root);
    }

    #[test]
    fn four_leaf_balanced_tree_matches_a_hand_computed_root() {
        let env = Env::default();
        let leaves: std::vec::Vec<BytesN<32>> =
            (0u8..4).map(|i| BytesN::from_array(&env, &[i + 1; 32])).collect();

        // Build the reference tree bottom-up: two internal nodes, then the
        // root over those.
        let node01 = hash_pair(&env, &leaves[0], &leaves[1]);
        let node23 = hash_pair(&env, &leaves[2], &leaves[3]);
        let expected_root = hash_pair(&env, &node01, &node23);

        let root = fold(&env, leaves[0].clone(), &vec![&env, leaves[1].clone(), node23.clone()]);
        assert_eq!(root, expected_root);

        // Every leaf's proof should fold to the same root.
        let root = fold(&env, leaves[2].clone(), &vec![&env, leaves[3].clone(), node01]);
        assert_eq!(root, expected_root);
    }
}

mod fixtures {
    use serde_json::Value;
    use soroban_sdk::vec;

    use crate::merkle::fold;

    use super::*;

    /// Parses a 64-hex-character digest into a BytesN<32>. Panics on
    /// malformed input — test-only, and a fixture that doesn't parse is a
    /// fixture bug worth failing loudly on.
    fn hex_to_bytesn(env: &Env, hex: &str) -> BytesN<32> {
        let mut bytes = [0u8; 32];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        BytesN::from_array(env, &bytes)
    }

    /// Asserts every (leaf, proof) pair in the fixture folds to its root.
    fn assert_fixture_folds_to_root(json: &str) {
        let env = Env::default();
        let parsed: Value = serde_json::from_str(json).unwrap();
        let root = hex_to_bytesn(&env, parsed["root"].as_str().unwrap());

        for entry in parsed["proofs"].as_array().unwrap() {
            let leaf = hex_to_bytesn(&env, entry["leaf"].as_str().unwrap());
            let mut proof = vec![&env];
            for node in entry["proof"].as_array().unwrap() {
                proof.push_back(hex_to_bytesn(&env, node.as_str().unwrap()));
            }
            assert_eq!(fold(&env, leaf, &proof), root);
        }
    }

    #[test]
    fn single_leaf_fixture_folds_to_its_root() {
        assert_fixture_folds_to_root(include_str!("../../../fixtures/merkle/single-leaf.json"));
    }

    #[test]
    fn four_leaves_fixture_folds_to_its_root() {
        assert_fixture_folds_to_root(include_str!("../../../fixtures/merkle/four-leaves.json"));
    }

    #[test]
    fn seven_leaves_fixture_folds_to_its_root() {
        // The unbalanced tree: leaf index 6 is the odd one out at the first
        // level and gets a shorter proof than the rest — exactly the case
        // sorted-pair implementations tend to diverge on.
        assert_fixture_folds_to_root(include_str!("../../../fixtures/merkle/seven-leaves.json"));
    }

    #[test]
    fn corrupted_proof_node_does_not_reach_the_root() {
        let env = Env::default();
        let parsed: Value =
            serde_json::from_str(include_str!("../../../fixtures/merkle/four-leaves.json"))
                .unwrap();
        let root = hex_to_bytesn(&env, parsed["root"].as_str().unwrap());
        let entry = &parsed["proofs"][0];
        let leaf = hex_to_bytesn(&env, entry["leaf"].as_str().unwrap());

        let mut proof = vec![&env];
        for (i, node) in entry["proof"].as_array().unwrap().iter().enumerate() {
            let mut node_bytes = hex_to_bytesn(&env, node.as_str().unwrap()).to_array();
            if i == 0 {
                // Flip a bit in the first proof node — same shape as a
                // valid proof, wrong content.
                node_bytes[0] ^= 0xff;
            }
            proof.push_back(BytesN::from_array(&env, &node_bytes));
        }

        assert_ne!(fold(&env, leaf, &proof), root);
    }
}

mod price_book_import {
    use soroban_sdk::String;

    use super::*;

    /// Registers the *real* compiled price_book.wasm — never a mock — and
    /// drives it through the generated client, proving the contractimport!
    /// wiring actually works end to end: publish a schedule, then read it
    /// back via both get_version and version_at.
    #[test]
    fn real_wasm_registers_and_answers_queries() {
        let env = Env::default();
        env.mock_all_auths();

        let price_book_admin = Address::generate(&env);
        let price_book_id = env.register(crate::price_book::WASM, (price_book_admin,));
        let price_book_client = crate::price_book::Client::new(&env, &price_book_id);

        let operator = Address::generate(&env);
        let schedule_hash = BytesN::from_array(&env, &[9u8; 32]);
        let uri = String::from_str(&env, "https://example.com/schedule.json");
        let effective_ledger = env.ledger().sequence();

        let version = price_book_client.publish(&operator, &schedule_hash, &uri, &effective_ledger);
        assert_eq!(version, 1);

        let fetched = price_book_client.get_version(&operator, &version);
        assert_eq!(fetched.schedule_hash, schedule_hash);

        assert_eq!(price_book_client.version_at(&operator, &effective_ledger), version);
    }
}

/// End-to-end walk through the full user journey, both contracts, one Env:
/// publish a schedule, anchor a statement against it, verify a leaf,
/// dispute, resolve. Uses only the public client API (no reaching into
/// storage.rs) since this is meant to read as what a real integrator does.
mod integration {
    use soroban_sdk::testutils::{Events as _, Ledger as _};
    use soroban_sdk::{vec, Event as _, String};

    use crate::event::{AnchorEvent, DisputeEvent, ResolveEvent};
    use crate::types::{Protocol, Status};
    use crate::StatementRegistryClient;

    use super::*;

    #[test]
    fn publish_anchor_verify_dispute_resolve() {
        let (env, contract_id, _admin, price_book_id) = setup();
        env.mock_all_auths();
        let statement_registry = StatementRegistryClient::new(&env, &contract_id);
        let price_book = crate::price_book::Client::new(&env, &price_book_id);

        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);

        // 1. Publish a schedule.
        let schedule_hash = BytesN::from_array(&env, &[3u8; 32]);
        let uri = String::from_str(&env, "https://example.com/schedule.json");
        let base = env.ledger().sequence();
        let version = price_book.publish(&operator, &schedule_hash, &uri, &base);
        assert_eq!(version, 1);
        env.ledger().set_sequence_number(base + 100);

        // 2. Build a real two-leaf merkle tree for the usage root — not a
        // placeholder value, so the verify step below actually exercises
        // merkle::fold against a genuine root.
        let leaf0 = BytesN::from_array(&env, &[10u8; 32]);
        let leaf1 = BytesN::from_array(&env, &[20u8; 32]);
        let usage_root = hash_pair(&env, &leaf0, &leaf1);

        // 3. Anchor a statement against the published schedule.
        let period_start = base;
        let period_end = base + 50;
        let seq = statement_registry.anchor(
            &operator,
            &consumer,
            &period_start,
            &period_end,
            &usage_root,
            &10,
            &token,
            &1000i128,
            &900i128,
            &version,
            &Protocol::X402,
            &None,
        );
        assert_eq!(seq, 1);
        let anchor_event = AnchorEvent {
            operator: operator.clone(),
            consumer: consumer.clone(),
            seq,
            usage_root: usage_root.clone(),
            amount_billed: 1000,
            amount_settled: 900,
            protocol: Protocol::X402,
        }
        .to_xdr(&env, &contract_id);
        assert_eq!(env.events().all(), std::vec![anchor_event]);

        let statement = statement_registry.get_statement(&operator, &seq);
        assert_eq!(statement.status, Status::Anchored);
        assert_eq!(statement.usage_root, usage_root);
        assert_eq!(statement_registry.list_statements(&operator, &consumer), vec![&env, seq]);

        // 4. A buyer verifies one charge against the anchored root.
        let verified =
            statement_registry.verify_usage(&operator, &seq, &leaf0, &vec![&env, leaf1.clone()]);
        assert!(verified);
        // A leaf that was never in the tree does not verify.
        let not_in_tree = BytesN::from_array(&env, &[99u8; 32]);
        assert!(!statement_registry.verify_usage(
            &operator,
            &seq,
            &not_in_tree,
            &vec![&env, leaf1]
        ));

        // 5. The buyer disputes the statement.
        let reason_hash = BytesN::from_array(&env, &[7u8; 32]);
        statement_registry.open_dispute(&operator, &seq, &consumer, &reason_hash);
        let dispute_event = DisputeEvent {
            operator: operator.clone(),
            consumer: consumer.clone(),
            seq,
            reason_hash,
        }
        .to_xdr(&env, &contract_id);
        assert_eq!(env.events().all(), std::vec![dispute_event]);
        assert_eq!(statement_registry.get_statement(&operator, &seq).status, Status::Disputed);

        // 6. Both parties resolve the dispute.
        let resolution_hash = BytesN::from_array(&env, &[8u8; 32]);
        statement_registry.resolve_dispute(&operator, &seq, &resolution_hash, &400i128);
        let resolve_event = ResolveEvent {
            operator: operator.clone(),
            consumer: consumer.clone(),
            seq,
            resolution_hash: resolution_hash.clone(),
            amount_credited: 400,
        }
        .to_xdr(&env, &contract_id);
        assert_eq!(env.events().all(), std::vec![resolve_event]);

        let final_statement = statement_registry.get_statement(&operator, &seq);
        assert_eq!(final_statement.status, Status::Resolved);

        let dispute = statement_registry.get_dispute(&operator, &seq);
        assert_eq!(dispute.resolution_hash, Some(resolution_hash));
        assert_eq!(dispute.amount_credited, 400);
        assert!(dispute.resolved_ledger.is_some());
    }
}

/// The record -> leaf derivation, which is the half of the merkle format
/// the `fixtures/merkle/*-leaves.json` trees never touch: those fixtures
/// start from leaves that are already hashes, so they prove `fold` agrees
/// across repos while saying nothing about how a billed request becomes a
/// leaf in the first place. `fixtures/merkle/records.json` covers that
/// half, and the collector in the `tallybook` repo tests its own encoder
/// (`internal/merkle/leaf.go`) against the same file.
mod leaf {
    use super::*;
    use crate::leaf::{leaf_hash, UsageRecord};
    use std::{format, fs, string::String as StdString, vec, vec::Vec as StdVec};

    /// Where the fixture lives, relative to this crate rather than to the
    /// working directory a test happens to be run from.
    const FIXTURE_PATH: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/merkle/records.json");

    fn hex(bytes: &[u8]) -> StdString {
        let mut out = StdString::with_capacity(bytes.len() * 2);
        for b in bytes {
            out.push_str(&format!("{b:02x}"));
        }
        out
    }

    /// One vector's inputs, in the form a collector would have them before
    /// encoding: a method and path template rather than an endpoint hash.
    struct Vector {
        label: &'static str,
        method: &'static str,
        path: &'static str,
        amount: i128,
        consumer: &'static str,
        ledger: u32,
        price_v: u32,
        reqid: [u8; 32],
        units: u64,
    }

    /// Fixed addresses, derived from fixed 32-byte payloads (0x11.., 0x22..,
    /// 0x33..) so the strkeys are reproducible from the raw bytes on either
    /// side of the repo boundary rather than copied on trust.
    const ACCOUNT_A: &str = "GAIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCF6M";
    const ACCOUNT_B: &str = "GARCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCFRVX";
    const CHANNEL_CONTRACT: &str = "CAZTGMZTGMZTGMZTGMZTGMZTGMZTGMZTGMZTGMZTGMZTGMZTGMZTGGJH";

    fn vectors() -> StdVec<Vector> {
        vec![
            // The floor case: one unit, one stroop, lowest sensible ledger.
            Vector {
                label: "minimal",
                method: "GET",
                path: "/v1/search",
                amount: 1,
                consumer: ACCOUNT_A,
                ledger: 1,
                price_v: 1,
                reqid: [0u8; 32],
                units: 1,
            },
            // What the overwhelming majority of real records look like:
            // a single request at 0.01 USDC, which is 100_000 in USDC's
            // seven-decimal smallest unit.
            Vector {
                label: "typical x402 request at 0.01 usdc",
                method: "GET",
                path: "/v1/search",
                amount: 100_000,
                consumer: ACCOUNT_A,
                ledger: 58_123_456,
                price_v: 3,
                reqid: [0xab; 32],
                units: 1,
            },
            // A negative amount: a credit line on a resolved dispute. This
            // is the vector that catches a two's-complement bug in an
            // i128 encoder, which is the most likely place for the two
            // implementations to diverge without anyone noticing.
            Vector {
                label: "credit line, negative amount",
                method: "POST",
                path: "/v1/embed",
                amount: -100_000,
                consumer: ACCOUNT_B,
                ledger: 58_123_457,
                price_v: 3,
                reqid: [0x01; 32],
                units: 1,
            },
            // MPP session mode, where `consumer` is the channel contract
            // (`C…`) and not an account (`G…`). An encoder that only
            // handles account strkeys fails outright on this one.
            Vector {
                label: "mpp session, channel contract consumer",
                method: "GET",
                path: "/v1/stream",
                amount: 250,
                consumer: CHANNEL_CONTRACT,
                ledger: 58_200_000,
                price_v: 4,
                reqid: [0x7f; 32],
                units: 5,
            },
            // Both ends of the i128 range, plus the u32 and u64 ceilings,
            // so the Hi/Lo split of an i128 and the full width of every
            // other field are pinned rather than assumed.
            Vector {
                label: "i128 max and field ceilings",
                method: "GET",
                path: "/v1/search",
                amount: i128::MAX,
                consumer: ACCOUNT_A,
                ledger: u32::MAX,
                price_v: u32::MAX,
                reqid: [0xff; 32],
                units: u64::MAX,
            },
            Vector {
                label: "i128 min and field floors",
                method: "GET",
                path: "/v1/search",
                amount: i128::MIN,
                consumer: ACCOUNT_B,
                ledger: 0,
                price_v: 0,
                reqid: [0x00; 32],
                units: 0,
            },
        ]
    }

    /// `sha256(method || " " || path_template)`, the record's `endpoint`
    /// field. The collector derives the same value for its pricing lookup,
    /// so it is pinned here too rather than left to the caller.
    fn endpoint_hash(env: &Env, method: &str, path: &str) -> BytesN<32> {
        let mut bytes = Bytes::new(env);
        bytes.extend_from_slice(method.as_bytes());
        bytes.extend_from_slice(b" ");
        bytes.extend_from_slice(path.as_bytes());
        env.crypto().sha256(&bytes).to_bytes()
    }

    fn rendered_fixture(env: &Env) -> StdString {
        let mut records = StdVec::new();
        for v in vectors() {
            let endpoint = endpoint_hash(env, v.method, v.path);
            let record = UsageRecord {
                amount: v.amount,
                consumer: Address::from_str(env, v.consumer),
                endpoint: endpoint.clone(),
                ledger: v.ledger,
                price_v: v.price_v,
                reqid: BytesN::from_array(env, &v.reqid),
                units: v.units,
            };
            // `record_bytes` is carried alongside the leaf so a mismatch
            // on the other side of the repo boundary localises itself: if
            // record_bytes differs the encoding drifted, if only the leaf
            // differs the hashing did.
            let record_bytes = record.clone().to_xdr(env);
            let mut raw = std::vec![0u8; record_bytes.len() as usize];
            record_bytes.copy_into_slice(&mut raw);

            records.push(serde_json::json!({
                "label": v.label,
                "endpoint_method": v.method,
                "endpoint_path": v.path,
                "amount": format!("{}", v.amount),
                "consumer": v.consumer,
                "endpoint": hex(&endpoint.to_array()),
                "ledger": v.ledger,
                "price_v": v.price_v,
                "reqid": hex(&v.reqid),
                "units": v.units,
                "record_bytes": hex(&raw),
                "leaf": hex(&leaf_hash(env, &record).to_array()),
            }));
        }

        let doc = serde_json::json!({
            "format": "XDR(ScVal::Map) over symbol keys sorted alphabetically, then leaf = sha256(sha256(record_bytes))",
            "records": records,
        });
        serde_json::to_string_pretty(&doc).expect("serialize fixture")
    }

    /// Regenerate with `TALLYBOOK_WRITE_FIXTURES=1 cargo test -p
    /// statement-registry record_vectors`. Without that variable the test
    /// only compares, so the committed file cannot drift from the code
    /// that produced it without CI failing.
    #[test]
    fn record_vectors_match_the_committed_fixture() {
        let env = Env::default();
        let rendered = rendered_fixture(&env);

        if std::env::var("TALLYBOOK_WRITE_FIXTURES").is_ok() {
            fs::write(FIXTURE_PATH, format!("{rendered}\n")).expect("write fixture");
            return;
        }

        let committed = fs::read_to_string(FIXTURE_PATH).expect("read fixture");
        assert_eq!(
            committed.trim_end(),
            rendered.trim_end(),
            "fixtures/merkle/records.json is out of sync with the leaf derivation that produced it"
        );
    }

    /// The leaf format's double hash is what keeps a leaf out of the
    /// folding domain, so it is asserted directly rather than inferred
    /// from the fixture: a leaf must be sha256 of the sha256 of the
    /// record, never the single hash.
    #[test]
    fn leaf_is_the_double_hash_of_the_record_bytes() {
        let env = Env::default();
        let record = UsageRecord {
            amount: 100_000,
            consumer: Address::from_str(&env, ACCOUNT_A),
            endpoint: endpoint_hash(&env, "GET", "/v1/search"),
            ledger: 58_123_456,
            price_v: 3,
            reqid: BytesN::from_array(&env, &[0xab; 32]),
            units: 1,
        };

        let record_bytes = record.clone().to_xdr(&env);
        let single = env.crypto().sha256(&record_bytes).to_bytes();
        let double = env.crypto().sha256(&Bytes::from(single.clone())).to_bytes();

        assert_eq!(leaf_hash(&env, &record), double);
        assert_ne!(leaf_hash(&env, &record), single);
    }
}

mod verify_charge {
    use soroban_sdk::testutils::Ledger as _;
    use soroban_sdk::{vec, Vec as SorobanVec};
    use std::vec::Vec as StdVec;

    use crate::leaf::{leaf_hash, UsageRecord};
    use crate::types::Protocol;
    use crate::{StatementRegistry, StatementRegistryClient};

    use super::*;

    /// A statement anchored over a real four-leaf tree of derived records,
    /// with everything a verify_charge call needs to be checked against it.
    struct Anchored {
        env: Env,
        contract_id: Address,
        operator: Address,
        seq: u64,
        version: u32,
        period_start: u32,
        period_end: u32,
        records: StdVec<UsageRecord>,
        proofs: StdVec<SorobanVec<BytesN<32>>>,
    }

    /// Folds four leaves into a balanced tree and returns its root plus one
    /// proof per leaf. Built with `hash_pair`, the reference pair hash, so
    /// the tree under test never shares logic with `merkle::fold`.
    fn four_leaf_tree(
        env: &Env,
        leaves: &[BytesN<32>; 4],
    ) -> (BytesN<32>, StdVec<SorobanVec<BytesN<32>>>) {
        let left = hash_pair(env, &leaves[0], &leaves[1]);
        let right = hash_pair(env, &leaves[2], &leaves[3]);
        let root = hash_pair(env, &left, &right);
        let proofs = std::vec![
            vec![env, leaves[1].clone(), right.clone()],
            vec![env, leaves[0].clone(), right],
            vec![env, leaves[3].clone(), left.clone()],
            vec![env, leaves[2].clone(), left],
        ];
        (root, proofs)
    }

    /// Anchors a statement over four records billed to one consumer inside
    /// one price version's window — the shape an honest period has.
    fn anchored() -> Anchored {
        let env = Env::default();
        let admin = Address::generate(&env);
        let price_book_admin = Address::generate(&env);
        let price_book_id = env.register(crate::price_book::WASM, (price_book_admin,));
        let contract_id = env.register(StatementRegistry, (admin, price_book_id.clone()));
        env.mock_all_auths();

        let operator = Address::generate(&env);
        let consumer = Address::generate(&env);
        let token = Address::generate(&env);

        // Start the period off ledger 0 so a test can name a ledger
        // before period_start without underflowing.
        env.ledger().set_sequence_number(1_000);
        let period_start = env.ledger().sequence();
        let version = publish_schedule(&env, &price_book_id, &operator, period_start);
        let period_end = period_start + 50;
        env.ledger().set_sequence_number(period_end + 10);

        let mut records = StdVec::new();
        for i in 0u8..4 {
            records.push(UsageRecord {
                amount: 100_000 + i128::from(i),
                consumer: consumer.clone(),
                endpoint: BytesN::from_array(&env, &[0x10 + i; 32]),
                ledger: period_start + 1 + u32::from(i),
                price_v: version,
                reqid: BytesN::from_array(&env, &[0x20 + i; 32]),
                units: 1,
            });
        }
        let leaves = [
            leaf_hash(&env, &records[0]),
            leaf_hash(&env, &records[1]),
            leaf_hash(&env, &records[2]),
            leaf_hash(&env, &records[3]),
        ];
        let (root, proofs) = four_leaf_tree(&env, &leaves);

        let client = StatementRegistryClient::new(&env, &contract_id);
        let seq = client.anchor(
            &operator,
            &consumer,
            &period_start,
            &period_end,
            &root,
            &4,
            &token,
            &400_006i128,
            &400_006i128,
            &version,
            &Protocol::X402,
            &None,
        );

        Anchored {
            env,
            contract_id,
            operator,
            seq,
            version,
            period_start,
            period_end,
            records,
            proofs,
        }
    }

    #[test]
    fn happy_path_verifies_every_record_in_the_tree() {
        let a = anchored();
        let client = StatementRegistryClient::new(&a.env, &a.contract_id);

        for (i, record) in a.records.iter().enumerate() {
            assert!(
                client.verify_charge(&a.operator, &a.seq, record, &a.proofs[i]),
                "record {i} should verify against the anchored root"
            );
        }
    }

    #[test]
    fn requires_no_auth() {
        let a = anchored();
        let client = StatementRegistryClient::new(&a.env, &a.contract_id);

        // Strict enforcement with zero valid auth entries: a buyer
        // checking a charge is not a party to anything and must not need
        // to be.
        a.env.mock_auths(&[]);
        assert!(client.verify_charge(&a.operator, &a.seq, &a.records[0], &a.proofs[0]));
    }

    #[test]
    fn a_record_the_contract_derives_differently_returns_false() {
        let a = anchored();
        let client = StatementRegistryClient::new(&a.env, &a.contract_id);

        // Consistent with the statement on every bound field — same
        // consumer, same price version, a ledger inside the period — but
        // the amount was altered, so the leaf the contract derives is not
        // the leaf that went into the tree. This is the case the old
        // verify_usage could not catch: handed a leaf computed from the
        // altered amount it would have folded it happily, and handed the
        // honest leaf it would have returned true while the caller read
        // the altered amount off the invoice.
        let mut tampered = a.records[0].clone();
        tampered.amount += 1;
        assert!(!client.verify_charge(&a.operator, &a.seq, &tampered, &a.proofs[0]));
    }

    #[test]
    fn a_record_from_another_leaf_position_returns_false() {
        let a = anchored();
        let client = StatementRegistryClient::new(&a.env, &a.contract_id);
        assert!(!client.verify_charge(&a.operator, &a.seq, &a.records[0], &a.proofs[1]));
    }

    #[test]
    fn record_consumer_mismatch_is_rejected() {
        let a = anchored();
        let client = StatementRegistryClient::new(&a.env, &a.contract_id);

        let mut other = a.records[0].clone();
        other.consumer = Address::generate(&a.env);
        assert_eq!(
            client.try_verify_charge(&a.operator, &a.seq, &other, &a.proofs[0]),
            Err(Ok(Error::RecordConsumerMismatch))
        );
    }

    #[test]
    fn record_price_mismatch_is_rejected() {
        let a = anchored();
        let client = StatementRegistryClient::new(&a.env, &a.contract_id);

        let mut other = a.records[0].clone();
        other.price_v = a.version + 1;
        assert_eq!(
            client.try_verify_charge(&a.operator, &a.seq, &other, &a.proofs[0]),
            Err(Ok(Error::RecordPriceMismatch))
        );
    }

    #[test]
    fn record_before_period_start_is_rejected() {
        let a = anchored();
        let client = StatementRegistryClient::new(&a.env, &a.contract_id);

        let mut other = a.records[0].clone();
        other.ledger = a.period_start - 1;
        assert_eq!(
            client.try_verify_charge(&a.operator, &a.seq, &other, &a.proofs[0]),
            Err(Ok(Error::RecordOutsidePeriod))
        );
    }

    #[test]
    fn record_after_period_end_is_rejected() {
        let a = anchored();
        let client = StatementRegistryClient::new(&a.env, &a.contract_id);

        let mut other = a.records[0].clone();
        other.ledger = a.period_end + 1;
        assert_eq!(
            client.try_verify_charge(&a.operator, &a.seq, &other, &a.proofs[0]),
            Err(Ok(Error::RecordOutsidePeriod))
        );
    }

    #[test]
    fn a_record_on_the_period_boundaries_is_accepted() {
        let a = anchored();
        let client = StatementRegistryClient::new(&a.env, &a.contract_id);

        // period_start and period_end are both inclusive, so a record on
        // either boundary is in the period. These two fail the membership
        // check rather than the bounds check — Ok(false), not an error —
        // which is exactly how an in-period record that isn't in the tree
        // should read.
        for ledger in [a.period_start, a.period_end] {
            let mut edge = a.records[0].clone();
            edge.ledger = ledger;
            assert_eq!(
                client.try_verify_charge(&a.operator, &a.seq, &edge, &a.proofs[0]),
                Ok(Ok(false))
            );
        }
    }

    #[test]
    fn proof_too_long_is_rejected() {
        let a = anchored();
        let client = StatementRegistryClient::new(&a.env, &a.contract_id);

        let mut proof = vec![&a.env];
        for _ in 0..=crate::storage::MAX_PROOF_NODES {
            proof.push_back(BytesN::from_array(&a.env, &[0u8; 32]));
        }
        assert_eq!(
            client.try_verify_charge(&a.operator, &a.seq, &a.records[0], &proof),
            Err(Ok(Error::ProofTooLong))
        );
    }

    #[test]
    fn not_found_for_missing_statement() {
        let a = anchored();
        let client = StatementRegistryClient::new(&a.env, &a.contract_id);
        assert_eq!(
            client.try_verify_charge(&a.operator, &(a.seq + 1), &a.records[0], &a.proofs[0]),
            Err(Ok(Error::NotFound))
        );
    }
}
