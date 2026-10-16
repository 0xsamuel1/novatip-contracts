# novatip-contracts

Soroban smart contracts for **Novatip** — tap-to-tip any creator in seconds, with
tips split across collaborators on-chain and settled in USDC on Stellar.

This repository holds the on-chain core. The backend (`novatip-backend`), web app
(`novatip-web`), shared SDK (`novatip-sdk`), and docs (`novatip-docs`) live in
separate repositories under the same organization.

## Contracts

| Contract | Status | Purpose |
|----------|--------|---------|
| `tip-splitter` | ✅ v0.1.0 | Receives a tip and splits it across recipients by basis points |
| `supporter-badge` | planned | Non-transferable badge minted to supporters |

See [`docs/CONTRACT.md`](./docs/CONTRACT.md) for the full `tip-splitter` interface.

## Quick start

```bash
# One-time: the Stellar CLI builds contracts for this target
rustup target add wasm32v1-none

# Build all contracts to wasm
make build

# Run the test suite
make test
```

> Without the target installed, `make build` fails with `can't find crate for
> core` — the CLI moved from `wasm32-unknown-unknown` to `wasm32v1-none`.
> `make test` runs natively and works either way.

## Scripts

Everything under [`scripts/`](./scripts) reads the same environment variables
(see [`.env.example`](./.env.example)) and signs with a configured Stellar CLI
identity. `CONTRACT_ID` is optional for every script that needs it: it falls
back to the `.contract-id` file `deploy.sh` writes.

Load your environment once per shell, then run whichever script you need:

```bash
set -a; source .env; set +a

./scripts/deploy.sh          # build, optimize, deploy; writes .contract-id
./scripts/create-jar.sh      # register a jar (RECIPIENT or SPLITS)
./scripts/transfer-jar.sh    # hand a jar to NEW_OWNER
./scripts/set-min-tip.sh     # set a jar's minimum tip, or clear it
```

| Script | Needs | Notes |
|--------|-------|-------|
| `deploy.sh` | `NETWORK`, `SOURCE`, `ADMIN`, `USDC_TOKEN` | Prints and saves the deployed contract id |
| `create-jar.sh` | `NETWORK`, `SOURCE`, `OWNER`, `SLUG`, and `RECIPIENT` or `SPLITS` | `SPLITS` is validated locally (1–20 recipients, bps summing to 10000) before any call |
| `transfer-jar.sh` | `NETWORK`, `SOURCE`, `SLUG`, `NEW_OWNER` | Reads and prints the jar's current owner first, so a wrong `SOURCE` or contract is visible before the hand-over |
| `set-min-tip.sh` | `NETWORK`, `SOURCE`, `SLUG`, and optionally `MIN_TIP` | `MIN_TIP` is a positive integer in the token's smallest unit. Unset or `none` **clears** the minimum — which means invoking without the `--min_amount` flag at all, the only way to send `Option::None`. Zero, negative and non-numeric values are rejected locally |

`set-min-tip.sh` reads the value back afterwards, since the CLI prints nothing
on success and clearing a jar that already had no floor is a silent no-op.

## Tech

- [Rust](https://www.rust-lang.org/) + [Soroban SDK](https://developers.stellar.org/docs/build/smart-contracts)
- [Stellar CLI](https://developers.stellar.org/docs/tools/cli) for build & deploy
- Tips settle in **USDC** (Stellar Asset Contract)

## Network Deployments

| Network | Contract ID | Status |
|---------|-------------|--------|
| Testnet | [`CCKPD2MPAYQYCL7QJKUMJHTEJWNHSIP7BZYIZBRO3C4DM4VHZKL6VBRL`](https://stellar.expert/explorer/testnet/contract/CCKPD2MPAYQYCL7QJKUMJHTEJWNHSIP7BZYIZBRO3C4DM4VHZKL6VBRL) | live |
| Mainnet | coming soon | pending audit |

The testnet deployment settles in the testnet USDC SAC below, and carries a
`@demo` jar splitting 70/30 between two accounts for anyone wanting to try the
read methods without deploying their own.

USDC Stellar Asset Contract addresses:

| Network | USDC SAC |
|---------|----------|
| Testnet | `CBIELTK6YBZJU5UP2WWQEUCYKLPU6AUNZ2BQ4WWFEIE3USCIHMXQDAMA` |
| Mainnet | `CCW67TSZV3SSS2HXMBQ5JFGCKJNXKZM7UQUWUZPUTHXSTZLEO7SJMI75` |

## Changelog

See [CHANGELOG.md](./CHANGELOG.md) for a full history of changes.

## License

MIT — see [LICENSE](./LICENSE).
