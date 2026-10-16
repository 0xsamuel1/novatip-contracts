# `tip_splitter` — contract interface

Receives a single USDC tip and splits it across one or more recipients by
basis-point shares, atomically, in one transaction.

## Concepts

- **Jar** — a creator's tip target, identified by a public slug (e.g. `@alice`).
  Holds an `owner` and a list of `Split`s. The slug (`jar_id`) must be
  non-empty and at most `64` bytes (`MAX_JAR_ID_LEN`) — it doubles as a
  storage key, an event topic, and a public URL slug.
- **Split** — a recipient `Address` and its share in basis points (`bps`).
  Every split must have `bps >= 1`, all splits in a jar must sum to exactly
  `10_000` (= 100%), and no address may appear more than once.
- **USDC token** — the Stellar Asset Contract id is fixed at deploy time; every
  tip settles in that asset.
- **Message** — the free-text note a supporter attaches to a tip. Capped at
  `280` bytes (`MAX_MESSAGE_LEN`) and echoed into the `tip` event.
- **Sequence number** — a contract-wide `u64`, incremented once per successful
  tip, returned by `tip` and published in the `tip` event. Gives each tip an
  identifier of its own where the transaction hash does not.
- **Reference** — an optional 32-byte opaque id a tipper attaches with
  `tip_with_ref`, echoed in the `tip` event so the calling integration can
  match the on-chain event back to whatever it created off-chain. Never stored
  and never checked by the contract.

## Types

```rust
struct Split          { to: Address, bps: u32 }
struct Jar            { owner: Address, splits: Vec<Split> }
struct Limits         { bps_denom: u32, max_recipients: u32, max_message_len: u32 }
struct JarStats       { tip_count: u64, total: i128 }
struct SupporterStats { tip_count: u64, total: i128 }
```

`JarStats` and `SupporterStats` are the on-chain totals behind
[reading tip totals](#reading-tip-totals). They share a shape but not a key:
one belongs to a jar, the other to a `(jar, supporter)` pair.

## Functions

| Function | Auth | Description |
|----------|------|-------------|
| `__constructor(admin, token)` | — | Deploy-time init. Stores the admin and USDC token address, and puts that token on the tip allowlist as its first entry. |
| `create_jar(owner, jar_id, splits)` | `owner` | Register a new jar. Fails if the slug is empty, over `MAX_JAR_ID_LEN` bytes, already exists, or splits are invalid. Emits a `jar_crtd` event. |
| `update_splits(jar_id, splits)` | jar `owner` | Replace a jar's splits. Subject to the same validation as `create_jar`. Emits a `splits` event. |
| `set_min_tip_amount(jar_id, min_amount)` | jar `owner` | Set or clear an optional minimum tip amount for a jar. `min_amount` must be strictly positive (`> 0`) or `None` to clear. Emits a `min_tip` event. |
| `transfer_jar_ownership(jar_id, new_owner)` | current jar `owner` | Hand control of a jar to `new_owner`. Splits are unchanged; the new owner does not need to authorize. Emits a `jar_xfer` event carrying both owners. |
| `tip(from, jar_id, amount, message)` | `from` | Transfer `amount` of the **default token** from `from`, split across the jar's recipients. Emits a `tip` event carrying the per-recipient breakdown and the token paid in. Rejects amounts below the jar's minimum with `BelowMinTipAmount`. |
| `tip_in_token(from, jar_id, token, amount, message)` | `from` | Same split, paid in `token`, which must be on the allowlist — otherwise `TokenNotAllowed`, raised before any transfer. The jar minimum applies only when `token` is the default token. Emits the same `tip` event. |
| `allow_token(token)` | contract `admin` | Add `token` to the allowlist. Idempotent. Emits a `tk_allow` event carrying the admin who authorised it. |
| `disallow_token(token)` | contract `admin` | Remove `token` from the allowlist. Refuses the default token with `CannotDisallowDefault`. Emits a `tk_dis` event carrying the admin. |
| `tip(from, jar_id, amount, message) -> u64` | `from` | Transfer `amount` USDC from `from`, split across the jar's recipients. Emits a `tip` event carrying the per-recipient breakdown and this tip's sequence number, and returns that number. Rejects amounts below the jar's minimum with `BelowMinTipAmount`. |
| `tip_with_ref(from, jar_id, amount, message, reference) -> u64` | `from` | Identical to `tip`, plus an optional 32-byte opaque `reference` echoed in the `tip` event. Same transfers, same validation, same counter — both entry points call one internal implementation. |
| `get_tip_count() -> u64` | — | How many tips the contract has taken, i.e. the number the next tip will receive. `0` on a fresh deploy. |
| `get_jar(jar_id) -> Jar` | — | Read a jar's configuration. Panics with `JarNotFound` if the slug is free. |
| `get_jar_owner(jar_id) -> Address` | — | Read just a jar's owner. Panics with `JarNotFound` if the slug is free. |
| `get_split_count(jar_id) -> u32` | — | How many recipients a jar pays. Panics with `JarNotFound` if the slug is free. |
| `preview_split(jar_id, amount) -> Vec<i128>` | — | What each recipient would receive from a tip of `amount`, in split order. Rejects amounts below the jar's minimum with `BelowMinTipAmount` as well as the amounts `tip` rejects; panics with `JarNotFound` if the slug is free. |
| `jar_exists(jar_id) -> bool` | — | Whether the slug is already registered. |
| `is_recipient(jar_id, address) -> bool` | — | Whether `address` appears in the jar's splits. Panics with `JarNotFound` if the slug is free. |
| `get_min_tip_amount(jar_id) -> Option<i128>` | — | Read a jar's optional minimum tip amount. Returns `None` if no minimum is set. Panics with `JarNotFound` if the slug is free. |
| `get_token() -> Address` | — | The USDC token address tips settle in. |
| `get_admin() -> Address` | — | The contract admin recorded at deploy time. |
| `get_limits() -> Limits` | — | The bounds this contract enforces: `bps_denom`, `max_recipients`, `max_message_len`. Reads no storage. |
| `version() -> u32` | — | Which interface shape this deployment speaks. Reads no storage. See [detecting the interface version](#detecting-the-interface-version). |
| `get_jar_stats(jar_id) -> JarStats` | — | A jar's on-chain tip count and total. Zeros for a jar nobody has tipped; `JarNotFound` if the slug is free. |
| `get_supporter_stats(jar_id, supporter) -> SupporterStats` | — | One address's tip count and total against one jar. Zeros for an address that has never tipped it; `JarNotFound` if the slug is free. |
| `is_token_allowed(token) -> bool` | — | Whether the admin has put `token` on the tip allowlist. The default token is always allowed. |
The **Errors** column lists the `tip_splitter` error codes a call can return,
**in the order the contract checks them** — only the first failure a call hits
is reported, so the order tells a client what it will actually see. An error is
written as raised only when every earlier check has passed.

| Function | Auth | Errors (in check order) | Description |
|----------|------|-------------------------|-------------|
| `__constructor(admin, token)` | — | — | Deploy-time init. Stores the admin and USDC token address. |
| `create_jar(owner, jar_id, splits)` | `owner` | `InvalidJarId` → `JarExists` → `SplitsEmpty` → `TooManyRecipients` → `SplitOutOfRange` / `DuplicateRecipient` (per entry) → `SplitSumNot100Pct` | Register a new jar. Fails if the slug is empty, over `MAX_JAR_ID_LEN` bytes, already exists, or splits are invalid. Emits a `jar_crtd` event. |
| `update_splits(jar_id, splits)` | jar `owner` | `JarNotFound` → `SplitsEmpty` → `TooManyRecipients` → `SplitOutOfRange` / `DuplicateRecipient` (per entry) → `SplitSumNot100Pct` | Replace a jar's splits. Subject to the same validation as `create_jar`. Emits a `splits` event. |
| `set_min_tip_amount(jar_id, min_amount)` | jar `owner` | `JarNotFound` → `InvalidAmount` | Set or clear an optional minimum tip amount for a jar. `min_amount` must be strictly positive (`> 0`) or `None` to clear. Emits a `min_tip` event. |
| `transfer_jar_ownership(jar_id, new_owner)` | current jar `owner` | `JarNotFound` | Hand control of a jar to `new_owner`. Splits are unchanged; the new owner does not need to authorize. Emits a `jar_xfer` event carrying both owners. |
| `tip(from, jar_id, amount, message)` | `from` | `InvalidAmount` (amount ≤ 0) → `MessageTooLong` → `JarNotFound` → `BelowMinTipAmount` → `NotInitialized` → `InvalidAmount` (a share truncates to zero, or `amount * bps` overflows `i128`) | Transfer `amount` USDC from `from`, split across the jar's recipients. Emits a `tip` event carrying the per-recipient breakdown. Rejects amounts below the jar's minimum with `BelowMinTipAmount`. A failing token `transfer` reverts the whole call with the **token contract's own error**, not one of this contract's codes (e.g. an insufficient balance). |
| `get_jar(jar_id) -> Jar` | — | `JarNotFound` | Read a jar's configuration. Panics with `JarNotFound` if the slug is free. |
| `get_jar_owner(jar_id) -> Address` | — | `JarNotFound` | Read just a jar's owner. Panics with `JarNotFound` if the slug is free. |
| `get_split_count(jar_id) -> u32` | — | `JarNotFound` | How many recipients a jar pays. Panics with `JarNotFound` if the slug is free. |
| `preview_split(jar_id, amount) -> Vec<i128>` | — | `JarNotFound` → `BelowMinTipAmount` → `InvalidAmount` | What each recipient would receive from a tip of `amount`, in split order. Rejects amounts below the jar's minimum with `BelowMinTipAmount` as well as the amounts `tip` rejects; panics with `JarNotFound` if the slug is free. |
| `jar_exists(jar_id) -> bool` | — | — | Whether the slug is already registered. Reads one key and returns a plain `bool`; never panics on a free slug. |
| `is_recipient(jar_id, address) -> bool` | — | `JarNotFound` | Whether `address` appears in the jar's splits. Panics with `JarNotFound` if the slug is free. |
| `get_min_tip_amount(jar_id) -> Option<i128>` | — | `JarNotFound` | Read a jar's optional minimum tip amount. Returns `None` if no minimum is set. Panics with `JarNotFound` if the slug is free. |
| `get_token() -> Address` | — | `NotInitialized` | The USDC token address tips settle in. Reads instance storage; `NotInitialized` if the deploy-time token is absent. |
| `get_admin() -> Address` | — | `NotInitialized` | The contract admin recorded at deploy time. Reads instance storage; `NotInitialized` if the deploy-time admin is absent. |
| `get_limits() -> Limits` | — | — | The bounds this contract enforces: `bps_denom`, `max_recipients`, `max_message_len`. Reads no storage. |

Two ordering notes that are easy to get wrong:

- **Auth is not a contract error code.** A call that lacks the required
  signature fails with Soroban's authorization error, not a `tip_splitter`
  variant. The owner-only setters (`update_splits`, `set_min_tip_amount`,
  `transfer_jar_ownership`) look the jar up *before* requiring the owner's
  signature, so an unauthenticated caller using a free slug sees `JarNotFound`,
  not an auth failure. `create_jar` requires the declared owner's signature
  first.
- **`tip` checks the message length before it looks up the jar**, so an
  over-long message to an unregistered slug reports `MessageTooLong`, not
  `JarNotFound`. Likewise `validate_splits` checks each entry's `bps` range
  before scanning for that entry's duplicates, so
  `[{to: a, bps: 0}, {to: a, bps: 10_000}]` reports `SplitOutOfRange`.

### Confirming you are a recipient

A collaborator added to someone else's jar has no cheap way to confirm they are
actually in it. The alternative is fetching the whole jar with `get_jar` and
scanning the split vector for your own address, which is awkward from a wallet
or a one-line script.

`is_recipient(jar_id, address)` answers it directly and returns a plain
`bool`. It reads the same single persistent key `get_jar` does, so it is
cheap enough for a lightweight client.

**Membership is not ownership.** The answer tracks the split vector only: a jar
owner who takes no share of a tip gets `false`, and an address listed in the
splits gets `true` whether or not it owns the jar. Use
[`get_jar_owner`](#reading-just-the-owner) to ask who controls a jar.

**A missing jar is an error, not a `false`.** An unregistered slug panics with
`JarNotFound`, exactly as `get_jar` does. Returning `false` would make a typo'd
jar id indistinguishable from a genuine non-membership. Use `jar_exists` when
the question is whether the slug is registered at all.

Membership follows `update_splits`: a collaborator removed by a splits update
immediately reads as `false`, and one added reads as `true`. There is no
historical view — the answer describes the jar's current splits, so it is not a
record of who was paid by past tips. For that, read the per-recipient
breakdown in the [`tip` event](#tip--published-on-every-successful-tip). For
what a *future* tip would pay each of them, use
[`preview_split`](#previewing-a-tip).

### Reading the contract's limits

`BPS_DENOM`, `MAX_RECIPIENTS` and `MAX_MESSAGE_LEN` are private constants, so a
client that wants to validate input before paying for a transaction has no way
to read them. The result was that every client carried its own copy — the tip
form capped messages at its own number, the splits editor hardcoded the twenty
recipient limit — and changing a bound here silently desynchronised the stack.

`get_limits` returns all three:

```rust
Limits {
    bps_denom: 10_000,       // BPS_DENOM — the denominator every `bps` is a share of
    max_recipients: 20,      // MAX_RECIPIENTS — most recipients in one jar
    max_message_len: 280,    // MAX_MESSAGE_LEN — longest tip message, in UTF-8 bytes
}
```

All three are compile-time constants, so the view reads no storage and costs
the same regardless of contract state. A client can fetch it once at startup
and cache it for the session.

`max_message_len` is a **byte** count, not a character count — see
[Message](#concepts) above. A client showing a remaining-characters indicator
must count UTF-8 bytes against this number, not `message.length`.

A test asserts the returned values equal the constants (`get_limits_matches_
contract_constants` in `test.rs`), so the view and the enforcement cannot drift
apart. The current values are also asserted literally, which means changing a
bound fails the suite until the constants are updated with it. The document
you are reading is checked too: `scripts/check-docs.sh` (`make check-docs`,
and a CI step) builds the contract, reads its public interface out of the
built wasm, and fails when the Functions table above or the Errors table
below no longer matches the contract — a function or error variant missing
from either table, listed there although the contract does not have it, or
documented with a different error number.

### Reading just the owner

`get_jar_owner` answers "who controls this jar?" without moving the recipient
list. The alternative — `get_jar(jar_id).owner` — deserializes the whole
`splits` vector to read one address, which for a jar near the 20 recipient cap
is a lot of data to render a "you own this jar" badge or to decide whether the
connected wallet may call `update_splits`.

It reads the same stored jar `get_jar` does, so the two never disagree, and it
tracks `transfer_jar_ownership` immediately. A free slug is a `JarNotFound`
panic rather than a placeholder address — a zero address in a return value
would read to a client as a jar somebody owns.

### Counting recipients

`get_split_count` returns the number of entries in a jar's `splits`. A tip page
rendering a "3 collaborators" badge needs that one integer, and
`get_jar(jar_id).splits.len()` makes it pay for the whole recipient vector —
up to 20 addresses and shares — to compute it. It also gives a client a cheap
way to decide whether fetching the full list is worth it at all.

The count is always between `1` and `20` (`MAX_RECIPIENTS`) for a stored jar,
since validation rejects an empty splits list. An unregistered slug therefore
panics with `JarNotFound` rather than returning `0`, which no real jar can
have.

### Previewing a tip

`preview_split` answers "what does each collaborator actually get?" before the
supporter signs. It returns one `i128` per recipient, in the same order as the
jar's `splits`, computed by the same code path `tip` pays out with — the two
share one internal helper, so the quoted split and the paid split cannot drift.

Clients should call it instead of recomputing the split themselves. Doing the
arithmetic client-side puts the rounding rule in two places, and the copy
inevitably falls behind.

```
splits:  alice 70%, bob 30%
preview_split(jar_id, 101) -> [70, 31]
```

Alice's share truncates from 70.7 to 70 and Bob absorbs the leftover 1 on top
of his 30 — the dust goes to the **last** recipient, so a UI that rounds evenly
would show Bob the wrong number.

The shares always sum to exactly `amount`. That holds for awkward amounts too:
a 3333 / 3333 / 3334 jar previewing `100` returns `[33, 33, 34]`, not `[33, 33,
33]` with a unit lost.

`preview_split` rejects exactly what `tip` rejects, so a preview that returns
at all describes a tip that can go through:

- `amount <= 0` — `InvalidAmount`.
- an `amount` small enough that some recipient's share would truncate to zero
  (`amount * bps < 10_000`) — `InvalidAmount`.
- an `amount` so large that `amount * bps` overflows `i128` — `InvalidAmount`.
- an unregistered slug — `JarNotFound`, rather than an empty vector, which
  would read as a jar that pays nobody.

One caveat: `tip` withholds a recipient's share when that recipient is also the
tipper, rather than making them pay themselves. `preview_split` does not take a
`from` address, so it reports every recipient's full share. A client whose
connected wallet appears in the jar's splits should account for that itself.
The `tip` event's [`breakdown`](#tip--published-on-every-successful-tip) does
know who tipped, so it is the authoritative record of what each recipient was
actually paid.

### Setting a minimum tip amount

A creator can opt out of micro-dust tips by setting an optional floor on their jar.
By default, jars have no minimum tip amount (`None`), accepting any positive amount.

`set_min_tip_amount(jar_id, min_amount)` lets the jar owner configure an optional
minimum tip amount (`Some(amount)` where `amount > 0`) or clear it (`None`). Only the
jar owner may authorize this call. Passing a non-positive amount (`<= 0`) panics with
`InvalidAmount` (error code 5).

When a minimum is configured, any tip with `amount < min_amount` is rejected with
`BelowMinTipAmount` (error code 13) before any tokens move or events emit. `preview_split`
similarly rejects amounts below the minimum with `BelowMinTipAmount`.

`get_min_tip_amount(jar_id)` reads the configured floor:
- Returns `None` if no minimum is set (the default for all newly created jars).
- Returns `Some(amount)` if a minimum floor has been configured.
- Panics with `JarNotFound` (error code 3) if the slug is not registered.

### Checking slug availability

`jar_exists` is the intended way to test whether a slug is taken. The
alternative — calling `get_jar` and catching the `JarNotFound` panic — is
awkward from the SDK, since a missing jar is an ordinary answer here rather
than an error. `jar_exists` reads one persistent key and returns a plain
`bool`, so the onboarding form can call it on every (debounced) keystroke.

Matching is exact: `jar_exists("@ali")` is `false` while `"@alice"` is taken.
A jar rejected by validation is never stored, so its slug stays free.

Note that availability is not a reservation. Between the check and the
`create_jar` call, another transaction can claim the slug — `create_jar` still
panics with `JarExists`, and clients must handle that rather than treating a
`false` from `jar_exists` as a guarantee.

### `jar_id` validation

`create_jar` rejects a `jar_id` that is empty or longer than `64` bytes
(`MAX_JAR_ID_LEN`) — `InvalidJarId`. The id is used as a storage key, an event
topic, and a public URL slug, so unbounded input would cost unnecessary rent
and could produce jars no frontend can address. The bound is inclusive: a
`jar_id` of exactly 64 bytes is accepted.

### Validation rules

`create_jar` and `update_splits` share one validator, so both enforce all of:

- The list must be non-empty — `SplitsEmpty`.
- At most 20 entries (`MAX_RECIPIENTS`) — `TooManyRecipients`.
- **No entry may have `bps == 0`** — `SplitOutOfRange`.
- **No address may appear twice** — `DuplicateRecipient`.
- **No entry may have `bps > 10_000`** — `SplitOutOfRange`.
- The `bps` values must sum to exactly `10_000` — `SplitSumNot100Pct`.

A `bps == 0` entry is rejected rather than accepted-and-ignored. Such a
recipient could never be paid (`tip` skips zero shares), but it would still
consume one of the 20 recipient slots and surface in clients as a collaborator
who never receives funds. Rejecting it at write time keeps a stored jar's
recipient list an accurate record of who actually gets paid.

Note that this is a validation-time rule about `bps`, not a guarantee about
transferred amounts: a recipient with a valid non-zero `bps` can still receive
`0` on a small tip, because `amount * bps / 10_000` truncates (e.g. `bps: 100`
on a tip of `50` yields `0`).

Duplicates are likewise rejected rather than merged. A repeated address is not a
loss-of-funds bug — the shares still total 100% — but it makes `tip` issue
several separate transfers to one destination in a single call, wasting fees,
and leaves an on-chain record that per-collaborator accounting has to
de-duplicate after the fact. Clients that want to let a user enter the same
collaborator twice should sum the shares before submitting.

The check is a pairwise comparison over the vector, so position doesn't matter:
`[a, b, a]` is rejected just as `[a, a, b]` is.
A `bps > 10_000` entry claims more than the whole tip, so it could never belong
to a set summing to 100% — the sum check would reject it anyway. It is rejected
per entry because that also bounds the running total: with at most 20 entries
of at most `10_000` each, the accumulator can never exceed `200_000`, far below
`u32::MAX`. The sum is additionally accumulated with `checked_add`, which fails
with `SplitSumNot100Pct` rather than trapping.

This matters because `bps` is caller-supplied and unbounded in the wire type.
Before, a set of shares whose true sum exceeded `u32::MAX` relied on
`overflow-checks = true` in the release profile to trap — which reverted the
transaction, so the 100% invariant did hold, but clients saw an opaque wasm
error instead of error code 4, and the guarantee lived in `Cargo.toml` rather
than in the validator. Both the per-entry bound and `checked_add` now put it in
the code, so flipping that profile setting cannot turn it into a bypass.

### Splitting rules

- Each non-final recipient receives `amount * bps / 10_000` (integer division).
- The **last** recipient receives `amount - (sum of prior shares)`, so rounding
  dust is never lost and the full amount is always distributed.
- The whole tip reverts if any single transfer fails — tips are all-or-nothing.
- `preview_split(jar_id, amount)` runs this same calculation as a view, so a
  client can show the exact per-recipient amounts before the supporter signs.
- The amounts actually transferred are published in the `tip` event's
  `breakdown`, so no consumer has to reproduce this arithmetic after the fact
  either.

## Authorization

Every function that names an address in its signature requires that address's
signature, and nothing else does. What a signature actually *commits to* is the
part clients get asked about, because it decides when a previously signed
transaction stops being valid.

### `tip`

A tipper authorizes the `tip` call itself, and the same authorization tree
includes each token `transfer(from, recipient, share)` that `tip` performs. The
transfers are nested under the contract invocation, carrying the exact recipient
address and the exact amount.

Consequences a client has to design for:

- **The wallet shows one transfer per recipient, not one payment to the
  contract.** A jar splitting 70/30 between two collaborators prompts the
  supporter to approve two token transfers. Support teams get asked why the
  tipper is never asked to send anything to the jar's contract address: the
  contract never holds the funds, it moves them from the tipper straight to
  each recipient.
- **A tip signed against one set of splits cannot pay a different set.** If the
  jar's splits (or its minimum tip amount) change between simulation and
  submission, the signed transaction no longer matches what the contract will
  actually do, and Stellar rejects it with `Error(Context, InvalidAction)`
  before any funds move. This is the answer to the obvious question about a jar
  owner "redirecting" a tip that is already in flight: they can call
  `update_splits`, and the tip in transit simply fails. The old signature cannot
  pay the new recipients.
- **The client's recovery is to re-simulate and ask for a fresh signature.**
  Without this documented the rejection looks random, because nothing in the
  tip request changed — the jar did. On `InvalidAction`, re-simulate
  (`preview_split` gives the same per-recipient amounts the new tree will
  commit to), re-sign, and submit; a wallet that only retries the same
  envelope will fail identically forever.
- A tipper who is also a recipient is not paid, so no transfer appears for them
  in the tree, and their notional share rolls into the last recipient's
  transfer. See [`breakdown`](#tip--published-on-every-successful-tip).

### Owner-only functions

| Function | Authorizes | Tree contains |
|----------|-----------|---------------|
| `create_jar(owner, jar_id, splits)` | `owner` | The contract call only. No token moves. |
| `update_splits(jar_id, splits)` | the jar's current `owner` | The contract call only. |
| `transfer_jar_ownership(jar_id, new_owner)` | the jar's current `owner` | The contract call only. `new_owner` signs nothing. |
| `set_min_tip_amount(jar_id, min_amount)` | the jar's current `owner` | The contract call only. |

Each of these is a single contract-level invocation with no nested token
transfers, so the signer approves one action, and the amount is not part of what
is signed. Reading the jar (`get_jar`, `get_split_count`, `preview_split`, …)
requires no authorization at all.

The same staleness rule applies to a different degree: these calls do not commit
to a jar's future state, so an `update_splits` signed against the current owner
still succeeds after that owner changes — it just targets whichever jar the
slug names at submission time.

> A test that pins the `tip` authorization behaviour is tracked separately from
> this document; link it here once it lands.

## Errors

| Code | Name | Cause |
|------|------|-------|
| 1 | `NotInitialized` | A deploy-time value is missing from instance storage: the USDC token address (raised by `get_token` and by `tip`) or the admin (raised by `get_admin`). Should never happen post-deploy. |
| 2 | `JarExists` | Slug already registered. |
| 3 | `JarNotFound` | Slug not registered. |
| 4 | `InvalidSplits` | **No longer raised.** Split validation now reports the specific failure as code 10, 11 or 12. The variant is retained so existing codes keep their values. |
| 5 | `InvalidAmount` | Amount ≤ 0, small enough that some recipient's share would truncate to zero, or so large that `amount * bps` overflows `i128` before the division. Raised by `tip` and `preview_split` (over the tip amount) and by `set_min_tip_amount` (for a non-positive minimum). |
| 6 | `TooManyRecipients` | More than 20 recipients. |
| 7 | `DuplicateRecipient` | The same address appears more than once in the splits. |
| 8 | `MessageTooLong` | Tip message exceeds 280 bytes. |
| 9 | `InvalidJarId` | `jar_id` is empty or exceeds 64 bytes (`MAX_JAR_ID_LEN`). |
| 10 | `SplitsEmpty` | The splits list is empty. |
| 11 | `SplitOutOfRange` | An entry has `bps == 0` or `bps > 10_000`. Checked per entry, before the sum. |
| 12 | `SplitSumNot100Pct` | The shares sum to something other than exactly 10 000 bps, including a sum that would overflow `u32`. |
| 13 | `BelowMinTipAmount` | Tip amount is strictly below the jar's configured minimum tip amount. Raised by `tip`, by `preview_split`, and by `tip_in_token` when it is paid in the default token. |
| 14 | `TokenNotAllowed` | `tip_in_token` was given a token the admin has not allowed. Raised before any transfer, so a rejected tip moves nothing. |
| 15 | `CannotDisallowDefault` | `disallow_token` was given the deploy-time token, which `tip` always pays through and which therefore stays non-removable. |
| 16 | `StatsOverflow` | A running tip total would not fit `u64`/`i128`. Effectively unreachable; the counters use checked arithmetic so an exhaustion reports as this code rather than a wasm trap. |
| 13 | `BelowMinTipAmount` | Tip amount is strictly below the jar's configured minimum tip amount. Raised by `tip` and `preview_split`. |
| 14 | `InsufficientBalance` | Tip amount exceeds the sender's USDC balance. Checked once per `tip`, before any transfer. |

Error codes are part of the public interface: `@novatip/sdk` and the frontend
both map these numbers to user-facing messages. New variants are **appended**
and existing values are never renumbered, which is why code 4 stays in place
rather than being removed.

### Token failures keep the token's own code

`tip` moves funds with the USDC Stellar Asset Contract's `transfer`. When that
call fails for a reason other than an empty wallet, the token's error
propagates unchanged — and the SAC numbers its errors from 1 just like this
contract does, so the numbers overlap:

| SAC code | SAC meaning | Collides with this contract's |
|----------|-------------|-------------------------------|
| 10 (`BalanceError`) | balance too low to spend (also: resulting balance too low/high, clawback on a non-clawback trustline) | 10 `SplitsEmpty` |
| 11 (`BalanceDeauthorizedError`) | balance authorization revoked (frozen) by the issuer | 11 `SplitOutOfRange` |
| 12 (`OverflowError`) | spender allowance would overflow | 12 `SplitSumNot100Pct` |
| 13 (`TrustlineMissingError`) | trustline entry missing for the account | 13 `BelowMinTipAmount` |

(The SAC also uses 3, 6, 8 and 9, which likewise coincide with this
contract's codes of the same numbers.)

The one exception is the empty wallet: `tip` reads the sender's token
`balance` once, before the first transfer, and fails with the contract's own
code 14 `InsufficientBalance` when `balance < amount`. A tip larger than the
sender's balance therefore never reaches the token, so it can never be
mislabelled as "Add at least one collaborator".

Any other token failure — a missing trustline on either end, a frozen
(deauthorized) trustline, an unauthorized or overflowing allowance — still
surfaces with the token's number. **Clients must check which contract raised
the error before mapping the number to a message.** On failure the diagnostic
events name the failing contract (the SAC's contract id vs this contract's
id); only errors raised by `tip_splitter` itself may be looked up in the table
above. A bare `Error(Contract, #10)` with no source is ambiguous between
`SplitsEmpty` and the SAC's `BalanceError`.

## Events

### `jar_crtd` — published on every successful `create_jar`

- **Topics:** `(symbol "jar_crtd", jar_id: String)`
- **Data:** `owner: Address`

Indexers must subscribe to this event to build and maintain the full list of
registered jars. There is no on-chain `get_jar_ids` function — event scanning
is the canonical discovery mechanism. This keeps `create_jar` cost constant
(O(1) storage writes) regardless of how many jars have been created.

Enumeration and existence are separate concerns: `jar_exists` answers "is this
one slug taken?" straight from storage, so a client never has to scan the event
log or an indexer's jar list just to validate a name.

### `splits` — published on every successful `update_splits`

- **Topics:** `(symbol "splits", jar_id: String)`
- **Data:** `split_count: u32`

Lets an indexer that has cached a jar's splits know they went stale, without
having to re-poll every jar on a schedule. The indexer refetches the jar via
`get_jar` when it sees this event.

### `jar_xfer` — published on every successful `transfer_jar_ownership`

- **Topics:** `(symbol "jar_xfer", jar_id: String)`
- **Data:** `(prev_owner: Address, new_owner: Address)`

Lets an indexer update who controls a jar without re-polling `get_jar` for
every jar on a schedule.

Both ends of the move are published, in that order, so each event is
independently meaningful. With only the new owner, a consumer replaying the log
could see where a jar went but not where it came from, unless it had already
indexed every prior event for that jar and kept the running state — which an
indexer starting from a partial history has not. Carrying `prev_owner` also
means successive transfers chain: each event's `prev_owner` is the previous
event's `new_owner`, so an ownership history can be reconstructed from the
events alone.

`prev_owner` is the address that authorized the call — only the current owner
may transfer a jar — and is always different from `new_owner` in practice,
though the contract does not reject a transfer to the existing owner. Splits
are unchanged by a transfer, so no `splits` event accompanies this one.

> **Consumer impact.** The data was a bare `Address` and is now a
> two-element tuple. A decoder that reads it as a single address must be
> updated before it reads events from a contract built from this version.

### `min_tip` — published on every successful `set_min_tip_amount`

- **Topics:** `(symbol "min_tip", jar_id: String)`
- **Data:** `min_amount: Option<i128>`

Lets indexers track the current minimum tip floor for a jar without polling `get_min_tip_amount`.

### `tip` — published on every successful tip

- **Topics:** `(symbol "tip", jar_id: String)`
- **Data:** `(from: Address, amount: i128, message: String, breakdown: Vec<(Address, i128)>, token: Address)`
- **Data:** `(from: Address, amount: i128, message: String, breakdown: Vec<(Address, i128)>, seq: u64, reference: Option<BytesN<32>>)`

The backend indexer subscribes to this event to update balances, leaderboards,
and notifications. `message` is at most 280 bytes, so the payload size is
bounded and a `varchar(280)` column is enough to store it.

The six elements occupy fixed, documented positions, in the order above, and
that order is part of the public interface: `from`, `amount`, `message` and
`breakdown` keep the positions they had, and `seq` and `reference` were appended.
Both `tip` and `tip_with_ref` publish all six, so one decoder serves both entry
points — an absent reference is an explicit `None` in position six, never a
shorter tuple.

`breakdown` lists one `(recipient, amount)` pair per split, **in split order**,
so the event is self-describing: a consumer that wants per-collaborator
earnings never has to fetch the jar and redo the arithmetic. That matters
because a recomputed figure can disagree with the contract's — the jar's splits
may have been changed by `update_splits` between the tip and the read, and the
last recipient's share includes rounding dust that depends on the exact
amount. The amounts always sum to `amount`.

The pairs report **what each transfer actually moved**, not the notional
`amount * bps / 10_000` that [`preview_split`](#previewing-a-tip) quotes. The
two differ in one case: `tip` skips paying a recipient who is also the sender,
so that entry reports `0` where the preview would have shown their full share. The recipient is
still listed, which lets a consumer tell "listed but paid nothing this tip"
apart from "not in this jar". Because the skipped share is never deducted from
the running total, the final recipient absorbs it — so a self-tip shows up as a
`0` entry and a correspondingly larger one at the end.

`breakdown.len()` equals the jar's split count at the moment of the tip, so it
is at most `MAX_RECIPIENTS` (20) pairs — see
[`get_limits`](#reading-the-contracts-limits). The payload stays bounded.

Note that `breakdown` is a snapshot, not the jar's current state. To ask
whether an address is a recipient *now*, use
[`is_recipient`](#confirming-you-are-a-recipient).

`token` is the address the tip was paid in — `get_token()` for a `tip` call,
the caller's argument for a `tip_in_token` call. It has to be in the event: the
other four elements carry a number and nothing else says which asset the number
counts, so an indexer could not tell a tip of 10 USDC from a tip of 10 XLM, and
a jar tipped in two assets would have its per-supporter balances summed across
them as if they were one currency. Reading it from the transaction instead is
not enough, because the contract calls the token itself and the event consumer
sees only the tip's own envelope.

> **Consumer impact.** The data tuple grew from three elements to four, and
> now from four to five. A decoder that reads it positionally —
> `decodeTipEvent` in `@novatip/sdk`, and the backend indexer — must be updated
> to accept the fifth element before it reads events from a contract built
> from this version. The first four are unchanged and keep their positions, so
> a consumer that ignores trailing elements is unaffected; a consumer that
> validates the element count rejects them all, which is why this shape change
> also bumps [`version()`](#detecting-the-interface-version).

## Reading tip totals

Two views report what has actually settled on-chain:

```rust
get_jar_stats(jar_id)                -> JarStats       { tip_count, total }
get_supporter_stats(jar_id, supporter) -> SupporterStats { tip_count, total }
```

Both are bumped at the end of a tip that paid out, and neither is written by a
tip that reverted — a transfer that traps aborts the whole call, counters
included. That is what makes them usable as a check rather than a second
opinion:

- `get_jar_stats` is the figure a creator compares against the backend's. The
  indexer has blind spots it cannot report: `planBatch` gives up on any ledger
  that returns a full page of events, and an event older than the RPC's
  retention window can never be backfilled. A missed tip silently lowers a
  creator's total, and until now nothing on-chain contradicted it. A third-party
  page or embeddable button also gets a total with no dependence on Novatip's
  backend.
- `get_supporter_stats` is the data the roadmap's `supporter_badge` contract
  needs. Milestones are phrased as "10 tips or $10 cumulative", and a contract
  cannot read a Postgres row; before this, a badge could only be minted by a
  trusted off-chain service asserting the milestone.

A jar that exists but has never been tipped reads `tip_count: 0, total: 0`, as
does a supporter who has not tipped a given jar. A **free slug** is
`JarNotFound`, not zeros — a typo'd handle must not look like a jar nobody has
tipped.

**Storage.** `DataKey::Stats(jar_id)` and `DataKey::Supporter(jar_id,
supporter)` are persistent entries under the same TTL policy as the jar itself
(`JAR_TTL_LEDGERS`, bumped at the same threshold), and both views bump on read.
The supporter entry describes one supporter's relationship with one jar, so an
archived entry means they restore it or tip again — the history is still in the
event log, and a badge check that reads zeros asks for a restore rather than
minting on a wrong count.

### What it costs per tip

The totals are two extra persistent writes on a path that already writes.
Measured with `env.cost_estimate()` in `test::tip_writes_the_two_totals_entries`
(one recipient, one supporter tipping a fresh jar twice), against the same tip
with the counters disabled:

| | without totals | with totals | difference |
|---|---|---|---|
| Ledger writes | 3 | 5 | **+2 entries** |
| Bytes written | 520 | 924 | **+404 bytes** |
| Persistent rent bumps | 2 | 4 | **+2** |
| Modelled CPU instructions | ~214k | ~268k | **~+25%** |

The difference is flat, not cumulative: a second tip by the same supporter to
the same jar rewrites the same two entries and adds no new ones (the test
asserts this). What the extra rent buys is the ability to disagree with the
indexer — a tip the counters show and the database does not is a bug report,
where previously the two were indistinguishable.

## Tipping in another token

`tip` settles in the deploy-time token, fixed at construction. A supporter
holding EURC had to swap before tipping anything. The admin now manages an
allowlist and there is a second entry point:

```rust
allow_token(token)                     // admin only, idempotent
disallow_token(token)                  // admin only, refuses the default token
is_token_allowed(token) -> bool
tip_in_token(from, jar_id, token, amount, message)
```

`tip_in_token` checks the allowlist before it loads the jar or moves funds, so
an unlisted token fails with `TokenNotAllowed` and pays nobody. Everything else
about it is identical to `tip`: the same `compute_shares`, the same
self-transfer skip, the same breakdown, the same counters, the same event —
which is why the event names the token.

`tip` itself is unchanged and always pays through `get_token()`, which is
allowlisted by the constructor. That keeps every existing integration working,
and means `get_token()` and `is_token_allowed(get_token())` can never disagree.

**Why the default token cannot be disallowed.** `tip` ignores the allowlist by
design — it is the entry point the whole product is built on. Removing its
token would leave a working tip path for an asset the view calls closed, and
`get_token()` would return something `is_token_allowed` rejects. So
`disallow_token` refuses it with `CannotDisallowDefault`; an operator who wants
to retire the default asset deploys a new contract.

**Why the list lives in instance storage.** An allowlist entry is contract
configuration, in the same category as `Admin` and `Token`. A persistent entry
that nobody touches is archived silently, and the failure would look like
"EURC stopped working" with no admin action behind it — the one behaviour an
allowlist must not have. Instance storage is extended with the contract
instance, so an entry cannot expire ahead of the contract that reads it.

### The minimum tip, and the decision recorded here

`set_min_tip_amount` stores a raw `i128` with no unit. Issue #170 asked for a
decision, and there were two defensible ones — apply the floor to the default
token only, or make it per-token:

**Chosen: the minimum applies to the default token only.** A tip in another
allowed asset is not gated by it, even if that asset is the one the minimum was
set in mind for. Making it per-token would mean changing `set_min_tip_amount`'s
signature and its storage key, which breaks every jar that already configured a
minimum — a migration for a feature nobody has asked to price per-asset. The
number is also genuinely unpriceable across assets: 500 is a different amount of
money in a 6-decimal token than in a 7-decimal one, so a per-token floor needs a
unit per entry, not a rename.

The consequence is real and deliberate: **a jar can be tipped below its floor in
another allowed asset.** A creator who cares about a floor should keep the
allowlist to the token they set the minimum in. `preview_split` keeps enforcing
the minimum as it always did, so it describes a default-token tip.

`compute_shares` needed no equivalent decision. It splits by basis points and
never interprets magnitude, so the same arithmetic is correct for any asset —
which is exactly what makes an EURC tip of 100 pay 70/30 just as a USDC tip of
100 does.

## Detecting the interface version

```rust
version() -> u32   // reads no storage
```

The shape of `tip`'s event has changed twice and `jar_xfer`'s data once, always
under the same function names, and `decodeTipEvent` in the SDK kept reading
three elements after the contract published four. A client given only a
contract id had no way to find out which build it was talking to before
decoding, so a mismatch surfaced as a parse failure — or as silently ignored
new data.

`version()` lets it ask first. It returns a compile-time constant, so it costs
no storage read and works against a contract with no jars and no callers.

**When the number moves.** Bump `INTERFACE_VERSION` by one when a function is
added or removed, an error code is added or renumbered, or an event's shape
changes. Do not bump it for an internal refactor, a new storage key, or a doc
edit — none of those are visible to a consumer, and a version that changes when
nothing observable did is a version consumers stop trusting.

This PR ships `version() == 1`, which describes the interface as of this build:
`tip` events with five elements, totals views, and the token allowlist.

**How a client should use it.** Treat an unrecognised number as "newer than I
was built for", not as an error. Every field a lower version documented still
exists at a higher one — changes are appended — so the safe response is to read
what you know and ignore the rest. Refusing to connect on a version you have
not seen would turn a future additive release into an outage.

`version()` is per-contract feature detection. It complements the per-event
schema version proposed in #83 rather than replacing it: that answers "what
shape is this record", this answers "what build is this deployment".

## Jar discovery — design decision

The previous contract exposed a `get_jar_ids() -> Vec<String>` view backed by
an instance-storage vector that grew by one entry on every `create_jar` call.
This had two problems:
#### `seq` — the tip's sequence number

`seq` is a contract-wide counter, incremented once per successful tip, and is
also the return value of `tip` / `tip_with_ref`. Numbers start at `1` and a
reverted tip consumes none, so the sequence has no holes: an indexer that has
stored `1..N` and reads `N` from `get_tip_count` has lost nothing, and a gap
means its own scan is incomplete rather than that a tip never happened.

This exists because consumers previously keyed on the transaction hash. The
backend indexer upserts on `txHash` and the SDK's `TipEvent` tells callers to
deduplicate on it — which holds only while there is one tip per transaction. A
smart wallet batching two tips, or any client submitting tips from one account
in a single transaction, puts several `tip` events in one transaction that all
share that hash, and every tip after the first is dropped as a duplicate.
`seq` is unique per tip by construction.

`seq` is deliberately in the **data**, not the topics. The SDK's
`tipTopicFilter` must have exactly one segment per published topic, so adding a
third topic would make its existing two-segment filter match nothing — silently,
for every consumer. `get_tip_count() -> u64` exposes the running total, `0` on a
fresh deploy.

#### `reference` — an optional client id

`reference` lets the integration that started a tip recognise it when the event
comes back through an indexer or a webhook. An embeddable button, a stream
overlay, and a Discord/Telegram bot each create something off-chain first (a
pending alert, a bot command, a checkout session) and then wait for the matching
`tip` event. Sender, amount and message are none of them unique, and the message
is visible to the creator, so it cannot carry an opaque id.

The value is opaque to the contract and the contract's role stops at echoing it:

- it is **not stored** — no key is written, and reading the jar or the tip count
  is unaffected by whether a tip carried one;
- it is **not checked for uniqueness** — two integrations may send the same
  reference, and two tips in the same jar may carry it. Reconciling a collision
  is the consumer's job; if a consumer needs uniqueness it must use `seq`, which
  the contract does guarantee;
- it **costs nothing when absent** — `tip` passes `None`, and an absent
  reference still occupies position six.

It is exactly 32 bytes (`Option<BytesN<32>>`): enough for a binary UUID, a
`sha256` tag, or a big-endian primary key, and bounded so the payload cannot grow
the way an unbounded string would. The entry point is `tip_with_ref`; `tip` is
unchanged and remains the positional call the SDK and the web tip form use.

> **Consumer impact.** The data tuple grew from four elements to six. A
> positional decoder — `decodeTipEvent` in `@novatip/sdk`, and the backend
> indexer — must be updated to accept `seq` and `reference` before it reads
> events from a contract built from this version. The first four elements are
> unchanged and keep their positions, so a consumer that ignores trailing
> elements is unaffected. `tip` now also *returns* a `u64` where it previously
> returned nothing; callers that ignored the empty result keep working, and the
> web tip form can use the returned number to link the supporter to the tip they
> just sent.
>
> An earlier change in the same section took the tuple from three elements to
> four by appending `breakdown`; that wording describes the `breakdown` addition
> and is kept for the migration note.

## Jar discovery — design note

Indexing needs a way to enumerate every registered jar. An earlier design kept
that list on-chain as a `get_jar_ids() -> Vec<String>` view backed by an
instance-storage vector that grew by one entry on every `create_jar`. It had
two problems:

1. **Unbounded growth.** The vector had no removal path and no size cap, so it
   grew permanently with every jar ever created.
2. **Escalating cost.** Instance storage is read and written in full on every
   `create_jar`. As the vector grew, each new jar creation cost more than the
   last, and the entry would eventually approach the ledger entry size limit,
   causing `create_jar` to fail for all callers.

**Decision — the design in use today:** keep no on-chain list and move discovery
to the event log. `create_jar` emits a `jar_crtd` event carrying the `jar_id`
and `owner`; indexers reconstruct the full jar list by scanning those events
from the contract's creation ledger (or from their last checkpoint). This is the
standard pattern for Soroban contracts where enumeration is needed but unbounded
on-chain state is not acceptable, and it keeps `create_jar` cost O(1) regardless
of how many jars exist.

### History, not instructions: this was never a `get_jar_ids` migration

The `get_jar_ids` view and its backing vector were **added and removed within a
single unreleased cycle** (see `CHANGELOG.md`), so no published build ever
exposed the function — there is nothing to migrate off or seed from. `v0.1.0`
ships only `__constructor`, `create_jar`, `update_splits`, `tip`, `get_jar` and
`get_token`.

The real gap is event coverage, not a removed function. `jar_crtd` is emitted
only by builds **after** `v0.1.0`:

- A deployment built from `v0.1.0` never emitted `jar_crtd` at all, so the jars
  it created are absent from the event log. The testnet contract referenced in
  the README predates the current event payloads as well.
- To find jars created on such a deployment, replay its `create_jar`
  invocations from Horizon (Soroban transaction history), or seed them from an
  off-chain snapshot — not from a `get_jar_ids` response, which no deployment
  has ever returned.

Every jar created by a build that emits `jar_crtd` needs no special handling:
scanning the event log from the contract's creation ledger is enough.

## Deploy & bootstrap

The scripts load `.env` from the repository root themselves — exported
variables win over `.env`, and no manual `source` step is needed. They work
when started from any directory.

```bash
./scripts/deploy.sh        # deploys, writes .contract-id
./scripts/create-jar.sh    # registers an example jar
./scripts/set-min-tip.sh   # sets (or clears) a jar's minimum tip
./scripts/transfer-jar.sh  # hands a jar to a new owner
```

See [`.env.example`](../.env.example) for required variables, and the
[README's scripts section](../README.md#scripts) for what each script needs and
how `MIN_TIP=none` reaches `Option::None`.
Once a jar exists:

```bash
./scripts/jar-info.sh                      # read-only: owner, recipients, minimum tip
AMOUNT=100000 ./scripts/jar-info.sh        # ... plus what a tip of that size pays each recipient
./scripts/update-splits.sh                 # replace a jar's splits (signed by the jar owner)
SPLITS='[{"to":"G...A","bps":7000},{"to":"G...B","bps":3000}]' ./scripts/update-splits.sh
```

`jar-info.sh` only simulates — it submits no transactions and pays no fee, and
any `SOURCE` identity works. `update-splits.sh` validates the new splits with
the same checks as `create-jar.sh` (both share `scripts/lib/splits.sh`), prints
the old and new splits side by side, refuses to send when they are unchanged,
and requires `SOURCE` to be the jar's owner before invoking `update_splits`.

See [`.env.example`](../.env.example) for required variables.

### USDC trustlines

Every classic `G...` account that sends **or receives** a tip needs a trustline
to the settlement asset first, and tips are all-or-nothing — one recipient
without a line fails the whole tip with the token's `Error(Contract, #13)`.
`scripts/trustline.sh` adds the line for the identities in `IDENTITIES`,
deriving the classic asset (`USDC:<issuer>`) from `USDC_TOKEN` itself, and
skips accounts that already have one. With `SLUG` set it also checks the jar's
recipients; `--check` reports without changing anything and exits non-zero if
anyone cannot be paid:

```bash
IDENTITIES="tipper alice bob" ./scripts/trustline.sh          # add lines
SLUG=@demo ./scripts/trustline.sh --check                     # report only
```

Testnet USDC is issued by the account encoded in the derived asset name
(`USDC:GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5`
on testnet). There is no on-chain claim — fund an account by payment from a
holder (see the usage comment at the top of `scripts/trustline.sh`), after
`stellar fund` covers the XLM reserves.

### Inspecting events

When an indexer misses something, the first question is whether the event was
ever emitted. `scripts/events.sh` computes the topic filters with
`stellar xdr encode --type ScVal` instead of hand-rolled base64 (a wrong
encoding fails loudly, a wrong segment count silently returns nothing — see
the encoding walkthrough in the issues thread #57):

```bash
EVENT=tip SLUG=@demo ./scripts/events.sh   # tips to one jar
EVENT=all ./scripts/events.sh              # every event type, recent window
```

It lists each of the five event types (`jar_crtd`, `splits`, `jar_xfer`,
`min_tip`, `tip`), one line per event with ledger, name, jar id and decoded
payload, defaulting to a recent ledger window; a `START_LEDGER` outside the
RPC's retention window gets an explanatory error. Sample output is in the
script's usage comment.

## Calling from another contract

`tip` authorizes `from` with `from.require_auth()`, which a calling contract
satisfies automatically — but the per-recipient `transfer(from, …)` calls it
then makes are invoked by `tip_splitter`, not by the caller, so they are not
covered. A contract tipping from its own balance must pre-authorize each
transfer with `env.authorize_as_current_contract`, listing every recipient and
the exact amount, or the tip fails with an auth error.

The amounts to authorize are what `preview_split(jar_id, amount)` returns, with
one exception: if the calling contract is itself a recipient, `tip` skips the
self-transfer and rolls that share into the last recipient, so authorize
`preview_split`'s values **minus** the contract's own entry, plus the same
amount on the final recipient.

`examples/tip-forwarder` is a worked example of exactly this — one function
that tips a jar from the contract's own balance — with tests that run it
against the real `TipSplitter` and a test SAC, including the failure mode where
the authorized amounts do not match the jar's splits. See its
[`README.md`](../examples/tip-forwarder/README.md).
