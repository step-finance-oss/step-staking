const anchor = require("@anchor-lang/core");
const { TOKEN_PROGRAM_ID } = require("@solana/spl-token");

// Creates only the configured vault; it does not create mints or set authorities.
// Review the program ID, mint constants and provider before using a deployment.
module.exports = async function (provider) {
  anchor.setProvider(provider);
  const program = anchor.workspace.StepStaking;
  const tokenMint = new anchor.web3.PublicKey(
    "StepAscQoEioFxxWGnh2sLBDFp9d8rvKz2Yp39iDpyT"
  );
  const [tokenVault, bump] = anchor.web3.PublicKey.findProgramAddressSync(
    [tokenMint.toBuffer()],
    program.programId
  );
  await program.methods
    .initialize(bump)
    .accountsStrict({
      tokenMint,
      tokenVault,
      initializer: provider.wallet.publicKey,
      systemProgram: anchor.web3.SystemProgram.programId,
      tokenProgram: TOKEN_PROGRAM_ID,
      rent: anchor.web3.SYSVAR_RENT_PUBKEY,
    })
    .rpc();
};
