#![cfg(test)]
use super::*;
use soroban_sdk::testutils::{Address as _, MockAuth, MockAuthInvoke};
use soroban_sdk::{token, Address, Env, IntoVal, String};
use tip_splitter::{TipSplitter, TipSplitterClient};

struct Setup {
    env: Env,
    /// Account that may trigger forwarded tips from the forwarder's balance.
    owner: Address,
    token: Address,
    splitter: Address,
    forwarder: Address,
}

/// Fresh env with the real `TipSplitter`, the forwarder, and a test SAC. The
/// forwarder is funded with 1 000 units of the token.
fn setup() -> Setup {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let token = sac.address();

    // The real contract under its real interface — registered side by side,
    // so no built wasm is involved anywhere in these tests.
    let splitter = env.register(TipSplitter, (admin.clone(), token.clone()));

    let owner = Address::generate(&env);
    let forwarder = env.register(TipForwarder, (owner.clone(), splitter.clone()));

    token::StellarAssetClient::new(&env, &token).mint(&forwarder, &1_000);

    Setup {
        env,
        owner,
        token,
        splitter,
        forwarder,
    }
}

/// Register `jar_id` on the splitter with the given (recipient, bps) pairs.
fn create_jar(s: &Setup, jar_id: &str, splits: &[(Address, u32)]) {
    let client = TipSplitterClient::new(&s.env, &s.splitter);
    let jar_owner = Address::generate(&s.env);
    let vec = {
        let mut v = Vec::new(&s.env);
        for (to, bps) in splits {
            v.push_back(tip_splitter::Split {
                to: to.clone(),
                bps: *bps,
            });
        }
        v
    };
    client.create_jar(&jar_owner, &String::from_str(&s.env, jar_id), &vec);
}

/// Strict-mode authorization: only `owner` signing a call of `fn_name` on the
/// forwarder. The transfers the forwarder then makes must be authorized by
/// the contract itself (`authorize_as_current_contract`), which is what this
/// example is demonstrating.
fn mock_owner_auth(s: &Setup, fn_name: &str, jar_id: &str, amount: i128, message: &String) {
    s.env.mock_auths(&[MockAuth {
        address: &s.owner,
        invoke: &MockAuthInvoke {
            contract: &s.forwarder,
            fn_name,
            args: (String::from_str(&s.env, jar_id), amount, message.clone()).into_val(&s.env),
            sub_invokes: &[],
        },
    }]);
}

#[test]
fn forwarded_tip_pays_the_preview_amounts() {
    let s = setup();
    let env = &s.env;
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    create_jar(&s, "@band", &[(alice.clone(), 7000), (bob.clone(), 3000)]);

    let message = String::from_str(env, "great show");
    mock_owner_auth(&s, "forward_tip", "@band", 100, &message);

    TipForwarderClient::new(env, &s.forwarder).forward_tip(
        &String::from_str(env, "@band"),
        &100,
        &message,
    );

    // 70/30 is both what preview_split returned and what was authorized.
    let token = token::Client::new(env, &s.token);
    assert_eq!(token.balance(&alice), 70);
    assert_eq!(token.balance(&bob), 30);
    assert_eq!(token.balance(&s.forwarder), 900);
}

#[test]
fn naive_authorization_fails_when_the_forwarder_is_a_recipient() {
    let s = setup();
    let env = &s.env;
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    // The forwarder holds a 20 % slice of its own jar. `tip` will skip the
    // self-transfer and roll that share into bob, so the amounts it actually
    // transfers — alice 600, bob 400 — differ from the preview [600, 200, 200].
    create_jar(
        &s,
        "@selfjar",
        &[
            (alice.clone(), 6000),
            (s.forwarder.clone(), 2000),
            (bob.clone(), 2000),
        ],
    );

    let message = String::from_str(env, "from me to me");
    mock_owner_auth(&s, "forward_tip_naive", "@selfjar", 1_000, &message);

    let res = TipForwarderClient::new(env, &s.forwarder).try_forward_tip_naive(
        &String::from_str(env, "@selfjar"),
        &1_000,
        &message,
    );
    assert!(
        res.is_err(),
        "authorizing the raw preview amounts must fail once tip skips the self-transfer"
    );

    // Nothing moved: the tip reverted with the auth error.
    let token = token::Client::new(env, &s.token);
    assert_eq!(token.balance(&alice), 0);
    assert_eq!(token.balance(&bob), 0);
    assert_eq!(token.balance(&s.forwarder), 1_000);
}

#[test]
fn adjusted_authorization_pays_when_the_forwarder_is_a_recipient() {
    let s = setup();
    let env = &s.env;
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    create_jar(
        &s,
        "@selfjar",
        &[
            (alice.clone(), 6000),
            (s.forwarder.clone(), 2000),
            (bob.clone(), 2000),
        ],
    );

    let message = String::from_str(env, "skimming myself");
    mock_owner_auth(&s, "forward_tip", "@selfjar", 1_000, &message);

    TipForwarderClient::new(env, &s.forwarder).forward_tip(
        &String::from_str(env, "@selfjar"),
        &1_000,
        &message,
    );

    // forward_tip authorizes the corrected list: alice 600, bob 400 (his 200
    // plus the withheld self-transfer). The forwarder pays itself nothing.
    let token = token::Client::new(env, &s.token);
    assert_eq!(token.balance(&alice), 600);
    assert_eq!(token.balance(&bob), 400);
    assert_eq!(token.balance(&s.forwarder), 0);
}
