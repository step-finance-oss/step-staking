#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "${SKIP_BUILD:-0}" != 1 ]]; then
  npm run build
fi
umask 077
run_dir=$(mktemp -d "${TMPDIR:-/tmp}/step-staking-test.XXXXXX")
validator_pid=
cleanup() {
  if [[ -n "$validator_pid" ]]; then
    kill "$validator_pid" 2>/dev/null || true
    wait "$validator_pid" 2>/dev/null || true
  fi
  rm -rf "$run_dir"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
solana-keygen new --no-bip39-passphrase --silent --outfile "$run_dir/wallet.json" >/dev/null
wallet_pubkey=$(solana-keygen pubkey "$run_dir/wallet.json")
rpc_port=${TEST_RPC_PORT:-18899}
export ANCHOR_PROVIDER_URL="http://127.0.0.1:$rpc_port"
export ANCHOR_WALLET="$run_dir/wallet.json"
node - "$rpc_port" <<'JS'
const net = require("node:net");
const port = Number(process.argv[2]);
if (!Number.isInteger(port) || port < 1024 || port > 65533) throw new Error("Invalid TEST_RPC_PORT");
Promise.all([port, port + 1, port + 2].map(p => new Promise((resolve, reject) => {
  const server = net.createServer();
  server.once("error", () => reject(new Error(`Local test port ${p} is already in use`)));
  server.listen(p, "127.0.0.1", () => server.close(resolve));
}))).catch(error => { console.error(error.message); process.exitCode = 1; });
JS
npm run compile:tests
node .test-build/tests/genesis.js "$run_dir"
solana-test-validator --reset --quiet --bind-address 127.0.0.1 \
  --rpc-port "$rpc_port" --faucet-port "$((rpc_port + 2))" \
  --ledger "$run_dir/ledger" --mint "$wallet_pubkey" \
  --account StepAscQoEioFxxWGnh2sLBDFp9d8rvKz2Yp39iDpyT "$run_dir/underlying.json" \
  --account xStpgUCss9piqeFUk2iLVcvJEGhAdJxJQuwLkXP555G "$run_dir/shares.json" \
  --upgradeable-program Stk5NCWomVN3itaFjLu382u9ibb5jMSHEsh6CuhaGjB target/deploy/step_staking.so "$wallet_pubkey" \
  >"$run_dir/validator.log" 2>&1 &
validator_pid=$!
ready=0
for ((i=0; i<120; i++)); do
  if ! kill -0 "$validator_pid" 2>/dev/null; then
    cat "$run_dir/validator.log"
    exit 1
  fi
  if curl --silent --fail --max-time 1 -H 'Content-Type: application/json' \
    --data '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' "$ANCHOR_PROVIDER_URL" | grep -q '"ok"'; then
    ready=1
    break
  fi
  sleep 0.5
done
if [[ "$ready" != 1 ]]; then
  tail -n 50 "$run_dir/validator.log"
  exit 1
fi
npm run test:client -- "$@"
