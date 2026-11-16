# bitcoin-lottery

Solo Bitcoin "lottery" miner in Rust: Stratum v1, multi-threaded CPU, SHA-NI midstate hashing.

```
cargo run --release -- --address <YOUR_BTC_ADDRESS> --worker lotto
cargo test --release
```

## Docker

```
cp .env.example .env      # then set BTC_ADDRESS
docker compose up -d --build
docker compose logs -f
```

Settings (env vars, or the matching `--flags`): `BTC_ADDRESS` (required), `POOL`, `WORKER`, `THREADS`,
`STATS_INTERVAL`, plus `CPU_LIMIT` for compose. `.env` is git-ignored.

Defaults to `stratum.ckpool.org:3333`. Your stats: https://raw.stats.ckpool.org/users/<your address>. If a block is found, the whole reward goes to your address.
Only a public receiving address is needed. Never put a private key or seed phrase anywhere in this project.

The odds of finding a block are astronomically small. Treat it as an educational lottery ticket.
