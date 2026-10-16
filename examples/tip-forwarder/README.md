# `tip-forwarder` — tipping a jar from another contract

A minimal example of calling `tip_splitter` as a caller that is itself a
contract — a marketplace routing part of a sale to a creator's jar — and the
authorization catch that comes with it.

## The catch

`tip(env, from, jar_id, amount, message)` calls `from.require_auth()`. When a
contract calls `tip` with `from` set to its own address, that check is
satisfied automatically. But `tip` then calls the token's
`transfer(from, to, share)` once per recipient, and **those transfers are
invoked by `tip_splitter`, not by the caller** — so they are not covered. The
calling contract must pre-authorize each transfer with
`env.authorize_as_current_contract`, listing every recipient and the exact
amount, or the tip fails with an auth error.

The amounts to authorize are what `preview_split(jar_id, amount)` returns,
with one exception: if the calling contract is itself a recipient, `tip` skips
the self-transfer and rolls that share into the **last recipient** — so the
preview is not enough, and the authorized amounts must be corrected the same
way.

## How the example does it

`forward_tip` (in `src/lib.rs`):

1. reads `preview_split(jar_id, amount)` for the shares and `get_jar` for the
   recipient order;
2. replays `tip`'s payment loop locally (`actual_transfers`) — skip shares
   going to the forwarder itself, roll the withheld amount into the last
   recipient — to get the transfers that will *actually* happen;
3. builds one `InvokerContractAuthEntry::Contract` per transfer
   (`token.transfer(from=forwarder, to, amount)`) and passes them to
   `env.authorize_as_current_contract` — in the same invocation, immediately
   before the cross-contract call;
4. calls `client.tip(&myself, &jar_id, &amount, &message)`.

`forward_tip_naive` skips step 2 and authorizes the raw preview amounts. It
exists only so the tests can show the failure mode; production callers should
use `forward_tip`.

## Tests

```bash
cargo test -p tip-forwarder
```

The tests register the real `TipSplitter` from `contracts/tip-splitter` and a
test SAC in the same env and run under strict `mock_auths` — only the
forwarder owner's signature is mocked, so the token transfers succeeding is
proof the runtime authorization works. `forward_tip_naive` is asserted to
*fail* when the forwarder is a jar recipient, and nothing moves when it does.

The `tip_splitter` interface is declared locally with `#[contractclient]`
rather than imported with `contractimport!` — `cargo test` needs no built
wasm, matching CI, which runs tests before it builds wasm.
