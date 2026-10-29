# Step Staking

A Solana program for single-token staking with transferable receipt tokens and shared rewards.

Step Staking lets holders deposit STEP into a pooled vault and receive xSTEP representing their share of its balance. Applications or reward funders add STEP to the vault, increasing the amount each xSTEP can redeem. Holders withdraw by exchanging their receipts for a proportional share of the underlying tokens.

The model follows xSushi/SushiBar: rewards accrue through the receipt's exchange rate, without a separate claim transaction. The program has no lockup period or scheduled reward emissions. Yield depends on external funding; APR must be calculated by the consuming application from historical data.

## Features

- **Single-token deposits.** Deposit the configured SPL token and receive proportional receipt shares.
- **Shared rewards.** Add rewards directly to the vault for the benefit of outstanding receipt holders.
- **Transferable positions.** Transfer receipt tokens using SPL Token; the redemption claim follows the receipts.
- **Proportional withdrawals.** Burn receipts to redeem their share of principal and rewards together.
- **Minimum outputs.** Set a minimum receipt or withdrawal amount on every trade. Zero-output trades are rejected.
- **Delegated transactions.** Use an approved SPL Token delegate and choose a different receipt or payout recipient.
- **Price events.** Read exchange-rate events through transaction logs or simulation.
- **Token recovery.** Sweep underlying tokens accidentally sent to the vault's associated token account into the actual vault.

## How it works

| Account             | Purpose                                                                                                                       |
| ------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| Underlying mint     | Defines the token accepted by the vault. The default build uses STEP.                                                         |
| Receipt mint        | Defines the transferable shares issued for deposits. The default build uses xSTEP.                                            |
| Vault               | Holds the underlying tokens. Its PDA is also its token authority and must be the receipt mint authority for deposits to work. |
| User token accounts | Hold underlying tokens and receipts. There is no separate staking-position account for each user.                             |

The vault uses a program-derived address (PDA):

```text
vault: [underlying mint public key bytes]
```

The vault address depends on both the underlying mint and the program ID. Creating it grants the initializer no administrative rights. The mints are configured separately; initialization does not create them or change their authorities.

### Shares, rewards and rounding

Let `T` be the vault's underlying token balance, `S` the total receipt supply, and `a` the input amount:

| Operation            | Output                              |
| -------------------- | ----------------------------------- |
| Deposit when `S = 0` | `a` receipt units                   |
| Deposit when `S > 0` | `floor(a × S / T)` receipt units    |
| Redeem receipt units | `floor(a × T / S)` underlying units |

For example, assuming matching token decimals:

| Action                          | Vault balance | Receipt supply | Underlying per receipt |
| ------------------------------- | ------------: | -------------: | ---------------------: |
| Holders deposit 100 STEP        |      100 STEP |      100 xSTEP |                 1 STEP |
| A funder adds 20 STEP           |      120 STEP |      100 xSTEP |               1.2 STEP |
| A holder redeems 10 xSTEP       |      108 STEP |       90 xSTEP |               1.2 STEP |
| Another holder deposits 12 STEP |      120 STEP |      100 xSTEP |               1.2 STEP |

All calculations use integer base units. Rounding favors the remaining pool. Both the input and minimum output must be positive, and the calculated output must meet that minimum. Intermediate products use `u128`; results and resulting token balances must fit within SPL Token's `u64` limits. Deposits reject a pool with outstanding receipts but no backing collateral.

Direct funding changes the exchange rate. Calculate the minimum from the user's accepted quote and tolerance before submitting a transaction. A minimum of one base unit offers little protection against a changed price.

### Empty pools and direct burns

When receipt supply is zero, the first depositor receives one-for-one receipt units even if the vault already contains tokens. That depositor can redeem the previously unowned balance. Fund rewards after holders have established the pool if that is the intended distribution.

Burning receipts directly through SPL Token leaves their collateral in the vault. It benefits the remaining holders, or the next depositor if all receipts have been burned. This also applies to permissionless SPL burns of receipts sent to sink authorities. Such a burn does not itself authorize withdrawing from this program.

## Build and run locally

### Requirements

| Tool                                          | Project version                                    |
| --------------------------------------------- | -------------------------------------------------- |
| Anchor CLI, Rust crates and TypeScript client | `1.2.2`                                            |
| Agave/Solana CLI and `solana-test-validator`  | `4.4.0`                                            |
| SBF platform tools / architecture             | `v1.57` / `v3`                                     |
| Node.js                                       | `22.12.0` or newer                                 |
| Rust and npm                                  | A working host Rust toolchain and npm installation |

Put the matching executables on `PATH`; CLI versions are recorded in [Anchor.toml](Anchor.toml). The Agave installation used here bundles `cargo-build-sbf` reporting version `4.5.0`. The local runner also uses Bash, `curl` and `solana-keygen`.

From the repository root:

```sh
npm ci --ignore-scripts
npm test
```

`npm test` builds the SBF program, IDL and generated TypeScript types under `target/`, runs Rust unit tests, and executes the integration suite against a fresh localhost validator. The build uses `--ignore-keys` because the historical program's private key is not included.

The runner creates a temporary wallet, mint-account fixtures and ledger, loads the default program at genesis, and initializes the vault. It supplies `ANCHOR_PROVIDER_URL` and `ANCHOR_WALLET` itself and removes its temporary state on exit. Tests need no existing wallet, historical mint keys or public-network funds.

The default RPC port is `18899`; it and the following two ports must be free. To use another range or reuse a build:

```sh
TEST_RPC_PORT=19899 npm run test:integration
SKIP_BUILD=1 npm run test:integration
```

`npm test` always runs its build step. `SKIP_BUILD` applies to the integration runner.

| Command                      | Purpose                                                |
| ---------------------------- | ------------------------------------------------------ |
| `npm run build`              | Build the SBF program, IDL and generated client types. |
| `npm run test:unit`          | Run Rust unit tests with `Cargo.lock`.                 |
| `npm run test:integration`   | Build and run the local-validator suite.               |
| `npm run typecheck`          | Check TypeScript after generating the client types.    |
| `npm run lint`               | Check TypeScript and migration-script formatting.      |
| `cargo fmt --all -- --check` | Check Rust formatting.                                 |

See [scripts/test-local.sh](scripts/test-local.sh) for the runner and [tests/step-staking.ts](tests/step-staking.ts) for executable staking, reward, delegation and rejection examples. `migrations/deploy.js` initializes a vault for an already configured deployment; the test runner initializes its fixture directly and does not execute migrations.

The transitive `bigint-buffer` dependency has an upstream [native buffer-overflow advisory](https://github.com/advisories/GHSA-3gc7-fjrx-p6mg). The installation command above leaves the native binding unbuilt, and the tests use its pure JavaScript fallback. A native-binding warning is expected in that setup. Account for the advisory before enabling native install scripts.

## Use the TypeScript client

The examples below run from the repository root after a build. They assume a running deployment, the configured token mints and a funded provider wallet. The automated test validator stops when the test command finishes; it is not a persistent environment for these examples.

```sh
export ANCHOR_PROVIDER_URL=http://127.0.0.1:18899
export ANCHOR_WALLET=/path/to/your/development-wallet.json
```

### Connect

Use the generated IDL and types for the deployment you intend to call. This example uses the default mint constants:

```ts
import { readFileSync } from "node:fs";
import * as anchor from "@anchor-lang/core";
import {
  TOKEN_PROGRAM_ID,
  createAssociatedTokenAccountIdempotentInstruction,
  createTransferInstruction,
  getAccount,
  getAssociatedTokenAddressSync,
  getMint,
} from "@solana/spl-token";
import type { StepStaking } from "./target/types/step_staking";

const provider = anchor.AnchorProvider.env();
const idl = JSON.parse(
  readFileSync("target/idl/step_staking.json", "utf8"),
) as StepStaking;
const program = new anchor.Program<StepStaking>(idl, provider);
const tokenMint = new anchor.web3.PublicKey(
  "StepAscQoEioFxxWGnh2sLBDFp9d8rvKz2Yp39iDpyT",
);
const xTokenMint = new anchor.web3.PublicKey(
  "xStpgUCss9piqeFUk2iLVcvJEGhAdJxJQuwLkXP555G",
);
const [tokenVault, bump] = anchor.web3.PublicKey.findProgramAddressSync(
  [tokenMint.toBuffer()],
  program.programId,
);
const owner = provider.wallet.publicKey;
const tokenAccount = getAssociatedTokenAddressSync(tokenMint, owner);
const receiptAccount = getAssociatedTokenAddressSync(xTokenMint, owner);
```

### Initialize the vault and prepare accounts

Vault initialization runs once. Any wallet can pay for it, and repeat initialization fails. Both mints must already exist, and the receipt mint authority must be set to the derived vault before deposits can work.

```ts
async function initializeVault() {
  return program.methods
    .initialize(bump)
    .accountsStrict({
      tokenMint,
      tokenVault,
      initializer: owner,
      systemProgram: anchor.web3.SystemProgram.programId,
      tokenProgram: TOKEN_PROGRAM_ID,
      rent: anchor.web3.SYSVAR_RENT_PUBKEY,
    })
    .rpc();
}

async function prepareUserAccounts() {
  return provider.sendAndConfirm(
    new anchor.web3.Transaction().add(
      createAssociatedTokenAccountIdempotentInstruction(
        owner,
        tokenAccount,
        owner,
        tokenMint,
      ),
      createAssociatedTokenAccountIdempotentInstruction(
        owner,
        receiptAccount,
        owner,
        xTokenMint,
      ),
    ),
  );
}
```

Create the user's token accounts before trading and fund the underlying account with the token being deposited. Creating an associated token account does not fund it. For an existing deployment, skip vault initialization and use its established vault and mint authorities.

### Quote and deposit

Amounts use raw base units: one token of a nine-decimal mint is `1_000_000_000n`. Use `bigint` or `BN` throughout. The examples read current balances and apply a caller-selected tolerance in basis points; `100n` is 1%.

```ts
function minimumOutput(quotedOutput: bigint, slippageBps: bigint) {
  if (quotedOutput <= 0n) throw new Error("Trade would produce zero output");
  if (slippageBps < 0n || slippageBps >= 10_000n) {
    throw new Error("Slippage must be between 0 and 9,999 basis points");
  }
  // Round up so base-unit rounding does not weaken the selected tolerance.
  const minimum = (quotedOutput * (10_000n - slippageBps) + 9_999n) / 10_000n;
  return new anchor.BN(minimum.toString());
}

async function deposit(amount: bigint, slippageBps: bigint) {
  if (amount <= 0n) throw new Error("Deposit amount must be positive");
  const [vault, receipts] = await Promise.all([
    getAccount(provider.connection, tokenVault),
    getMint(provider.connection, xTokenMint),
  ]);
  if (receipts.supply > 0n && vault.amount === 0n) {
    throw new Error("Outstanding receipts have no backing collateral");
  }
  const quotedShares =
    receipts.supply === 0n ? amount : (amount * receipts.supply) / vault.amount;

  return program.methods
    .stake(
      bump,
      new anchor.BN(amount.toString()),
      minimumOutput(quotedShares, slippageBps),
    )
    .accountsStrict({
      tokenMint,
      xTokenMint,
      tokenVault,
      tokenFrom: tokenAccount,
      tokenFromAuthority: owner,
      xTokenTo: receiptAccount,
      tokenProgram: TOKEN_PROGRAM_ID,
    })
    .rpc();
}
```

For example, after calling `prepareUserAccounts()` and funding the token account, `deposit(1_000_000_000n, 100n)` deposits one nine-decimal token with a 1% tolerance from the fetched quote. If the output falls below the submitted minimum, the transaction fails without transferring tokens or minting receipts.

These reads are a quote, not a reservation of the exchange rate. Another transaction can change the pool before execution. Applications should show the quote and its minimum to the user and obtain a new accepted quote when retrying a rejected trade.

### Redeem receipts

Redemption burns receipt units and returns underlying tokens to the chosen account. This example uses the provider's own receipt and underlying accounts:

```ts
async function redeem(receiptAmount: bigint, slippageBps: bigint) {
  const [vault, receipts] = await Promise.all([
    getAccount(provider.connection, tokenVault),
    getMint(provider.connection, xTokenMint),
  ]);
  if (receiptAmount <= 0n || receiptAmount > receipts.supply) {
    throw new Error("Invalid receipt amount");
  }
  const quotedTokens = (receiptAmount * vault.amount) / receipts.supply;

  return program.methods
    .unstake(
      bump,
      new anchor.BN(receiptAmount.toString()),
      minimumOutput(quotedTokens, slippageBps),
    )
    .accountsStrict({
      tokenMint,
      xTokenMint,
      tokenVault,
      xTokenFrom: receiptAccount,
      xTokenFromAuthority: owner,
      tokenTo: tokenAccount,
      tokenProgram: TOKEN_PROGRAM_ID,
    })
    .rpc();
}
```

A holder needs enough receipts in the source account. For delegated trades, supply the approved delegate as `tokenFromAuthority` or `xTokenFromAuthority`, sign with that delegate, and stay within its allowance. Receipt and payout destinations can belong to another wallet. See the delegation tests for complete examples.

### Add rewards

Rewards are a direct SPL Token transfer to the vault. They do not mint receipts for the funder. This example transfers tokens from the provider's underlying account:

```ts
async function addRewards(amount: bigint) {
  return provider.sendAndConfirm(
    new anchor.web3.Transaction().add(
      createTransferInstruction(tokenAccount, tokenVault, owner, amount),
    ),
  );
}
```

Send rewards to `tokenVault`, the actual token account holding the pool. Sending them to the vault's associated token account requires a subsequent `withdrawNested` call before they count toward the redemption ratio.

## Instruction reference

Names below use the generated TypeScript client. Rust handlers and account constraints are in [lib.rs](programs/step-staking/src/lib.rs); quotes and authorization checks are in [math.rs](programs/step-staking/src/math.rs).

| Instruction                             | Caller                             | Function                                                                                                               |
| --------------------------------------- | ---------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `initialize(nonce)`                     | Any rent payer                     | Create the canonical vault. The legacy nonce argument is unused; Anchor derives the canonical bump.                    |
| `stake(nonce, amount, minimumShares)`   | Token owner or approved delegate   | Deposit underlying tokens and mint at least the requested receipt output.                                              |
| `unstake(nonce, amount, minimumTokens)` | Receipt owner or approved delegate | Burn receipts and transfer at least the requested underlying output.                                                   |
| `emitPrice()`                           | Anyone                             | Emit the current underlying-to-receipt ratio; clients can simulate the instruction.                                    |
| `withdrawNested()`                      | Anyone                             | Sweep the vault's underlying-token ATA into the vault, close the ATA and send its rent to the supplied system account. |
| `reclaimMintAuthority(nonce)`           | Underlying mint authority          | Move receipt mint authority from the vault PDA to the underlying mint authority.                                       |

Trade and reclaim nonces must match the vault bump. Owner and delegate authorities must sign. A System Program or incinerator-owned receipt account cannot be redeemed through this program using SPL Token's permissionless-burn exception.

`Price` and `PriceChange` events retain their original field layouts. The `*_e9` fields express the raw base-unit ratio scaled by one billion and saturate at `u64::MAX` for extreme ratios. Decimal strings are truncated to nine fractional places without that integer cap. Use vault balance and receipt supply for exact trade quotes, and account for each mint's decimals when displaying human-readable prices.

## Integration and compatibility notes

- **Use your own deployment identity.** The checked-in program ID and mint constants are retained for reference and local testing. Adapt the program ID, mint constants, IDL and client configuration for a deployment you control. The optional `test-id` and `local-testing` features retain historical alternate IDs; the automated suite uses the default build with fresh local account state.
- **Version 0.2 changes trade arguments.** `stake` and `unstake` now require the third, minimum-output argument. Old two-argument instruction data is rejected. Regenerate clients from the new IDL and review integrators before upgrading an existing installation.
- **Token support.** Accounts use the original SPL Token program. Token-2022 extensions are not supported. The fixed mint pair means this is one pool per program identity, rather than a factory for arbitrary pools.
- **Receipt mint authority is trusted.** The underlying mint authority can reclaim it, then mint unbacked receipts and dilute holders. Reclaim prevents new deposits until receipt mint authority is restored to the vault; existing receipts remain redeemable. A revoked underlying mint authority cannot call reclaim.
- **Review other authorities.** Mint freeze authorities, if configured, can affect transfers and burns. A program upgrade authority can change program behavior. Neither is provisioned by the initialization instruction.
- **External burns change the ratio.** Burning receipts outside `unstake` relinquishes their collateral to the pool. A zero-supply vault retains the first-depositor behavior described above.

The default historical addresses are:

| Account    | Address                                       |
| ---------- | --------------------------------------------- |
| Program    | `Stk5NCWomVN3itaFjLu382u9ibb5jMSHEsh6CuhaGjB` |
| STEP mint  | `StepAscQoEioFxxWGnh2sLBDFp9d8rvKz2Yp39iDpyT` |
| xSTEP mint | `xStpgUCss9piqeFUk2iLVcvJEGhAdJxJQuwLkXP555G` |

The implementation is provided for reference and adaptation. This renewal has not been independently audited or deployed on-chain, and it makes no claim about the code or authorities currently deployed at those historical addresses. Run the local checks and review deployment assumptions for your application. CI and GitHub Actions are not configured in this repository.

## License and attribution

The original **GPLv2** [LICENSE](LICENSE) is retained, together with the Step Finance and SushiBar lineage. This repository is based on the [original Step Finance source](https://github.com/step-finance/step-staking/tree/0ea372b6aab7e2b0c9d922de32ec02484fc0fea7), imported without its Git history, CI configuration or serialized test keys. Preserve applicable third-party notices when reusing or distributing the code.
