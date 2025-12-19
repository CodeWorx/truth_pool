import { Connection, Keypair, PublicKey, Transaction } from "@solana/web3.js";
import { Program, AnchorProvider, Wallet, BN } from "@coral-xyz/anchor";
import { keccak_256 } from "@noble/hashes/sha3";
import { v4 as uuidv4 } from "uuid";
import * as fs from "fs";
import IDL from "../../target/idl/truth_pool.json";

// ============================================
// CONFIGURATION
// ============================================

const PROGRAM_ID = new PublicKey("TrutHPooL11111111111111111111111111111111");
const RPC_URL = process.env.RPC_URL || "https://api.devnet.solana.com";
const WALLET_PATH = process.env.WALLET_PATH || "miner_id.json";
const SALT_CACHE_PATH = process.env.SALT_CACHE_PATH || "salt_cache.json";

const CONFIG = {
  maxGasLamports: 5000,
  categories: ["SPORTS", "CRYPTO"],
  pollIntervalMs: 30000, // 30 seconds
  revealBufferSeconds: 60, // Reveal 60s before deadline
};

// ============================================
// TYPES
// ============================================

/**
 * SECURITY CONSIDERATIONS for Salt Cache:
 *
 * The salt cache is stored in plaintext on disk. This is acceptable for development
 * but has security implications for production:
 *
 * 1. RISK: If an attacker gains read access to the cache file during the commit phase,
 *    they could see your vote before reveal and potentially front-run or manipulate.
 *
 * 2. MITIGATIONS for production:
 *    - Set restrictive file permissions (chmod 600 salt_cache.json)
 *    - Use encrypted storage (e.g., Keyring, encrypted filesystem)
 *    - Store in memory only (lose on crash, but more secure)
 *    - Use HSM or secure enclave for high-value operations
 *
 * 3. The commit-reveal scheme ensures votes cannot be changed after commitment,
 *    but pre-reveal privacy depends on salt secrecy.
 */
interface SaltCache {
  [queryKey: string]: {
    salt: string;
    answer: string;
    committedAt: number;
  };
}

interface QueryData {
  uniqueEventId: string;
  categoryId: string;
  status: { commitPhase?: {}; revealPhase?: {} };
  format: { binary?: {}; score?: {}; decimal?: {}; string?: {}; optionIndex?: {} };
  commitDeadline: BN;
  revealDeadline: BN;
}

// ============================================
// UTILITIES
// ============================================

/**
 * Normalize raw data into expected format
 * MUST match on-chain format expectations
 */
function normalizeData(rawData: any, format: any): string {
  const formatKey = Object.keys(format)[0];

  switch (formatKey) {
    case "binary": {
      const s = String(rawData).toUpperCase().trim();
      if (["TRUE", "1", "YES", "Y"].includes(s)) return "YES";
      return "NO";
    }
    case "optionIndex": {
      const idx = parseInt(String(rawData));
      if (isNaN(idx)) throw new Error("Invalid Index");
      return idx.toString();
    }
    case "decimal": {
      // Store as integer cents (e.g., $123.45 -> "12345")
      return Math.round(parseFloat(rawData) * 100).toString();
    }
    case "score": {
      return Math.round(parseFloat(rawData)).toString();
    }
    default:
      return String(rawData).trim();
  }
}

/**
 * Compute vote hash using keccak256
 * MUST match on-chain: keccak256(value_bytes || salt_bytes)
 */
function computeVoteHash(value: string, salt: string): Uint8Array {
  const preimage = Buffer.concat([
    Buffer.from(value, "utf-8"),
    Buffer.from(salt, "utf-8"),
  ]);
  return keccak_256(preimage);
}

/**
 * Load or create wallet
 */
function loadOrCreateWallet(path: string): Keypair {
  if (!fs.existsSync(path)) {
    console.log("Creating new miner wallet...");
    const keypair = Keypair.generate();
    fs.writeFileSync(path, JSON.stringify(Array.from(keypair.secretKey)));
    return keypair;
  }
  return Keypair.fromSecretKey(
    new Uint8Array(JSON.parse(fs.readFileSync(path, "utf-8")))
  );
}

/**
 * Load salt cache from disk
 */
function loadSaltCache(): SaltCache {
  if (!fs.existsSync(SALT_CACHE_PATH)) {
    return {};
  }
  try {
    return JSON.parse(fs.readFileSync(SALT_CACHE_PATH, "utf-8"));
  } catch {
    return {};
  }
}

/**
 * Save salt cache to disk
 */
function saveSaltCache(cache: SaltCache): void {
  fs.writeFileSync(SALT_CACHE_PATH, JSON.stringify(cache, null, 2));
}

/**
 * Fetch data from external API (mock implementation)
 * In production, implement real API calls here
 */
async function fetchExternalData(
  eventId: string,
  category: string
): Promise<string | null> {
  // Mock implementation - replace with real API calls
  console.log(`  Fetching data for ${eventId} (${category})`);

  // Example: NBA game result
  if (eventId.includes("NBA")) {
    // In prod: call ESPN API, parse result
    return "YES"; // Home team won
  }

  // Example: Crypto price
  if (eventId.includes("BTC-PRICE")) {
    // In prod: call CoinGecko/Binance API
    return "YES"; // Price above threshold
  }

  // Unknown event type
  return null;
}

/**
 * Derive PDA addresses
 */
async function derivePDAs(
  programId: PublicKey,
  minerPubkey: PublicKey,
  queryPubkey: PublicKey,
  categoryId: string
) {
  const [minerProfile] = PublicKey.findProgramAddressSync(
    [Buffer.from("miner"), minerPubkey.toBuffer()],
    programId
  );

  const [categoryStats] = PublicKey.findProgramAddressSync(
    [Buffer.from("category"), Buffer.from(categoryId)],
    programId
  );

  const [voterRecord] = PublicKey.findProgramAddressSync(
    [Buffer.from("vote"), queryPubkey.toBuffer(), minerProfile.toBuffer()],
    programId
  );

  return { minerProfile, categoryStats, voterRecord };
}

// ============================================
// MAIN AGENT
// ============================================

async function main() {
  const connection = new Connection(RPC_URL, "confirmed");
  const keypair = loadOrCreateWallet(WALLET_PATH);
  const provider = new AnchorProvider(connection, new Wallet(keypair), {
    commitment: "confirmed",
  });
  const program = new Program(IDL as any, PROGRAM_ID, provider);

  let saltCache = loadSaltCache();

  console.log("Miner Agent Active");
  console.log(`   Address: ${keypair.publicKey.toBase58()}`);
  console.log(`   Categories: ${CONFIG.categories.join(", ")}`);
  console.log("");

  // Main loop
  while (true) {
    try {
      // Check gas prices
      const fees = await connection.getRecentPrioritizationFees();
      const currentGas = fees.length ? fees[fees.length - 1].prioritizationFee : 0;

      if (currentGas > CONFIG.maxGasLamports) {
        console.warn(`Gas spike (${currentGas} lamports). Sleeping...`);
        await sleep(CONFIG.pollIntervalMs);
        continue;
      }

      // Run commit and reveal cycles
      await runCommitCycle(program, keypair, saltCache);
      await runRevealCycle(program, keypair, saltCache);

      // Persist cache
      saveSaltCache(saltCache);
    } catch (e) {
      console.error("Cycle Error:", e);
    }

    await sleep(CONFIG.pollIntervalMs);
  }
}

// ============================================
// COMMIT CYCLE
// ============================================

async function runCommitCycle(
  program: Program,
  keypair: Keypair,
  saltCache: SaltCache
) {
  console.log("Scanning for commit opportunities...");

  const queries = await program.account.queryAccount.all();
  const now = Math.floor(Date.now() / 1000);

  for (const query of queries) {
    const data = query.account as unknown as QueryData;
    const queryKey = query.publicKey.toBase58();

    // Skip if not in our categories
    if (!CONFIG.categories.includes(data.categoryId)) continue;

    // Skip if not in commit phase
    if (!("commitPhase" in data.status)) continue;

    // Skip if deadline passed
    if (now > data.commitDeadline.toNumber()) continue;

    // Skip if already committed
    if (saltCache[queryKey]) {
      console.log(`  Already committed: ${data.uniqueEventId}`);
      continue;
    }

    console.log(`  Processing: ${data.uniqueEventId}`);

    try {
      // Fetch answer from external source
      const rawAnswer = await fetchExternalData(data.uniqueEventId, data.categoryId);
      if (!rawAnswer) {
        console.log(`  No data available`);
        continue;
      }

      // Normalize answer
      let finalAnswer: string;
      try {
        finalAnswer = normalizeData(rawAnswer, data.format);
      } catch (e) {
        console.error(`  Format error: ${e}`);
        continue;
      }

      // Generate salt and compute hash
      const salt = uuidv4();
      const voteHash = computeVoteHash(finalAnswer, salt);

      console.log(`  Answer: ${finalAnswer}`);
      console.log(`  Hash: ${Buffer.from(voteHash).toString("hex").slice(0, 16)}...`);

      // Derive PDAs
      const { minerProfile, categoryStats, voterRecord } = await derivePDAs(
        program.programId,
        keypair.publicKey,
        query.publicKey,
        data.categoryId
      );

      // Execute commit with retry
      await withRetry(
        () =>
          program.methods
            .commitVote(
              Array.from(voteHash) as any, // [u8; 32]
              Buffer.from([]) // Empty encrypted salt (simplified - no Lit Protocol)
            )
            .accounts({
              voter: keypair.publicKey,
              minerProfile: minerProfile,
              queryAccount: query.publicKey,
              categoryStats: categoryStats,
              voterRecord: voterRecord,
              systemProgram: PublicKey.default,
            })
            .rpc(),
        "Commit"
      );

      // Cache salt for reveal
      saltCache[queryKey] = {
        salt,
        answer: finalAnswer,
        committedAt: now,
      };

      console.log(`  Committed!`);
    } catch (e: any) {
      console.error(`  Commit failed: ${e.message}`);
    }
  }
}

// ============================================
// REVEAL CYCLE
// ============================================

async function runRevealCycle(
  program: Program,
  keypair: Keypair,
  saltCache: SaltCache
) {
  console.log("Scanning for reveal opportunities...");

  const queries = await program.account.queryAccount.all();
  const now = Math.floor(Date.now() / 1000);

  for (const query of queries) {
    const data = query.account as unknown as QueryData;
    const queryKey = query.publicKey.toBase58();

    // Skip if not in our categories
    if (!CONFIG.categories.includes(data.categoryId)) continue;

    // Skip if not in reveal phase
    if (!("revealPhase" in data.status)) continue;

    // Check if we have cached data for this query
    const cached = saltCache[queryKey];
    if (!cached) continue;

    // Skip if deadline passed
    if (now > data.revealDeadline.toNumber()) {
      console.log(`  Reveal deadline passed: ${data.uniqueEventId}`);
      // Clean up cache
      delete saltCache[queryKey];
      continue;
    }

    console.log(`  Revealing: ${data.uniqueEventId}`);

    try {
      // Derive PDAs
      const { minerProfile, voterRecord } = await derivePDAs(
        program.programId,
        keypair.publicKey,
        query.publicKey,
        data.categoryId
      );

      // Fetch VoteStats PDA
      const [voteStats] = PublicKey.findProgramAddressSync(
        [Buffer.from("stats"), query.publicKey.toBuffer()],
        program.programId
      );

      // Execute reveal with retry
      await withRetry(
        () =>
          program.methods
            .revealVote(cached.answer, cached.salt)
            .accounts({
              voter: keypair.publicKey,
              minerProfile: minerProfile,
              queryAccount: query.publicKey,
              voterRecord: voterRecord,
              voteStats: voteStats,
            })
            .rpc(),
        "Reveal"
      );

      console.log(`  Revealed: ${cached.answer}`);

      // Remove from cache (successfully revealed)
      delete saltCache[queryKey];
    } catch (e: any) {
      if (e.message?.includes("AlreadyRevealed")) {
        console.log(`  Already revealed`);
        delete saltCache[queryKey];
      } else {
        console.error(`  Reveal failed: ${e.message}`);
      }
    }
  }
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
        "AlreadyRevealed",
        "AlreadyClaimed",
        "HashMismatch",
        "WrongPhase",
        "PhaseClosed",
        "NotCommitted",
        "Unauthorized",
        "InsufficientFreeCapital",
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
// ENTRY POINT
// ============================================

main().catch(console.error);
