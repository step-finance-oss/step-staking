# Step Staking

Step Staking is a Solana program for single-token staking with transferable receipt tokens. Users deposit STEP into a shared vault and receive xSTEP representing their share of that vault. Rewards are added by transferring more STEP into the vault; holders receive their share of those rewards when they redeem xSTEP.

This repository renews the original [Step Finance implementation](https://github.com/step-finance/step-staking/tree/0ea372b6aab7e2b0c9d922de32ec02484fc0fea7) as a source reference, with current build tooling, explicit trade limits and regression tests. Its design follows the xSushi/SushiBar model. The initial commit is a cleaned source snapshot; the following commit contains the renewal work. Original Git history, committed test keys and CI configuration are excluded.

## What it does

- Pools one underlying SPL token and issues proportional receipt shares.
- Accrues externally funded rewards without a reward schedule or individual claim accounts.
- Lets receipt holders redeem principal and rewards together, without a lockup period.
- Supports SPL Token delegates and sending receipts or redeemed tokens to another account.
- Requires a minimum output on every deposit and redemption, and rejects zero-output trades.
- Emits price and price-change events for clients and indexers.
- Recovers underlying tokens accidentally sent to the vault's associated token account.

The program does not generate yield itself, promise an APR, or distribute rewards over time. APR must be estimated externally from historical vault funding. It uses the original SPL Token program; Token-2022 extensions are not supported.

## How shares and rewards work

Let `T` be the vault's underlying token balance, `S` the total receipt supply, and `a` the input amount. All calculations use integer base units:

| Operation | Output |
| --- | --- |
| Deposit when `S = 0` | `a` receipt units |
| Deposit when `S > 0` | `floor(a × S / T)` receipt units |
| Redeem receipt units | `floor(a × T / S)` underlying units |

For example, a vault with 100 STEP and 100 xSTEP has a 1:1 exchange rate. Adding 20 STEP as rewards raises the rate to 1.2 STEP per xSTEP. Redeeming 10 xSTEP then returns 12 STEP. A new 12 STEP deposit receives 10 xSTEP at that rate.

Rounding favors the existing pool. Both output amounts must be positive and meet the caller's minimum. Products use `u128` intermediates and reject results outside the SPL token's `u64` limits. Deposits also reject a pool with outstanding receipts and no collateral.

An empty pool retains the original bootstrap rule: if STEP has already been sent to a vault with no receipts outstanding, the first depositor receives one-for-one receipts and can claim those unowned funds. Fund rewards after holders have established the pool if that is not the intended distribution.

Burning receipts directly through SPL Token, rather than redeeming through this program, leaves their collateral in the vault. It benefits the remaining receipt holders, or the next depositor if all receipts have been burned. This includes permissionless burns of receipts sent to SPL Token's sink authorities.

Direct donations can change the exchange rate between quoting and execution. Set a meaningful minimum from the user's accepted quote and tolerance; a minimum of one base unit provides little price protection.

## Instructions

| Instruction | Purpose |
| --- | --- |
| `initialize(nonce)` | Creates the canonical, self-authorized token vault. Anyone can pay for initialization; it grants no administrator role. The supplied legacy nonce is unused here: Anchor derives the canonical bump. |
| `stake(nonce, amount, minimum_shares)` | Transfers underlying tokens from an authorized owner/delegate and mints receipts to the chosen destination. |
| `unstake(nonce, amount, minimum_tokens)` | Burns authorized receipt tokens and sends underlying tokens to the chosen destination. |
| `emit_price()` | Emits the current underlying-to-receipt ratio; clients can simulate it. |
| `withdraw_nested()` | Permissionlessly sweeps the vault's underlying-token ATA into the real vault, closes the ATA and refunds its rent to the supplied system account. |
| `reclaim_mint_authority(nonce)` | Lets the underlying mint authority reclaim the receipt mint authority from the vault PDA. |

Trade and reclaim nonces must match the canonical vault bump. Token source authorities must sign as the owner or an approved delegate with sufficient allowance. A permissionless SPL burn from a System Program or incinerator-owned account does not authorize redemption from this vault.

`Price` and `PriceChange` retain the original event field layouts. Their `*_e9` fields express the ratio scaled by one billion and saturate at `u64::MAX` for extreme ratios. Decimal string fields are truncated to nine fractional places without that integer cap. Use token balances and receipt supply for exact trade quotes.

### Authority and deployment model

The vault PDA is derived from `[underlying_mint.toBuffer()]` under the program ID. It owns the vault token account and must be the receipt mint authority before deposits can work. Receipt mints are configured separately; `initialize` does not create them or change their authority.

The underlying mint authority is trusted. After reclaiming receipt mint authority, it can mint unbacked receipts and dilute holders. Reclaim stops new deposits until authority is restored to the vault; existing receipts remain redeemable. A revoked underlying mint authority cannot use the reclaim instruction. Mint freeze authorities, if configured externally, and any program upgrade authority are additional trust assumptions.

The default build preserves these historical public addresses:

| Account | Address |
| --- | --- |
| Program | `Stk5NCWomVN3itaFjLu382u9ibb5jMSHEsh6CuhaGjB` |
| STEP mint | `StepAscQoEioFxxWGnh2sLBDFp9d8rvKz2Yp39iDpyT` |
| xSTEP mint | `xStpgUCss9piqeFUk2iLVcvJEGhAdJxJQuwLkXP555G` |

This source has not been independently audited or deployed by this renewal. It makes no claim about the code or authorities currently deployed at those addresses. For your own deployment, set your own program ID, mint constants and matching client configuration, derive its vault, and review its mint and upgrade authorities. The optional `test-id` and `local-testing` features retain historical alternate public IDs; the test harness below uses the default build with fresh local account state instead.

**Version 0.2 changes the instruction interface:** `stake` and `unstake` now require their third, minimum-output argument. Regenerate clients from the new IDL. Old two-argument instruction data is rejected; review integrators before considering an upgrade. No on-chain deployment is part of this repository's setup or test commands.

## Build and test

Prerequisites:

- Rust and Cargo, with the Solana SBF build tools available.
- Anchor CLI **1.2.2**.
- Agave/Solana CLI and `solana-test-validator` **4.4.0**. The bundled `cargo-build-sbf` reports 4.5.0 and uses platform-tools v1.57.
- Node.js **22.12+** and npm. The renewal was tested with Node 26.7.0.

Make sure the matching CLI versions are on `PATH`; `Anchor.toml` records them. Then run:

```bash
npm ci --ignore-scripts
npm test
```

`npm test` builds the SBF program and IDL, runs Rust unit tests, and executes integration tests against a disposable localhost validator. It creates a fresh wallet and public mint-account fixtures, loads the program under its default ID, and deletes the temporary wallet and ledger on exit. It needs no production keys, RPC credentials or live network accounts. Default test ports are 18899–18901; set `TEST_RPC_PORT` to use another free range.

The JavaScript SPL dependency can emit a native-bigint binding warning after installation with `--ignore-scripts`; the tests use its pure JavaScript fallback.

The transitive `bigint-buffer` dependency has an upstream [native buffer-overflow advisory](https://github.com/advisories/GHSA-3gc7-fjrx-p6mg). The installation command above leaves that native binding unbuilt; account for the advisory before enabling native install scripts. npm currently reports this as three affected package entries along the same dependency path.

Individual checks:

```bash
npm run build                 # SBF binary, IDL and generated TypeScript types
npm run test:unit             # Rust tests with Cargo.lock
npm run test:integration      # Build, then start the disposable validator suite
npm run typecheck            # After generating the types with a build
npm run lint
cargo fmt --all -- --check
```

Generated output lives in `target/`, `.test-build/` and `.anchor/`, which are ignored by Git. GitHub Actions and other CI configuration are intentionally absent.

## Calling the program

Load the generated IDL with `@anchor-lang/core` and a provider for your chosen deployment. Resolve or create the user's underlying and receipt token accounts first. The example assumes the configured token mints and program ID match that deployment:

```ts
import { AnchorProvider, BN, Program, web3 } from "@anchor-lang/core";
import { TOKEN_PROGRAM_ID } from "@solana/spl-token";
import { readFileSync } from "node:fs";
import type { StepStaking } from "./target/types/step_staking";

const provider = AnchorProvider.env();
const idl = JSON.parse(
  readFileSync("target/idl/step_staking.json", "utf8")
) as StepStaking;
const program = new Program<StepStaking>(idl, provider);
const tokenMint = new web3.PublicKey("StepAscQoEioFxxWGnh2sLBDFp9d8rvKz2Yp39iDpyT");
const xTokenMint = new web3.PublicKey("xStpgUCss9piqeFUk2iLVcvJEGhAdJxJQuwLkXP555G");
const [tokenVault, bump] = web3.PublicKey.findProgramAddressSync(
  [tokenMint.toBuffer()], program.programId
);

async function deposit(
  tokenFrom: web3.PublicKey,
  xTokenTo: web3.PublicKey,
  amount: BN,
  minimumShares: BN // Computed from the user's accepted quote and tolerance.
) {
  return program.methods.stake(bump, amount, minimumShares).accountsStrict({
    tokenMint, xTokenMint, tokenVault, tokenFrom, xTokenTo,
    tokenFromAuthority: provider.wallet.publicKey,
    tokenProgram: TOKEN_PROGRAM_ID,
  }).rpc();
}

async function redeem(
  xTokenFrom: web3.PublicKey,
  tokenTo: web3.PublicKey,
  receiptAmount: BN,
  minimumTokens: BN
) {
  return program.methods.unstake(bump, receiptAmount, minimumTokens).accountsStrict({
    tokenMint, xTokenMint, tokenVault, xTokenFrom, tokenTo,
    xTokenFromAuthority: provider.wallet.publicKey,
    tokenProgram: TOKEN_PROGRAM_ID,
  }).rpc();
}
```

For quote calculations, fetch the vault with SPL Token's `getAccount` and the receipt mint with `getMint`, then use the formulas above with `bigint` or `BN`. Never use floating-point token amounts. The receipt mint's decimal count determines its display units; the formulas operate only on raw base units. The local mint fixtures use nine decimals.

`migrations/deploy.js` is a vault-initialization helper for an already configured deployment. It does not create token mints, provision a wallet, fund the vault or change mint authorities. The disposable test harness initializes its own fixture directly; `npm test` does not invoke migrations.

## License and attribution

The original **GPLv2** [LICENSE](LICENSE) is retained, together with the Step Finance and SushiBar lineage. Review that license when reusing or distributing this code and preserve applicable third-party notices.
