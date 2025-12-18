import * as LitJsSdk from "@lit-protocol/lit-node-client-nodejs";
import { Connection, Keypair, PublicKey } from "@solana/web3.js";
import { Program, AnchorProvider, Wallet } from "@coral-xyz/anchor";
import { sha256 } from "js-sha256";
import { v4 as uuidv4 } from 'uuid';
import * as fs from 'fs';
import IDL from "../../target/idl/truth_pool.json";

const PROGRAM_ID = new PublicKey("TrutHPooL11111111111111111111111111111111");
const RPC_URL = "https://api.devnet.solana.com";
const WALLET_PATH = process.env.WALLET_PATH || "miner_id.json";

const CONFIG = {
    maxGasLamports: 5000,
    categories: ['SPORTS', 'CRYPTO'],
    apis: ['ESPN', 'YAHOO'] 
};

function normalizeData(rawData: any, format: any): string {
    const formatStr = Object.keys(format)[0]; 
    if (formatStr === 'binary') {
        const s = String(rawData).toUpperCase().trim();
        if (["TRUE", "1", "YES", "Y"].includes(s)) return "YES";
        return "NO";
    }
    if (formatStr === 'optionIndex') {
        const idx = parseInt(String(rawData));
        if (isNaN(idx)) throw new Error("Invalid Index");
        return idx.toString();
    }
    if (formatStr === 'decimal') {
        return Math.round(parseFloat(rawData) * 100).toString();
    }
    return String(rawData).trim();
}

async function main() {
    const connection = new Connection(RPC_URL);
    // In prod, create keypair if not exists
    if (!fs.existsSync(WALLET_PATH)) {
        fs.writeFileSync(WALLET_PATH, JSON.stringify(Array.from(Keypair.generate().secretKey)));
    }
    const keypair = Keypair.fromSecretKey(new Uint8Array(JSON.parse(fs.readFileSync(WALLET_PATH, 'utf-8'))));
    const provider = new AnchorProvider(connection, new Wallet(keypair), {});
    const program = new Program(IDL as any, PROGRAM_ID, provider);

    const litNodeClient = new LitJsSdk.LitNodeClientNodeJs({
        litNetwork: "cayenne",
        alertWhenUnauthorized: false
    });
    await litNodeClient.connect();

    console.log("🤖 Miner Active. Address:", keypair.publicKey.toBase58());

    while (true) {
        try {
            const fees = await connection.getRecentPrioritizationFees();
            const currentGas = fees.length ? fees[fees.length - 1].prioritizationFee : 0;
            
            if (currentGas > CONFIG.maxGasLamports) {
                console.warn(`Gas Spike (${currentGas}). Sleeping...`);
            } else {
                await runCycle(program, litNodeClient, keypair);
            }
        } catch (e) {
            console.error("Cycle Error:", e);
        }
        await new Promise(r => setTimeout(r, 60000));
    }
}

async function runCycle(program: Program, lit: any, keypair: Keypair) {
    const commitJobs = await program.account.queryAccount.all(); 

    for (const job of commitJobs) {
        const data = job.account;
        
        if (!CONFIG.categories.includes(data.categoryId)) continue;

        if (JSON.stringify(data.status) === JSON.stringify({ commitPhase: {} })) {
            console.log(`Processing Commit: ${data.uniqueEventId}`);
            // Mock Fetch
            const rawAnswer = "Lakers 110-105"; 
            
            let finalAnswer;
            try {
                finalAnswer = normalizeData(rawAnswer, data.format);
            } catch (e) {
                console.error(`Skipping ${data.uniqueEventId}: Format Error`);
                continue;
            }

            const salt = uuidv4();
            const voteHash = sha256.digest(finalAnswer + salt);

            // Lit Encryption
            const authSig = await LitJsSdk.checkAndSignAuthSig({ chain: "solana", nonce: await lit.getLatestBlockhash() });
            const { ciphertext } = await LitJsSdk.encryptString({
                accessControlConditions: [{
                    method: "solRpc", params: [":userAddress"], pdaParams: [], pdaInterface: { offset: 0, fields: {} }, pdaKey: "", chain: "solana", returnValueTest: { key: "", comparator: "=", value: keypair.publicKey.toBase58() }
                }],
                authSig, chain: 'solana', dataToEncrypt: salt,
            }, lit);

            await program.methods.commitVote(Array.from(voteHash), Buffer.from(ciphertext))
                .accounts({
                    voter: keypair.publicKey,
                    queryAccount: job.publicKey,
                    minerProfile: (await PublicKey.findProgramAddress([Buffer.from("miner"), keypair.publicKey.toBuffer()], PROGRAM_ID))[0]
                })
                .rpc();
            
            console.log("Vote Committed.");
        }
    }
}

main();
