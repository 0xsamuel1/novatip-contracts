# Changelog

All notable changes to `novatip-contracts` are documented here.

## [Unreleased]

### Added
- `version() -> u32` returns `INTERFACE_VERSION`, a compile-time constant that
  is bumped whenever the public interface changes shape (#169). A client that
  knows only a contract id can now ask which build it is talking to instead of
  discovering a mismatch when an event fails to decode. It reads no storage, so
  it works against a contract that has no jars. Initial value `1`
- `get_jar_stats(jar_id) -> JarStats { tip_count, total }` and
  `get_supporter_stats(jar_id, supporter) -> SupporterStats` report tip totals
  kept on-chain, bumped inside the tip that pays out (#167, #168). The jar
  counters give an authoritative figure to compare the indexer against —
  `planBatch` skips any ledger that returns a full page of events, and an event
  older than the RPC's retention window can never be backfilled, so a missed
  tip previously lowered a creator's totals with nothing to contradict it. The
  supporter counters are the data the planned `supporter_badge` contract needs
  to verify a milestone without trusting an off-chain service. Both read zeros
  for a jar or address that has not been tipped, and `JarNotFound` for a free
  slug
- `allow_token(token)` / `disallow_token(token)`, `is_token_allowed(token)`,
  and `tip_in_token(from, jar_id, token, amount, message)` (#170). The admin
  manages an allowlist of assets tips may be paid in, so a supporter holding
  EURC no longer has to swap into the deploy-time token first. The allowlist is
  checked before any transfer, so an unlisted token fails with
  `TokenNotAllowed` and moves nothing; the split arithmetic is shared with
  `tip`, so an EURC tip of 100 pays 70/30 exactly as a USDC tip of 100 does.
  `tip` itself is unchanged — it settles in `get_token()`, which the
  constructor allowlists — so existing integrations keep working.
  `disallow_token` refuses the default token with `CannotDisallowDefault`
- Error codes 14 `TokenNotAllowed`, 15 `CannotDisallowDefault` and 16
  `StatsOverflow`, appended so existing codes keep their values

### Changed (breaking: event payload)
- The `tip` event's data tuple gained a fifth element, `token: Address`, naming
  the asset the tip was paid in. Without it an indexer cannot tell 10 USDC from
  10 XLM, and a jar tipped in two assets would have its totals summed across
  them as one currency. `decodeTipEvent` in `@novatip/sdk` and the backend
  indexer must accept the fifth element before reading events from a contract
  built from this version; the first four keep their positions and meaning, so
  a consumer ignoring trailing elements is unaffected. `version()` is how a
  client detects this in advance

### Decisions
- The per-jar minimum from `set_min_tip_amount` is a raw `i128` with no unit, so
  it now applies to the default token only: a tip in another allowed asset is
  not gated by it. Making it per-token would change that function's signature
  and storage key, breaking every jar that already configured a minimum. The
  trade-off is stated in `docs/CONTRACT.md` — a jar can be tipped below its
  floor in another asset, so a creator who cares keeps the allowlist to one
  token
- The allowlist lives in instance storage beside `Admin` and `Token`. A
  persistent entry nobody touches is archived silently, which would present as
  "this token stopped working" with no admin action behind it

### Costs
- A tip now performs two extra persistent writes (the jar's and the
  supporter's totals) and two extra rent bumps: 5 ledger writes and 924 bytes
  instead of 3 and 520, roughly +25% modelled CPU, measured by
  `tip_writes_the_two_totals_entries`. The difference is flat — a repeat tip
  rewrites the same two entries and adds none

### Added
- `tip` returns the tip's sequence number and every successful tip increments a
  contract-wide `u64`, published as the fifth element of the `tip` event. A
  consumer can now order tips and prove it has seen all of them, which the
  transaction hash could not do: two tips in one batched transaction share a
  hash, and an indexer upserting on it dropped every one after the first.
  `get_tip_count() -> u64` exposes the running total (`0` on a fresh deploy) as a
  cheap completeness check
- `tip_with_ref(from, jar_id, amount, message, reference)` — the same tip with an
  optional opaque 32-byte client reference echoed in the `tip` event, so an
  integration that created something off-chain (a checkout session, a bot
  command, a pending alert) can match the event back to it. The contract neither
  stores it nor enforces uniqueness; omitting it costs nothing. `tip` keeps its
  existing signature and publishes an explicit `None` in the same position, and
  both entry points share one internal implementation so they cannot drift
- `scripts/transfer-jar.sh` and `scripts/set-min-tip.sh` cover the two remaining
  jar-owner calls, which until now had to be assembled as `stellar contract
  invoke` commands by hand. `set-min-tip.sh` clears a minimum by omitting
  `--min_amount` entirely — the only spelling of `Option::None` the CLI accepts —
  and rejects zero, negative and non-numeric amounts locally, before paying for a
  network call. `.env.example` documents `NEW_OWNER` and `MIN_TIP`, and the
  README gained a scripts section
- `docs/CONTRACT.md` gained an **Authorization** section describing what a
  signature actually commits to: a tipper authorizes the `tip` call plus one
  token transfer per paid recipient, with the exact recipient and amount. That
  explains why wallets show N transfers rather than one payment to the contract,
  why a tip signed against old splits fails with `Error(Context, InvalidAction)`
  once `update_splits` lands (so an owner cannot redirect a tip in flight), and
  that the client's recovery is to re-simulate and re-sign rather than retry

### Changed (breaking: event payload)
- `scripts/update-splits.sh` replaces a jar's splits with the same local
  validation `create-jar.sh` applies (both now share `scripts/lib/splits.sh`).
  It prints the old and new splits before sending, refuses unchanged splits,
  and refuses to sign when `SOURCE` is not the jar's owner
- `scripts/jar-info.sh`, a read-only inspector: owner, each recipient with its
  share as a percentage, the minimum tip, and — with `AMOUNT` set — what a tip
  of that size pays each recipient (`preview_split`). It submits nothing
- `scripts/check-docs.sh` (`make check-docs`, CI step `docs-check`) builds the
  contract and fails when `docs/CONTRACT.md`'s function or error tables no
  longer match the built interface — missing or extra entries, or an error
  documented with a different number
- `scripts/lib/env.sh`, sourced by every script: loads the repository-root
  `.env` (exported variables win), cds to the repo root so scripts work from
  any directory, and reports missing settings by name

### Changed
- `make deploy` and `scripts/*.sh` no longer require a manual
  `set -a; source .env; set +a`; `.env.example` documents the optional
  `CONTRACT_ID` override and the testnet/mainnet USDC SAC ids

### Added
- `scripts/events.sh` lists the contract's published events with topic filters
  computed via `stellar xdr encode --type ScVal` instead of hand-encoded
  base64. Takes `EVENT` (one of the five event names or `all`), optional
  `SLUG`, and an optional ledger window, and explains the RPC's event
  retention when a start ledger is too old
- `scripts/trustline.sh` adds the USDC trustline every classic account needs
  to tip or receive tips, deriving the classic `CODE:ISSUER` line from
  `USDC_TOKEN`'s own `name()`. Idempotent (accounts that already have the
  line are skipped); `--check` with `SLUG` reports a jar's recipients that
  cannot receive a tip and exits non-zero. Its usage comment and
  `docs/CONTRACT.md` explain where testnet USDC comes from
- `examples/tip-forwarder`: a worked example of `tip_splitter` called from
  another contract — the caller must pre-authorize each per-recipient
  `transfer` with `env.authorize_as_current_contract`, and the amounts differ
  from `preview_split` when the caller is itself a recipient. Tests run the
  real `TipSplitter` and a test SAC under strict `mock_auths`; the interface
  is declared with a local `#[contractclient]` so `cargo test` needs no built
  wasm
- `.github/`: issue forms (`bug_report.yml` requiring network, contract id,
  CLI version, command and full output; `feature_request.yml`), `config.yml`
  disabling blank issues and routing security reports to the private channel,
  and a pull request template with interface/changelog/consumer-impact
  checks
- `docs/CONTRACT.md` gains "Calling from another contract", "USDC
  trustlines" and "Inspecting events" sections
- `InsufficientBalance` (code 14, appended so existing codes are unchanged).
  `tip` reads the sender's token balance once before the first transfer and
  fails with this code when `balance < amount`, instead of letting the SAC's
  overlapping `BalanceError` (code 10) propagate as `SplitsEmpty`. Other token
  failures (missing or frozen trustline, allowance overflow) still surface
  with the token's own number — see `docs/CONTRACT.md`
- `get_limits() -> Limits` returns `bps_denom`, `max_recipients` and
  `max_message_len`, so clients read the bounds from the contract they are
  talking to instead of hardcoding their own copy
- `is_recipient(jar_id, address) -> bool` answers whether an address appears in
  a jar's splits, so a collaborator can confirm their share without fetching
  the whole jar and scanning it. An unregistered slug is `JarNotFound`, not a
  quiet `false`

### Changed (breaking: event payload)
- The `tip` event's data tuple grew from four elements to six: `seq: u64` and
  `reference: Option<BytesN<32>>` were appended after `breakdown`. A positional
  decoder (`decodeTipEvent` in `@novatip/sdk`, the backend indexer) must accept
  both before reading events from a contract built from this version; the first
  four elements keep their positions, so a consumer that ignores trailing
  elements is unaffected. Neither joins the topics, because the SDK's
  `tipTopicFilter` requires exactly one segment per published topic
- The `tip` event's data tuple gained a fourth element, `breakdown:
  Vec<(Address, i128)>`, listing each recipient and the amount they were
  actually paid in split order. An indexer no longer has to fetch the jar and
  redo the split arithmetic — which could disagree with the contract if the
  jar's splits changed between the tip and the read. A positional decoder
  (`decodeTipEvent` in `@novatip/sdk`, the backend indexer) must be updated to
  accept the fourth element; the first three keep their positions and meaning
- The `jar_xfer` event's data changed from a bare `new_owner: Address` to
  `(prev_owner, new_owner)`, so each event says where a jar came from as well
  as where it went. An indexer starting from a partial history no longer needs
  the running state to interpret one, and successive transfers chain. A
  decoder reading the data as a single address must be updated

### Fixed
- `extend_ttl` was called with two arguments where the SDK takes three
  (`key`, `threshold`, `extend_to`), so the crate did not compile. A
  `JAR_TTL_THRESHOLD` constant now supplies the missing bound, and the TTL is
  only bumped once the remaining lifetime drops below it rather than on every
  access
- The `JAR_TTL_LEDGERS` doc comment claimed 1 000 000 ledgers was "roughly 78
  years"; at the 5-second close rate it is roughly 58 days
- Event assertions in `test.rs` compared `Val` with `==`, which it does not
  implement. The payloads are now decoded to their concrete types, which also
  asserts the events decode to the shape consumers expect
- Split-validation tests still expected the removed catch-all `InvalidSplits`
  after error codes were split into `SplitsEmpty`, `SplitOutOfRange` and
  `SplitSumNot100Pct`
- The workspace used `resolver = "2"`, under which the `rust-version` pin is
  ignored when selecting dependency versions. Cargo resolved a `getrandom`
  requiring `edition2024`, which the pinned 1.84 toolchain cannot parse, so no
  build succeeded. Switching to `resolver = "3"` makes the MSRV pin bind, and
  `Cargo.lock` is committed with the resolved set
- `jar_id.len() == 0` replaced with `is_empty()` (clippy, `-D warnings`)

### Changed
- `rust-toolchain.toml` now lists `wasm32v1-none` alongside
  `wasm32-unknown-unknown`, so `rustup show` installs the target
  `stellar contract build` needs without a separate step
- `docs/CONTRACT.md` documents error codes 10, 11 and 12, records that code 4
  is retained but no longer raised, and notes that codes are appended rather
  than renumbered
- Added `CONTRIBUTING.md` and `.gitattributes`


### Deployed
- Testnet deployment at
  [`CCKPD2MPAYQYCL7QJKUMJHTEJWNHSIP7BZYIZBRO3C4DM4VHZKL6VBRL`](https://stellar.expert/explorer/testnet/contract/CCKPD2MPAYQYCL7QJKUMJHTEJWNHSIP7BZYIZBRO3C4DM4VHZKL6VBRL),
  settling in the testnet USDC SAC, with a `@demo` jar splitting 70/30

### Fixed
- `validate_splits` counted each entry's `bps` twice, so any valid split vector
  was rejected as `InvalidSplits` — a merge kept the accumulate line from two
  separate changes
- `test.rs` was missing the `MockAuth`, `MockAuthInvoke` and `IntoVal` imports
  the auth tests use, leaving the crate uncompilable
- `scripts/deploy.sh` looked for the wasm under `wasm32-unknown-unknown`; the
  Stellar CLI now builds to `wasm32v1-none`, so the path is discovered instead

### Added
- `jar_crtd` event published on every successful `create_jar`, carrying the jar
  slug and owner, so indexers can discover jars from the event log
- `DuplicateRecipient` (code 7) error variant, raised when a split vector names the same address twice
- Edge case tests for single recipient tip and max recipients rejection
- Tests covering zero-bps rejection on `create_jar` (leading, trailing, and sole entry) and on `update_splits`
- Tests for duplicate recipients (adjacent and non-adjacent) on `create_jar` and `update_splits`, plus valid-case coverage for distinct recipients and a full 20-recipient jar
- Test that `tip` fails when the sender's balance is insufficient
- Tests covering a share above 100% and a `bps` set whose sum overflows `u32`,
  on both `create_jar` and `update_splits`
- `jar_exists(jar_id) -> bool` view function, so a slug-availability check no
  longer has to call `get_jar` and catch the `JarNotFound` panic. Reads one
  persistent key, matches slugs exactly, and is cheap enough for the debounced
  onboarding check. Not yet exposed from `@novatip/sdk`.
- Tests covering `jar_exists` before and after registration, against a rejected
  jar, and across `update_splits`
- `MAX_MESSAGE_LEN` (280 bytes) bound on the `tip` message, with a new
  `MessageTooLong` error (code 8, appended so existing codes are unchanged)
- Tests covering a message one byte over the limit and one at exactly the limit
- `MAX_JAR_ID_LEN` (64 bytes) bound on `jar_id` in `create_jar`, with a new
  `InvalidJarId` error (code 9, appended so existing codes are unchanged).
  `jar_id` is used as a storage key, an event topic, and a public URL slug, so
  an empty or unbounded id costs unnecessary rent and can produce jars no
  frontend can address
- Tests covering an empty `jar_id`, one byte over the limit, and one at
  exactly the limit
- `scripts/create-jar.sh` now accepts a `SPLITS` env var carrying a full
  splits JSON array, so it can create multi-recipient jars instead of only
  the single-recipient case. `RECIPIENT` still works unchanged as shorthand
  for a 100% split. `SPLITS` is validated locally (valid JSON array, 1-20
  recipients, `bps` summing to exactly 10000) before invoking the contract
- `make optimize` target, and a matching optimize step in `scripts/deploy.sh`
  between build and deploy. The deployed wasm was previously the unoptimized
  build output, which permanently inflates both the upload cost and the
  contract code entry's ongoing state rent; the before/after byte size is now
  printed during deploy

### Changed
- Jar discovery moved from an on-chain list to the event log. The `get_jar_ids()`
  view and its `JarIds` instance-storage vector — both added and removed within
  this unreleased cycle, so never shipped in a release — are gone: the vector
  grew without bound and made each `create_jar` cost more than the last.
  Indexers reconstruct the jar list by scanning `jar_crtd` events; see
  [`docs/CONTRACT.md`](docs/CONTRACT.md) for how discovery works and how to find
  jars created before a build emitted `jar_crtd`.
- `validate_splits()` now rejects any split with `bps == 0` (`InvalidSplits`).
  Such a recipient could never be paid but still consumed one of the 20
  recipient slots and appeared in clients as a collaborator. This applies to
  both `create_jar` and `update_splits`; jars written before this change are
  unaffected on read but must drop zero-bps entries before their next
  `update_splits` call.
- `validate_splits()` now rejects any split vector containing the same recipient
  address more than once, on both `create_jar` and `update_splits`. Duplicates
  were not a loss-of-funds bug, but they made `tip` issue several transfers to
  one destination in a single call and forced per-collaborator accounting to
  de-duplicate after the fact. Clients that allow entering a collaborator twice
  must sum the shares before submitting. Jars written before this change are
  unaffected on read, but must drop duplicates before their next `update_splits`
  call.
- `create_jar_rejects_bad_bps_sum` now uses two distinct recipients, so it still
  exercises the sum check rather than tripping the new duplicate check first

### Fixed
- `validate_splits()` added each entry's `bps` to the running total twice, so
  splits summing to the required `10_000` were rejected as `InvalidSplits` and
  no valid jar could be created or updated. The zero-bps and duplicate-recipient
  branches were merged with both sides of the accumulator line left in place.

## [0.1.0] - 2025-07-01

### Added
- `tip_splitter` Soroban contract (Rust) deployed on Stellar testnet
- `create_jar(owner, jar_id, splits)` - register a tip jar with basis-point splits
- `tip(from, jar_id, amount, message)` - atomic USDC split across all recipients
- `update_splits(jar_id, splits)` - replace a jar's collaborator splits
- `get_jar(jar_id)` - read a jar's on-chain configuration
- `get_token()` - return the configured USDC Stellar Asset Contract address
- Atomic payment routing with rounding dust sent to the last recipient
- Typed contract errors (`NotInitialized`, `JarExists`, `JarNotFound`, `InvalidSplits`, `InvalidAmount`, `TooManyRecipients`)
- `tip` event emission on every successful tip, with topics `(symbol "tip", jar_id)`
- Full test coverage for all contract functions
- Deploy scripts for testnet and mainnet (`scripts/deploy.sh`, `scripts/create-jar.sh`)
- GitHub Actions CI pipeline (fmt, clippy, test)
