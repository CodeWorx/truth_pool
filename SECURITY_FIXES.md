# TruthPool Security Audit - Fix Summary

## Overview

This document summarizes all fixes applied to the TruthPool codebase following a comprehensive security audit.

---

## 🔴 Critical Fixes

### 1. Hash Function Mismatch (FIXED)
**Problem:** Miner agent used `js-sha256` while on-chain used `solana_program::hash::hash`. Different implementations could produce different outputs.

**Solution:** Standardized on `keccak256` for both:
- On-chain: `anchor_lang::solana_program::keccak::hash`
- Off-chain: `@noble/hashes/sha3` → `keccak_256`

```rust
// On-chain (lib.rs)
let mut preimage = Vec::new();
preimage.extend_from_slice(value.as_bytes());
preimage.extend_from_slice(salt.as_bytes());
let calculated_hash = keccak::hash(&preimage).to_bytes();
```

```typescript
// Off-chain (miner-agent/index.ts)
import { keccak_256 } from "@noble/hashes/sha3";

function computeVoteHash(value: string, salt: string): Uint8Array {
  const preimage = Buffer.concat([
    Buffer.from(value, "utf-8"),
    Buffer.from(salt, "utf-8"),
  ]);
  return keccak_256(preimage);
}
```

### 2. Predictable Randomness (FIXED)
**Problem:** Used `Clock::slot` for lottery selection, which validators can manipulate.

**Solution:** Implemented XOR Accumulator - each revealer's salt contributes to final entropy:

```rust
// In reveal_vote
let salt_hash = keccak::hash(salt.as_bytes()).to_bytes();
for i in 0..32 {
    query.random_accumulator[i] ^= salt_hash[i];
}

// In tally_votes
let final_entropy = query.random_accumulator;
let random_u64 = u64::from_le_bytes(final_entropy[0..8].try_into().unwrap());
let winning_ticket = (random_u64 % (max_votes as u64)) + 1;
```

### 3. Missing Authority Check on CommitVote (FIXED)
**Problem:** Anyone could commit votes using another miner's profile.

**Solution:** Added `has_one = authority` constraint:

```rust
#[derive(Accounts)]
pub struct CommitVote<'info> {
    #[account(mut)]
    pub voter: Signer<'info>,
    #[account(
        mut,
        has_one = authority,  // ← NEW
        seeds = [b"miner", voter.key().as_ref()],
        bump
    )]
    pub miner_profile: Account<'info, MinerProfile>,
    // ...
}
```

### 4. Unchecked Treasury/Winner Accounts (FIXED)
**Problem:** No verification that treasury, gas tank, and winner wallet were legitimate.

**Solution:** Added config verification:

```rust
// In claim_stake
require!(
    ctx.accounts.treasury.key() == config.treasury,
    CustomError::InvalidTreasury
);
require!(
    ctx.accounts.winner_wallet.key() == voter_record.authority,
    CustomError::InvalidWinnerWallet
);
```

### 5. Missing Category Initialization (FIXED)
**Problem:** `CategoryStats` PDA was expected to exist but no instruction to create it.

**Solution:** Added `initialize_category` instruction:

```rust
pub fn initialize_category(ctx: Context<InitCategory>, category_id: String) -> Result<()> {
    require!(category_id.len() <= 32, CustomError::CategoryIdTooLong);
    
    let category = &mut ctx.accounts.category_stats;
    category.category_id = category_id;
    category.active_miners = 0;
    Ok(())
}
```

---

## 🟠 High-Severity Fixes

### 6. Sentinel Cap Integer Division Bug (FIXED)
**Problem:** Cap check happened after incrementing, allowing 51%+ sentinel dominance.

**Solution:** Check BEFORE incrementing:

```rust
if miner.is_sentinel {
    if query.commit_count > 2 {
        let max_allowed = (query.commit_count - 1) / 2; // -1 because not added yet
        require!(
            query.sentinel_commit_count < max_allowed,
            CustomError::SentinelCapReached
        );
    }
    // THEN increment
    query.sentinel_commit_count += 1;
}
```

### 7. Speed Trigger Race Condition (FIXED)
**Problem:** Auto-transition to reveal phase caused late committers to fail.

**Solution:** Removed speed trigger. Added explicit `advance_to_reveal` instruction:

```rust
pub fn advance_to_reveal(ctx: Context<AdvancePhase>) -> Result<()> {
    let query = &mut ctx.accounts.query_account;
    let now = Clock::get()?.unix_timestamp;

    require!(query.status == QueryStatus::CommitPhase, CustomError::WrongPhase);
    require!(now > query.commit_deadline, CustomError::CommitWindowOpen);

    query.status = QueryStatus::RevealPhase;
    Ok(())
}
```

### 8. Miner Agent Missing Reveal Logic (FIXED)
**Problem:** Bot could commit but never revealed, guaranteeing bond loss.

**Solution:** Implemented complete reveal cycle with salt caching:

```typescript
// Salt cache persisted to disk
interface SaltCache {
  [queryKey: string]: {
    salt: string;
    answer: string;
    committedAt: number;
  };
}

// Reveal cycle scans for RevealPhase queries and submits cached data
async function runRevealCycle(program, keypair, saltCache) {
  // ... implementation in miner-agent/index.ts
}
```

### 9. Broken Lit Protocol Integration (FIXED)
**Problem:** Used deprecated/non-existent Lit Protocol methods.

**Solution:** Removed Lit Protocol dependency. Salt is stored locally in cache file. For production, implement proper encryption if needed.

### 10. Missing React Native Dependency (FIXED)
**Problem:** `@react-native-async-storage/async-storage` was imported but not in package.json.

**Solution:** Added to dependencies:

```json
{
  "dependencies": {
    "@react-native-async-storage/async-storage": "^1.22.0"
  }
}
```

---

## 🟡 Medium-Severity Fixes

### 11. Phase Transition Handling (FIXED)
**Problem:** No way to transition from CommitPhase to RevealPhase after deadline.

**Solution:** Added `advance_to_reveal` instruction (see #7).

### 12. Voided Round Recovery (FIXED)
**Problem:** If a round was voided, participants couldn't recover their bonds.

**Solution:** Added `recover_from_void` instruction:

```rust
pub fn recover_from_void(ctx: Context<RecoverVoid>) -> Result<()> {
    let query = &ctx.accounts.query_account;
    let voter_record = &mut ctx.accounts.voter_record;
    let miner = &mut ctx.accounts.miner_profile;

    require!(query.status == QueryStatus::Voided, CustomError::NotVoided);
    require!(!voter_record.bond_released, CustomError::AlreadyClaimed);

    // Release appropriate locked funds
    if voter_record.has_revealed {
        miner.pending_settlements = miner.pending_settlements.saturating_sub(VOTE_BOND);
    } else if voter_record.has_committed {
        miner.locked_liquidity = miner.locked_liquidity.saturating_sub(VOTE_BOND);
    }

    voter_record.bond_released = true;
    Ok(())
}
```

### 13. Non-Revealer Slashing (NEW)
**Problem:** Miners who committed but didn't reveal faced no penalty.

**Solution:** Added `slash_non_revealer` instruction to confiscate bonds after reveal deadline.

### 14. VoterRecord Authority Tracking (FIXED)
**Problem:** `VoterRecord.authority` stored miner PDA instead of actual authority.

**Solution:** Store both:

```rust
#[account]
pub struct VoterRecord {
    pub authority: Pubkey,      // The actual wallet
    pub miner_profile: Pubkey,  // The miner PDA
    // ...
}
```

### 15. Account Space Calculation (FIXED)
**Problem:** Hardcoded 500 bytes could overflow with long strings.

**Solution:** Used `InitSpace` derive macro:

```rust
#[account]
#[derive(InitSpace)]
pub struct MinerProfile {
    pub authority: Pubkey,
    #[max_len(32)]
    pub category_id: String,
    // ...
}
```

---

## 🔵 Code Quality Improvements

### 16. Error Messages (IMPROVED)
Added descriptive `#[msg(...)]` to all error codes:

```rust
#[error_code]
pub enum CustomError {
    #[msg("Phase is closed")]
    PhaseClosed,
    #[msg("Hash does not match commitment")]
    HashMismatch,
    // ...
}
```

### 17. Event Emissions (ADDED)
Added `ClaimEvent` for tracking successful claims:

```rust
#[event]
pub struct ClaimEvent {
    pub query: Pubkey,
    pub winner: Pubkey,
    pub amount: u64,
}
```

### 18. SolanaContext Types (FIXED)
Added proper TypeScript types and null checks:

```typescript
interface SolanaContextState {
  connection: Connection;
  publicKey: PublicKey | null;
  isConnecting: boolean;
  error: string | null;
  // ...
}
```

### 19. Pulse AMM Market Checking (FIXED)
Added proper existence check before creating markets:

```typescript
async function marketExists(program, categoryId, eventId): Promise<boolean> {
  const [queryPDA] = await getQueryPDA(program.programId, categoryId, eventId);
  const account = await program.account.queryAccount.fetchNullable(queryPDA);
  return account !== null;
}
```

---

## Files Modified

| File | Changes |
|------|---------|
| `programs/truth-pool/src/lib.rs` | Complete rewrite with all security fixes |
| `bots/miner-agent/index.ts` | New implementation with reveal logic |
| `bots/miner-agent/package.json` | Updated dependencies |
| `bots/pulse-amm/index.ts` | Added market existence checking |
| `bots/pulse-amm/package.json` | Updated dependencies |
| `src/context/SolanaContext.tsx` | Fixed types, added error handling |
| `package.json` | Added missing AsyncStorage dependency |

---

## Migration Steps

1. **Deploy new program** - The lib.rs changes require redeployment
2. **Initialize categories** - Admin must call `initialize_category` for each category
3. **Update bots** - Replace bot code and run `npm install`
4. **Update mobile app** - Run `npm install` for new dependencies

---

## Testing Recommendations

1. **Hash consistency test** - Verify off-chain hash matches on-chain
2. **Sentinel cap test** - Ensure sentinels can't exceed 49%
3. **Phase transition test** - Test commit → reveal → tally flow
4. **Void recovery test** - Test bond recovery after voided rounds
5. **Authority test** - Verify only authorized users can commit/reveal
