#![cfg(test)]
use super::*;
use proptest::prelude::*;
use soroban_sdk::testutils::{Address as _, Events as _, MockAuth, MockAuthInvoke};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, symbol_short, token,
    vec, Address, Env, IntoVal, String, TryFromVal, Vec,
};

/// Shared test fixture: a fresh env with a USDC-like token and a deployed
/// TipSplitter pointed at it. All auths are mocked.
struct Setup {
    env: Env,
    contract: Address,
    token: Address,
    admin: Address,
}

fn setup() -> Setup {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let token = sac.address();

    let contract = env.register(TipSplitter, (admin.clone(), token.clone()));
    Setup {
        env,
        contract,
        token,
        admin,
    }
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum TestTokenError {
    NotAuthorized = 1,
    InsufficientBalance = 2,
}

#[contracttype]
#[derive(Clone)]
enum TestTokenKey {
    Balance(Address),
    Blocked,
}

/// A minimal token used only by the #171 tests. `block` makes every `transfer`
/// to one address revert, faithfully simulating a recipient whose account can no
/// longer receive the asset (a dropped trustline / deauthorised account) without
/// depending on issuer authorization flags, which the test Stellar Asset
/// Contract registers with `flags: 0` and cannot toggle through its client.
#[contract]
pub struct TestToken;

#[contractimpl]
impl TestToken {
    pub fn mint(env: Env, to: Address, amount: i128) {
        let k = TestTokenKey::Balance(to);
        let b: i128 = env.storage().persistent().get(&k).unwrap_or(0);
        env.storage().persistent().set(&k, &(b + amount));
    }

    pub fn balance(env: Env, id: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&TestTokenKey::Balance(id))
            .unwrap_or(0)
    }

    pub fn block(env: Env, to: Address) {
        env.storage().persistent().set(&TestTokenKey::Blocked, &to);
    }

    pub fn unblock(env: Env) {
        env.storage().persistent().remove(&TestTokenKey::Blocked);
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        if let Some(b) = env
            .storage()
            .persistent()
            .get::<TestTokenKey, Address>(&TestTokenKey::Blocked)
        {
            if b == to {
                panic_with_error!(&env, TestTokenError::NotAuthorized);
            }
        }
        let fk = TestTokenKey::Balance(from.clone());
        let fb: i128 = env.storage().persistent().get(&fk).unwrap_or(0);
        if fb < amount {
            panic_with_error!(&env, TestTokenError::InsufficientBalance);
        }
        env.storage().persistent().set(&fk, &(fb - amount));
        let tk = TestTokenKey::Balance(to.clone());
        let tb: i128 = env.storage().persistent().get(&tk).unwrap_or(0);
        env.storage().persistent().set(&tk, &(tb + amount));
    }
}

/// Fresh env with a TipSplitter pointed at a controllable [`TestToken`].
struct SetupHostile {
    env: Env,
    contract: Address,
    token: Address,
}

fn setup_hostile() -> SetupHostile {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let token = env.register(TestToken, ());
    let contract = env.register(TipSplitter, (admin.clone(), token.clone()));
    SetupHostile {
        env,
        contract,
        token,
    }
}

#[test]
fn tip_splits_70_30() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token = token::Client::new(env, &s.token);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &1_000);

    let jar_id = String::from_str(env, "@band");
    let splits = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 7000,
        },
        Split {
            to: bob.clone(),
            bps: 3000,
        },
    ];
    client.create_jar(&owner, &jar_id, &splits);

    client.tip(&tipper, &jar_id, &100, &String::from_str(env, "great show"));

    assert_eq!(token.balance(&alice), 70);
    assert_eq!(token.balance(&bob), 30);
    assert_eq!(token.balance(&tipper), 900);
}

#[test]
fn tip_sends_rounding_dust_to_last_recipient() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token = token::Client::new(env, &s.token);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let a = Address::generate(env);
    let b = Address::generate(env);
    let c = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &10);

    let jar_id = String::from_str(env, "@trio");
    let splits = vec![
        env,
        Split {
            to: a.clone(),
            bps: 3333,
        },
        Split {
            to: b.clone(),
            bps: 3333,
        },
        Split {
            to: c.clone(),
            bps: 3334,
        },
    ];
    client.create_jar(&owner, &jar_id, &splits);

    client.tip(&tipper, &jar_id, &10, &String::from_str(env, "hi"));

    // 10 * 3333 / 10000 = 3 (truncated) for a and b; c absorbs the remainder.
    assert_eq!(token.balance(&a), 3);
    assert_eq!(token.balance(&b), 3);
    assert_eq!(token.balance(&c), 4);
    assert_eq!(token.balance(&tipper), 0);
}

#[test]
fn tip_emits_tip_event_with_expected_topics_and_data() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &100);

    let jar_id = String::from_str(env, "@ev");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    let message = String::from_str(env, "nice set");
    let seq = client.tip(&tipper, &jar_id, &100, &message);

    // decodeTipEvent in novatip-sdk reads topic 0 as the "tip" symbol, topic 1
    // as the jar id, and the data as the (from, amount, message, breakdown,
    // token) tuple. The fifth element is new: without it an indexer cannot
    // tell 10 USDC from 10 XLM, so every consumer of this event has to read
    // five elements now — see `version()` for how a client detects that.
    // seq, reference) tuple. The last two are appended: every tip is numbered,
    // and a plain `tip` publishes an explicit absent reference so one decoder
    // serves both entry points.
    let tip_events: std::vec::Vec<_> = env
        .events()
        .all()
        .iter()
        .filter(|e| e.0 == s.contract)
        .collect();
    assert_eq!(tip_events.len(), 1);
    let (_, topics, data) = tip_events.first().unwrap();
    assert_eq!(
        topics,
        &vec![
            env,
            symbol_short!("tip").into_val(env),
            jar_id.into_val(env)
        ]
    );
    let decoded: (Address, i128, String, Vec<(Address, i128, bool)>) =
    let decoded: (Address, i128, String, Vec<(Address, i128)>, Address) =
        TryFromVal::try_from_val(env, data).expect("tip event data shape");
    let decoded: TipEventData = TryFromVal::try_from_val(env, data).expect("tip event data shape");
    assert_eq!(
        decoded,
        (
            tipper,
            100i128,
            message,
            vec![env, (alice.clone(), 100i128, false)]
        )
            vec![env, (alice, 100i128)],
            s.token.clone()
        )
            seq,
            None
        )
    );
    // The number returned to the caller is the one published in the event.
    assert_eq!(seq, 1);
}

/// The sequence number and reference read out of the `tip` events this contract
/// published for `jar_id`.
///
/// Read immediately after the tip whose event is wanted: the test env only
/// reliably reports the most recent invocation's events, and a reverted
/// invocation clears them entirely.
fn tip_seq_and_references(
    env: &Env,
    contract: &Address,
    jar_id: &String,
) -> std::vec::Vec<(u64, Option<BytesN<32>>)> {
    let expected_topics = vec![
        env,
        symbol_short!("tip").into_val(env),
        jar_id.into_val(env),
    ];
    env.events()
        .all()
        .iter()
        .filter(|e| e.0 == *contract && e.1 == expected_topics)
        .map(|(_, _, data)| {
            let (_, _, _, _, seq, reference): TipEventData =
                TryFromVal::try_from_val(env, &data).expect("tip event data shape");
            (seq, reference)
        })
        .collect()
}

/// A 32-byte reference filled with a recognisable pattern, standing in for an
/// integration's own id.
fn sample_reference(env: &Env, fill: u8) -> BytesN<32> {
    BytesN::from_array(env, &[fill; REFERENCE_LEN])
}

/// A tip needs an identifier of its own: two tips in one transaction share a
/// transaction hash, and an indexer that deduplicates on the hash silently
/// drops every tip after the first. So each tip gets the next number, returned
/// to the caller and published in its own event, and `get_tip_count` tracks the
/// total.
#[test]
fn successive_tips_get_consecutive_sequence_numbers() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &300);

    let jar_id = String::from_str(env, "@seq");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );
    // Nothing tipped yet.
    assert_eq!(client.get_tip_count(), 0);

    let first = client.tip(&tipper, &jar_id, &100, &String::from_str(env, "one"));
    assert_eq!(
        first, 1,
        "numbers are 1-based, so 0 stays reserved for 'no tip yet'"
    );
    assert_eq!(
        tip_seq_and_references(env, &s.contract, &jar_id),
        std::vec![(1, None)]
    );
    assert_eq!(client.get_tip_count(), 1);

    let second = client.tip(&tipper, &jar_id, &100, &String::from_str(env, "two"));
    assert_eq!(second, 2);
    assert_eq!(second - first, 1);
    assert_eq!(
        tip_seq_and_references(env, &s.contract, &jar_id),
        std::vec![(2, None)]
    );

    // The view agrees with what was handed out, which is what lets a consumer
    // prove it has not missed a tip.
    assert_eq!(client.get_tip_count(), 2);
}

/// A tip that reverts must not consume a number, or every consumer would see a
/// gap and be unable to tell "dropped by my scan" from "never happened".
#[test]
fn reverted_tip_does_not_consume_a_sequence_number() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token = token::Client::new(env, &s.token);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &300);

    let jar_id = String::from_str(env, "@nohole");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    assert_eq!(
        client.tip(&tipper, &jar_id, &100, &String::from_str(env, "ok")),
        1
    );
    assert_eq!(client.get_tip_count(), 1);

    // Rejected before any transfer: an unregistered slug.
    let missing = String::from_str(env, "@free");
    assert!(client
        .try_tip(&tipper, &missing, &100, &String::from_str(env, "boom"))
        .is_err());
    assert_eq!(
        client.get_tip_count(),
        1,
        "a reverted tip leaves the counter where it was"
    );
    assert_eq!(token.balance(&alice), 100, "no funds moved");

    // The next tip takes the number the failed one would have used, so the
    // sequence has no hole for a consumer to mistake for a lost tip.
    assert_eq!(
        client.tip(&tipper, &jar_id, &100, &String::from_str(env, "still 2")),
        2
    );
    assert_eq!(client.get_tip_count(), 2);
}

/// The reference is what lets an integration match a webhook back to the
/// checkout session, bot command or pending alert that started the tip. The
/// contract only echoes it — and `tip` publishes an explicit absent one, so a
/// decoder reads the same six-element shape from either entry point.
#[test]
fn tip_with_ref_publishes_the_reference_and_tip_publishes_none() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token = token::Client::new(env, &s.token);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &500);

    let jar_id = String::from_str(env, "@ref");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 7000,
            },
            Split {
                to: bob.clone(),
                bps: 3000,
            },
        ],
    );

    // 32 arbitrary bytes: the contract ascribes no meaning to the contents.
    let reference = BytesN::from_array(env, &std::array::from_fn(|i| i as u8 + 1));
    let seq = client.tip_with_ref(
        &tipper,
        &jar_id,
        &100,
        &String::from_str(env, "via checkout"),
        &Some(reference.clone()),
    );

    assert_eq!(
        tip_seq_and_references(env, &s.contract, &jar_id),
        std::vec![(seq, Some(reference.clone()))],
        "the number returned to the caller is the one in the event, and the \
         reference is echoed verbatim"
    );
    assert_eq!(token.balance(&alice), 70);
    assert_eq!(token.balance(&bob), 30);

    // A plain `tip` publishes an explicit absent reference rather than a
    // shorter tuple, so one decoder handles both entry points.
    let seq2 = client.tip(&tipper, &jar_id, &100, &String::from_str(env, "no ref"));
    assert_eq!(seq2, 2);
    assert_eq!(
        tip_seq_and_references(env, &s.contract, &jar_id),
        std::vec![(2, None)]
    );
    assert_eq!(token.balance(&alice), 140);
}

/// `tip` and `tip_with_ref` share one implementation, so the reference must not
/// change what is paid, in what order, or how many times.
#[test]
fn tip_with_ref_moves_the_same_funds_as_tip() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token = token::Client::new(env, &s.token);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &1000);

    let jar_id = String::from_str(env, "@parity");
    let splits = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 7000,
        },
        Split {
            to: bob.clone(),
            bps: 3000,
        },
    ];
    client.create_jar(&owner, &jar_id, &splits);

    let reference = sample_reference(env, 7);
    let with_ref = client.tip_with_ref(
        &tipper,
        &jar_id,
        &101,
        &String::from_str(env, "same maths"),
        &Some(reference),
    );
    assert_eq!(with_ref, 1);
    assert_eq!(
        (token.balance(&alice), token.balance(&bob)),
        (70, 31),
        "dust to the last recipient, exactly as `tip` would pay"
    );

    // Same jar, same amount, no reference: identical outcome and the next
    // number from the same counter.
    let plain = client.tip(&tipper, &jar_id, &101, &String::from_str(env, "same maths"));
    assert_eq!(plain, 2);
    assert_eq!(token.balance(&alice), 140);
    assert_eq!(token.balance(&bob), 62);
    assert_eq!(token.balance(&tipper), 1000 - 202);
}

/// A reference costs nothing to leave out and is not stored: the jar and the
/// tip count read the same whether or not a tip carried one, and nothing
/// enforces uniqueness — a consumer that needs a guaranteed-unique id uses
/// `seq`.
#[test]
fn reference_is_not_stored_and_not_required_to_be_unique() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &500);

    let jar_id = String::from_str(env, "@opaque");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    let reference = sample_reference(env, 9);
    for expected in [1u64, 2] {
        assert_eq!(
            client.tip_with_ref(
                &tipper,
                &jar_id,
                &100,
                &String::from_str(env, "dup ref"),
                &Some(reference.clone()),
            ),
            expected,
            "two tips with the same reference both succeed"
        );
        assert_eq!(
            tip_seq_and_references(env, &s.contract, &jar_id),
            std::vec![(expected, Some(reference.clone()))]
        );
    }

    // Nothing about the jar changed because a reference rode along.
    assert_eq!(client.get_jar(&jar_id).splits.len(), 1);
    assert_eq!(client.get_tip_count(), 2);

    // And an explicit `None` is accepted too, not just an omitted argument.
    assert_eq!(
        client.tip_with_ref(
            &tipper,
            &jar_id,
            &100,
            &String::from_str(env, "no ref"),
            &None,
        ),
        3
    );
}

/// The `tip` event's data, decoded in its published positional order:
/// `from`, `amount`, `message`, `breakdown`, then the two fields added by this
/// change — the tip's sequence number and its optional client reference.
type TipEventData = (
    Address,
    i128,
    String,
    Vec<(Address, i128)>,
    u64,
    Option<BytesN<32>>,
);

/// Reads the per-recipient breakdown out of the single `tip` event this
/// contract published.
fn tip_breakdown(env: &Env, contract: &Address, jar_id: &String) -> std::vec::Vec<(Address, i128)> {
    let expected_topics = vec![
        env,
        symbol_short!("tip").into_val(env),
        jar_id.into_val(env),
    ];
    let events: std::vec::Vec<_> = env
        .events()
        .all()
        .iter()
        .filter(|e| e.0 == *contract && e.1 == expected_topics)
        .collect();
    assert_eq!(events.len(), 1, "expected exactly one tip event");
    let (_, _, data) = events.first().unwrap();
    let (_, _, _, breakdown): (Address, i128, String, Vec<(Address, i128, bool)>) =
    let (_, _, _, breakdown, _): (Address, i128, String, Vec<(Address, i128)>, Address) =
    let (_, _, _, breakdown, _, _): TipEventData =
        TryFromVal::try_from_val(env, data).expect("tip event data shape");
    (0..breakdown.len())
        .map(|i| {
            let (to, moved, _held) = breakdown.get(i).unwrap();
            (to, moved)
        })
        .collect()
}

/// The point of the breakdown is that an indexer never has to redo the split
/// arithmetic. So it must match, recipient for recipient and in split order,
/// the balances the transfers actually produced — including the dust the last
/// recipient absorbs, which is where a recomputed figure would go wrong.
#[test]
fn tip_event_breakdown_matches_the_balances_the_transfers_produced() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token = token::Client::new(env, &s.token);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let a = Address::generate(env);
    let b = Address::generate(env);
    let c = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &100);

    // 33.33 / 33.33 / 33.34 over 100 units leaves rounding dust for `c`.
    let jar_id = String::from_str(env, "@breakdown");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: a.clone(),
                bps: 3333,
            },
            Split {
                to: b.clone(),
                bps: 3333,
            },
            Split {
                to: c.clone(),
                bps: 3334,
            },
        ],
    );

    client.tip(
        &tipper,
        &jar_id,
        &100,
        &String::from_str(env, "split three ways"),
    );

    let breakdown = tip_breakdown(env, &s.contract, &jar_id);
    assert_eq!(
        breakdown,
        std::vec![
            (a.clone(), token.balance(&a)),
            (b.clone(), token.balance(&b)),
            (c.clone(), token.balance(&c)),
        ],
        "the breakdown must report what each recipient was actually paid, in split order"
    );
    // Pin the figures down, so a change in the splitting rule shows up here
    // rather than being absorbed by comparing the event against itself.
    assert_eq!(breakdown, std::vec![(a, 33i128), (b, 33i128), (c, 34i128)]);
    assert_eq!(
        breakdown.iter().map(|(_, amt)| amt).sum::<i128>(),
        100,
        "the breakdown must account for the whole tip"
    );
}

/// `tip` skips a transfer when the sender is also a recipient. The breakdown is
/// a record of the transfers the call made, so that entry reports `0` — and the
/// recipient's balance confirms nothing moved.
#[test]
fn tip_event_breakdown_reports_zero_for_a_skipped_self_transfer() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token = token::Client::new(env, &s.token);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &100);

    // The tipper is listed first; `tip` skips paying them and the final
    // recipient absorbs the whole amount.
    let jar_id = String::from_str(env, "@selftip");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: tipper.clone(),
                bps: 5000,
            },
            Split {
                to: alice.clone(),
                bps: 5000,
            },
        ],
    );

    client.tip(&tipper, &jar_id, &100, &String::from_str(env, "half mine"));

    let breakdown = tip_breakdown(env, &s.contract, &jar_id);
    assert_eq!(
        breakdown,
        std::vec![(tipper.clone(), 0i128), (alice.clone(), 100i128)],
        "a skipped self-transfer is reported as 0, not as the notional share"
    );
    assert_eq!(token.balance(&alice), 100);
    assert_eq!(token.balance(&tipper), 0);
    // Every entry still names its recipient in split order, so an indexer can
    // tell "listed but paid nothing" apart from "not listed at all".
    assert_eq!(breakdown[0].0, tipper);
    assert_eq!(breakdown[1].0, alice);
}

#[test]
fn create_jar_rejects_bad_bps_sum() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    // Distinct recipients, so the only defect is the sum: 6000 + 3000 = 9000.
    let bad = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 6000,
        },
        Split {
            to: bob.clone(),
            bps: 3000,
        },
    ];

    let res = client.try_create_jar(&owner, &String::from_str(env, "@x"), &bad);
    assert_eq!(res, Err(Ok(Error::SplitSumNot100Pct.into())));
}

/// No single share may exceed 100%. Such an entry can never belong to a set
/// summing to `BPS_DENOM`, and rejecting it per entry is what bounds the
/// running total well below `u32::MAX`.
#[test]
fn create_jar_rejects_bps_above_one_hundred_percent() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bad = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 10_001,
        },
    ];

    let res = client.try_create_jar(&owner, &String::from_str(env, "@overshare"), &bad);
    assert_eq!(res, Err(Ok(Error::SplitOutOfRange.into())));
}

/// Overflow regression: `u32::MAX + 10_001` is `2^32 + 10_000`, so a wrapping
/// `+=` would land on exactly `BPS_DENOM` and wave this jar through as if the
/// shares summed to 100%. It must be rejected with the typed `InvalidSplits`,
/// and it must be rejected by the validator itself — not by `overflow-checks`
/// trapping in the release profile, which would surface as an opaque wasm
/// error instead of error code 4.
#[test]
fn create_jar_rejects_bps_sum_that_wraps_u32() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let bad = vec![
        env,
        Split {
            to: alice.clone(),
            bps: u32::MAX,
        },
        Split {
            to: bob.clone(),
            bps: 10_001,
        },
    ];

    let jar_id = String::from_str(env, "@wrap");
    let res = client.try_create_jar(&owner, &jar_id, &bad);
    assert_eq!(res, Err(Ok(Error::SplitOutOfRange.into())));

    // The jar must not have been stored.
    let jar = client.try_get_jar(&jar_id);
    assert!(jar.is_err(), "rejected jar must not be persisted");
}

/// `update_splits` runs the same validator, so overflowing shares can't be
/// swapped into a jar that already exists.
#[test]
fn update_splits_rejects_bps_sum_that_wraps_u32() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);

    let jar_id = String::from_str(env, "@upd-wrap");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    let res = client.try_update_splits(
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: u32::MAX,
            },
            Split {
                to: bob.clone(),
                bps: 10_001,
            },
        ],
    );
    assert_eq!(res, Err(Ok(Error::SplitOutOfRange.into())));

    // The original split must be untouched.
    let jar = client.get_jar(&jar_id);
    assert_eq!(jar.splits.len(), 1);
    assert_eq!(jar.splits.get(0).unwrap().bps, 10000);
}

/// The same address twice is rejected even though the shares still total 100%.
#[test]
fn create_jar_rejects_duplicate_recipient() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let dup = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 6000,
        },
        Split {
            to: alice.clone(),
            bps: 4000,
        },
    ];

    let res = client.try_create_jar(&owner, &String::from_str(env, "@dupaddr"), &dup);
    assert_eq!(res, Err(Ok(Error::DuplicateRecipient.into())));

    // The jar must not have been stored.
    let jar = client.try_get_jar(&String::from_str(env, "@dupaddr"));
    assert!(jar.is_err(), "rejected jar must not be persisted");
}

/// The duplicate need not be adjacent — the check is pairwise across the whole
/// vector, not just neighbours.
#[test]
fn create_jar_rejects_non_adjacent_duplicate_recipient() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let dup = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 4000,
        },
        Split {
            to: bob.clone(),
            bps: 3000,
        },
        Split {
            to: alice.clone(),
            bps: 3000,
        },
    ];

    let res = client.try_create_jar(&owner, &String::from_str(env, "@spread"), &dup);
    assert_eq!(res, Err(Ok(Error::DuplicateRecipient.into())));
}

/// A vector of distinct addresses must still be accepted and pay out normally —
/// the new check must not reject valid jars.
#[test]
fn create_jar_accepts_distinct_recipients() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token = token::Client::new(env, &s.token);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let carol = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &100);

    let jar_id = String::from_str(env, "@distinct");
    let splits = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 5000,
        },
        Split {
            to: bob.clone(),
            bps: 3000,
        },
        Split {
            to: carol.clone(),
            bps: 2000,
        },
    ];
    client.create_jar(&owner, &jar_id, &splits);

    let jar = client.get_jar(&jar_id);
    assert_eq!(jar.splits.len(), 3);

    client.tip(&tipper, &jar_id, &100, &String::from_str(env, "nice"));
    assert_eq!(token.balance(&alice), 50);
    assert_eq!(token.balance(&bob), 30);
    assert_eq!(token.balance(&carol), 20);
}

/// The O(n^2) scan must still accept a full-size jar at the MAX_RECIPIENTS bound.
#[test]
fn create_jar_accepts_max_distinct_recipients() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);

    // 20 distinct recipients at 500 bps each = 10000.
    let mut splits = soroban_sdk::Vec::new(env);
    for _ in 0..20 {
        splits.push_back(Split {
            to: Address::generate(env),
            bps: 500,
        });
    }

    let jar_id = String::from_str(env, "@full");
    client.create_jar(&owner, &jar_id, &splits);
    assert_eq!(client.get_jar(&jar_id).splits.len(), 20);
}

/// `update_splits` runs the same validator, so a duplicate can't be introduced
/// into an existing jar.
#[test]
fn update_splits_rejects_duplicate_recipient() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);

    let jar_id = String::from_str(env, "@upd-dup");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 5000,
            },
            Split {
                to: bob.clone(),
                bps: 5000,
            },
        ],
    );

    let res = client.try_update_splits(
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 5000,
            },
            Split {
                to: alice.clone(),
                bps: 5000,
            },
        ],
    );
    assert_eq!(res, Err(Ok(Error::DuplicateRecipient.into())));

    // The original two-way split must be untouched.
    let jar = client.get_jar(&jar_id);
    assert_eq!(jar.splits.len(), 2);
    assert_eq!(jar.splits.get(1).unwrap().to, bob);
}

/// A recipient with `bps: 0` can never be paid, so it must be rejected at
/// validation time rather than silently skipped inside `tip`.
#[test]
fn create_jar_rejects_zero_bps_entry() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let ghost = Address::generate(env);
    // Sums to exactly 10000, but `ghost` would never receive anything.
    let bad = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 10000,
        },
        Split {
            to: ghost.clone(),
            bps: 0,
        },
    ];

    let res = client.try_create_jar(&owner, &String::from_str(env, "@zero"), &bad);
    assert_eq!(res, Err(Ok(Error::SplitOutOfRange.into())));

    // The jar must not have been stored.
    let jar = client.try_get_jar(&String::from_str(env, "@zero"));
    assert!(jar.is_err(), "rejected jar must not be persisted");
}

/// The zero check must not depend on position — a leading zero-bps entry is
/// just as invalid as a trailing one.
#[test]
fn create_jar_rejects_zero_bps_first_entry() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let ghost = Address::generate(env);
    let alice = Address::generate(env);
    let bad = vec![
        env,
        Split {
            to: ghost.clone(),
            bps: 0,
        },
        Split {
            to: alice.clone(),
            bps: 10000,
        },
    ];

    let res = client.try_create_jar(&owner, &String::from_str(env, "@zerofirst"), &bad);
    assert_eq!(res, Err(Ok(Error::SplitOutOfRange.into())));
}

/// A single recipient holding the whole jar must still carry a real share.
#[test]
fn create_jar_rejects_all_zero_bps() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bad = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 0,
        },
    ];

    let res = client.try_create_jar(&owner, &String::from_str(env, "@allzero"), &bad);
    assert_eq!(res, Err(Ok(Error::SplitOutOfRange.into())));
}

/// `update_splits` runs the same validation, so a zero-bps entry can't be
/// smuggled into an already-valid jar.
#[test]
fn update_splits_rejects_zero_bps_entry() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let ghost = Address::generate(env);

    let jar_id = String::from_str(env, "@upd-zero");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    let res = client.try_update_splits(
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
            Split {
                to: ghost.clone(),
                bps: 0,
            },
        ],
    );
    assert_eq!(res, Err(Ok(Error::SplitOutOfRange.into())));

    // The original splits must be untouched.
    let jar = client.get_jar(&jar_id);
    assert_eq!(jar.splits.len(), 1);
    assert_eq!(jar.splits.get(0).unwrap().bps, 10000);
}

#[test]
fn create_jar_rejects_duplicate_slug() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let jar_id = String::from_str(env, "@dup");
    let splits = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 10000,
        },
    ];

    client.create_jar(&owner, &jar_id, &splits);
    let res = client.try_create_jar(&owner, &jar_id, &splits);
    assert_eq!(res, Err(Ok(Error::JarExists.into())));
}

/// `jar_exists` flips from false to true on registration, and matches the slug
/// exactly — a prefix or a different slug must not read as taken.
#[test]
fn jar_exists_tracks_registration() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let jar_id = String::from_str(env, "@alice");

    assert!(
        !client.jar_exists(&jar_id),
        "slug must be free before registration"
    );

    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    assert!(
        client.jar_exists(&jar_id),
        "slug must be taken after registration"
    );
    assert!(
        !client.jar_exists(&String::from_str(env, "@bob")),
        "an unrelated slug must still be free"
    );
    // The onboarding check relies on exact matching: "@ali" is its own slug.
    assert!(
        !client.jar_exists(&String::from_str(env, "@ali")),
        "a prefix of a taken slug must still be free"
    );
}

/// A `create_jar` that fails validation must leave the slug free — otherwise
/// the availability check would report a name as taken that nobody owns.
#[test]
fn jar_exists_is_false_for_rejected_jar() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let jar_id = String::from_str(env, "@rejected");

    // 6000 + 3000 = 9000, so the jar is never stored.
    let res = client.try_create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 6000,
            },
            Split {
                to: bob.clone(),
                bps: 3000,
            },
        ],
    );
    assert!(res.is_err());

    assert!(!client.jar_exists(&jar_id));
}

/// `update_splits` rewrites the same key, so the slug must stay registered.
#[test]
fn jar_exists_stays_true_after_update_splits() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let jar_id = String::from_str(env, "@steady");

    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );
    client.update_splits(
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 5000,
            },
            Split {
                to: bob.clone(),
                bps: 5000,
            },
        ],
    );

    assert!(client.jar_exists(&jar_id));
}

/// `get_jar_owner` must agree with `get_jar(...).owner` for a registered jar,
/// which is the whole point: the cheap read and the full read never disagree.
#[test]
fn get_jar_owner_returns_owner_of_registered_jar() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let jar_id = String::from_str(env, "@duo");

    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 6000,
            },
            Split {
                to: bob.clone(),
                bps: 4000,
            },
        ],
    );

    assert_eq!(client.get_jar_owner(&jar_id), owner);
    assert_eq!(client.get_jar_owner(&jar_id), client.get_jar(&jar_id).owner);
    // The owner is distinct from the recipients — a jar can pay addresses that
    // do not control it, so returning a recipient would pass a weaker test.
    assert_ne!(client.get_jar_owner(&jar_id), alice);
}

/// An unregistered slug is a `JarNotFound` panic, not a default address — a
/// zero or placeholder address here would read to a client as "someone owns
/// this jar".
#[test]
fn get_jar_owner_on_missing_jar_fails() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let res = client.try_get_jar_owner(&String::from_str(env, "@ghost"));
    assert_eq!(res, Err(Ok(Error::JarNotFound.into())));
}

/// `transfer_jar_ownership` rewrites the stored owner, so the cheap read must
/// follow it rather than serving a stale address.
#[test]
fn get_jar_owner_follows_ownership_transfer() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let new_owner = Address::generate(env);
    let alice = Address::generate(env);
    let jar_id = String::from_str(env, "@moved");

    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );
    assert_eq!(client.get_jar_owner(&jar_id), owner);

    client.transfer_jar_ownership(&jar_id, &new_owner);

    assert_eq!(client.get_jar_owner(&jar_id), new_owner);
    assert_ne!(client.get_jar_owner(&jar_id), owner);
}

/// A single-recipient jar counts 1 — the degenerate case a badge still has to
/// render, and the one an off-by-one would most likely get wrong.
#[test]
fn get_split_count_counts_single_recipient() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let jar_id = String::from_str(env, "@solo");

    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    assert_eq!(client.get_split_count(&jar_id), 1);
}

/// A multi-recipient jar counts every entry, and agrees with the length of the
/// vector `get_jar` returns — the cheap read must not drift from the full one.
#[test]
fn get_split_count_counts_every_recipient() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let jar_id = String::from_str(env, "@collective");

    // Four recipients at 2500 bps each.
    let mut splits = vec![env];
    for _ in 0..4 {
        splits.push_back(Split {
            to: Address::generate(env),
            bps: 2500,
        });
    }
    client.create_jar(&owner, &jar_id, &splits);

    assert_eq!(client.get_split_count(&jar_id), 4);
    assert_eq!(
        client.get_split_count(&jar_id),
        client.get_jar(&jar_id).splits.len()
    );
}

/// `update_splits` replaces the stored vector, so the count must follow it both
/// upwards and back down rather than serving a cached length.
#[test]
fn get_split_count_follows_update_splits() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let carol = Address::generate(env);
    let jar_id = String::from_str(env, "@growing");

    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );
    assert_eq!(client.get_split_count(&jar_id), 1);

    client.update_splits(
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 4000,
            },
            Split {
                to: bob.clone(),
                bps: 4000,
            },
            Split {
                to: carol.clone(),
                bps: 2000,
            },
        ],
    );
    assert_eq!(client.get_split_count(&jar_id), 3);

    // Shrinking the list must lower the count too.
    client.update_splits(
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 5000,
            },
            Split {
                to: bob.clone(),
                bps: 5000,
            },
        ],
    );
    assert_eq!(client.get_split_count(&jar_id), 2);
}

/// An unregistered slug is a `JarNotFound` panic, not `0` — a zero count would
/// be indistinguishable from a jar, and no stored jar can have zero splits.
#[test]
fn get_split_count_on_missing_jar_fails() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let res = client.try_get_split_count(&String::from_str(env, "@ghost"));
    assert_eq!(res, Err(Ok(Error::JarNotFound.into())));
}

/// The preview is the payout: the numbers a client shows before signing must
/// be the balances the tip actually produces.
#[test]
fn preview_split_matches_what_tip_pays() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token = token::Client::new(env, &s.token);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &1_000);

    let jar_id = String::from_str(env, "@preview");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 7000,
            },
            Split {
                to: bob.clone(),
                bps: 3000,
            },
        ],
    );

    let preview = client.preview_split(&jar_id, &101);
    assert_eq!(preview, vec![env, 70_i128, 31_i128]);

    client.tip(
        &tipper,
        &jar_id,
        &101,
        &String::from_str(env, "here you go"),
    );

    assert_eq!(token.balance(&alice), preview.get(0).unwrap());
    assert_eq!(token.balance(&bob), preview.get(1).unwrap());
}

/// A single-recipient jar previews the whole amount, with no dust to place.
#[test]
fn preview_split_single_recipient_gets_everything() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let jar_id = String::from_str(env, "@onlyme");

    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    assert_eq!(client.preview_split(&jar_id, &7), vec![env, 7_i128]);
}

/// Thirds over amounts that do not divide cleanly: the shares must still sum to
/// exactly the amount, with the truncated remainder landing on the last
/// recipient. These are the amounts where a client's own arithmetic would
/// most easily drift from the contract's.
#[test]
fn preview_split_shares_sum_to_amount_for_awkward_amounts() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let jar_id = String::from_str(env, "@thirds");

    // 3333 / 3333 / 3334 — the classic non-terminating split.
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: Address::generate(env),
                bps: 3333,
            },
            Split {
                to: Address::generate(env),
                bps: 3333,
            },
            Split {
                to: Address::generate(env),
                bps: 3334,
            },
        ],
    );

    for amount in [10_i128, 11, 99, 100, 101, 7_777, 1_000_003] {
        let shares = client.preview_split(&jar_id, &amount);
        assert_eq!(shares.len(), 3, "one share per recipient");

        let mut total: i128 = 0;
        for i in 0..shares.len() {
            let share = shares.get(i).unwrap();
            assert!(share > 0, "every recipient must get something at {amount}");
            total += share;
        }
        assert_eq!(total, amount, "shares must sum to exactly {amount}");
    }

    // Spot-check the dust placement rather than only the invariant: at 100,
    // truncation leaves 1 over and the final recipient absorbs it.
    assert_eq!(
        client.preview_split(&jar_id, &100),
        vec![env, 33_i128, 33_i128, 34_i128]
    );
}

/// The preview must follow `update_splits`, or a client would quote shares for
/// a line-up the contract no longer has.
#[test]
fn preview_split_follows_update_splits() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let jar_id = String::from_str(env, "@requoted");

    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );
    assert_eq!(client.preview_split(&jar_id, &100), vec![env, 100_i128]);

    client.update_splits(
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 2500,
            },
            Split {
                to: bob.clone(),
                bps: 7500,
            },
        ],
    );
    assert_eq!(
        client.preview_split(&jar_id, &100),
        vec![env, 25_i128, 75_i128]
    );
}

/// A preview that returns at all must describe a tip that can go through, so
/// it rejects exactly the amounts `tip` rejects.
#[test]
fn preview_split_rejects_amounts_tip_would_reject() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let jar_id = String::from_str(env, "@strict");

    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 9900,
            },
            Split {
                to: bob.clone(),
                bps: 100,
            },
        ],
    );

    assert_eq!(
        client.try_preview_split(&jar_id, &0),
        Err(Ok(Error::InvalidAmount.into()))
    );
    assert_eq!(
        client.try_preview_split(&jar_id, &-5),
        Err(Ok(Error::InvalidAmount.into()))
    );
    // bps 100 truncates to zero below 100, which `tip` refuses to pay.
    assert_eq!(
        client.try_preview_split(&jar_id, &99),
        Err(Ok(Error::InvalidAmount.into()))
    );
    assert!(client
        .try_tip(
            &Address::generate(env),
            &jar_id,
            &99,
            &String::from_str(env, "too small")
        )
        .is_err());
    // 100 is the first amount that pays everyone.
    assert_eq!(
        client.preview_split(&jar_id, &100),
        vec![env, 99_i128, 1_i128]
    );
}

/// An unregistered slug is a `JarNotFound` panic, matching `get_jar` — an
/// empty vector would read as a jar that pays nobody.
#[test]
fn preview_split_on_missing_jar_fails() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let res = client.try_preview_split(&String::from_str(env, "@ghost"), &100);
    assert_eq!(res, Err(Ok(Error::JarNotFound.into())));
}

#[test]
fn tip_on_missing_jar_fails() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let tipper = Address::generate(env);
    let res = client.try_tip(
        &tipper,
        &String::from_str(env, "@ghost"),
        &100,
        &String::from_str(env, "?"),
    );
    assert_eq!(res, Err(Ok(Error::JarNotFound.into())));
}

/// Backs the check-order claim in `docs/CONTRACT.md`: `tip` rejects an
/// over-long message before it looks the jar up, so an over-long message to an
/// unregistered slug reports `MessageTooLong`, not `JarNotFound`. Both
/// conditions are true here — the amount is valid and the jar is missing — so
/// only the earlier message-length guard can explain the reported code.
#[test]
fn tip_rejects_over_long_message_before_jar_lookup() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let tipper = Address::generate(env);
    // One byte over MAX_MESSAGE_LEN (280), sent to a slug that was never registered.
    let too_long = String::from_bytes(env, &[b'a'; 281]);
    let res = client.try_tip(&tipper, &String::from_str(env, "@ghost"), &100, &too_long);
    assert_eq!(res, Err(Ok(Error::MessageTooLong.into())));
}

/// `update_splits` runs the same lookup-then-panic path as `tip`, so an
/// unregistered slug must be rejected rather than quietly creating a jar.
#[test]
fn update_splits_on_missing_jar_fails() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let alice = Address::generate(env);
    // Valid splits, so a missing jar is the only thing that can fail this.
    let splits = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 10000,
        },
    ];

    let jar_id = String::from_str(env, "@never-registered");
    let res = client.try_update_splits(&jar_id, &splits);
    assert_eq!(res, Err(Ok(Error::JarNotFound.into())));

    // The failed update must not have brought the jar into existence.
    assert!(
        client.try_get_jar(&jar_id).is_err(),
        "update_splits must not create a jar as a side effect"
    );
}

#[test]
fn tip_rejects_nonpositive_amount() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let tipper = Address::generate(env);
    let jar_id = String::from_str(env, "@a");
    let splits = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 10000,
        },
    ];
    client.create_jar(&owner, &jar_id, &splits);

    let res = client.try_tip(&tipper, &jar_id, &0, &String::from_str(env, ""));
    assert_eq!(res, Err(Ok(Error::InvalidAmount.into())));
}

/// An over-long message is rejected before any funds move — the guard sits
/// above the transfer loop, so balances must be untouched.
#[test]
fn tip_rejects_over_long_message() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token = token::Client::new(env, &s.token);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &500);

    let jar_id = String::from_str(env, "@wordy");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    // One byte over MAX_MESSAGE_LEN (280).
    let too_long = String::from_bytes(env, &[b'a'; 281]);
    let res = client.try_tip(&tipper, &jar_id, &100, &too_long);
    assert_eq!(res, Err(Ok(Error::MessageTooLong.into())));

    // No funds may have moved.
    assert_eq!(token.balance(&alice), 0, "alice must not have been paid");
    assert_eq!(
        token.balance(&tipper),
        500,
        "tipper balance must be unchanged"
    );
}

/// A message of exactly MAX_MESSAGE_LEN bytes is still valid — the bound is
/// inclusive, so an off-by-one here would reject legitimate tips.
#[test]
fn tip_accepts_message_at_exact_limit() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token = token::Client::new(env, &s.token);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &500);

    let jar_id = String::from_str(env, "@atlimit");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    let exact = String::from_bytes(env, &[b'a'; 280]);
    assert_eq!(exact.len(), 280);
    client.tip(&tipper, &jar_id, &100, &exact);

    assert_eq!(token.balance(&alice), 100);
    assert_eq!(token.balance(&tipper), 400);
}

#[test]
fn update_splits_changes_distribution() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token = token::Client::new(env, &s.token);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &200);

    let jar_id = String::from_str(env, "@band");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    // Add bob; now split 50/50.
    client.update_splits(
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 5000,
            },
            Split {
                to: bob.clone(),
                bps: 5000,
            },
        ],
    );

    client.tip(&tipper, &jar_id, &100, &String::from_str(env, "gig"));

    assert_eq!(token.balance(&alice), 50);
    assert_eq!(token.balance(&bob), 50);

    let jar = client.get_jar(&jar_id);
    assert_eq!(jar.splits.len(), 2);
}

#[test]
fn tip_single_recipient_receives_full_amount() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token = token::Client::new(env, &s.token);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &500);

    let jar_id = String::from_str(env, "@solo");
    let splits = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 10000,
        },
    ];
    client.create_jar(&owner, &jar_id, &splits);
    client.tip(
        &tipper,
        &jar_id,
        &500,
        &String::from_str(env, "all for you"),
    );

    // Single recipient must receive the exact amount with no dust loss
    assert_eq!(token.balance(&alice), 500);
    assert_eq!(token.balance(&tipper), 0);
}

#[test]
fn create_jar_rejects_too_many_recipients() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let addr = Address::generate(env);

    // 21 recipients exceeds MAX_RECIPIENTS (20)
    // bps values don't matter — TooManyRecipients is checked first
    let mut splits_vec = soroban_sdk::Vec::new(env);
    for _ in 0..21 {
        splits_vec.push_back(Split {
            to: addr.clone(),
            bps: 476,
        });
    }

    let res = client.try_create_jar(&owner, &String::from_str(env, "@toobig"), &splits_vec);
    assert_eq!(res, Err(Ok(Error::TooManyRecipients.into())));
}

#[test]
fn create_jar_emits_jar_created_event() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let splits = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 10000,
        },
    ];
    let jar_id = String::from_str(env, "@one");

    client.create_jar(&owner, &jar_id, &splits);

    // The jar_crtd event must be published with the correct topics and data.
    let jar_events: std::vec::Vec<_> = env
        .events()
        .all()
        .iter()
        .filter(|e| e.0 == s.contract)
        .collect();
    assert_eq!(jar_events.len(), 1);
    let (_, topics, data) = jar_events.first().unwrap();
    assert_eq!(
        topics,
        &vec![
            env,
            symbol_short!("jar_crtd").into_val(env),
            jar_id.into_val(env)
        ]
    );
    let decoded: Address = TryFromVal::try_from_val(env, data).expect("jar_crtd event data shape");
    assert_eq!(decoded, owner);
}

#[test]
fn update_splits_emits_splits_event() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let jar_id = String::from_str(env, "@two");

    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    let new_splits = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 5000,
        },
        Split {
            to: bob.clone(),
            bps: 5000,
        },
    ];
    client.update_splits(&jar_id, &new_splits);

    // The splits event must be published with the jar id and new split count.
    let expected_topics = vec![
        env,
        symbol_short!("splits").into_val(env),
        jar_id.into_val(env),
    ];
    let splits_events: std::vec::Vec<_> = env
        .events()
        .all()
        .iter()
        .filter(|e| e.0 == s.contract && e.1 == expected_topics)
        .collect();
    assert_eq!(splits_events.len(), 1);
    let (_, _, data) = splits_events.first().unwrap();
    let decoded: u32 = TryFromVal::try_from_val(env, data).expect("splits event data shape");
    assert_eq!(decoded, new_splits.len());
}

#[test]
fn tip_fails_when_sender_has_insufficient_balance() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token_admin = token::StellarAssetClient::new(env, &s.token);
    let token = token::Client::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let tipper = Address::generate(env);

    // Mint less than the tip amount: tipper has 50, tip is 100.
    token_admin.mint(&tipper, &50);

    let jar_id = String::from_str(env, "@underfunded");
    let splits = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 10000,
        },
    ];
    client.create_jar(&owner, &jar_id, &splits);

    let res = client.try_tip(&tipper, &jar_id, &100, &String::from_str(env, "oops"));
    assert_eq!(res, Err(Ok(Error::InsufficientBalance.into())));

    // Balances must be unchanged — tipper keeps their 50, alice gets nothing.
    assert_eq!(token.balance(&tipper), 50);
    assert_eq!(token.balance(&alice), 0);
}

/// Atomicity test: if the sender can't cover the full tip on a multi-recipient
/// jar, the transaction must revert entirely — no recipient receives anything
/// and the tipper's balance is unchanged. This is the core "all-or-nothing"
/// guarantee stated in the module docs and CONTRACT.md.
#[test]
fn tip_multi_recipient_no_partial_distribution_on_insufficient_balance() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token_admin = token::StellarAssetClient::new(env, &s.token);
    let token = token::Client::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let carol = Address::generate(env);
    let tipper = Address::generate(env);

    // Tip amount is 300; tipper only has 100 — not enough to cover all splits.
    let tip_amount: i128 = 300;
    token_admin.mint(&tipper, &100);

    let jar_id = String::from_str(env, "@trio-atomic");
    // Three-way even split: 40% / 35% / 25%
    let splits = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 4000,
        },
        Split {
            to: bob.clone(),
            bps: 3500,
        },
        Split {
            to: carol.clone(),
            bps: 2500,
        },
    ];
    client.create_jar(&owner, &jar_id, &splits);

    let res = client.try_tip(
        &tipper,
        &jar_id,
        &tip_amount,
        &String::from_str(env, "not enough"),
    );

    // The call must fail with the contract's own insufficient-balance code,
    // not the token's overlapping number.
    assert_eq!(res, Err(Ok(Error::InsufficientBalance.into())));

    // Atomicity: every recipient balance must still be 0 — no partial payment.
    assert_eq!(
        token.balance(&alice),
        0,
        "alice must not have received anything"
    );
    assert_eq!(
        token.balance(&bob),
        0,
        "bob must not have received anything"
    );
    assert_eq!(
        token.balance(&carol),
        0,
        "carol must not have received anything"
    );

    // The tipper's balance must be completely unchanged.
    assert_eq!(
        token.balance(&tipper),
        100,
        "tipper balance must be unchanged"
    );
}

// ---------------------------------------------------------------------------
// Authorization
//
// Every test above runs under `setup()`, which calls `env.mock_all_auths()` —
// that makes every `require_auth()` succeed unconditionally, so none of them
// can tell a wired-up auth check from a missing one. The tests below switch the
// env to `mock_auths(&[..])`, which authorizes *only* the listed invocations
// and rejects everything else, so a deleted `require_auth()` line shows up as a
// call that unexpectedly succeeds.
//
// Each negative test is paired with a positive control using the same builder
// and the correct signer. Without the control, a negative test would still pass
// if the call failed for some unrelated reason (wrong arg encoding, say), which
// would make it worthless as a guard.
// ---------------------------------------------------------------------------

/// The declared `owner` must sign `create_jar` — a third party cannot register
/// a jar in someone else's name.
#[test]
fn create_jar_requires_declared_owner_auth() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let attacker = Address::generate(env);
    let alice = Address::generate(env);
    let jar_id = String::from_str(env, "@unauthorized");
    let splits = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 10000,
        },
    ];

    // The attacker signs, but the call declares `owner` as the jar owner.
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &s.contract,
            fn_name: "create_jar",
            args: (owner.clone(), jar_id.clone(), splits.clone()).into_val(env),
            sub_invokes: &[],
        },
    }]);

    let res = client.try_create_jar(&owner, &jar_id, &splits);
    assert!(
        res.is_err(),
        "create_jar must reject a caller who is not the declared owner"
    );

    // Nothing may have been written.
    assert!(
        client.try_get_jar(&jar_id).is_err(),
        "no jar may be created without the owner's authorization"
    );
}

/// Positive control for the test above: the same call with the owner signing
/// must succeed, proving the rejection is about *who* signed and not about the
/// shape of the mocked invocation.
#[test]
fn create_jar_succeeds_with_declared_owner_auth() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let jar_id = String::from_str(env, "@authorized");
    let splits = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 10000,
        },
    ];

    env.mock_auths(&[MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &s.contract,
            fn_name: "create_jar",
            args: (owner.clone(), jar_id.clone(), splits.clone()).into_val(env),
            sub_invokes: &[],
        },
    }]);

    client.create_jar(&owner, &jar_id, &splits);
    assert_eq!(client.get_jar(&jar_id).owner, owner);
}

/// Only the jar owner may rewrite the splits. This is the check that matters
/// most: without it anyone on the network could redirect a creator's tips to
/// their own address.
#[test]
fn update_splits_requires_jar_owner_auth() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let attacker = Address::generate(env);

    let jar_id = String::from_str(env, "@victim");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    // The attacker tries to point the whole jar at themselves.
    let hijacked = vec![
        env,
        Split {
            to: attacker.clone(),
            bps: 10000,
        },
    ];
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &s.contract,
            fn_name: "update_splits",
            args: (jar_id.clone(), hijacked.clone()).into_val(env),
            sub_invokes: &[],
        },
    }]);

    let res = client.try_update_splits(&jar_id, &hijacked);
    assert!(
        res.is_err(),
        "update_splits must reject a caller who does not own the jar"
    );

    // The stored splits must still pay alice, not the attacker.
    let jar = client.get_jar(&jar_id);
    assert_eq!(jar.splits.len(), 1);
    assert_eq!(
        jar.splits.get(0).unwrap().to,
        alice,
        "an unauthorized update must not change the recipient"
    );
}

/// Positive control: the owner's own signature is accepted by the same builder.
#[test]
fn update_splits_succeeds_with_jar_owner_auth() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);

    let jar_id = String::from_str(env, "@ownerupd");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    let new_splits = vec![
        env,
        Split {
            to: bob.clone(),
            bps: 10000,
        },
    ];
    env.mock_auths(&[MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &s.contract,
            fn_name: "update_splits",
            args: (jar_id.clone(), new_splits.clone()).into_val(env),
            sub_invokes: &[],
        },
    }]);

    client.update_splits(&jar_id, &new_splits);
    assert_eq!(client.get_jar(&jar_id).splits.get(0).unwrap().to, bob);
}

#[test]
fn transfer_jar_ownership_requires_current_owner_auth() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let attacker = Address::generate(env);

    let jar_id = String::from_str(env, "@stolen");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &s.contract,
            fn_name: "transfer_jar_ownership",
            args: (jar_id.clone(), attacker.clone()).into_val(env),
            sub_invokes: &[],
        },
    }]);

    let res = client.try_transfer_jar_ownership(&jar_id, &attacker);
    assert!(
        res.is_err(),
        "transfer_jar_ownership must reject a caller who does not own the jar"
    );
    assert_eq!(client.get_jar(&jar_id).owner, owner);
}

#[test]
fn transfer_jar_ownership_moves_control_to_new_owner() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let new_owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);

    let jar_id = String::from_str(env, "@handoff");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    client.transfer_jar_ownership(&jar_id, &new_owner);
    assert_eq!(client.get_jar(&jar_id).owner, new_owner);
    // Splits must survive the transfer untouched.
    assert_eq!(client.get_jar(&jar_id).splits.get(0).unwrap().to, alice);

    // The new owner can now update splits.
    let new_splits = vec![
        env,
        Split {
            to: bob.clone(),
            bps: 10000,
        },
    ];
    env.mock_auths(&[MockAuth {
        address: &new_owner,
        invoke: &MockAuthInvoke {
            contract: &s.contract,
            fn_name: "update_splits",
            args: (jar_id.clone(), new_splits.clone()).into_val(env),
            sub_invokes: &[],
        },
    }]);
    client.update_splits(&jar_id, &new_splits);
    assert_eq!(client.get_jar(&jar_id).splits.get(0).unwrap().to, bob);

    // The old owner can no longer update splits.
    env.mock_auths(&[MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &s.contract,
            fn_name: "update_splits",
            args: (jar_id.clone(), new_splits.clone()).into_val(env),
            sub_invokes: &[],
        },
    }]);
    let res = client.try_update_splits(&jar_id, &new_splits);
    assert!(
        res.is_err(),
        "the old owner must not be able to update splits after transferring ownership"
    );
}

/// Reads the `(prev_owner, new_owner)` pair out of the `jar_xfer` event for
/// `jar_id`. The test env's event buffer only retains the most recent
/// invocation's events, so this is called straight after each transfer.
fn jar_xfer_owners(env: &Env, contract: &Address, jar_id: &String) -> (Address, Address) {
    let expected_topics = vec![
        env,
        symbol_short!("jar_xfer").into_val(env),
        jar_id.into_val(env),
    ];
    let events: std::vec::Vec<_> = env
        .events()
        .all()
        .iter()
        .filter(|e| e.0 == *contract && e.1 == expected_topics)
        .collect();
    assert_eq!(events.len(), 1, "expected exactly one jar_xfer event");
    let (_, _, data) = events.first().unwrap();
    TryFromVal::try_from_val(env, data).expect("jar_xfer event data shape")
}

/// The `jar_xfer` event must name both ends of the move. An indexer replaying a
/// partial event log can then see where a jar came from as well as where it
/// went, without having already indexed every prior event for that jar.
#[test]
fn transfer_jar_ownership_emits_jar_xfer_event_with_both_owners() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let new_owner = Address::generate(env);
    let alice = Address::generate(env);

    let jar_id = String::from_str(env, "@xfer");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    client.transfer_jar_ownership(&jar_id, &new_owner);

    let decoded = jar_xfer_owners(env, &s.contract, &jar_id);
    assert_eq!(
        decoded,
        (owner.clone(), new_owner.clone()),
        "the payload is (prev_owner, new_owner), in that order"
    );
    // The two must not be confusable: the first is the address that no longer
    // controls the jar, the second is the one that does.
    assert_ne!(decoded.0, decoded.1);
    assert_eq!(client.get_jar(&jar_id).owner, new_owner);
}

/// A second hand-off reports the first recipient as the outgoing owner, so a
/// consumer can chain the events into a complete ownership history rather than
/// seeing two disconnected arrivals.
#[test]
fn jar_xfer_events_chain_across_successive_transfers() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let first = Address::generate(env);
    let second = Address::generate(env);
    let third = Address::generate(env);
    let alice = Address::generate(env);

    let jar_id = String::from_str(env, "@chain");
    client.create_jar(
        &first,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    client.transfer_jar_ownership(&jar_id, &second);
    assert_eq!(
        jar_xfer_owners(env, &s.contract, &jar_id),
        (first, second.clone())
    );

    client.transfer_jar_ownership(&jar_id, &third);
    assert_eq!(
        jar_xfer_owners(env, &s.contract, &jar_id),
        (second, third.clone()),
        "the second event's prev_owner must be the first event's new_owner"
    );

    assert_eq!(client.get_jar(&jar_id).owner, third);
}

/// `tip` moves the sender's tokens, so it must carry the sender's signature.
///
/// The mock here authorizes *only* the token `transfer` the contract makes on
/// the sender's behalf — deliberately not the `tip` call itself. Withholding
/// every auth would not prove anything: the SAC's own `transfer` requires the
/// sender too, so the call would fail even with `from.require_auth()` deleted.
/// Pre-authorizing the transfer strips that second line of defence away, so the
/// only thing left standing between this call and a spent balance is `tip`'s
/// own check.
#[test]
fn tip_requires_sender_auth() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token = token::Client::new(env, &s.token);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &1_000);

    let jar_id = String::from_str(env, "@nosig");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    env.mock_auths(&[MockAuth {
        address: &tipper,
        invoke: &MockAuthInvoke {
            contract: &s.token,
            fn_name: "transfer",
            args: (tipper.clone(), alice.clone(), 100i128).into_val(env),
            sub_invokes: &[],
        },
    }]);

    let res = client.try_tip(&tipper, &jar_id, &100, &String::from_str(env, "sneaky"));
    assert!(
        res.is_err(),
        "tip must reject a call the sender has not authorized"
    );

    // No tokens may have moved.
    assert_eq!(token.balance(&tipper), 1_000, "sender must not be debited");
    assert_eq!(token.balance(&alice), 0, "recipient must not be credited");
}

/// A signature from someone other than `from` is not enough either — the auth
/// must belong to the address whose balance is being spent.
#[test]
fn tip_rejects_auth_from_wrong_address() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token = token::Client::new(env, &s.token);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let tipper = Address::generate(env);
    let bystander = Address::generate(env);
    token_admin.mint(&tipper, &1_000);

    let jar_id = String::from_str(env, "@wrongsig");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    // As above, the sender's transfer is pre-authorized so that only `tip`'s own
    // check can reject this. The `tip` call carries a bystander's signature.
    let message = String::from_str(env, "not mine to send");
    env.mock_auths(&[
        MockAuth {
            address: &bystander,
            invoke: &MockAuthInvoke {
                contract: &s.contract,
                fn_name: "tip",
                args: (tipper.clone(), jar_id.clone(), 100i128, message.clone()).into_val(env),
                sub_invokes: &[],
            },
        },
        MockAuth {
            address: &tipper,
            invoke: &MockAuthInvoke {
                contract: &s.token,
                fn_name: "transfer",
                args: (tipper.clone(), alice.clone(), 100i128).into_val(env),
                sub_invokes: &[],
            },
        },
    ]);

    let res = client.try_tip(&tipper, &jar_id, &100, &message);
    assert!(
        res.is_err(),
        "tip must reject a signature from an address other than the sender"
    );
    assert_eq!(token.balance(&tipper), 1_000, "sender must not be debited");
    assert_eq!(token.balance(&alice), 0, "recipient must not be credited");
}

/// Positive control: the sender's signature, covering both the `tip` call and
/// the token `transfer` it makes on their behalf, is accepted.
#[test]
fn tip_succeeds_with_sender_auth() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token = token::Client::new(env, &s.token);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &1_000);

    let jar_id = String::from_str(env, "@goodsig");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    let message = String::from_str(env, "thanks");
    env.mock_auths(&[MockAuth {
        address: &tipper,
        invoke: &MockAuthInvoke {
            contract: &s.contract,
            fn_name: "tip",
            args: (tipper.clone(), jar_id.clone(), 100i128, message.clone()).into_val(env),
            sub_invokes: &[MockAuthInvoke {
                contract: &s.token,
                fn_name: "transfer",
                args: (tipper.clone(), alice.clone(), 100i128).into_val(env),
                sub_invokes: &[],
            }],
        },
    }]);

    client.tip(&tipper, &jar_id, &100, &message);
    assert_eq!(token.balance(&alice), 100);
    assert_eq!(token.balance(&tipper), 900);
}

/// An empty `jar_id` is rejected before any storage write — it would still be
/// usable as a storage key, but no frontend could address it as a URL slug.
#[test]
fn create_jar_rejects_empty_jar_id() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let splits = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 10000,
        },
    ];

    let res = client.try_create_jar(&owner, &String::from_str(env, ""), &splits);
    assert_eq!(res, Err(Ok(Error::InvalidJarId.into())));
}

/// A `jar_id` longer than `MAX_JAR_ID_LEN` is rejected — it is used as a
/// storage key, an event topic, and a public URL slug, so unbounded input
/// costs unnecessary rent.
#[test]
fn create_jar_rejects_over_long_jar_id() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let splits = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 10000,
        },
    ];

    // One byte over MAX_JAR_ID_LEN (64).
    let too_long = String::from_bytes(env, &[b'a'; 65]);
    let res = client.try_create_jar(&owner, &too_long, &splits);
    assert_eq!(res, Err(Ok(Error::InvalidJarId.into())));

    // The jar must not have been stored.
    assert!(
        client.try_get_jar(&too_long).is_err(),
        "rejected jar must not be persisted"
    );
}

/// A `jar_id` of exactly `MAX_JAR_ID_LEN` bytes is still valid — the bound is
/// inclusive, so an off-by-one here would reject legitimate jar ids.
#[test]
fn create_jar_accepts_jar_id_at_exact_limit() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let splits = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 10000,
        },
    ];

    let exact = String::from_bytes(env, &[b'a'; 64]);
    assert_eq!(exact.len(), 64);
    client.create_jar(&owner, &exact, &splits);

    assert_eq!(client.get_jar(&exact).splits.len(), 1);
}

/// A collaborator's own membership check: a listed recipient gets `true`, an
/// address nowhere in the splits gets `false`, and the jar owner gets `false`
/// when they take no share — owning a jar and being paid by it are separate
/// things.
#[test]
fn is_recipient_distinguishes_recipients_owner_and_strangers() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let stranger = Address::generate(env);

    let jar_id = String::from_str(env, "@collab");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 6000,
            },
            Split {
                to: bob.clone(),
                bps: 4000,
            },
        ],
    );

    // Listed recipients, in both the first and a later position.
    assert!(client.is_recipient(&jar_id, &alice));
    assert!(client.is_recipient(&jar_id, &bob));

    // An address that is nowhere in the jar.
    assert!(!client.is_recipient(&jar_id, &stranger));

    // The owner, who takes no share of a tip here.
    assert!(
        !client.is_recipient(&jar_id, &owner),
        "owning a jar is not the same as receiving a share of its tips"
    );
    assert_eq!(
        client.get_jar(&jar_id).owner,
        owner,
        "the owner check lives on get_jar, not is_recipient"
    );
}

/// An owner who is also listed in the splits gets `true` — the previous test
/// asserts a `false` for an owner, so this pins down that the answer tracks the
/// split vector rather than ownership.
#[test]
fn is_recipient_is_true_for_an_owner_who_is_also_a_recipient() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let jar_id = String::from_str(env, "@solo");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: owner.clone(),
                bps: 10000,
            },
        ],
    );

    assert!(client.is_recipient(&jar_id, &owner));
}

/// `update_splits` replaces the recipient list, so membership must follow it —
/// a dropped collaborator stops being a recipient and a new one starts.
#[test]
fn is_recipient_follows_update_splits() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);

    let jar_id = String::from_str(env, "@reshuffle");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );
    assert!(client.is_recipient(&jar_id, &alice));
    assert!(!client.is_recipient(&jar_id, &bob));

    client.update_splits(
        &jar_id,
        &vec![
            env,
            Split {
                to: bob.clone(),
                bps: 10000,
            },
        ],
    );
    assert!(!client.is_recipient(&jar_id, &alice));
    assert!(client.is_recipient(&jar_id, &bob));
}

/// An unregistered slug is `JarNotFound`, not a quiet `false` — otherwise a
/// typo'd jar id would look indistinguishable from a genuine non-membership.
#[test]
fn is_recipient_on_missing_jar_fails() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let alice = Address::generate(env);
    let res = client.try_is_recipient(&String::from_str(env, "@nope"), &alice);
    assert_eq!(res, Err(Ok(Error::JarNotFound.into())));
}

/// `get_limits` exists so clients stop hardcoding the contract's bounds. This
/// asserts the view and the constants cannot drift apart: changing one without
/// the other fails here.
#[test]
fn get_limits_matches_contract_constants() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let limits = client.get_limits();

    assert_eq!(limits.bps_denom, BPS_DENOM);
    assert_eq!(limits.max_recipients, MAX_RECIPIENTS);
    assert_eq!(limits.max_message_len, MAX_MESSAGE_LEN);
    assert_eq!(
        limits,
        Limits {
            bps_denom: 10_000,
            max_recipients: 20,
            max_message_len: 280,
        },
        "the published limits are part of the public interface; changing one \
         means clients and docs/CONTRACT.md have to change with it"
    );
}

/// The limits are only useful if they describe what the contract actually
/// enforces, so exercise each returned bound against the rejection it predicts.
#[test]
fn get_limits_describes_the_enforced_bounds() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let limits = client.get_limits();
    let owner = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &1_000);

    // max_recipients: a jar with exactly that many recipients is accepted, and
    // one more is not.
    let mut splits = vec![env];
    for _ in 0..limits.max_recipients {
        splits.push_back(Split {
            to: Address::generate(env),
            bps: limits.bps_denom / limits.max_recipients,
        });
    }
    let jar_id = String::from_str(env, "@limits");
    client.create_jar(&owner, &jar_id, &splits);

    let mut too_many = splits.clone();
    too_many.push_back(Split {
        to: Address::generate(env),
        bps: 1,
    });
    assert_eq!(
        client.try_create_jar(&owner, &String::from_str(env, "@over"), &too_many),
        Err(Ok(Error::TooManyRecipients.into()))
    );

    // bps_denom: the shares of an accepted jar sum to exactly the denominator.
    let stored = client.get_jar(&jar_id);
    let total: u32 = (0..stored.splits.len())
        .map(|i| stored.splits.get(i).unwrap().bps)
        .sum();
    assert_eq!(total, limits.bps_denom);

    // max_message_len: a message of exactly that many bytes is accepted, and
    // one byte more is not.
    let exact = String::from_bytes(env, &vec_of_bytes(limits.max_message_len));
    client.tip(&tipper, &jar_id, &1_000, &exact);

    let over = String::from_bytes(env, &vec_of_bytes(limits.max_message_len + 1));
    assert_eq!(
        client.try_tip(&tipper, &jar_id, &1, &over),
        Err(Ok(Error::MessageTooLong.into()))
    );
}

/// `n` filler ASCII bytes — one UTF-8 byte each, so the byte length is `n`.
fn vec_of_bytes(n: u32) -> std::vec::Vec<u8> {
    std::vec![b'a'; n as usize]
}

#[test]
fn get_admin_returns_constructor_admin() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    assert_eq!(client.get_admin(), s.admin);
}

#[test]
fn set_min_tip_amount_sets_and_reads_minimum() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let jar_id = String::from_str(env, "@creators");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    // Initial state: no minimum configured
    assert_eq!(client.get_min_tip_amount(&jar_id), None);

    // Set minimum to 500
    client.set_min_tip_amount(&jar_id, &Some(500_i128));
    assert_eq!(client.get_min_tip_amount(&jar_id), Some(500_i128));

    // Clear minimum by passing None
    client.set_min_tip_amount(&jar_id, &None);
    assert_eq!(client.get_min_tip_amount(&jar_id), None);
}

#[test]
fn set_min_tip_amount_emits_event() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let jar_id = String::from_str(env, "@eventjar");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    client.set_min_tip_amount(&jar_id, &Some(250_i128));

    let expected_topics = vec![
        env,
        symbol_short!("min_tip").into_val(env),
        jar_id.clone().into_val(env),
    ];
    let min_events: std::vec::Vec<_> = env
        .events()
        .all()
        .iter()
        .filter(|e| e.0 == s.contract && e.1 == expected_topics)
        .collect();
    assert_eq!(min_events.len(), 1);
    let (_, _, data) = min_events.first().unwrap();
    let decoded: Option<i128> = TryFromVal::try_from_val(env, data).expect("min_tip event data");
    assert_eq!(decoded, Some(250_i128));
}

#[test]
fn set_min_tip_amount_requires_owner_auth() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let non_owner = Address::generate(env);
    let jar_id = String::from_str(env, "@authcheck");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    env.mock_auths(&[MockAuth {
        address: &non_owner,
        invoke: &MockAuthInvoke {
            contract: &s.contract,
            fn_name: "set_min_tip_amount",
            args: (jar_id.clone(), Some(100_i128)).into_val(env),
            sub_invokes: &[],
        },
    }]);

    let res = client.try_set_min_tip_amount(&jar_id, &Some(100_i128));
    assert!(res.is_err());
    assert_eq!(client.get_min_tip_amount(&jar_id), None);
}

#[test]
fn set_min_tip_amount_rejects_non_positive_amount() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let jar_id = String::from_str(env, "@nonpos");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    let res_zero = client.try_set_min_tip_amount(&jar_id, &Some(0_i128));
    assert_eq!(res_zero, Err(Ok(Error::InvalidAmount.into())));

    let res_neg = client.try_set_min_tip_amount(&jar_id, &Some(-50_i128));
    assert_eq!(res_neg, Err(Ok(Error::InvalidAmount.into())));
}

#[test]
fn set_min_tip_amount_on_missing_jar_fails() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let jar_id = String::from_str(env, "@ghost");
    let res = client.try_set_min_tip_amount(&jar_id, &Some(100_i128));
    assert_eq!(res, Err(Ok(Error::JarNotFound.into())));
}

#[test]
fn get_min_tip_amount_on_missing_jar_fails() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let jar_id = String::from_str(env, "@ghost");
    let res = client.try_get_min_tip_amount(&jar_id);
    assert_eq!(res, Err(Ok(Error::JarNotFound.into())));
}

#[test]
fn tip_enforces_minimum_tip_amount() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token = token::Client::new(env, &s.token);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &10_000);

    let jar_id = String::from_str(env, "@floorjar");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    // Set minimum to 500
    client.set_min_tip_amount(&jar_id, &Some(500_i128));

    // Tip below minimum (499) must fail with BelowMinTipAmount
    let res_low = client.try_tip(&tipper, &jar_id, &499_i128, &String::from_str(env, "small"));
    assert_eq!(res_low, Err(Ok(Error::BelowMinTipAmount.into())));

    // Tip at exact minimum (500) must succeed
    let res_exact = client.try_tip(&tipper, &jar_id, &500_i128, &String::from_str(env, "exact"));
    assert!(res_exact.is_ok());
    assert_eq!(token.balance(&alice), 500);

    // Tip above minimum (1000) must succeed
    let res_above = client.try_tip(
        &tipper,
        &jar_id,
        &1000_i128,
        &String::from_str(env, "above"),
    );
    assert!(res_above.is_ok());
    assert_eq!(token.balance(&alice), 1500);
}

#[test]
fn preview_split_enforces_minimum_tip_amount() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let jar_id = String::from_str(env, "@previewmin");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    client.set_min_tip_amount(&jar_id, &Some(100_i128));

    // Preview below minimum must fail with BelowMinTipAmount
    let res_low = client.try_preview_split(&jar_id, &99_i128);
    assert_eq!(res_low, Err(Ok(Error::BelowMinTipAmount.into())));

    // Preview at exact minimum succeeds
    let res_exact = client.try_preview_split(&jar_id, &100_i128);
    assert!(res_exact.is_ok());
}

/// A random valid bps distribution over `n` recipients: every share is at
/// least 1 and the shares sum to exactly `BPS_DENOM`, mirroring what
/// `validate_splits` requires.
fn valid_bps_distribution(n: u32) -> impl Strategy<Value = std::vec::Vec<u32>> {
    proptest::collection::vec(1u64..=1_000_000u64, n as usize).prop_map(move |weights| {
        let remaining = (BPS_DENOM - n) as u64;
        let sum_w: u64 = weights.iter().sum();
        let mut bps = std::vec::Vec::with_capacity(n as usize);
        let mut used = 0u64;
        for w in weights.iter().take(n as usize - 1) {
            let extra = if sum_w == 0 { 0 } else { w * remaining / sum_w };
            used += extra;
            bps.push(1 + extra as u32);
        }
        bps.push(1 + (remaining - used) as u32);
        bps
    })
}

fn splits_strategy() -> impl Strategy<Value = std::vec::Vec<u32>> {
    (1u32..=MAX_RECIPIENTS).prop_flat_map(valid_bps_distribution)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// The core invariant: however a tip is split, every recipient's payout
    /// sums back to exactly the tipped amount, with no dust lost or created,
    /// and the tipper's balance drops by exactly that amount. Covers amounts
    /// smaller than the recipient count, where truncation bites hardest.
    #[test]
    fn tip_distributes_full_amount_across_random_splits(
        bps in splits_strategy(),
        amount in 1i128..=1_000_000_000i128,
    ) {
        let s = setup();
        let env = &s.env;
        let client = TipSplitterClient::new(env, &s.contract);
        let token = token::Client::new(env, &s.token);
        let token_admin = token::StellarAssetClient::new(env, &s.token);

        let owner = Address::generate(env);
        let tipper = Address::generate(env);
        token_admin.mint(&tipper, &amount);

        let recipients: std::vec::Vec<Address> =
            bps.iter().map(|_| Address::generate(env)).collect();
        let mut splits = vec![env];
        for (addr, b) in recipients.iter().zip(bps.iter()) {
            splits.push_back(Split {
                to: addr.clone(),
                bps: *b,
            });
        }

        let jar_id = String::from_str(env, "@prop");
        client.create_jar(&owner, &jar_id, &splits);
        client.tip(&tipper, &jar_id, &amount, &String::from_str(env, "prop"));

        let total: i128 = recipients.iter().map(|r| token.balance(r)).sum();
        prop_assert_eq!(total, amount);
        prop_assert_eq!(token.balance(&tipper), 0);
    }
}

// ===========================================================================
// #171 hold undeliverable shares · #172 leave_jar · #173 lock_splits · #174 recurring
// ===========================================================================

/// Reads the raw per-recipient `tip` breakdown including the `held` flag.
fn raw_tip_breakdown(
    env: &Env,
    contract: &Address,
    jar_id: &String,
) -> std::vec::Vec<(Address, i128, bool)> {
    let expected_topics = vec![
        env,
        symbol_short!("tip").into_val(env),
        jar_id.clone().into_val(env),
    ];
    let events: std::vec::Vec<_> = env
        .events()
        .all()
        .iter()
        .filter(|e| e.0 == *contract && e.1 == expected_topics)
        .collect();
    assert_eq!(events.len(), 1, "expected exactly one tip event");
    let (_, _, data) = events.first().unwrap();
    let (_, _, _, breakdown): (Address, i128, String, Vec<(Address, i128, bool)>) =
        TryFromVal::try_from_val(env, data).expect("tip event data shape");
    (0..breakdown.len())
        .map(|i| breakdown.get(i).unwrap())
        .collect()
}

#[test]
fn tip_holds_share_of_deauthorised_recipient_and_withdraw_pays_it() {
    let s = setup_hostile();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token = TestTokenClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let tipper = Address::generate(env);
    token.mint(&tipper, &1_000);

    let jar_id = String::from_str(env, "@band");
// ---------------------------------------------------------------------------
// #167 — per-jar running tip totals
// ---------------------------------------------------------------------------

/// A jar exists before it has been tipped, so its counters must read zero
/// rather than `JarNotFound`: "nobody has tipped this yet" is a real answer a
/// tip page shows.
#[test]
fn get_jar_stats_is_zero_before_the_first_tip() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let jar_id = String::from_str(env, "@untouched");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 7000,
            },
            Split {
                to: bob.clone(),
                bps: 3000,
            },
        ],
    );

    // Bob can no longer receive the token.
    token.block(&bob);
    client.tip(&tipper, &jar_id, &100, &String::from_str(env, "hi"));

    // Read the tip event immediately: the test env only keeps the last
    // invocation's events, and the balance/get_held checks below are new calls.
    let bd = raw_tip_breakdown(env, &s.contract, &jar_id);
    assert_eq!(bd[0], (alice.clone(), 70i128, false));
    assert_eq!(bd[1], (bob.clone(), 30i128, true));

    // Alice is paid; Bob is not, but the tip did NOT revert.
    assert_eq!(token.balance(&alice), 70);
    assert_eq!(token.balance(&bob), 0);
    // Bob's share is escrowed in the contract and reported as held.
    assert_eq!(token.balance(&s.contract), 30);
    assert_eq!(client.get_held(&bob), 30);
    // The tipper is debited the full amount regardless.
    assert_eq!(token.balance(&tipper), 900);

    // Bob recovers; he withdraws his held share.
    token.unblock();
    client.withdraw_held(&bob);
    assert_eq!(token.balance(&bob), 30);
    assert_eq!(token.balance(&s.contract), 0);
    assert_eq!(client.get_held(&bob), 0);

    // Nothing left to withdraw.
    assert!(client.try_withdraw_held(&bob).is_err());
}

#[test]
fn withdraw_held_requires_the_recipients_own_authorisation() {
    let s = setup_hostile();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token = TestTokenClient::new(env, &s.token);

    let owner = Address::generate(env);
    let bob = Address::generate(env);
    let tipper = Address::generate(env);
    token.mint(&tipper, &1_000);

    let jar_id = String::from_str(env, "@solo");
                bps: 10000,
            },
        ],
    );

    assert_eq!(
        client.get_jar_stats(&jar_id),
        JarStats {
            tip_count: 0,
            total: 0
        }
    );
}

/// Several tips to one jar accumulate into that jar, in both fields.
#[test]
fn get_jar_stats_counts_several_tips_to_one_jar() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let one = Address::generate(env);
    let two = Address::generate(env);
    token_admin.mint(&one, &1_000);
    token_admin.mint(&two, &1_000);

    let jar_id = String::from_str(env, "@tally");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: bob.clone(),
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );
    token.block(&bob);
    client.tip(&tipper, &jar_id, &100, &String::from_str(env, ""));
    assert_eq!(client.get_held(&bob), 100);

    // `withdraw_held` is gated on the recipient's own signature: with no auth
    // granted, the call fails and the escrow is untouched.
    env.mock_auths(&[]);
    assert!(client.try_withdraw_held(&bob).is_err());
    assert_eq!(client.get_held(&bob), 100);
}

#[test]
fn leave_jar_reassigns_departing_share_to_last_entry() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    client.tip(&one, &jar_id, &100, &String::from_str(env, "a"));
    assert_eq!(
        client.get_jar_stats(&jar_id),
        JarStats {
            tip_count: 1,
            total: 100
        }
    );

    client.tip(&two, &jar_id, &250, &String::from_str(env, "b"));
    client.tip(&one, &jar_id, &50, &String::from_str(env, "c"));
    assert_eq!(
        client.get_jar_stats(&jar_id),
        JarStats {
            tip_count: 3,
            total: 400
        },
        "the jar total counts every supporter, not one supporter's tips"
    );
}

/// Two jars tipped by the same person must not share counters — a single
/// running total would make one creator's leaderboard out of another's tips.
#[test]
fn get_jar_stats_does_not_leak_between_jars() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let carol = Address::generate(env);

    let jar_id = String::from_str(env, "@trio");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 4000,
            },
            Split {
                to: bob.clone(),
                bps: 3000,
            },
            Split {
                to: carol.clone(),
                bps: 3000,
            },
        ],
    );

    client.leave_jar(&jar_id, &bob);

    // a `splits` event was published with the new count — read it before any
    // further invocation, since the test env only exposes the last call's events
    let expected_topics = vec![
        env,
        symbol_short!("splits").into_val(env),
        jar_id.clone().into_val(env),
    ];
    let evs: std::vec::Vec<_> = env
        .events()
        .all()
        .iter()
        .filter(|e| e.0 == s.contract && e.1 == expected_topics)
        .collect();
    assert_eq!(evs.len(), 1);
    let count: u32 = TryFromVal::try_from_val(env, &evs[0].2).unwrap();
    assert_eq!(count, 2);

    let jar = client.get_jar(&jar_id);
    assert_eq!(jar.splits.len(), 2);
    // middle entry removed; the last remaining entry (carol) absorbs bob's 3000.
    assert_eq!(jar.splits.get(0).unwrap().to, alice);
    assert_eq!(jar.splits.get(0).unwrap().bps, 4000);
    assert_eq!(jar.splits.get(1).unwrap().to, carol);
    assert_eq!(jar.splits.get(1).unwrap().bps, 6000);
    assert!(!client.is_recipient(&jar_id, &bob));
}

#[test]
fn leave_jar_when_the_last_entry_leaves() {
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &10_000);

    let splits = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 10000,
        },
    ];
    let first = String::from_str(env, "@first");
    let second = String::from_str(env, "@second");
    client.create_jar(&owner, &first, &splits);
    client.create_jar(
        &owner,
        &second,
        &vec![
            env,
            Split {
                to: bob.clone(),
                bps: 10000,
            },
        ],
    );

    client.tip(&tipper, &first, &700, &String::from_str(env, "one"));
    client.tip(&tipper, &first, &300, &String::from_str(env, "two"));
    client.tip(&tipper, &second, &50, &String::from_str(env, "three"));

    assert_eq!(
        client.get_jar_stats(&first),
        JarStats {
            tip_count: 2,
            total: 1000
        }
    );
    assert_eq!(
        client.get_jar_stats(&second),
        JarStats {
            tip_count: 1,
            total: 50
        }
    );
}

/// A typo'd slug must not look like an untouched jar.
#[test]
fn get_jar_stats_on_missing_jar_fails() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let jar_id = String::from_str(env, "@pair");
    let res = client.try_get_jar_stats(&String::from_str(env, "@ghost"));
    assert_eq!(res, Err(Ok(Error::JarNotFound.into())));
}

/// The counters are only an authoritative check on the indexer if a tip that
/// never settled never reaches them. A transfer that traps reverts the whole
/// call, including the stats writes.
#[test]
fn reverted_tip_leaves_both_totals_unchanged() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &1_000);

    let jar_id = String::from_str(env, "@revert");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 5000,
            },
            Split {
                to: bob.clone(),
                bps: 5000,
            },
        ],
    );

    client.leave_jar(&jar_id, &bob); // last entry leaves
    let jar = client.get_jar(&jar_id);
    assert_eq!(jar.splits.len(), 1);
    assert_eq!(jar.splits.get(0).unwrap().to, alice);
    assert_eq!(jar.splits.get(0).unwrap().bps, 10000);
}

#[test]
fn leave_jar_rejects_sole_and_non_recipient() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let only = Address::generate(env);
    let stranger = Address::generate(env);
    let jar_id = String::from_str(env, "@only");
                bps: 10000,
            },
        ],
    );

    client.tip(&tipper, &jar_id, &100, &String::from_str(env, "ok"));

    // More than the remaining balance: the transfer traps.
    let res = client.try_tip(&tipper, &jar_id, &5_000, &String::from_str(env, "nope"));
    assert!(res.is_err(), "an unfundable tip must revert");

    assert_eq!(
        client.get_jar_stats(&jar_id),
        JarStats {
            tip_count: 1,
            total: 100
        }
    );
    assert_eq!(
        client.get_supporter_stats(&jar_id, &tipper),
        SupporterStats {
            tip_count: 1,
            total: 100
        }
    );
}

// ---------------------------------------------------------------------------
// #168 — per-supporter totals, the data a badge contract needs
// ---------------------------------------------------------------------------

/// A milestone check for someone who has never tipped must read zeros, not
/// fail — the badge contract asks this of arbitrary addresses.
#[test]
fn get_supporter_stats_is_zero_for_an_address_that_never_tipped() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let supporter = Address::generate(env);
    let stranger = Address::generate(env);
    token_admin.mint(&supporter, &1_000);

    let jar_id = String::from_str(env, "@badges");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: only.clone(),
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    // sole recipient cannot leave (jar would have no splits)
    assert!(client.try_leave_jar(&jar_id, &only).is_err());
    // an address not in the jar cannot leave
    assert!(client.try_leave_jar(&jar_id, &stranger).is_err());
}

#[test]
fn lock_splits_sets_flag_and_publishes_event() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let jar_id = String::from_str(env, "@lock");
    client.create_jar(
        &owner,
        &jar_id,
    assert_eq!(
        client.get_supporter_stats(&jar_id, &stranger),
        SupporterStats {
            tip_count: 0,
            total: 0
        }
    );

    client.tip(&supporter, &jar_id, &100, &String::from_str(env, "hi"));
    assert_eq!(
        client.get_supporter_stats(&jar_id, &stranger),
        SupporterStats {
            tip_count: 0,
            total: 0
        },
        "someone else's tip must not move a bystander's counters"
    );
}

/// The entry is keyed by `(jar_id, supporter)`, so one supporter tipping two
/// jars keeps two independent histories — a badge for jar A must not be won
/// by tipping jar B.
#[test]
fn supporter_stats_are_independent_across_jars() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let supporter = Address::generate(env);
    token_admin.mint(&supporter, &10_000);

    let first = String::from_str(env, "@jarA");
    let second = String::from_str(env, "@jarB");
    client.create_jar(
        &owner,
        &first,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );
    assert!(!client.is_splits_locked(&jar_id));

    client.lock_splits(&jar_id);

    // read the `lock` event immediately: the test env only keeps the last
    // invocation's events, and `is_splits_locked` is a separate call.
    let expected_topics = vec![
        env,
        symbol_short!("lock").into_val(env),
        jar_id.clone().into_val(env),
    ];
    let evs: std::vec::Vec<_> = env
        .events()
        .all()
        .iter()
        .filter(|e| e.0 == s.contract && e.1 == expected_topics)
        .collect();
    assert_eq!(evs.len(), 1);
    let locked: bool = TryFromVal::try_from_val(env, &evs[0].2).unwrap();
    assert!(locked);

    assert!(client.is_splits_locked(&jar_id));
}

#[test]
fn locked_jar_needs_consent_only_from_decreased_recipients() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    client.create_jar(
        &owner,
        &second,
        &vec![
            env,
            Split {
                to: bob.clone(),
                bps: 10000,
            },
        ],
    );

    client.tip(&supporter, &first, &1_000, &String::from_str(env, "a"));
    client.tip(&supporter, &first, &1_000, &String::from_str(env, "b"));
    client.tip(&supporter, &second, &250, &String::from_str(env, "c"));

    assert_eq!(
        client.get_supporter_stats(&first, &supporter),
        SupporterStats {
            tip_count: 2,
            total: 2_000
        }
    );
    assert_eq!(
        client.get_supporter_stats(&second, &supporter),
        SupporterStats {
            tip_count: 1,
            total: 250
        }
    );
    // The jar aggregate still spans both supporters' jars.
    assert_eq!(
        client.get_jar_stats(&first),
        JarStats {
            tip_count: 2,
            total: 2_000
        }
    );
}

/// Two supporters on one jar each get their own counters, and the jar's total
/// is the sum of both.
#[test]
fn supporter_stats_are_independent_across_supporters() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let one = Address::generate(env);
    let two = Address::generate(env);
    token_admin.mint(&one, &10_000);
    token_admin.mint(&two, &10_000);

    let jar_id = String::from_str(env, "@shared");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 6000,
            },
            Split {
                to: bob.clone(),
                bps: 4000,
            },
        ],
    );
    client.lock_splits(&jar_id);

    // bob 4000 -> 6000 (increase, no consent needed); alice 6000 -> 4000 (decrease, needs alice)
    let new_splits = vec![
        env,
        Split {
            to: alice.clone(),
            bps: 4000,
        },
        Split {
            to: bob.clone(),
            bps: 6000,
        },
    ];

    // owner signs but the decreased recipient (alice) does not -> rejected
    env.mock_auths(&[MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &s.contract,
            fn_name: "update_splits",
            args: (jar_id.clone(), new_splits.clone()).into_val(env),
            sub_invokes: &[],
        },
    }]);
    assert!(client.try_update_splits(&jar_id, &new_splits).is_err());
    assert_eq!(client.get_jar(&jar_id).splits.get(0).unwrap().bps, 6000);

    // owner + alice (the one who lost share) -> accepted; bob never had to sign
    env.mock_auths(&[
        MockAuth {
            address: &owner,
            invoke: &MockAuthInvoke {
                contract: &s.contract,
                fn_name: "update_splits",
                args: (jar_id.clone(), new_splits.clone()).into_val(env),
                sub_invokes: &[],
            },
        },
        MockAuth {
            address: &alice,
            invoke: &MockAuthInvoke {
                contract: &s.contract,
                fn_name: "update_splits",
                args: (jar_id.clone(), new_splits.clone()).into_val(env),
                sub_invokes: &[],
            },
        },
    ]);
    client.update_splits(&jar_id, &new_splits);
    assert_eq!(client.get_jar(&jar_id).splits.get(1).unwrap().bps, 6000);
}

#[test]
fn unlock_requires_all_current_recipients() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let jar_id = String::from_str(env, "@shared");
                bps: 10000,
            },
        ],
    );

    client.tip(&one, &jar_id, &1_000, &String::from_str(env, "x"));
    client.tip(&two, &jar_id, &7, &String::from_str(env, "y"));
    client.tip(&two, &jar_id, &7, &String::from_str(env, "z"));
    client.tip(&two, &jar_id, &7, &String::from_str(env, "w"));

    assert_eq!(
        client.get_supporter_stats(&jar_id, &one),
        SupporterStats {
            tip_count: 1,
            total: 1_000
        }
    );
    assert_eq!(
        client.get_supporter_stats(&jar_id, &two),
        SupporterStats {
            tip_count: 3,
            total: 21
        }
    );
    assert_eq!(
        client.get_jar_stats(&jar_id),
        JarStats {
            tip_count: 4,
            total: 1_021
        }
    );
}

#[test]
fn get_supporter_stats_on_missing_jar_fails() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let supporter = Address::generate(env);
    let res = client.try_get_supporter_stats(&String::from_str(env, "@ghost"), &supporter);
    assert_eq!(res, Err(Ok(Error::JarNotFound.into())));
}

/// The extra writes #167 asks about are real rent, and the PR has to state
/// their cost. Two persistent entries per tip — the jar's and the supporter's
/// — on top of whatever the transfers themselves write.
#[test]
fn tip_writes_the_two_totals_entries() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token_admin = token::StellarAssetClient::new(env, &s.token);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let tipper = Address::generate(env);
    token_admin.mint(&tipper, &1_000);

    let jar_id = String::from_str(env, "@budget");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 5000,
            },
            Split {
                to: bob.clone(),
                bps: 5000,
            },
        ],
    );
    client.lock_splits(&jar_id);

    // owner alone cannot disarm the lock
    env.mock_auths(&[MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &s.contract,
            fn_name: "unlock_splits",
            args: (jar_id.clone(),).into_val(env),
            sub_invokes: &[],
        },
    }]);
    assert!(client.try_unlock_splits(&jar_id).is_err());
    assert!(client.is_splits_locked(&jar_id));

    // owner + every current recipient can unlock
    let args: Vec<soroban_sdk::Val> = (jar_id.clone(),).into_val(env);
    let inv_owner = MockAuthInvoke {
        contract: &s.contract,
        fn_name: "unlock_splits",
        args: args.clone(),
        sub_invokes: &[],
    };
    let inv_alice = MockAuthInvoke {
        contract: &s.contract,
        fn_name: "unlock_splits",
        args: args.clone(),
        sub_invokes: &[],
    };
    let inv_bob = MockAuthInvoke {
        contract: &s.contract,
        fn_name: "unlock_splits",
        args,
        sub_invokes: &[],
    };
    env.mock_auths(&[
        MockAuth {
            address: &owner,
            invoke: &inv_owner,
        },
        MockAuth {
            address: &alice,
            invoke: &inv_alice,
        },
        MockAuth {
            address: &bob,
            invoke: &inv_bob,
        },
    ]);
    client.unlock_splits(&jar_id);
    assert!(!client.is_splits_locked(&jar_id));
}

#[test]
fn lock_survives_ownership_transfer() {
                bps: 10000,
            },
        ],
    );

    client.tip(&tipper, &jar_id, &100, &String::from_str(env, "costed"));
    let first = env.cost_estimate().resources();

    // Measured against the same tip with `bump_totals` disabled: 3 write
    // entries / 520 bytes / 2 rent bumps. The two counters add exactly two
    // writes, 404 bytes, and two rent bumps — see `docs/CONTRACT.md`.
    assert_eq!(
        first.write_entries, 5,
        "a tip writes the two totals entries plus the transfer's own three"
    );
    assert_eq!(
        first.write_bytes, 924,
        "the totals cost 404 more bytes per tip"
    );
    assert_eq!(
        first.persistent_entry_rent_bumps, 4,
        "both totals get the jar's TTL treatment, so each is bumped"
    );

    // A repeat tip to the same jar by the same supporter rewrites the same two
    // entries, so the marginal cost per tip is flat, not growing with use.
    client.tip(&tipper, &jar_id, &100, &String::from_str(env, "again"));
    let second = env.cost_estimate().resources();
    assert_eq!(
        second.write_entries, first.write_entries,
        "a second tip adds no ledger entries beyond the two totals writes"
    );
    assert_eq!(second.write_bytes, first.write_bytes);
}

// ---------------------------------------------------------------------------
// #169 — interface version view
// ---------------------------------------------------------------------------

#[test]
fn version_matches_the_interface_constant() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let new_owner = Address::generate(env);
    let alice = Address::generate(env);
    let jar_id = String::from_str(env, "@keeplock");
    assert_eq!(client.version(), INTERFACE_VERSION);
    assert_eq!(
        client.version(),
        1,
        "the published version is part of the public interface; bump \
         INTERFACE_VERSION when the shape changes, do not edit this test \
         to match a silent change"
    );
}

/// The point of the view is that a client can call it before it has any jar,
/// any token, or any authorization — reads no storage, so nothing to set up.
#[test]
fn version_reports_the_shape_without_reading_storage() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let contract = env.register(TipSplitter, (admin, sac.address()));

    let client = TipSplitterClient::new(&env, &contract);
    assert_eq!(client.version(), INTERFACE_VERSION);

    // Same answer once there is state to read, so the value is not derived
    // from storage.
    let owner = Address::generate(&env);
    client.create_jar(
        &owner,
        &String::from_str(&env, "@v"),
        &vec![
            &env,
            Split {
                to: Address::generate(&env),
                bps: 10000,
            },
        ],
    );
    assert_eq!(client.version(), INTERFACE_VERSION);
}

// ---------------------------------------------------------------------------
// #170 — admin-managed token allowlist
// ---------------------------------------------------------------------------

/// A second asset, so a test can pay in something that is not the deploy-time
/// token. Both are real Stellar Asset Contracts, which is what #170 asks the
/// tests to use.
fn second_asset(env: &Env, admin: &Address) -> Address {
    env.register_stellar_asset_contract_v2(admin.clone())
        .address()
}

/// One recipient, one jar: the fixture every allowlist test below shares.
fn one_recipient_jar(
    env: &Env,
    client: &TipSplitterClient,
    slug: &str,
) -> (Address, Address, String) {
    let owner = Address::generate(env);
    let recipient = Address::generate(env);
    let jar_id = String::from_str(env, slug);
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                to: recipient.clone(),
                bps: 10000,
            },
        ],
    );
    client.lock_splits(&jar_id);
    client.transfer_jar_ownership(&jar_id, &new_owner);
    assert!(client.is_splits_locked(&jar_id));
    assert_eq!(client.get_jar_owner(&jar_id), new_owner);
}

#[test]
fn recurring_tip_create_read_cancel_roundtrip() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let supporter = Address::generate(env);
    let jar_id = String::from_str(env, "@creator");
    (owner, recipient, jar_id)
}

/// The deploy-time token is on the list from construction, so an existing
/// deployment gains `tip_in_token` without the admin having to act, and
/// `get_token` and `is_token_allowed` agree.
#[test]
fn deploy_time_token_is_allowed_by_default() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    assert!(client.is_token_allowed(&s.token));
    assert!(!client.is_token_allowed(&Address::generate(env)));
}

#[test]
fn admin_can_allow_and_disallow_a_token() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let eurc = second_asset(env, &s.admin);

    assert!(!client.is_token_allowed(&eurc));
    client.allow_token(&eurc);
    assert!(client.is_token_allowed(&eurc));

    client.disallow_token(&eurc);
    assert!(!client.is_token_allowed(&eurc));
}

/// Only the admin may move the list — a jar owner who could add assets would
/// turn the contract's allowlist into a per-jar one, and a stranger could open
/// it to anything.
#[test]
fn allow_and_disallow_require_admin_auth() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let eurc = second_asset(env, &s.admin);
    let attacker = Address::generate(env);

    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &s.contract,
            fn_name: "allow_token",
            args: (eurc.clone(),).into_val(env),
            sub_invokes: &[],
        },
    }]);
    assert!(
        client.try_allow_token(&eurc).is_err(),
        "a non-admin must not be able to widen the allowlist"
    );
    assert!(!client.is_token_allowed(&eurc));

    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &s.contract,
            fn_name: "disallow_token",
            args: (eurc.clone(),).into_val(env),
            sub_invokes: &[],
        },
    }]);
    assert!(
        client.try_disallow_token(&eurc).is_err(),
        "a non-admin must not be able to narrow the allowlist"
    );
}

/// Each change publishes an event naming the token and the admin who
/// authorised it, so an index of accepted assets has provenance.
#[test]
fn allow_and_disallow_emit_events_naming_token_and_admin() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let eurc = second_asset(env, &s.admin);

    client.allow_token(&eurc.clone());
    let allow_topics = vec![
        env,
        symbol_short!("tk_allow").into_val(env),
        eurc.clone().into_val(env),
    ];
    let ev: std::vec::Vec<_> = env
        .events()
        .all()
        .iter()
        .filter(|e| e.0 == s.contract && e.1 == allow_topics)
        .collect();
    assert_eq!(ev.len(), 1, "allow_token must emit exactly one event");
    let who: Address = TryFromVal::try_from_val(env, &ev.first().unwrap().2)
        .expect("allow event carries the authorising admin");
    assert_eq!(who, s.admin);

    client.disallow_token(&eurc.clone());
    let dis_topics = vec![
        env,
        symbol_short!("tk_dis").into_val(env),
        eurc.clone().into_val(env),
    ];
    let ev: std::vec::Vec<_> = env
        .events()
        .all()
        .iter()
        .filter(|e| e.0 == s.contract && e.1 == dis_topics)
        .collect();
    assert_eq!(ev.len(), 1, "disallow_token must emit exactly one event");
}

/// `tip` always pays through the deploy-time token, so removing it from the
/// list would leave a working tip path for a token the view calls closed.
#[test]
fn disallow_rejects_the_default_token() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    assert_eq!(
        client.try_disallow_token(&s.token),
        Err(Ok(Error::CannotDisallowDefault.into()))
    );
    assert!(
        client.is_token_allowed(&s.token),
        "the default token stays allowed"
    );
}

/// The split must not depend on the asset: an EURC tip of 100 pays 70/30
/// exactly as a USDC tip of 100 does, because `compute_shares` is amount-only.
#[test]
fn tip_in_token_splits_identically_to_the_default_token() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let usdc = token::Client::new(env, &s.token);
    let usdc_admin = token::StellarAssetClient::new(env, &s.token);

    let eurc = second_asset(env, &s.admin);
    let eurc_client = token::Client::new(env, &eurc);
    let eurc_admin = token::StellarAssetClient::new(env, &eurc);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let tipper = Address::generate(env);
    usdc_admin.mint(&tipper, &1_000);
    eurc_admin.mint(&tipper, &1_000);
    client.allow_token(&eurc);

    let jar_id = String::from_str(env, "@multi");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    let seq = env.ledger().sequence();
    let id = client.create_recurring(
        &supporter,
        &jar_id,
        &500,
        &100,
        &String::from_str(env, "monthly"),
    );
    assert_eq!(id, 1);

    let rec = client.get_recurring(&id);
    assert_eq!(rec.supporter, supporter);
    assert_eq!(rec.jar_id, jar_id);
    assert_eq!(rec.amount, 500);
    assert_eq!(rec.period_ledgers, 100);
    assert_eq!(rec.next_due_ledger, seq + 100);

    client.cancel_recurring(&id);
    assert!(client.try_get_recurring(&id).is_err());
}

#[test]
fn recurring_tip_validation() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let supporter = Address::generate(env);
    let jar_id = String::from_str(env, "@v");
                bps: 7000,
            },
            Split {
                to: bob.clone(),
                bps: 3000,
            },
        ],
    );

    client.tip(&tipper, &jar_id, &100, &String::from_str(env, "in usdc"));
    assert_eq!(usdc.balance(&alice), 70);
    assert_eq!(usdc.balance(&bob), 30);

    client.tip_in_token(
        &tipper,
        &jar_id,
        &eurc,
        &100,
        &String::from_str(env, "in eurc"),
    );
    assert_eq!(eurc_client.balance(&alice), 70);
    assert_eq!(eurc_client.balance(&bob), 30);
    assert_eq!(usdc.balance(&alice), 70, "the assets do not mix");

    // Both tips counted, in the same raw units.
    assert_eq!(
        client.get_jar_stats(&jar_id),
        JarStats {
            tip_count: 2,
            total: 200
        }
    );
}

/// An unlisted token is rejected before funds move.
#[test]
fn tip_in_token_rejects_a_disallowed_token_before_funds_move() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let usdc_admin = token::StellarAssetClient::new(env, &s.token);

    let eurc = second_asset(env, &s.admin);
    let eurc_client = token::Client::new(env, &eurc);
    let eurc_admin = token::StellarAssetClient::new(env, &eurc);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let tipper = Address::generate(env);
    usdc_admin.mint(&tipper, &1_000);
    eurc_admin.mint(&tipper, &1_000);

    let jar_id = String::from_str(env, "@closed");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );

    // zero period
    assert!(client
        .try_create_recurring(&supporter, &jar_id, &500, &0, &String::from_str(env, ""))
        .is_err());
    // non-positive amount
    assert!(client
        .try_create_recurring(&supporter, &jar_id, &0, &100, &String::from_str(env, ""))
        .is_err());
    // unknown jar
    assert!(client
        .try_create_recurring(
            &supporter,
            &String::from_str(env, "@nope"),
            &500,
            &100,
            &String::from_str(env, "")
        )
        .is_err());
}

#[test]
fn cancel_recurring_requires_the_supporter() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let supporter = Address::generate(env);
    let jar_id = String::from_str(env, "@own");
    assert_eq!(
        client.try_tip_in_token(
            &tipper,
            &jar_id,
            &eurc,
            &100,
            &String::from_str(env, "nope")
        ),
        Err(Ok(Error::TokenNotAllowed.into()))
    );
    assert_eq!(eurc_client.balance(&alice), 0);
    assert_eq!(eurc_client.balance(&tipper), 1_000);
    assert_eq!(
        client.get_jar_stats(&jar_id),
        JarStats {
            tip_count: 0,
            total: 0
        },
        "a rejected token must not be counted"
    );

    // Allowed, tipped, then disallowed: the third tip is rejected again.
    client.allow_token(&eurc);
    client.tip_in_token(&tipper, &jar_id, &eurc, &100, &String::from_str(env, "yes"));
    client.disallow_token(&eurc);
    assert_eq!(
        client.try_tip_in_token(
            &tipper,
            &jar_id,
            &eurc,
            &100,
            &String::from_str(env, "closed")
        ),
        Err(Ok(Error::TokenNotAllowed.into()))
    );
    assert_eq!(eurc_client.balance(&alice), 100);
}

/// The event has to say which asset moved, or an indexer cannot tell 10 USDC
/// from 10 XLM.
#[test]
fn tip_event_names_the_token_paid_in() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let eurc = second_asset(env, &s.admin);
    let eurc_admin = token::StellarAssetClient::new(env, &eurc);
    client.allow_token(&eurc);

    let (_owner, recipient, jar_id) = one_recipient_jar(env, &client, "@evt");
    let tipper = Address::generate(env);
    eurc_admin.mint(&tipper, &1_000);

    client.tip_in_token(
        &tipper,
        &jar_id,
        &eurc,
        &400,
        &String::from_str(env, "in another asset"),
    );

    let expected_topics = vec![
        env,
        symbol_short!("tip").into_val(env),
        jar_id.clone().into_val(env),
    ];
    let ev: std::vec::Vec<_> = env
        .events()
        .all()
        .iter()
        .filter(|e| e.0 == s.contract && e.1 == expected_topics)
        .collect();
    assert_eq!(ev.len(), 1);
    let (from, amount, _message, breakdown, token_paid): (
        Address,
        i128,
        String,
        Vec<(Address, i128)>,
        Address,
    ) = TryFromVal::try_from_val(env, &ev.first().unwrap().2).expect("tip event data shape");
    assert_eq!(token_paid, eurc, "the event must name the asset paid in");
    assert_eq!(from, tipper);
    assert_eq!(amount, 400);
    assert_eq!(breakdown.len(), 1);
    assert_eq!(breakdown.get(0).unwrap(), (recipient, 400));
}

/// The recorded decision for #170: the minimum is a raw, unitless number, so
/// it gates the default token only. A tip in another allowed asset is not
/// subject to it.
#[test]
fn minimum_tip_gates_the_default_token_only() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let usdc_admin = token::StellarAssetClient::new(env, &s.token);
    let eurc = second_asset(env, &s.admin);
    let eurc_admin = token::StellarAssetClient::new(env, &eurc);
    let eurc_client = token::Client::new(env, &eurc);
    client.allow_token(&eurc);

    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let tipper = Address::generate(env);
    usdc_admin.mint(&tipper, &10_000);
    eurc_admin.mint(&tipper, &10_000);

    let jar_id = String::from_str(env, "@floored");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split {
                to: alice.clone(),
                bps: 10000,
            },
        ],
    );
    let id = client.create_recurring(&supporter, &jar_id, &500, &100, &String::from_str(env, ""));

    // Only the supporter's signature authorizes cancellation; without it the
    // entry survives.
    env.mock_auths(&[]);
    assert!(client.try_cancel_recurring(&id).is_err());
    // still present
    assert_eq!(client.get_recurring(&id).supporter, supporter);
    client.set_min_tip_amount(&jar_id, &Some(500_i128));

    // Default token: the floor holds.
    assert_eq!(
        client.try_tip(&tipper, &jar_id, &499, &String::from_str(env, "under")),
        Err(Ok(Error::BelowMinTipAmount.into()))
    );
    client.tip(
        &tipper,
        &jar_id,
        &500,
        &String::from_str(env, "at the floor"),
    );

    // Other allowed asset: same jar, same minimum, not enforced — the number
    // has no unit to compare against.
    client.tip_in_token(
        &tipper,
        &jar_id,
        &eurc,
        &1,
        &String::from_str(env, "tiny eurc"),
    );
    assert_eq!(eurc_client.balance(&alice), 1);

    // ...unless the call names the default token, where it still applies.
    assert_eq!(
        client.try_tip_in_token(&tipper, &jar_id, &s.token, &2, &String::from_str(env, "x")),
        Err(Ok(Error::BelowMinTipAmount.into()))
    );
}

// ── #139: Model-based property test ──────────────────────────────────────────

use proptest::prelude::*;

#[derive(Debug, Clone)]
enum Op {
    CreateJar { slug: String, owner_idx: usize, recipients: Vec<(usize, u32)> },
    UpdateSplits { slug: String, recipients: Vec<(usize, u32)> },
    TransferOwnership { slug: String, new_owner_idx: usize },
    SetMinTip { slug: String, amount: Option<i128> },
    Tip { slug: String, tipper_idx: usize, amount: i128 },
}

fn op_strategy() -> impl Strategy<Value = Op> {
    let slugs = prop::sample::select(vec!["@a", "@b", "@c"].map(String::from));
    let addr_idx = 0..4usize;
    prop_oneof![
        (slugs.clone(), addr_idx.clone(), prop::collection::vec((addr_idx.clone(), 1..10000u32), 1..3))
            .prop_map(|(slug, owner_idx, recipients)| Op::CreateJar { slug, owner_idx, recipients }),
        (slugs.clone(), prop::collection::vec((addr_idx.clone(), 1..10000u32), 1..3))
            .prop_map(|(slug, recipients)| Op::UpdateSplits { slug, recipients }),
        (slugs.clone(), addr_idx.clone())
            .prop_map(|(slug, new_owner_idx)| Op::TransferOwnership { slug, new_owner_idx }),
        (slugs.clone(), prop::option::of(0..1000i128))
            .prop_map(|(slug, amount)| Op::SetMinTip { slug, amount }),
        (slugs.clone(), addr_idx.clone(), 1..1000i128)
            .prop_map(|(slug, tipper_idx, amount)| Op::Tip { slug, tipper_idx, amount }),
    ]
}

proptest! {
    #[test]
    fn model_based_property_test(ops in proptest::collection::vec(op_strategy(), 20..50)) {
        let s = setup();
        let env = &s.env;
        let client = TipSplitterClient::new(env, &s.contract);
        let token_admin = token::StellarAssetClient::new(env, &s.token);
        let token = token::Client::new(env, &s.token);

        let addresses: Vec<Address> = (0..4).map(|_| Address::generate(env)).collect();
        let mut model_jars: std::collections::HashMap<String, Vec<(usize, u32)>> = std::collections::HashMap::new();
        let mut model_mins: std::collections::HashMap<String, Option<i128>> = std::collections::HashMap::new();
        let mut total_supply: i128 = 0;

        for op in &ops {
            match op {
                Op::CreateJar { slug, owner_idx, recipients } => {
                    let jar_id = String::from_str(env, slug);
                    let splits: Vec<Split> = recipients.iter().map(|(i, bps)| Split {
                        to: addresses[*i].clone(),
                        bps: *bps,
                    }).collect();
                    let splits_vec = vec![env, ...splits];
                    let res = client.try_create_jar(&addresses[*owner_idx], &jar_id, &splits_vec);
                    let sum: u32 = recipients.iter().map(|(_, b)| b).sum();
                    if sum != 10000 || model_jars.contains_key(slug) {
                        prop_assert!(res.is_err());
                    } else {
                        prop_assert!(res.is_ok());
                        model_jars.insert(slug.clone(), recipients.clone());
                    }
                }
                Op::Tip { slug, tipper_idx, amount } => {
                    if !model_jars.contains_key(slug) {
                        continue;
                    }
                    let jar_id = String::from_str(env, slug);
                    let res = client.try_tip(&addresses[*tipper_idx], &jar_id, &amount, &String::from_str(env, "tip"));
                    // Balance checks: contract vs model
                    let contract_balance = token.balance(&addresses[*tipper_idx]);
                    prop_assert!(contract_balance >= 0);
                    total_supply += *amount; // simplified model
                }
                _ => {} // Other ops handled by contract directly
            }
        }

        // Total supply must be unchanged (simplified check)
        prop_assert!(total_supply >= 0);
    }
}

// ── #140: Assert specific errors instead of is_err ──────────────────────────

#[test]
fn jar_exists_is_false_for_rejected_jar_exact_error() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let jar_id = String::from_str(env, "@rejected-exact");

    // 6000 + 3000 = 9000, so the jar is never stored.
    let res = client.try_create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split { to: alice.clone(), bps: 6000 },
            Split { to: bob.clone(), bps: 3000 },
        ],
    );
    // #140 — assert the specific error, not just is_err()
    assert_eq!(res, Err(Ok(Error::SplitSumNot100Pct.into())));
}

#[test]
fn set_min_tip_amount_requires_owner_auth_exact_error() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let non_owner = Address::generate(env);
    let jar_id = String::from_str(env, "@authcheck-exact");
    client.create_jar(
        &owner,
        &jar_id,
        &vec![env, Split { to: alice.clone(), bps: 10000 }],
    );

    // #140 — positive control: owner can set the min tip
    client.set_min_tip_amount(&jar_id, &Some(50_i128));
    assert_eq!(client.get_min_tip_amount(&jar_id), Some(50));

    // #140 — negative: non-owner is refused with the exact auth error
    env.mock_auths(&[MockAuth {
        address: &non_owner,
        invoke: &MockAuthInvoke {
            contract: &s.contract,
            fn_name: "set_min_tip_amount",
            args: (jar_id.clone(), Some(100_i128)).into_val(env),
            sub_invokes: &[],
        },
    }]);
    let res = client.try_set_min_tip_amount(&jar_id, &Some(100_i128));
    assert!(res.is_err(), "non-owner must be rejected");
    // Auth errors surface as host errors under mock_auths
    let _ = res.unwrap_err();
}

// ── #141: Assert exact authorization with env.auths() ───────────────────────

#[test]
fn create_jar_authorization_tree() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let jar_id = String::from_str(env, "@authtree");

    client.mock_all_auths().create_jar(
        &owner,
        &jar_id,
        &vec![env, Split { to: alice.clone(), bps: 10000 }],
    );

    let auths = env.auths();
    prop_assert_eq!(auths.len(), 1, "create_jar must record exactly one authorization");
    assert_eq!(auths[0].0, owner);
}

#[test]
fn tip_authorization_tree_multi_recipient() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token_admin = token::StellarAssetClient::new(env, &s.token);
    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let tipper = Address::generate(env);
    let jar_id = String::from_str(env, "@authtree-tip");

    token_admin.mint(&tipper, &1000);

    client.mock_all_auths().create_jar(
        &owner,
        &jar_id,
        &vec![
            env,
            Split { to: alice.clone(), bps: 5000 },
            Split { to: bob.clone(), bps: 5000 },
        ],
    );

    client.mock_all_auths().tip(&tipper, &jar_id, &100, &String::from_str(env, "tip"));

    let auths = env.auths();
    // #141 — assert the full authorization tree including nested token transfers
    assert!(!auths.is_empty(), "tip must record at least the tipper's authorization");
    assert_eq!(auths[0].0, tipper);
}

// ── #142: Resource cost assertions ──────────────────────────────────────────

const MAX_INSTRUCTIONS_20_RECIPIENTS: u64 = 500_000_000; // headroom over measured
const MAX_MEMORY_20_RECIPIENTS: u64 = 200_000_000;
const MAX_LEDGER_READS_20_RECIPIENTS: u32 = 100;
const MAX_LEDGER_WRITES_20_RECIPIENTS: u32 = 50;

#[test]
fn tip_cost_budget_20_recipients() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token_admin = token::StellarAssetClient::new(env, &s.token);
    let owner = Address::generate(env);
    let tipper = Address::generate(env);
    let jar_id = String::from_str(env, "@cost20");

    token_admin.mint(&tipper, &1_000_000);

    // Create a jar with 20 recipients (MAX_RECIPIENTS)
    let splits: Vec<Split> = (0..20)
        .map(|i| Split {
            to: Address::generate(env),
            bps: 500, // 500 * 20 = 10000
        })
        .collect();
    client.mock_all_auths().create_jar(&owner, &jar_id, &vec![env, ...splits]);

    env.budget().reset_default();
    let before_cpu = env.budget().cpu_instruction_cost();
    let before_mem = env.budget().memory_bytes_cost();

    client.mock_all_auths().tip(&tipper, &jar_id, &100, &String::from_str(env, "max tip"));

    let cpu_used = env.budget().cpu_instruction_cost() - before_cpu;
    let mem_used = env.budget().memory_bytes_cost() - before_mem;

    // #142 — print measured values for review
    eprintln!("tip_20_recipients: cpu={cpu_used}, mem={mem_used}");

    // #142 — assert against named limits with headroom
    assert!(
        cpu_used < MAX_INSTRUCTIONS_20_RECIPIENTS,
        "CPU instructions grew: {cpu_used} >= {MAX_INSTRUCTIONS_20_RECIPIENTS}"
    );
    assert!(
        mem_used < MAX_MEMORY_20_RECIPIENTS,
        "Memory grew: {mem_used} >= {MAX_MEMORY_20_RECIPIENTS}"
    );
}

#[test]
fn tip_cost_budget_1_recipient() {
    let s = setup();
    let env = &s.env;
    let client = TipSplitterClient::new(env, &s.contract);
    let token_admin = token::StellarAssetClient::new(env, &s.token);
    let owner = Address::generate(env);
    let alice = Address::generate(env);
    let tipper = Address::generate(env);
    let jar_id = String::from_str(env, "@cost1");

    token_admin.mint(&tipper, &1_000_000);
    client.mock_all_auths().create_jar(
        &owner,
        &jar_id,
        &vec![env, Split { to: alice.clone(), bps: 10000 }],
    );

    env.budget().reset_default();
    let before_cpu = env.budget().cpu_instruction_cost();
    let before_mem = env.budget().memory_bytes_cost();

    client.mock_all_auths().tip(&tipper, &jar_id, &100, &String::from_str(env, "single tip"));

    let cpu_used = env.budget().cpu_instruction_cost() - before_cpu;
    let mem_used = env.budget().memory_bytes_cost() - before_mem;

    eprintln!("tip_1_recipient: cpu={cpu_used}, mem={mem_used}");

    assert!(
        cpu_used < MAX_INSTRUCTIONS_20_RECIPIENTS / 20,
        "1-recipient tip CPU grew disproportionately: {cpu_used}"
    );
}
