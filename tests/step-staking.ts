import assert from "node:assert/strict";
import * as anchor from "@anchor-lang/core";
import * as token from "@solana/spl-token";
import { Fixture, TOKEN_MINT, SHARE_MINT, VAULT, VAULT_BUMP } from "./utils";

const f = new Fixture();
async function rejectCode(promise: Promise<unknown>, code: string) {
  await assert.rejects(
    promise,
    (error: any) => error.error?.errorCode?.code === code
  );
}

describe("Step staking", () => {
  before(async () => {
    await f.setup();
  });
  afterEach(async () => {
    await f.cleanup();
  });

  it("initializes one self-authorized vault for the configured underlying mint", async () => {
    const vault = await token.getAccount(f.provider.connection, VAULT);
    assert.ok(vault.owner.equals(VAULT));
    assert.ok(vault.mint.equals(TOKEN_MINT));
    assert.equal(vault.amount, 0n);
  });

  it("mints one-for-one shares for an empty pool", async () => {
    const before = await f.balance(f.owner.tokens);
    await f.stake(5_000_000_000n);
    assert.equal(await f.balance(f.owner.tokens), before - 5_000_000_000n);
    assert.equal(await f.balance(f.owner.shares), 5_000_000_000n);
    assert.equal(await f.balance(VAULT), 5_000_000_000n);
  });

  it("distributes added rewards proportionally on redemption", async () => {
    const before = await f.balance(f.owner.tokens);
    await f.stake(5_000_000_000n);
    await f.donate(1_000_000_000n);
    await f.unstake(5_000_000_000n);
    assert.equal(await f.balance(f.owner.tokens), before);
    assert.equal(await f.balance(f.owner.shares), 0n);
    assert.equal(await f.balance(VAULT), 0n);
  });

  it("quotes the current redemption ratio without mutating balances", async () => {
    await f.stake(5_000_000_000n);
    await f.donate(1_000_000_000n);
    const result = await f.program.methods
      .emitPrice()
      .accountsStrict({
        tokenMint: TOKEN_MINT,
        xTokenMint: SHARE_MINT,
        tokenVault: VAULT,
      })
      .simulate();
    const price = result.events[0].data as any;
    assert.equal(price.stepPerXstepE9.toString(), "1200000000");
    assert.equal(price.stepPerXstep, "1.2");
    assert.equal(await f.balance(VAULT), 6_000_000_000n);
  });

  it("prices later deposits at the existing reward-adjusted ratio", async () => {
    await f.stake(1000n);
    await f.donate(1000n);
    await f.stake(1000n, 500n, {
      from: f.other.tokens,
      authority: f.other.signer,
      to: f.other.shares,
    });
    assert.equal(await f.balance(f.other.shares), 500n);
    const before = await f.balance(f.other.tokens);
    await f.unstake(500n, 1000n, {
      from: f.other.shares,
      authority: f.other.signer,
      to: f.other.tokens,
    });
    assert.equal(await f.balance(f.other.tokens), before + 1000n);
  });

  it("keeps the original prefilled-empty-pool reward behavior", async () => {
    await f.donate(5000n);
    const before = await f.balance(f.owner.tokens);
    await f.stake(5000n);
    assert.equal(await f.balance(f.owner.shares), 5000n);
    await f.unstake(5000n);
    assert.equal(await f.balance(f.owner.tokens), before + 5000n);
  });

  it("sweeps a nested associated token account into the vault and closes it", async () => {
    const nested = await token.createAssociatedTokenAccount(
      f.provider.connection,
      f.payer,
      TOKEN_MINT,
      VAULT,
      undefined,
      token.TOKEN_PROGRAM_ID,
      token.ASSOCIATED_TOKEN_PROGRAM_ID,
      true
    );
    await token.transfer(
      f.provider.connection,
      f.payer,
      f.owner.tokens,
      nested,
      f.payer,
      1000n
    );
    await f.program.methods
      .withdrawNested()
      .accountsStrict({
        refundee: f.payer.publicKey,
        tokenMint: TOKEN_MINT,
        tokenVault: VAULT,
        tokenVaultNestedAta: nested,
        tokenProgram: token.TOKEN_PROGRAM_ID,
        associatedTokenProgram: token.ASSOCIATED_TOKEN_PROGRAM_ID,
      })
      .rpc();
    assert.equal(await f.provider.connection.getAccountInfo(nested), null);
    assert.equal(await f.balance(VAULT), 1000n);
  });
  it("rejects zero deposits without changing receipt supply", async () => {
    await rejectCode(f.stake(0n), "InvalidAmount");
    assert.equal(await f.supply(), 0n);
  });

  it("rejects zero redemptions before burning or dividing", async () => {
    await rejectCode(f.unstake(0n), "InvalidAmount");
    assert.equal(await f.supply(), 0n);
  });

  it("rejects a zero minimum output", async () => {
    await rejectCode(f.stake(1n, 0n), "InvalidMinimumOutput");
  });

  it("rejects a donated-pool deposit that would mint zero shares", async () => {
    await f.stake(1n, 1n, {
      from: f.other.tokens,
      authority: f.other.signer,
      to: f.other.shares,
    });
    await f.donate(1000n);
    const balance = await f.balance(f.owner.tokens);
    const vault = await f.balance(VAULT);
    await rejectCode(f.stake(1000n, 1n), "ZeroOutput");
    assert.equal(await f.balance(f.owner.tokens), balance);
    assert.equal(await f.balance(f.owner.shares), 0n);
    assert.equal(await f.balance(VAULT), vault);
    assert.equal(await f.supply(), 1n);
  });

  it("enforces stake slippage even when the rounded output is positive", async () => {
    await f.stake(1n, 1n, {
      from: f.other.tokens,
      authority: f.other.signer,
      to: f.other.shares,
    });
    await f.donate(999n);
    const before = await f.balance(f.owner.tokens);
    await rejectCode(f.stake(1999n, 2n), "SlippageExceeded");
    assert.equal(await f.balance(f.owner.tokens), before);
    await f.stake(2000n, 2n);
    assert.equal(await f.balance(f.owner.shares), 2n);
  });

  it("enforces redemption slippage without consuming shares", async () => {
    await f.stake(1000n);
    await rejectCode(f.unstake(1n, 2n), "SlippageExceeded");
    assert.equal(await f.balance(f.owner.shares), 1000n);
    assert.equal(await f.balance(VAULT), 1000n);
  });

  it("supports approved deposit delegates and a third-party share recipient", async () => {
    await token.approve(
      f.provider.connection,
      f.payer,
      f.owner.tokens,
      f.other.signer.publicKey,
      f.owner.signer,
      1000n
    );
    await f.stake(1000n, 1000n, {
      authority: f.other.signer,
      to: f.recipient.shares,
    });
    assert.equal(await f.balance(f.recipient.shares), 1000n);
  });

  it("supports approved redemption delegates and a third-party payout recipient", async () => {
    await f.stake(1000n);
    await token.approve(
      f.provider.connection,
      f.payer,
      f.owner.shares,
      f.other.signer.publicKey,
      f.owner.signer,
      1000n
    );
    const before = await f.balance(f.recipient.tokens);
    await f.unstake(1000n, 1000n, {
      authority: f.other.signer,
      to: f.recipient.tokens,
    });
    assert.equal(await f.balance(f.recipient.tokens), before + 1000n);
  });

  it("rejects unrelated redemption signers", async () => {
    await f.stake(1000n);
    await rejectCode(
      f.unstake(1000n, 1n, { authority: f.other.signer, to: f.other.tokens }),
      "InvalidTokenAuthority"
    );
    assert.equal(await f.balance(f.owner.shares), 1000n);
  });

  it("rejects unrelated deposit signers without minting receipts", async () => {
    await token.revoke(
      f.provider.connection,
      f.payer,
      f.owner.tokens,
      f.owner.signer
    );
    const before = await f.balance(f.owner.tokens);
    await rejectCode(
      f.stake(1000n, 1000n, { authority: f.other.signer, to: f.other.shares }),
      "InvalidTokenAuthority"
    );
    assert.equal(await f.balance(f.owner.tokens), before);
    assert.equal(await f.balance(f.other.shares), 0n);
    assert.equal(await f.supply(), 0n);
  });

  it("rejects insufficient delegate allowances for both trade directions", async () => {
    await token.approve(
      f.provider.connection,
      f.payer,
      f.owner.tokens,
      f.other.signer.publicKey,
      f.owner.signer,
      99n
    );
    await rejectCode(
      f.stake(100n, 100n, { authority: f.other.signer }),
      "InsufficientDelegation"
    );
    await f.stake(100n, 100n);
    await token.approve(
      f.provider.connection,
      f.payer,
      f.owner.shares,
      f.other.signer.publicKey,
      f.owner.signer,
      99n
    );
    await rejectCode(
      f.unstake(100n, 100n, { authority: f.other.signer }),
      "InsufficientDelegation"
    );
    assert.equal(await f.balance(f.owner.shares), 100n);
    assert.equal(await f.balance(VAULT), 100n);
  });

  it("rejects invalid vault seeds and configured mints", async () => {
    await rejectCode(
      f.stake(100n, 100n, { nonce: (VAULT_BUMP + 1) % 256 }),
      "ConstraintSeeds"
    );
    await rejectCode(
      f.stake(100n, 100n, { tokenMint: SHARE_MINT }),
      "ConstraintAddress"
    );
    await rejectCode(
      f.stake(100n, 100n, { shareMint: TOKEN_MINT }),
      "ConstraintAddress"
    );
    assert.equal(await f.supply(), 0n);
    assert.equal(await f.balance(VAULT), 0n);
  });

  it("rolls back receipt issuance and burns when a token account has the wrong mint", async () => {
    const before = await f.balance(f.owner.tokens);
    await assert.rejects(f.stake(100n, 100n, { to: f.owner.tokens }));
    await assert.rejects(f.stake(100n, 100n, { from: f.owner.shares }));
    assert.equal(await f.balance(f.owner.tokens), before);
    assert.equal(await f.supply(), 0n);
    await f.stake(100n, 100n);
    await assert.rejects(f.unstake(100n, 100n, { to: f.owner.shares }));
    assert.equal(await f.balance(f.owner.shares), 100n);
    assert.equal(await f.balance(VAULT), 100n);
  });

  it("fails closed on legacy instruction data with no minimum output", async () => {
    const stake = await f.program.methods
      .stake(VAULT_BUMP, new anchor.BN(100), new anchor.BN(100))
      .accountsStrict({
        tokenMint: TOKEN_MINT,
        xTokenMint: SHARE_MINT,
        tokenVault: VAULT,
        tokenFrom: f.owner.tokens,
        tokenFromAuthority: f.owner.signer.publicKey,
        xTokenTo: f.owner.shares,
        tokenProgram: token.TOKEN_PROGRAM_ID,
      })
      .instruction();
    const unstake = await f.program.methods
      .unstake(VAULT_BUMP, new anchor.BN(100), new anchor.BN(100))
      .accountsStrict({
        tokenMint: TOKEN_MINT,
        xTokenMint: SHARE_MINT,
        tokenVault: VAULT,
        xTokenFrom: f.owner.shares,
        xTokenFromAuthority: f.owner.signer.publicKey,
        tokenTo: f.owner.tokens,
        tokenProgram: token.TOKEN_PROGRAM_ID,
      })
      .instruction();
    for (const instruction of [stake, unstake]) {
      instruction.data = instruction.data.subarray(0, -8);
      await assert.rejects(
        f.provider.sendAndConfirm(
          new anchor.web3.Transaction().add(instruction)
        ),
        /InstructionDidNotDeserialize/
      );
    }
    assert.equal(await f.supply(), 0n);
    assert.equal(await f.balance(VAULT), 0n);
  });

  it("rejects unrelated mint-authority reclaim signers", async () => {
    await rejectCode(f.reclaim(f.other.signer), "InvalidAuthority");
    assert.ok(
      (
        await token.getMint(f.provider.connection, SHARE_MINT)
      ).mintAuthority!.equals(VAULT)
    );
  });

  it("allows the underlying mint authority to reclaim minting while holders can still exit", async () => {
    await f.stake(1000n);
    await f.reclaim();
    try {
      assert.ok(
        (
          await token.getMint(f.provider.connection, SHARE_MINT)
        ).mintAuthority!.equals(f.payer.publicKey)
      );
      await f.unstake(1000n, 1000n);
      assert.equal(await f.balance(f.owner.shares), 0n);
    } finally {
      await token.setAuthority(
        f.provider.connection,
        f.payer,
        SHARE_MINT,
        f.payer,
        token.AuthorityType.MintTokens,
        VAULT
      );
    }
  });

  it("completes redemption even when its new price exceeds the legacy integer range", async () => {
    await f.stake(2n);
    await f.donate(36_893_488_145n);
    const before = await f.balance(f.owner.tokens);
    await f.unstake(1n, 18_446_744_073n);
    assert.equal(await f.balance(f.owner.tokens), before + 18_446_744_073n);
    assert.equal(await f.balance(VAULT), 18_446_744_074n);
  });

  it("keeps the price at extreme ratios readable and the vault redeemable", async () => {
    await f.stake(1n);
    await f.donate(18_446_744_073n);
    const result = await f.program.methods
      .emitPrice()
      .accountsStrict({
        tokenMint: TOKEN_MINT,
        xTokenMint: SHARE_MINT,
        tokenVault: VAULT,
      })
      .simulate();
    const price = result.events[0].data as any;
    assert.equal(price.stepPerXstepE9.toString(), "18446744073709551615");
    assert.equal(price.stepPerXstep, "18446744074");
    await f.unstake(1n, 18_446_744_074n);
    assert.equal(await f.balance(VAULT), 0n);
  });

  for (const [label, owner] of [
    ["system", anchor.web3.SystemProgram.programId],
    [
      "incinerator",
      new anchor.web3.PublicKey("1nc1nerator11111111111111111111111111111111"),
    ],
  ] as const) {
    it(`rejects unauthorized redemption from a ${label}-authority receipt account`, async () => {
      const lockedShares = await token.createAccount(
        f.provider.connection,
        f.payer,
        SHARE_MINT,
        owner,
        anchor.web3.Keypair.generate()
      );
      await f.stake(100n, 100n, { to: lockedShares });
      const before = await f.balance(f.other.tokens);
      await rejectCode(
        f.unstake(100n, 1n, {
          from: lockedShares,
          authority: f.other.signer,
          to: f.other.tokens,
        }),
        "InvalidTokenAuthority"
      );
      assert.equal(await f.balance(lockedShares), 100n);
      assert.equal(await f.balance(f.other.tokens), before);
    });
  }

  it("reports a revoked underlying mint authority without panicking", async () => {
    await token.setAuthority(
      f.provider.connection,
      f.payer,
      TOKEN_MINT,
      f.payer,
      token.AuthorityType.MintTokens,
      null
    );
    await rejectCode(f.reclaim(), "MissingMintAuthority");
  });
});
