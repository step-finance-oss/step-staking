import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { Keypair, PublicKey } from "@solana/web3.js";
import { MintLayout, TOKEN_PROGRAM_ID } from "@solana/spl-token";
import { TOKEN_MINT, SHARE_MINT, VAULT } from "./utils";

const directory = process.argv[2];
if (!directory || !process.env.ANCHOR_WALLET)
  throw new Error("A temporary fixture directory and wallet are required");
const wallet = Keypair.fromSecretKey(
  Uint8Array.from(JSON.parse(readFileSync(process.env.ANCHOR_WALLET, "utf8")))
);
mkdirSync(directory, { recursive: true });
for (const [name, address, authority] of [
  ["underlying", TOKEN_MINT, wallet.publicKey],
  ["shares", SHARE_MINT, VAULT],
] as const) {
  const data = Buffer.alloc(MintLayout.span);
  MintLayout.encode(
    {
      mintAuthorityOption: 1,
      mintAuthority: authority,
      supply: 0n,
      decimals: 9,
      isInitialized: true,
      freezeAuthorityOption: 0,
      freezeAuthority: PublicKey.default,
    },
    data
  );
  writeFileSync(
    join(directory, `${name}.json`),
    JSON.stringify({
      pubkey: address.toBase58(),
      account: {
        lamports: 10_000_000,
        data: [data.toString("base64"), "base64"],
        owner: TOKEN_PROGRAM_ID.toBase58(),
        executable: false,
        rentEpoch: 0,
      },
    })
  );
}
