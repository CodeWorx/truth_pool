#!/bin/bash

# TruthPool Config & AMM Installer
# Restores missing TOML configs and the Pulse AMM bot
# Usage: chmod +x finalize_config.sh && ./finalize_config.sh

echo "🔧 Restoring missing configurations..."

cd truth-pool

# ==========================================
# 1. RUST CONFIG (Cargo.toml)
# ==========================================
echo "Writing programs/truth-pool/Cargo.toml..."
cat << 'EOF' > programs/truth-pool/Cargo.toml
[package]
name = "truth-pool"
version = "0.1.0"
description = "Created with Anchor"
edition = "2021"

[lib]
crate-type = ["cdylib", "lib"]
name = "truth_pool"

[features]
no-entrypoint = []
no-idl = []
no-log-ix-name = []
cpi = ["no-entrypoint"]
default = []

[dependencies]
anchor-lang = "0.30.1"
EOF

# ==========================================
# 2. ANCHOR CONFIG (Anchor.toml)
# ==========================================
echo "Writing Anchor.toml..."
cat << 'EOF' > Anchor.toml
[toolchain]

[features]
seeds = false
skip-lint = false

[programs.localnet]
truth_pool = "TrutHPooL11111111111111111111111111111111"

[programs.devnet]
truth_pool = "TrutHPooL11111111111111111111111111111111"

[registry]
url = "https://api.apr.dev"

[provider]
cluster = "devnet"
wallet = "~/.config/solana/id.json"

[scripts]
test = "yarn run ts-mocha -p ./tsconfig.json -t 1000000 tests/**/*.ts"
EOF

# ==========================================
# 3. PULSE AMM BOT (Restored)
# ==========================================
echo "Writing Pulse AMM Bot..."
mkdir -p bots/pulse-amm

cat << 'EOF' > bots/pulse-amm/index.ts
import { Connection, Keypair, PublicKey } from "@solana/web3.js";
import { Program, AnchorProvider, Wallet, BN } from "@coral-xyz/anchor";
import * as fs from 'fs';
import IDL from "../../target/idl/truth_pool.json";

// The "House" Wallet
const WALLET_PATH = process.env.WALLET_PATH || "pulse_admin.json";
const PROGRAM_ID = new PublicKey("TrutHPooL11111111111111111111111111111111");
const RPC_URL = process.env.RPC_URL || "https://api.devnet.solana.com";

async function main() {
    const connection = new Connection(RPC_URL, 'confirmed');
    
    if (!fs.existsSync(WALLET_PATH)) {
        console.log("Generating Pulse Wallet...");
        fs.writeFileSync(WALLET_PATH, JSON.stringify(Array.from(Keypair.generate().secretKey)));
    }
    
    const keypair = Keypair.fromSecretKey(
        new Uint8Array(JSON.parse(fs.readFileSync(WALLET_PATH, 'utf-8')))
    );
    const provider = new AnchorProvider(connection, new Wallet(keypair), {});
    const program = new Program(IDL as any, PROGRAM_ID, provider);

    console.log("❤️ Pulse AMM Waking Up...");

    // 1. Fetch Today's Games (Mock)
    const games = [
        { id: "NBA-2025-10-25-LAL-GSW", category: "SPORTS" },
        { id: "NBA-2025-10-25-BOS-NYK", category: "SPORTS" },
        { id: "BTC-PRICE-2025-10-25", category: "CRYPTO" }
    ];

    // 2. Create Markets
    for (const game of games) {
        console.log(`Creating Market: ${game.id}`);
        try {
            // Check if market exists logic omitted (Anchor init_if_needed handles it mostly)
            await program.methods.requestData(
                game.id,
                game.category,
                new BN(50000000), // 0.05 SOL Bounty
                0 // Format: Binary/Default
            )
            .accounts({
                requester: keypair.publicKey,
                categoryStats: (await PublicKey.findProgramAddress([Buffer.from("category"), Buffer.from(game.category)], PROGRAM_ID))[0],
            })
            .rpc();
            console.log("✅ Success");
        } catch (e) {
            console.log(`⚠️ Market likely exists or error:`, e);
        }
    }
}

main();
EOF

cat << 'EOF' > bots/pulse-amm/package.json
{
  "name": "truthpool-pulse",
  "version": "1.0.0",
  "main": "index.ts",
  "scripts": {
    "start": "ts-node index.ts"
  },
  "dependencies": {
    "@coral-xyz/anchor": "^0.30.1",
    "@solana/web3.js": "^1.91.0",
    "ts-node": "^10.9.2",
    "typescript": "^5.3.3"
  }
}
EOF

echo "✅ Configuration Finalized."