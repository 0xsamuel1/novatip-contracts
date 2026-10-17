# Changelog

All notable changes to `novatip-contracts` are documented here.

## [Unreleased]

### Added
- `version() -> u32` returns `INTERFACE_VERSION`, a compile-time constant bumped whenever the public interface changes shape (#169)
- `get_jar_stats(jar_id) -> JarStats { tip_count, total }` and `get_supporter_stats(jar_id, supporter) -> SupporterStats` report tip totals kept on-chain (#167, #168)
- `allow_token(token)` / `disallow_token(token)`, `is_token_allowed(token)`, and `tip_in_token(from, jar_id, token, amount, message)` for multi-asset tips (#170)
- `tip` returns the tip's sequence number; `get_tip_count() -> u64` exposes the running total
- `tip_with_ref(from, jar_id, amount, message, reference)` adds an optional opaque 32-byte client reference echoed in the `tip` event
- `transfer_jar_ownership(jar_id, new_owner)` and `jar_xfer` event
- `update_splits` emits a `splits` event
- `get_jar_owner(jar_id)`, `get_split_count(jar_id)`, `preview_split(jar_id, amount)`
- `get_admin()`, `set_min_tip_amount(jar_id, min_amount)`, `get_min_tip_amount(jar_id)`
- `min_tip` event and `BelowMinTipAmount` error (code 13)
- `scripts/transfer-jar.sh`, `scripts/set-min-tip.sh`, `scripts/update-splits.sh`
- Error codes 14 `TokenNotAllowed`, 15 `CannotDisallowDefault`, 16 `StatsOverflow`

### Changed (breaking: event payload)
- The `tip` event's data tuple gained `token: Address` (5th element) and `tip_count: u64` (6th element)
- `tip` withholds a recipient's share when that recipient is also the tipper

### Fixed
- Zero-share splits rejected with `SplitOutOfRange` (was `InvalidSplits`)
- Split validation reports specific codes: `SplitsEmpty` (10), `SplitOutOfRange` (11), `SplitSumNot100Pct` (12)
- u32 overflow in split sum handled by `checked_add` instead of relying on `overflow-checks`

### Removed
- `InvalidSplits` (code 4) is no longer raised; variant retained for code stability

### Changed
- `0.1.0` release date corrected to match git tag date

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
