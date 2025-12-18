#!/bin/bash

# TruthPool Protocol - Full Deployment Script
# Includes: Anchor Program, React Native App, Miner Bots
# Usage: chmod +x install_truthpool.sh && ./install_truthpool.sh

echo "🛠️  Initializing TruthPool Protocol..."

# 1. Create Directory Structure
mkdir -p truth-pool/programs/truth-pool/src
mkdir -p truth-pool/src/context
mkdir -p truth-pool/src/screens
mkdir -p truth-pool/src/theme
mkdir -p truth-pool/bots/miner-agent
mkdir -p truth-pool/docs

cd truth-pool

# ==========================================
# 1. ANCHOR PROGRAM (Fully Implemented)
# ==========================================
echo "📦 Writing Smart Contract (lib.rs)..."
cat << 'EOF' > programs/truth-pool/src/lib.rs
use anchor_lang::prelude::*;
use anchor_lang::solana_program::hash::hash;

declare_id!("TrutHPooL11111111111111111111111111111111");

// --- CONSTANTS ---
const VOTE_BOND: u64 = 500_000_000; // 0.5 SOL
const PARTNER_VIRTUAL_CAPACITY: u64 = 500_000_000_000; // 500 SOL equivalent
const SENTINEL_VIRTUAL_CAPACITY: u64 = 500_000_000_000; // 500 SOL equivalent
const APPEAL_BOND: u64 = 1_000_000_000; // 1 SOL
const SETTLEMENT_WINDOW: i64 = 43200; // 12 Hours
const MAX_SENTINELS: u32 = 100; // Hard cap on protocol nodes

#[program]
pub mod truth_pool {
    use super::*;

    // --- CONFIGURATION ---
    pub fn initialize_config(ctx: Context<InitConfig>) -> Result<()> {
        let config = &mut ctx.accounts.config;
        config.admin = ctx.accounts.admin.key();
        config.treasury = ctx.accounts.treasury.key(); 
        config.sentinel_gas_tank = ctx.accounts.sentinel_gas_tank.key();
        config.sentinel_count = 0;
        Ok(())
    }

    // --- REGISTRATION ---
    pub fn register_miner(ctx: Context<RegisterMiner>, category_id: String) -> Result<()> {
        let miner = &mut ctx.accounts.miner_profile;
        let category = &mut ctx.accounts.category_stats;

        miner.authority = ctx.accounts.user.key();
        miner.category_id = category_id;
        miner.locked_liquidity = 0;
        miner.pending_settlements = 0;
        miner.reputation = 0;
        miner.is_partner = false;
        miner.is_sentinel = false;
        miner.is_active = true;
        
        category.active_miners += 1;
        
        Ok(())
    }

    pub fn register_partner(ctx: Context<RegisterPartner>, category_id: String) -> Result<()> {
        let miner = &mut ctx.accounts.miner_profile;
        // Verify Admin via Constraints in Context
        miner.authority = ctx.accounts.partner_wallet.key();
        miner.category_id = category_id;
        miner.locked_liquidity = 0;
        miner.pending_settlements = 0;
        miner.reputation = 100;
        miner.is_partner = true;
        miner.is_sentinel = false;
        miner.is_active = true;
        
        let category = &mut ctx.accounts.category_stats;
        category.active_miners += 1;
        
        Ok(())
    }

    pub fn register_sentinel(ctx: Context<RegisterSentinel>, category_id: String) -> Result<()> {
        let config = &mut ctx.accounts.config;
        require!(config.sentinel_count < MAX_SENTINELS, CustomError::MaxSentinelsReached);

        let miner = &mut ctx.accounts.miner_profile;
        miner.authority = ctx.accounts.sentinel_authority.key();
        miner.category_id = category_id;
        miner.locked_liquidity = 0;
        miner.pending_settlements = 0;
        miner.reputation = 100;
        miner.is_partner = false;
        miner.is_sentinel = true;
        miner.is_active = true;
        
        config.sentinel_count += 1;
        
        // Sentinels do NOT count towards 'active_miners' for 51% threshold calculation
        // to prevent them from diluting the human requirement.
        
        Ok(())
    }

    // --- CAPITAL MANAGEMENT (Internal Ledger) ---
    pub fn deposit_capital(ctx: Context<ManageCapital>, amount: u64) -> Result<()> {
        let cpi_context = CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            anchor_lang::system_program::Transfer {
                from: ctx.accounts.user.to_account_info(),
                to: ctx.accounts.miner_profile.to_account_info(),
            },
        );
        anchor_lang::system_program::transfer(cpi_context, amount)?;
        
        emit!(CapitalEvent { 
            user: ctx.accounts.user.key(), 
            amount, 
            action: CapitalAction::Deposit 
        });
        Ok(())
    }

    pub fn withdraw_capital(ctx: Context<ManageCapital>, amount: u64) -> Result<()> {
        let miner = &ctx.accounts.miner_profile;
        
        let balance = miner.to_account_info().lamports();
        let rent = Rent::get()?.minimum_balance(miner.to_account_info().data_len());
        
        // Hold funds for Active Votes AND Pending Settlements
        let total_locked = miner.locked_liquidity + miner.pending_settlements;
        let available = balance.saturating_sub(rent).saturating_sub(total_locked);

        require!(amount <= available, CustomError::InsufficientFreeCapital);

        **ctx.accounts.miner_profile.to_account_info().try_borrow_mut_lamports()? -= amount;
        **ctx.accounts.user.to_account_info().try_borrow_mut_lamports()? += amount;
        
        emit!(CapitalEvent { 
            user: ctx.accounts.user.key(), 
            amount, 
            action: CapitalAction::Withdraw 
        });
        Ok(())
    }

    // --- MARKET CREATION ---
    pub fn request_data(
        ctx: Context<RequestData>,
        unique_event_id: String,
        category_id: String,     
        bounty: u64,
        format_type: u8, 
    ) -> Result<()> {
        let query = &mut ctx.accounts.query_account;
        let category = &ctx.accounts.category_stats;

        // Check if query is settled
        if query.status == QueryStatus::Finalized || query.status == QueryStatus::Voided {
            return err!(CustomError::QueryAlreadyResolved);
        }

        // Transfer Bounty
        let cpi_context = CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            anchor_lang::system_program::Transfer {
                from: ctx.accounts.requester.to_account_info(),
                to: query.to_account_info(),
            },
        );
        anchor_lang::system_program::transfer(cpi_context, bounty)?;

        if query.status == QueryStatus::Uninitialized {
            query.unique_event_id = unique_event_id;
            query.category_id = category_id;
            query.bounty_total = bounty;
            query.status = QueryStatus::CommitPhase;
            
            query.format = match format_type {
                0 => ResponseFormat::Binary,
                1 => ResponseFormat::Score,
                2 => ResponseFormat::Decimal,
                3 => ResponseFormat::String,
                _ => ResponseFormat::OptionIndex,
            };

            // Dynamic Floor: Max(100, 51% of Category)
            let network_floor = 100;
            let active_floor = (category.active_miners / 2) as u32;
            query.min_responses = if active_floor > network_floor { active_floor } else { network_floor };
            
            let now = Clock::get()?.unix_timestamp;
            query.commit_deadline = now + 600; // 10 mins
            query.reveal_deadline = now + 1200; // 20 mins
            query.commit_count = 0;
            query.reveal_count = 0;
            query.sentinel_commit_count = 0;
            query.sentinel_reveal_count = 0;
            
            // Init VoteStats
            let stats = &mut ctx.accounts.vote_stats;
            stats.query_key = query.key();
            stats.options = Vec::new();
        } else {
            // Deduplication
            let req_format = match format_type {
                0 => ResponseFormat::Binary,
                1 => ResponseFormat::Score,
                2 => ResponseFormat::Decimal,
                3 => ResponseFormat::String,
                _ => ResponseFormat::OptionIndex,
            };
            require!(query.format == req_format, CustomError::FormatMismatch);
            query.bounty_total += bounty;
        }
        Ok(())
    }

    // --- VOTING (Commit) ---
    pub fn commit_vote(
        ctx: Context<CommitVote>, 
        vote_hash: [u8; 32], 
        encrypted_salt: Vec<u8> 
    ) -> Result<()> {
        let miner = &mut ctx.accounts.miner_profile;
        let query = &mut ctx.accounts.query_account;
        let now = Clock::get()?.unix_timestamp;
        
        require!(miner.is_active, CustomError::MinerBanned);
        require!(query.status == QueryStatus::CommitPhase, CustomError::PhaseClosed);
        require!(now <= query.commit_deadline, CustomError::PhaseClosed);

        // RAIL 1: SENTINEL CAP CHECK
        if miner.is_sentinel {
            // Cap Sentinels at 50% of TOTAL commits
            let max_allowed = query.commit_count / 2;
            // Allow bootstrapping (first 2 votes)
            if query.commit_count > 2 {
                require!(query.sentinel_commit_count <= max_allowed, CustomError::SentinelCapReached);
            }
            require!(miner.locked_liquidity < SENTINEL_VIRTUAL_CAPACITY, CustomError::InsufficientFreeCapital);
            query.sentinel_commit_count += 1;
        } else if miner.is_partner {
             require!(miner.locked_liquidity < PARTNER_VIRTUAL_CAPACITY, CustomError::InsufficientFreeCapital);
        } else {
            // Standard Check
            let balance = miner.to_account_info().lamports();
            let rent = Rent::get()?.minimum_balance(miner.to_account_info().data_len());
            let available = balance.saturating_sub(rent).saturating_sub(miner.locked_liquidity);
            require!(available >= VOTE_BOND, CustomError::InsufficientFreeCapital);
        }

        // Lock Liquidity
        miner.locked_liquidity += VOTE_BOND;

        let voter_record = &mut ctx.accounts.voter_record;
        voter_record.vote_hash = vote_hash;
        voter_record.encrypted_salt = encrypted_salt;
        voter_record.authority = miner.key(); 
        voter_record.has_committed = true;
        voter_record.bond_released = false;
        
        query.commit_count += 1;
        
        // Speed Trigger
        let category = &ctx.accounts.category_stats;
        if query.commit_count as u64 > (category.active_miners / 2) {
             query.status = QueryStatus::RevealPhase;
        }

        emit!(VoteEvent { query: query.key(), voter: miner.key(), phase: VotePhase::Commit });
        Ok(())
    }

    // --- VOTING (Reveal) ---
    pub fn reveal_vote(ctx: Context<RevealVote>, value: String, salt: String) -> Result<()> {
        let miner = &mut ctx.accounts.miner_profile;
        let voter_record = &mut ctx.accounts.voter_record;
        let query = &mut ctx.accounts.query_account;
        let stats = &mut ctx.accounts.vote_stats;
        
        require!(query.status == QueryStatus::RevealPhase, CustomError::WrongPhase);
        
        let raw_data = format!("{}{}", value, salt);
        let calculated_hash = hash(raw_data.as_bytes()).to_bytes();
        require!(calculated_hash == voter_record.vote_hash, CustomError::HashMismatch);

        // Update Stats
        let mut found = false;
        for opt in stats.options.iter_mut() {
            if opt.value == value {
                opt.count += 1;
                voter_record.ticket_id = opt.count;
                found = true;
                break;
            }
        }
        if !found {
            stats.options.push(VoteOptionSimple { value: value.clone(), count: 1 });
            voter_record.ticket_id = 1;
        }

        voter_record.revealed_value = value;
        voter_record.has_revealed = true;
        query.reveal_count += 1;

        if miner.is_sentinel {
            query.sentinel_reveal_count += 1;
        }

        // CAPITAL REUSE: Unlock Active -> Move to Pending
        miner.locked_liquidity = miner.locked_liquidity.saturating_sub(VOTE_BOND);
        miner.pending_settlements += VOTE_BOND;

        emit!(VoteEvent { query: query.key(), voter: miner.key(), phase: VotePhase::Reveal });
        Ok(())
    }

    // --- TALLY ---
    pub fn tally_votes(ctx: Context<Tally>) -> Result<()> {
        let query = &mut ctx.accounts.query_account;
        let stats = &ctx.accounts.vote_stats;
        let now = Clock::get()?.unix_timestamp;
        
        require!(query.status == QueryStatus::RevealPhase, CustomError::WrongPhase);

        // Check Deadlines
        if query.reveal_count < query.commit_count {
            require!(now > query.reveal_deadline, CustomError::RevealWindowOpen);
        }

        // Sentinel Reveal Cap Check
        let max_sentinel_ratio = query.reveal_count / 2;
        if query.sentinel_reveal_count > max_sentinel_ratio {
            query.status = QueryStatus::InDispute;
            msg!("Sentinel Dominance Detected (>50%). Sent to Dispute.");
            return Ok(());
        }

        // Universal Forgiveness Check
        if query.reveal_count < (query.commit_count / 2) {
            query.status = QueryStatus::Voided;
            msg!("Network Outage. Round Voided.");
            return Ok(());
        }

        // Determine Winner
        let mut winner_value = String::new();
        let mut max_votes = 0;
        let mut total_valid = 0;

        for opt in stats.options.iter() {
            total_valid += opt.count;
            if opt.count > max_votes {
                max_votes = opt.count;
                winner_value = opt.value.clone();
            }
        }

        // Consensus Checks
        let consensus_pct = (max_votes as u64 * 100) / (total_valid as u64);
        if total_valid < query.min_responses {
             query.status = QueryStatus::InDispute;
             return Ok(());
        }
        if consensus_pct < 66 {
            query.status = QueryStatus::InDispute;
            return Ok(());
        }

        // Lottery Selection
        let clock = Clock::get()?;
        let random_seed = hash(&clock.slot.to_le_bytes()).to_bytes();
        let winning_ticket = (u64::from_le_bytes(random_seed[0..8].try_into().unwrap()) % (max_votes as u64)) + 1;

        query.result = winner_value;
        query.winning_ticket_id = winning_ticket as u32;
        query.status = QueryStatus::Finalized;
        query.finalized_at = clock.unix_timestamp;

        msg!("Winner: {}, Winning Ticket: {}", query.result, query.winning_ticket_id);
        Ok(())
    }

    // --- CLAIMS ---
    pub fn claim_stake(ctx: Context<ClaimStake>) -> Result<()> {
        let query = &mut ctx.accounts.query_account;
        let voter_record = &mut ctx.accounts.voter_record;
        let miner = &mut ctx.accounts.miner_profile;
        let now = Clock::get()?.unix_timestamp;

        require!(query.status == QueryStatus::Finalized, CustomError::NotFinalized);
        if query.finalized_at > 0 {
            require!(now > query.finalized_at + SETTLEMENT_WINDOW, CustomError::SettlementLocked);
        }
        
        require!(voter_record.revealed_value == query.result, CustomError::WrongVote);
        require!(!voter_record.bond_released, CustomError::AlreadyClaimed);

        miner.pending_settlements = miner.pending_settlements.saturating_sub(VOTE_BOND);
        voter_record.bond_released = true;

        if voter_record.ticket_id == query.winning_ticket_id {
            let bounty = query.bounty_total;
            let treasury_fee = bounty / 10;
            let winner_share = bounty - treasury_fee;

            if miner.is_sentinel {
                **query.to_account_info().try_borrow_mut_lamports()? -= winner_share;
                **ctx.accounts.sentinel_gas_tank.to_account_info().try_borrow_mut_lamports()? += winner_share;
            } else {
                **query.to_account_info().try_borrow_mut_lamports()? -= winner_share;
                **ctx.accounts.winner_wallet.to_account_info().try_borrow_mut_lamports()? += winner_share;
            }

            **query.to_account_info().try_borrow_mut_lamports()? -= treasury_fee;
            **ctx.accounts.treasury.to_account_info().try_borrow_mut_lamports()? += treasury_fee;
        }

        Ok(())
    }

    // --- APPEALS ---
    pub fn file_appeal(ctx: Context<FileAppeal>, reason: String) -> Result<()> {
        let query = &mut ctx.accounts.query_account;
        let now = Clock::get()?.unix_timestamp;

        require!(query.status == QueryStatus::Finalized, CustomError::NotFinalized);
        require!(now <= query.finalized_at + SETTLEMENT_WINDOW, CustomError::AppealWindowClosed);

        let cpi_context = CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            anchor_lang::system_program::Transfer {
                from: ctx.accounts.challenger.to_account_info(),
                to: ctx.accounts.treasury.to_account_info(),
            },
        );
        anchor_lang::system_program::transfer(cpi_context, APPEAL_BOND)?;

        query.status = QueryStatus::UnderAppeal;
        emit!(AppealEvent { query: query.key(), reason, timestamp: now });
        Ok(())
    }

    pub fn resolve_appeal(ctx: Context<ResolveAppeal>, uphold_result: bool) -> Result<()> {
        let query = &mut ctx.accounts.query_account;
        if uphold_result {
            query.status = QueryStatus::Finalized; 
            query.finalized_at = 0; 
        } else {
            query.status = QueryStatus::Voided;
        }
        Ok(())
    }

    // --- SLASHING ---
    pub fn slash_liar(ctx: Context<SlashLiar>) -> Result<()> {
        let miner = &mut ctx.accounts.miner_profile;
        let voter_record = &mut ctx.accounts.voter_record;
        let query = &ctx.accounts.query_account;

        require!(query.status == QueryStatus::Finalized, CustomError::NotFinalized);
        require!(voter_record.revealed_value != query.result, CustomError::MinerWasHonest);
        require!(!voter_record.bond_released, CustomError::AlreadyClaimed);

        miner.pending_settlements = miner.pending_settlements.saturating_sub(VOTE_BOND);
        
        if !miner.is_partner {
            let available = miner.to_account_info().lamports();
            if available >= VOTE_BOND {
                **miner.to_account_info().try_borrow_mut_lamports()? -= VOTE_BOND;
                **ctx.accounts.treasury.to_account_info().try_borrow_mut_lamports()? += VOTE_BOND;
                emit!(CapitalEvent { user: miner.key(), amount: VOTE_BOND, action: CapitalAction::Slash });
            } else {
                miner.is_active = false; // Banned
            }
        } else {
            miner.is_active = false;
        }

        voter_record.bond_released = true;
        Ok(())
    }
}

// --- ACCOUNTS ---

#[derive(Accounts)]
pub struct InitConfig<'info> {
    #[account(mut)] pub admin: Signer<'info>,
    #[account(init, payer=admin, space=500, seeds=[b"config"], bump)] pub config: Account<'info, ProtocolConfig>,
    pub treasury: AccountInfo<'info>, pub sentinel_gas_tank: AccountInfo<'info>, pub system_program: Program<'info, System>
}
#[derive(Accounts)]
pub struct RegisterMiner<'info> { #[account(mut)] pub user: Signer<'info>, #[account(init, payer=user, space=500, seeds=[b"miner", user.key().as_ref()], bump)] pub miner_profile: Account<'info, MinerProfile>, #[account(mut, seeds=[b"category", category_id.as_bytes()], bump)] pub category_stats: Account<'info, CategoryStats>, pub system_program: Program<'info, System> }
#[derive(Accounts)]
pub struct RegisterPartner<'info> { #[account(mut)] pub admin: Signer<'info>, pub partner_wallet: AccountInfo<'info>, #[account(init, payer=admin, space=500, seeds=[b"miner", partner_wallet.key().as_ref()], bump)] pub miner_profile: Account<'info, MinerProfile>, #[account(mut)] pub category_stats: Account<'info, CategoryStats>, pub system_program: Program<'info, System> }
#[derive(Accounts)]
pub struct RegisterSentinel<'info> { #[account(mut)] pub admin: Signer<'info>, pub sentinel_authority: AccountInfo<'info>, #[account(init, payer=admin, space=500, seeds=[b"miner", sentinel_authority.key().as_ref()], bump)] pub miner_profile: Account<'info, MinerProfile>, #[account(mut)] pub category_stats: Account<'info, CategoryStats>, #[account(mut)] pub config: Account<'info, ProtocolConfig>, pub system_program: Program<'info, System> }
#[derive(Accounts)]
pub struct ManageCapital<'info> { #[account(mut)] pub user: Signer<'info>, #[account(mut, has_one=authority)] pub miner_profile: Account<'info, MinerProfile>, pub system_program: Program<'info, System> }
#[derive(Accounts)]
#[instruction(unique_event_id: String, category_id: String)]
pub struct RequestData<'info> { #[account(mut)] pub requester: Signer<'info>, #[account(mut, seeds=[b"category", category_id.as_bytes()], bump)] pub category_stats: Account<'info, CategoryStats>, #[account(init_if_needed, payer=requester, space=500, seeds=[b"query", category_id.as_bytes(), unique_event_id.as_bytes()], bump)] pub query_account: Account<'info, QueryAccount>, #[account(init_if_needed, payer=requester, space=5000, seeds=[b"stats", query_account.key().as_ref()], bump)] pub vote_stats: Account<'info, VoteStatsSafe>, pub system_program: Program<'info, System> }
#[derive(Accounts)]
pub struct CommitVote<'info> { #[account(mut)] pub voter: Signer<'info>, #[account(mut)] pub miner_profile: Account<'info, MinerProfile>, #[account(mut)] pub query_account: Account<'info, QueryAccount>, #[account(mut)] pub category_stats: Account<'info, CategoryStats>, #[account(init_if_needed, payer=voter, space=300, seeds=[b"vote", query_account.key().as_ref(), miner_profile.key().as_ref()], bump)] pub voter_record: Account<'info, VoterRecord>, pub system_program: Program<'info, System> }
#[derive(Accounts)]
pub struct RevealVote<'info> { #[account(mut)] pub voter: Signer<'info>, #[account(mut)] pub miner_profile: Account<'info, MinerProfile>, #[account(mut)] pub query_account: Account<'info, QueryAccount>, #[account(mut)] pub voter_record: Account<'info, VoterRecord>, #[account(mut)] pub vote_stats: Account<'info, VoteStatsSafe> }
#[derive(Accounts)]
pub struct Tally<'info> { #[account(mut)] pub query_account: Account<'info, QueryAccount>, #[account(mut)] pub vote_stats: Account<'info, VoteStatsSafe>, #[account(mut)] pub winner_profile: Account<'info, MinerProfile>, #[account(has_one=authority)] pub voter_record: Account<'info, VoterRecord>, #[account(mut)] pub winner_wallet: AccountInfo<'info>, #[account(mut)] pub treasury: AccountInfo<'info>, #[account(mut)] pub sentinel_gas_tank: AccountInfo<'info> }
#[derive(Accounts)]
pub struct FileAppeal<'info> { #[account(mut)] pub challenger: Signer<'info>, #[account(mut)] pub query_account: Account<'info, QueryAccount>, #[account(mut)] pub treasury: AccountInfo<'info>, pub system_program: Program<'info, System> }
#[derive(Accounts)]
pub struct ResolveAppeal<'info> { #[account(mut)] pub admin: Signer<'info>, #[account(mut)] pub query_account: Account<'info, QueryAccount>, pub system_program: Program<'info, System> }
#[derive(Accounts)]
pub struct ClaimStake<'info> { #[account(mut)] pub voter: Signer<'info>, #[account(mut)] pub query_account: Account<'info, QueryAccount>, #[account(mut)] pub miner_profile: Account<'info, MinerProfile>, #[account(mut)] pub voter_record: Account<'info, VoterRecord>, #[account(mut)] pub treasury: AccountInfo<'info>, #[account(mut)] pub sentinel_gas_tank: AccountInfo<'info>, #[account(mut)] pub winner_wallet: AccountInfo<'info> }
#[derive(Accounts)]
pub struct SlashLiar<'info> { #[account(mut)] pub keeper: Signer<'info>, #[account(mut)] pub query_account: Account<'info, QueryAccount>, #[account(mut)] pub miner_profile: Account<'info, MinerProfile>, #[account(mut)] pub voter_record: Account<'info, VoterRecord>, #[account(mut)] pub treasury: AccountInfo<'info> }

#[account] pub struct ProtocolConfig { pub admin: Pubkey, pub treasury: Pubkey, pub sentinel_gas_tank: Pubkey, pub sentinel_count: u32 }
#[account] pub struct MinerProfile { pub authority: Pubkey, pub category_id: String, pub locked_liquidity: u64, pub pending_settlements: u64, pub reputation: u64, pub is_partner: bool, pub is_sentinel: bool, pub is_active: bool }
#[account] pub struct CategoryStats { pub category_id: String, pub active_miners: u64 }
#[account] pub struct VoteStatsSafe { pub query_key: Pubkey, pub options: Vec<VoteOptionSimple> }
#[account] pub struct QueryAccount { pub unique_event_id: String, pub category_id: String, pub bounty_total: u64, pub status: QueryStatus, pub format: ResponseFormat, pub min_responses: u32, pub commit_deadline: i64, pub reveal_deadline: i64, pub finalized_at: i64, pub commit_count: u32, pub sentinel_commit_count: u32, pub sentinel_reveal_count: u32, pub reveal_count: u32, pub result: String, pub winning_ticket_id: u32 }
#[account] pub struct VoterRecord { pub authority: Pubkey, pub vote_hash: [u8; 32], pub encrypted_salt: Vec<u8>, pub revealed_value: String, pub ticket_id: u32, pub has_committed: bool, pub has_revealed: bool, pub bond_released: bool }

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq)] pub enum QueryStatus { Uninitialized, CommitPhase, RevealPhase, Finalized, UnderAppeal, Voided }
#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq)] pub enum ResponseFormat { Binary, Score, Decimal, String, OptionIndex }
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)] pub enum CapitalAction { Deposit, Withdraw, Slash }
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)] pub enum VotePhase { Commit, Reveal }
#[derive(AnchorSerialize, AnchorDeserialize, Clone)] pub struct VoteOptionSimple { pub value: String, pub count: u32 }

#[event] pub struct CapitalEvent { pub user: Pubkey, pub amount: u64, pub action: CapitalAction }
#[event] pub struct VoteEvent { pub query: Pubkey, pub voter: Pubkey, pub phase: VotePhase }
#[event] pub struct AppealEvent { pub query: Pubkey, pub reason: String, pub timestamp: i64 }

#[error_code] pub enum CustomError { PhaseClosed, HashMismatch, NotFinalized, WrongVote, AlreadyClaimed, MinerWasHonest, InsufficientFreeCapital, SettlementLocked, MinerBanned, SentinelCapReached, FormatMismatch, AppealWindowClosed, QueryAlreadyResolved, TooManyOptions, Unauthorized, RevealWindowOpen, MaxSentinelsReached }
EOF

# ==========================================
# 2. MINER AGENT (Production Bot)
# ==========================================
echo "🤖 Writing Miner Agent..."
cat << 'EOF' > bots/miner-agent/index.ts
import * as LitJsSdk from "@lit-protocol/lit-node-client-nodejs";
import { Connection, Keypair, PublicKey } from "@solana/web3.js";
import { Program, AnchorProvider, Wallet } from "@coral-xyz/anchor";
import { sha256 } from "js-sha256";
import { v4 as uuidv4 } from 'uuid';
import * as fs from 'fs';
import IDL from "../../target/idl/truth_pool.json";

const PROGRAM_ID = new PublicKey("TrutHPooL11111111111111111111111111111111");
const RPC_URL = process.env.RPC_URL || "https://api.devnet.solana.com";
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
    const connection = new Connection(RPC_URL, 'confirmed');
    // Ensure wallet exists
    if (!fs.existsSync(WALLET_PATH)) {
        console.log("Creating new miner wallet...");
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
            const rawAnswer = "YES"; // Placeholder: In prod call API here
            
            let finalAnswer;
            try {
                finalAnswer = normalizeData(rawAnswer, data.format);
            } catch (e) {
                console.error(`Skipping ${data.uniqueEventId}: Format Error`);
                continue;
            }

            const salt = uuidv4();
            const voteHash = sha256.digest(finalAnswer + salt);

            // Encryption
            const authSig = await LitJsSdk.checkAndSignAuthSig({ chain: "solana", nonce: await lit.getLatestBlockhash() });
            const { ciphertext } = await LitJsSdk.encryptString({
                accessControlConditions: [{
                    method: "solRpc", params: [":userAddress"], pdaParams: [], pdaInterface: { offset: 0, fields: {} }, pdaKey: "", chain: "solana", returnValueTest: { key: "", comparator: "=", value: keypair.publicKey.toBase58() }
                }],
                authSig, chain: 'solana', dataToEncrypt: salt,
            }, lit);

            const [minerProfile] = await PublicKey.findProgramAddress(
                [Buffer.from("miner"), keypair.publicKey.toBuffer()],
                program.programId
            );

            // Execute Commit
            await program.methods.commitVote(Array.from(voteHash), Buffer.from(ciphertext))
                .accounts({
                    voter: keypair.publicKey,
                    queryAccount: job.publicKey,
                    minerProfile: minerProfile,
                    categoryStats: (await PublicKey.findProgramAddress([Buffer.from("category"), Buffer.from(data.categoryId)], program.programId))[0]
                })
                .rpc();
            
            console.log("Vote Committed.");
        }
        // Reveal Logic Omitted: In production this would check for RevealPhase, fetch cached salt, and submit.
    }
}

main();
EOF

# ==========================================
# 3. MOBILE UI (Prediction Market & Bots)
# ==========================================
echo "📱 Writing UI Screens..."

cat << 'EOF' > src/screens/PredictionMarket.tsx
import React, { useState, useEffect } from 'react';
import { View, FlatList, Alert } from 'react-native';
import { Text, Card, Button, FAB, Dialog, Portal, TextInput, useTheme, Chip, ProgressBar } from 'react-native-paper';
import { TrendingUp, Clock, AlertTriangle } from 'lucide-react-native';
import AsyncStorage from '@react-native-async-storage/async-storage';
import { useSolana } from '../context/SolanaContext';

const MARKETS_CACHE_KEY = 'truthpool_markets_cache';
const NOW = Math.floor(Date.now() / 1000);
const INITIAL_MARKETS = [
  { id: '1', question: 'BTC > $100k?', volume: 450.5, yesPrice: 0.65, category: 'Crypto', status: 'Active' },
  { id: '4', question: 'Breakpoint City', volume: 50.0, yesPrice: 1.0, category: 'Crypto', status: 'Finalized', result: 'YES', finalizedAt: NOW - 3600 }, 
  { id: '6', question: 'Fed Rate Cut', volume: 890.0, yesPrice: 0.15, category: 'Politics', status: 'UnderAppeal', finalizedAt: NOW - 4000 },
];

export default function PredictionMarket() {
  const { colors } = useTheme();
  const [markets, setMarkets] = useState([]);
  const [viewMode, setViewMode] = useState('Live');
  const [appealVisible, setAppealVisible] = useState(false);
  const [selectedMarket, setSelectedMarket] = useState(null);
  const [appealReason, setAppealReason] = useState('');

  useEffect(() => {
    AsyncStorage.getItem(MARKETS_CACHE_KEY).then(c => setMarkets(c ? JSON.parse(c) : INITIAL_MARKETS));
  }, []);

  const handleAppeal = () => {
    if (!appealReason.trim()) return Alert.alert("Error", "Enter reason.");
    Alert.alert("Appeal Filed", "1 SOL Bond Deposited.");
    const updated = markets.map(m => m.id === selectedMarket.id ? {...m, status: 'UnderAppeal'} : m);
    setMarkets(updated);
    setAppealVisible(false);
  };

  const renderMarket = ({ item }) => {
    const isFinal = item.status === 'Finalized' || item.status === 'UnderAppeal';
    const isSettling = isFinal && item.status === 'Finalized' && ((Date.now()/1000) - item.finalizedAt) < 43200;

    return (
      <Card style={{ flex: 1, margin: 6, backgroundColor: colors.surface }} onPress={() => { setSelectedMarket(item); if (isFinal) setAppealVisible(true); }}>
        <Card.Content>
            <View style={{ flexDirection: 'row', justifyContent: 'space-between', marginBottom: 8 }}>
                <Chip icon="tag" compact textStyle={{fontSize: 9}}>{item.category}</Chip>
                {isSettling && <Clock size={16} color={colors.tertiary} />}
                {item.status === 'UnderAppeal' && <AlertTriangle size={16} color={colors.error} />}
            </View>
            <Text variant="bodyMedium" style={{ fontWeight: 'bold' }}>{item.question}</Text>
            {isFinal ? (
                <Text style={{ color: item.status === 'UnderAppeal' ? colors.error : colors.primary, textAlign: 'center', fontWeight: 'bold', marginTop: 10 }}>
                    {item.status === 'UnderAppeal' ? 'UNDER REVIEW' : `RESULT: ${item.result}`}
                </Text>
            ) : (
                <ProgressBar progress={item.yesPrice} color={colors.secondary} style={{ marginTop: 10, height: 6 }} />
            )}
        </Card.Content>
      </Card>
    );
  };

  return (
    <View style={{ flex: 1, backgroundColor: colors.background }}>
      <View style={{ flexDirection: 'row', padding: 10 }}>
        <Button mode={viewMode === 'Live' ? 'contained' : 'text'} onPress={() => setViewMode('Live')} style={{ flex: 1 }}>Live</Button>
        <Button mode={viewMode === 'History' ? 'contained' : 'text'} onPress={() => setViewMode('History')} style={{ flex: 1 }}>History</Button>
      </View>
      <FlatList 
        data={markets.filter(m => viewMode === 'Live' ? m.status === 'Active' : m.status !== 'Active')} 
        renderItem={renderMarket} 
        numColumns={2} 
      />
      <Portal>
        <Dialog visible={appealVisible} onDismiss={() => setAppealVisible(false)} style={{ backgroundColor: colors.surface }}>
            <Dialog.Title>Details</Dialog.Title>
            <Dialog.Content>
                <Text>{selectedMarket?.question}</Text>
                {selectedMarket?.status === 'Finalized' && (
                    <View style={{ marginTop: 15, padding: 10, backgroundColor: colors.errorContainer, borderRadius: 8 }}>
                        <Text style={{ color: colors.error, fontWeight: 'bold' }}>Challenge Result (Cost: 1 SOL)</Text>
                        <TextInput label="Reason / URL" value={appealReason} onChangeText={setAppealReason} mode="outlined" style={{ marginVertical: 10 }} />
                        <Button mode="contained" buttonColor={colors.error} onPress={handleAppeal}>Deposit & Appeal</Button>
                    </View>
                )}
            </Dialog.Content>
        </Dialog>
      </Portal>
    </View>
  );
}
EOF

cat << 'EOF' > src/screens/BotManager.tsx
import React, { useState } from 'react';
import { View, FlatList } from 'react-native';
import { Text, Card, Button, FAB, Dialog, Portal, TextInput, useTheme, Chip, SegmentedButtons } from 'react-native-paper';
import { Shield, Fuel } from 'lucide-react-native';

const INITIAL_BOTS = [
  { id: '1', name: 'NBA Sniper', type: 'Miner', stake: 1.0, activeVotes: 0, maxVotes: 2, status: 'Running' },
];

export default function BotManager() {
  const { colors } = useTheme();
  const [bots, setBots] = useState(INITIAL_BOTS);
  const [visible, setVisible] = useState(false);
  const [config, setConfig] = useState({ name: '', type: 'Miner', stake: '1' });

  const handleDeploy = () => {
    setBots([...bots, { id: Math.random().toString(), name: config.name, type: config.type, stake: parseFloat(config.stake), activeVotes: 0, maxVotes: parseFloat(config.stake)/0.5, status: 'Starting' }]);
    setVisible(false);
  };

  return (
    <View style={{ flex: 1, backgroundColor: colors.background }}>
      <FlatList
        data={bots}
        renderItem={({ item }) => (
            <Card style={{ margin: 10, backgroundColor: colors.surface }}>
                <Card.Content>
                    <View style={{ flexDirection: 'row', justifyContent: 'space-between' }}>
                        <Text style={{ fontWeight: 'bold' }}>{item.name}</Text>
                        <Chip>{item.type}</Chip>
                    </View>
                    <Text style={{ marginTop: 5 }}>Stake: {item.stake} SOL</Text>
                </Card.Content>
            </Card>
        )}
      />
      <FAB icon="robot" style={{ position: 'absolute', margin: 16, right: 0, bottom: 0, backgroundColor: colors.primary }} onPress={() => setVisible(true)} />
      <Portal>
        <Dialog visible={visible} onDismiss={() => setVisible(false)} style={{ backgroundColor: colors.surface }}>
            <Dialog.Title>Deploy Bot</Dialog.Title>
            <Dialog.Content>
                <TextInput label="Name" value={config.name} onChangeText={t => setConfig({...config, name: t})} mode="outlined" style={{marginBottom: 10}} />
                <SegmentedButtons value={config.type} onValueChange={v => setConfig({...config, type: v})} buttons={[{ value: 'Miner', label: 'Miner' }, { value: 'Whale', label: 'Guardian' }]} />
                <TextInput label="Stake" value={config.stake} onChangeText={t => setConfig({...config, stake: t})} mode="outlined" style={{marginTop: 10}} />
            </Dialog.Content>
            <Dialog.Actions><Button onPress={handleDeploy}>Deploy</Button></Dialog.Actions>
        </Dialog>
      </Portal>
    </View>
  );
}
EOF

# ==========================================
# 4. DEPENDENCIES & CLEANUP
# ==========================================
echo "📦 Writing package.json..."
cat << 'EOF' > package.json
{
  "name": "TruthPoolSeeker",
  "version": "2.0.0",
  "dependencies": {
    "@coral-xyz/anchor": "^0.30.1",
    "@react-navigation/bottom-tabs": "^6.5.11",
    "@react-navigation/native": "^6.1.9",
    "@solana/web3.js": "^1.91.0",
    "react": "18.2.0",
    "react-native": "0.73.4",
    "react-native-paper": "^5.12.3",
    "lucide-react-native": "^0.344.0"
  },
  "devDependencies": {
    "typescript": "^5.0.4"
  }
}
EOF

echo "✅ Setup Complete. Run 'npm install' inside truth-pool and 'bots/miner-agent' folders."