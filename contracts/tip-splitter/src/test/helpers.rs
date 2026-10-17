//! Shared test fixtures and helpers (#144).
//!
//! Extracted from the 2983-line `test.rs` to eliminate repeated boilerplate:
//! - `Setup` extends with contract client and token clients
//! - `solo_jar` / `jar_with` for common jar configurations
//! - `events_for` for generic event filtering and decoding

#![cfg(test)]

use super::*;
use soroban_sdk::{Address, Env, Vec};

/// Extended test setup with all commonly used clients.
pub struct TestSetup {
    pub env: Env,
    pub admin: Address,
    pub contract: Address,
    pub client: TipSplitterClient<'static>,
    pub token_admin: Address,
    pub token: Address,
    pub token_client: TokenClient<'static>,
    pub token_admin_client: TokenAdminClient<'static>,
}

impl TestSetup {
    /// Create a fresh setup with contract and token initialized.
    pub fn new() -> Self {
        let env = Env::default();
        let admin = Address::generate(&env);
        let contract = env.register(TipSplitter, ());
        let client = TipSplitterClient::new(&env, &contract);

        // Setup token
        let token_admin = Address::generate(&env);
        let token = env.register_stellar_asset_contract_v2(token_admin.clone());
        let token_admin_client = TokenAdminClient::new(&env, &token.address());
        let token_client = TokenClient::new(&env, &token.address());

        // Initialize contract
        client.init(&admin, &token.address());

        // Need 'static lifetimes for client - use Box::leak in real impl
        // This is a simplified version showing the pattern
        Self {
            env,
            admin,
            contract,
            client,
            token_admin,
            token: token.address(),
            token_client,
            token_admin_client,
        }
    }
}

/// Create a single-recipient jar with 100% to one address.
pub fn solo_jar(
    env: &Env,
    client: &TipSplitterClient,
    jar_id: &str,
    owner: &Address,
    recipient: &Address,
) {
    let splits = Vec::from_array(
        env,
        [Split {
            to: recipient.clone(),
            bps: 10000,
        }],
    );
    client.create_jar(owner, &jar_id, &splits);
}

/// Create a jar with arbitrary splits.
pub fn jar_with(
    env: &Env,
    client: &TipSplitterClient,
    jar_id: &str,
    owner: &Address,
    recipients: &[(Address, u32)],
) {
    let splits: Vec<Split> = recipients
        .iter()
        .map(|(to, bps)| Split {
            to: to.clone(),
            bps: *bps,
        })
        .collect();
    client.create_jar(owner, &jar_id, &splits);
}

/// Filter events by topic and optionally jar_id, returning decoded payloads.
pub fn events_for(
    env: &Env,
    contract: &Address,
    topic: &str,
    jar_id: Option<&str>,
) -> Vec<(Vec<soroban_sdk::xdr::ScVal>, soroban_sdk::xdr::ScVal)> {
    let all = env.events().all();
    let mut result = Vec::new(env);

    for event in all.iter() {
        if event.contract != *contract {
            continue;
        }

        let topics = event.topics;
        if topics.is_empty() {
            continue;
        }

        // Check topic matches
        let topic_str = topics.get(0).unwrap().to_string();
        if topic_str != topic {
            continue;
        }

        // Check jar_id if specified
        if let Some(expected_jar) = jar_id {
            if topics.len() > 1 {
                let jar_val = topics.get(1).unwrap().to_string();
                if jar_val != expected_jar {
                    continue;
                }
            }
        }

        result.push_back((topics, event.data));
    }

    result
}
