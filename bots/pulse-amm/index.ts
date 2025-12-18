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
