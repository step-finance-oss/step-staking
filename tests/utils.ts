import * as anchor from "@anchor-lang/core";
import * as token from "@solana/spl-token";
import { readFileSync } from "node:fs";
import type { StepStaking } from "../target/types/step_staking";

export const PROGRAM_ID = new anchor.web3.PublicKey(
  "Stk5NCWomVN3itaFjLu382u9ibb5jMSHEsh6CuhaGjB"
);
export const TOKEN_MINT = new anchor.web3.PublicKey(
  "StepAscQoEioFxxWGnh2sLBDFp9d8rvKz2Yp39iDpyT"
);
export const SHARE_MINT = new anchor.web3.PublicKey(
  "xStpgUCss9piqeFUk2iLVcvJEGhAdJxJQuwLkXP555G"
);
export const [VAULT, VAULT_BUMP] = anchor.web3.PublicKey.findProgramAddressSync(
  [TOKEN_MINT.toBuffer()],
  PROGRAM_ID
);
export type Actor = {
  signer: anchor.web3.Keypair;
  tokens: anchor.web3.PublicKey;
  shares: anchor.web3.PublicKey;
};
type TradeOptions = {
  from?: anchor.web3.PublicKey;
  authority?: anchor.web3.Keypair;
  to?: anchor.web3.PublicKey;
  nonce?: number;
  vault?: anchor.web3.PublicKey;
  tokenMint?: anchor.web3.PublicKey;
  shareMint?: anchor.web3.PublicKey;
};

export class Fixture {
  provider = anchor.AnchorProvider.env();
  payer = (this.provider.wallet as anchor.Wallet).payer;
  program = new anchor.Program<StepStaking>(
    JSON.parse(
      readFileSync("target/idl/step_staking.json", "utf8")
    ) as StepStaking,
    this.provider
  );
  actors: Actor[] = [];
  owner!: Actor;
  other!: Actor;
  recipient!: Actor;

  async actor(signer = anchor.web3.Keypair.generate()): Promise<Actor> {
    if (!signer.publicKey.equals(this.payer.publicKey)) {
      await this.provider.sendAndConfirm(
        new anchor.web3.Transaction().add(
          anchor.web3.SystemProgram.transfer({
            fromPubkey: this.payer.publicKey,
            toPubkey: signer.publicKey,
            lamports: 1_000_000_000,
          })
        )
      );
    }
    const tokens = await token.createAssociatedTokenAccount(
      this.provider.connection,
      this.payer,
      TOKEN_MINT,
      signer.publicKey
    );
    const shares = await token.createAssociatedTokenAccount(
      this.provider.connection,
      this.payer,
      SHARE_MINT,
      signer.publicKey
    );
    const actor = { signer, tokens, shares };
    this.actors.push(actor);
    await token.mintTo(
      this.provider.connection,
      this.payer,
      TOKEN_MINT,
      tokens,
      this.payer,
      1_000_000_000_000n
    );
    return actor;
  }

  async setup() {
    this.owner = await this.actor(this.payer);
    this.other = await this.actor();
    this.recipient = await this.actor();
    await this.program.methods
      .initialize(VAULT_BUMP)
      .accountsStrict({
        tokenMint: TOKEN_MINT,
        tokenVault: VAULT,
        initializer: this.payer.publicKey,
        systemProgram: anchor.web3.SystemProgram.programId,
        tokenProgram: token.TOKEN_PROGRAM_ID,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .rpc();
  }

  stake(amount: bigint, minimum = 1n, options: TradeOptions = {}) {
    const authority = options.authority ?? this.owner.signer;
    return this.program.methods
      .stake(
        options.nonce ?? VAULT_BUMP,
        new anchor.BN(amount.toString()),
        new anchor.BN(minimum.toString())
      )
      .accountsStrict({
        tokenMint: options.tokenMint ?? TOKEN_MINT,
        xTokenMint: options.shareMint ?? SHARE_MINT,
        tokenFrom: options.from ?? this.owner.tokens,
        tokenFromAuthority: authority.publicKey,
        tokenVault: options.vault ?? VAULT,
        xTokenTo: options.to ?? this.owner.shares,
        tokenProgram: token.TOKEN_PROGRAM_ID,
      })
      .signers([authority])
      .rpc();
  }

  unstake(amount: bigint, minimum = 1n, options: TradeOptions = {}) {
    const authority = options.authority ?? this.owner.signer;
    return this.program.methods
      .unstake(
        options.nonce ?? VAULT_BUMP,
        new anchor.BN(amount.toString()),
        new anchor.BN(minimum.toString())
      )
      .accountsStrict({
        tokenMint: options.tokenMint ?? TOKEN_MINT,
        xTokenMint: options.shareMint ?? SHARE_MINT,
        xTokenFrom: options.from ?? this.owner.shares,
        xTokenFromAuthority: authority.publicKey,
        tokenVault: options.vault ?? VAULT,
        tokenTo: options.to ?? this.owner.tokens,
        tokenProgram: token.TOKEN_PROGRAM_ID,
      })
      .signers([authority])
      .rpc();
  }

  async balance(address: anchor.web3.PublicKey) {
    return (await token.getAccount(this.provider.connection, address)).amount;
  }
  async supply() {
    return (await token.getMint(this.provider.connection, SHARE_MINT)).supply;
  }
  async donate(amount: bigint) {
    await token.transfer(
      this.provider.connection,
      this.payer,
      this.owner.tokens,
      VAULT,
      this.payer,
      amount
    );
  }
  async cleanup() {
    for (const actor of this.actors) {
      const amount = await this.balance(actor.shares);
      if (amount)
        await this.unstake(amount, 1n, {
          from: actor.shares,
          authority: actor.signer,
          to: actor.tokens,
        });
    }
    if ((await this.supply()) === 0n && (await this.balance(VAULT)) > 0n) {
      await this.stake(1n);
      await this.unstake(1n);
    }
  }
  reclaim(authority = this.payer) {
    return this.program.methods
      .reclaimMintAuthority(VAULT_BUMP)
      .accountsStrict({
        tokenMint: TOKEN_MINT,
        xTokenMint: SHARE_MINT,
        tokenVault: VAULT,
        authority: authority.publicKey,
        tokenProgram: token.TOKEN_PROGRAM_ID,
      })
      .signers([authority])
      .rpc();
  }
}
