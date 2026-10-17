//! Wasm smoke test (#143).
//!
//! Runs tests against the compiled wasm instead of the native build.
//! Requires `tip_splitter.wasm` to be built first:
//!
//! ```bash
//! cargo build --target wasm32v1-none --release
//! cargo test --features wasm-test
//! ```

#![cfg(all(test, feature = "wasm-test"))]

use super::*;
use soroban_sdk::testutils::{Address as _, AuthorizedFunction, Events};
use soroban_sdk::{Address, Env, Vec};

/// Import the compiled wasm.
/// This requires `tip_splitter.wasm` to exist in the target directory.
mod wasm {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/tip_splitter.wasm");
}

fn setup_wasm() -> (
    Env,
    Address,
    Address,
    wasm::Client<'static>,
    Address,
    wasm::TokenClient<'static>,
) {
    let env = Env::default();

    // Register the compiled wasm
    let contract = env.register(wasm::Wasm, ());
    let client = wasm::Client::new(&env, &contract);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_client = wasm::TokenClient::new(&env, &token.address());

    client.init(&admin, &token.address());

    // Leak to get 'static lifetime for test convenience
    let env = Box::leak(Box::new(env));
    let client = Box::leak(Box::new(client));

    (
        env.clone(),
        admin,
        contract,
        client.clone(),
        token.address(),
        token_client.clone(),
    )
}

#[test]
fn wasm_create_tip_and_read() {
    let (env, admin, contract, client, token, token_client) = setup_wasm();

    // Create a two-recipient jar
    let owner = Address::generate(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    let splits = Vec::from_array(
        &env,
        [
            wasm::Split {
                to: alice.clone(),
                bps: 7000,
            },
            wasm::Split {
                to: bob.clone(),
                bps: 3000,
            },
        ],
    );

    client.create_jar(&owner, &"test-jar", &splits);

    // Fund the tipper
    let tipper = Address::generate(&env);
    token_admin_client.mint(&tipper, &1000000);

    // Tip
    client.tip(&tipper, &"test-jar", &1000, &Some("great work".into()));

    // Check balances
    assert_eq!(token_client.balance(&alice), 700);
    assert_eq!(token_client.balance(&bob), 300);

    // Check the tip event
    let events = env.events().all();
    assert!(!events.is_empty());

    // Verify tip event exists
    let tip_events: Vec<_> = events
        .iter()
        .filter(|e| e.topics.get(0).unwrap().to_string() == "tip")
        .collect();
    assert_eq!(tip_events.len(), 1);
}

#[test]
fn wasm_validation_error_splits_empty() {
    let (env, admin, contract, client, token, token_client) = setup_wasm();

    let owner = Address::generate(&env);
    let empty_splits = Vec::new(&env);

    // Should fail with SplitsEmpty (code 10)
    let result = client.try_create_jar(&owner, &"empty-jar", &empty_splits);
    assert!(result.is_err());
}

#[test]
fn wasm_auth_failure_not_owner() {
    let (env, admin, contract, client, token, token_client) = setup_wasm();

    let owner = Address::generate(&env);
    let stranger = Address::generate(&env);

    let splits = Vec::from_array(
        &env,
        [wasm::Split {
            to: Address::generate(&env),
            bps: 10000,
        }],
    );

    client.create_jar(&owner, &"auth-jar", &splits);

    // Non-owner tries to update splits - should fail with auth error
    let result = client.try_update_splits(&stranger, &"auth-jar", &splits);
    assert!(result.is_err());
}
