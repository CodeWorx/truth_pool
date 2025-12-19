import { Connection, Keypair, PublicKey } from "@solana/web3.js";
import { Program, AnchorProvider, Wallet, BN } from "@coral-xyz/anchor";
import * as fs from "fs";
import IDL from "../../target/idl/truth_pool.json";

// ============================================
// CONFIGURATION
// ============================================

const WALLET_PATH = process.env.WALLET_PATH || "pulse_admin.json";
const PROGRAM_ID = new PublicKey("TrutHPooL11111111111111111111111111111111");
const RPC_URL = process.env.RPC_URL || "https://api.devnet.solana.com";

const DEFAULT_BOUNTY = 50_000_000; // 0.05 SOL

// ============================================
// TYPES
// ============================================

interface MarketConfig {
  id: string;
  category: string;
  bounty?: number;
  format?: number; // 0=Binary, 1=Score, 2=Decimal, 3=String, 4=OptionIndex
}

// ============================================
// UTILITIES
// ============================================

function loadOrCreateWallet(path: string): Keypair {
  if (!fs.existsSync(path)) {
    console.log("Generating Pulse Wallet...");
    const keypair = Keypair.generate();
    fs.writeFileSync(path, JSON.stringify(Array.from(keypair.secretKey)));
    return keypair;
  }
  return Keypair.fromSecretKey(
    new Uint8Array(JSON.parse(fs.readFileSync(path, "utf-8")))
  );
}

async function getQueryPDA(
  programId: PublicKey,
  categoryId: string,
  eventId: string
): Promise<[PublicKey, number]> {
  return PublicKey.findProgramAddressSync(
    [
      Buffer.from("query"),
      Buffer.from(categoryId),
      Buffer.from(eventId),
    ],
    programId
  );
}

async function getCategoryPDA(
  programId: PublicKey,
  categoryId: string
): Promise<[PublicKey, number]> {
  return PublicKey.findProgramAddressSync(
    [Buffer.from("category"), Buffer.from(categoryId)],
    programId
  );
}

async function getVoteStatsPDA(
  programId: PublicKey,
  queryPubkey: PublicKey
): Promise<[PublicKey, number]> {
  return PublicKey.findProgramAddressSync(
    [Buffer.from("stats"), queryPubkey.toBuffer()],
    programId
  );
}

/**
 * Check if a market already exists
 */
async function marketExists(
  program: Program,
  categoryId: string,
  eventId: string
): Promise<boolean> {
  try {
    const [queryPDA] = await getQueryPDA(program.programId, categoryId, eventId);
    const account = await program.account.queryAccount.fetchNullable(queryPDA);
    return account !== null;
  } catch {
    return false;
  }
}

/**
 * Fetch today's events from external APIs
 * In production, implement real API calls
 */
async function fetchTodaysEvents(): Promise<MarketConfig[]> {
  const today = new Date().toISOString().split("T")[0];

  // Mock implementation - replace with real API calls
  // Example: fetch from ESPN, TheOddsAPI, CoinGecko, etc.

  return [
    {
      id: `NBA-${today}-LAL-GSW`,
      category: "SPORTS",
      format: 0, // Binary: Did LAL win?
    },
    {
      id: `NBA-${today}-BOS-NYK`,
      category: "SPORTS",
      format: 0,
    },
    {
      id: `BTC-PRICE-${today}-100K`,
      category: "CRYPTO",
      format: 0, // Binary: Is BTC > 100k?
    },
    {
      id: `ETH-PRICE-${today}`,
      category: "CRYPTO",
      format: 2, // Decimal: ETH price
    },
  ];
}

// ============================================
// HELPERS
// ============================================

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Execute a transaction with exponential backoff retry
 * Retries up to 4 times with delays: 2s, 4s, 8s, 16s
 */
async function withRetry<T>(
  operation: () => Promise<T>,
  operationName: string
): Promise<T> {
  const MAX_RETRIES = 4;
  const BASE_DELAY_MS = 2000;

  let lastError: Error | null = null;

  for (let attempt = 0; attempt <= MAX_RETRIES; attempt++) {
    try {
      return await operation();
    } catch (e: any) {
      lastError = e;

      // Don't retry on non-recoverable errors
      const nonRecoverable = [
        "already in use",
        "Unauthorized",
        "CategoryIdTooLong",
        "EventIdTooLong",
      ];

      if (nonRecoverable.some((code) => e.message?.includes(code))) {
        throw e;
      }

      if (attempt < MAX_RETRIES) {
        const delay = BASE_DELAY_MS * Math.pow(2, attempt);
        console.log(`  ${operationName} failed (attempt ${attempt + 1}/${MAX_RETRIES + 1}). Retrying in ${delay}ms...`);
        await sleep(delay);
      }
    }
  }

  throw lastError;
}

// ============================================
// MAIN
// ============================================

async function main() {
  const connection = new Connection(RPC_URL, "confirmed");
  const keypair = loadOrCreateWallet(WALLET_PATH);
  const provider = new AnchorProvider(connection, new Wallet(keypair), {
    commitment: "confirmed",
  });
  const program = new Program(IDL as any, PROGRAM_ID, provider);

  console.log("Pulse AMM Starting...");
  console.log(`   Wallet: ${keypair.publicKey.toBase58()}`);
  console.log("");

  // Check wallet balance
  const balance = await connection.getBalance(keypair.publicKey);
  console.log(`   Balance: ${balance / 1e9} SOL`);
  if (balance < 0.1 * 1e9) {
    console.warn("Low balance! Fund wallet to create markets.");
  }
  console.log("");

  // Fetch today's events
  const events = await fetchTodaysEvents();
  console.log(`Found ${events.length} events for today`);
  console.log("");

  // Create markets
  for (const event of events) {
    console.log(`Processing: ${event.id}`);

    try {
      // Check if market already exists
      const exists = await marketExists(program, event.category, event.id);
      if (exists) {
        console.log(`  Market already exists`);
        continue;
      }

      // Derive PDAs
      const [categoryStats] = await getCategoryPDA(program.programId, event.category);
      const [queryAccount] = await getQueryPDA(program.programId, event.category, event.id);
      const [voteStats] = await getVoteStatsPDA(program.programId, queryAccount);

      // Check if category exists
      const categoryAccount = await program.account.categoryStats.fetchNullable(categoryStats);
      if (!categoryAccount) {
        console.log(`  Category "${event.category}" not initialized. Skipping.`);
        console.log(`     Admin must run: initialize_category("${event.category}")`);
        continue;
      }

      // Create market with retry
      const bounty = event.bounty || DEFAULT_BOUNTY;
      const format = event.format ?? 0;

      await withRetry(
        () =>
          program.methods
            .requestData(event.id, event.category, new BN(bounty), format)
            .accounts({
              requester: keypair.publicKey,
              categoryStats: categoryStats,
              queryAccount: queryAccount,
              voteStats: voteStats,
              systemProgram: PublicKey.default,
            })
            .rpc(),
        "CreateMarket"
      );

      console.log(`  Created! Bounty: ${bounty / 1e9} SOL`);
    } catch (e: any) {
      if (e.message?.includes("already in use")) {
        console.log(`  Market already exists`);
      } else {
        console.error(`  Error: ${e.message}`);
      }
    }
  }

  console.log("");
  console.log("Pulse AMM Complete");
}

// ============================================
// ENTRY POINT
// ============================================

main().catch(console.error);
