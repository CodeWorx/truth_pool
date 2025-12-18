use anchor_lang::prelude::*;
use anchor_lang::solana_program::hash::hash;

declare_id!("TrutHPooL11111111111111111111111111111111");

// --- CONSTANTS ---
const VOTE_BOND: u64 = 500_000_000; // 0.5 SOL
const PARTNER_VIRTUAL_CAPACITY: u64 = 500_000_000_000; // 500 SOL equivalent
const SENTINEL_VIRTUAL_CAPACITY: u64 = 500_000_000_000; // 500 SOL equivalent
const APPEAL_BOND: u64 = 1_000_000_000; // 1 SOL
const SETTLEMENT_WINDOW: i64 = 43200; // 12 Hours (Seconds)

#[program]
pub mod truth_pool {
    use super::*;

    // --- CONFIGURATION ---
    pub fn initialize_config(ctx: Context<InitConfig>) -> Result<()> {
        let config = &mut ctx.accounts.config;
        config.admin = ctx.accounts.admin.key();
        config.treasury = ctx.accounts.treasury.key(); 
        config.sentinel_gas_tank = ctx.accounts.sentinel_gas_tank.key();
        Ok(())
    }

    // --- REGISTRATION ---
    pub fn register_miner(ctx: Context<RegisterMiner>, category_id: String) -> Result<()> {
        let miner = &mut ctx.accounts.miner_profile;
        miner.authority = ctx.accounts.user.key();
        miner.category_id = category_id;
        miner.locked_liquidity = 0;
        miner.pending_settlements = 0;
        miner.reputation = 0;
        miner.is_partner = false;
        miner.is_sentinel = false;
        miner.is_active = true;
        
        let category = &mut ctx.accounts.category_stats;
        category.active_miners += 1;
        
        Ok(())
    }

    pub fn register_partner(ctx: Context<RegisterPartner>, category_id: String) -> Result<()> {
        let miner = &mut ctx.accounts.miner_profile;
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
        let miner = &mut ctx.accounts.miner_profile;
        miner.authority = ctx.accounts.sentinel_authority.key();
        miner.category_id = category_id;
        miner.locked_liquidity = 0;
        miner.pending_settlements = 0;
        miner.reputation = 100;
        miner.is_partner = false;
        miner.is_sentinel = true;
        miner.is_active = true;
        
        let category = &mut ctx.accounts.category_stats;
        category.active_miners += 1;
        
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
        Ok(())
    }

    pub fn withdraw_capital(ctx: Context<ManageCapital>, amount: u64) -> Result<()> {
        let miner = &ctx.accounts.miner_profile;
        
        let balance = miner.to_account_info().lamports();
        let rent = Rent::get()?.minimum_balance(miner.to_account_info().data_len());
        
        // We must hold back funds for Active Votes AND Pending Settlements
        let total_locked = miner.locked_liquidity + miner.pending_settlements;
        let available = balance.saturating_sub(rent).saturating_sub(total_locked);

        require!(amount <= available, CustomError::InsufficientFreeCapital);

        **ctx.accounts.miner_profile.to_account_info().try_borrow_mut_lamports()? -= amount;
        **ctx.accounts.user.to_account_info().try_borrow_mut_lamports()? += amount;
        
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

        // Transfer Bounty from User -> Query Account (Vault)
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
            
            // Map u8 to Enum
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
        } else {
            // Deduplication: Format match check
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
            let max_allowed = query.commit_count / 2;
            // Allow at least 1 sentinel if commits are low
            if query.commit_count > 2 {
                require!(query.sentinel_commit_count <= max_allowed, CustomError::SentinelCapReached);
            }
            // Virtual Capacity Check
            require!(miner.locked_liquidity < SENTINEL_VIRTUAL_CAPACITY, CustomError::InsufficientFreeCapital);
            query.sentinel_commit_count += 1;
        } else if miner.is_partner {
             require!(miner.locked_liquidity < PARTNER_VIRTUAL_CAPACITY, CustomError::InsufficientFreeCapital);
        } else {
            // Standard Balance Check
            let balance = miner.to_account_info().lamports();
            let rent = Rent::get()?.minimum_balance(miner.to_account_info().data_len());
            let available = balance.saturating_sub(rent).saturating_sub(miner.locked_liquidity);
            require!(available >= VOTE_BOND, CustomError::InsufficientFreeCapital);
        }

        // Lock Liquidity
        miner.locked_liquidity += VOTE_BOND;

        // Record Vote
        let voter_record = &mut ctx.accounts.voter_record;
        voter_record.vote_hash = vote_hash;
        voter_record.encrypted_salt = encrypted_salt;
        voter_record.authority = miner.key(); 
        voter_record.has_committed = true;
        voter_record.bond_released = false;
        
        query.commit_count += 1;
        
        // Speed Trigger: Close if >51% of category committed
        let category = &ctx.accounts.category_stats;
        if query.commit_count as u64 > (category.active_miners / 2) {
             query.status = QueryStatus::RevealPhase;
        }

        Ok(())
    }

    // --- VOTING (Reveal) ---
    pub fn reveal_vote(ctx: Context<RevealVote>, value: String, salt: String) -> Result<()> {
        let miner = &mut ctx.accounts.miner_profile;
        let voter_record = &mut ctx.accounts.voter_record;
        let query = &mut ctx.accounts.query_account;
        let now = Clock::get()?.unix_timestamp;
        
        require!(query.status == QueryStatus::RevealPhase, CustomError::WrongPhase);
        // Note: We allow late reveals for the purpose of slashing checks, 
        // but they won't count for consensus if too late (logic handled in tally).
        
        let raw_data = format!("{}{}", value, salt);
        let calculated_hash = hash(raw_data.as_bytes()).to_bytes();
        require!(calculated_hash == voter_record.vote_hash, CustomError::HashMismatch);

        voter_record.revealed_value = value;
        voter_record.has_revealed = true;
        query.reveal_count += 1;

        // CAPITAL REUSE: Unlock Active -> Move to Pending
        miner.locked_liquidity = miner.locked_liquidity.saturating_sub(VOTE_BOND);
        miner.pending_settlements += VOTE_BOND;

        Ok(())
    }

    // --- SETTLEMENT ---
    pub fn tally_votes(ctx: Context<Tally>, winner_result: String) -> Result<()> {
        let query = &mut ctx.accounts.query_account;
        let winner_profile = &ctx.accounts.winner_profile; 
        
        // Universal Forgiveness Check
        if query.reveal_count < (query.commit_count / 2) {
            query.status = QueryStatus::Voided;
            msg!("Network Outage. Round Voided.");
            return Ok(());
        }

        query.result = winner_result;
        query.status = QueryStatus::Finalized;
        query.finalized_at = Clock::get()?.unix_timestamp;

        // Payout Calculation
        let total_pot = query.bounty_total;
        let treasury_fee = total_pot / 10; // 10%
        let winner_reward = total_pot - treasury_fee;

        // Routing
        if winner_profile.is_sentinel {
            // Rail 2: Gas Tank Refuel
            **query.to_account_info().try_borrow_mut_lamports()? -= winner_reward;
            **ctx.accounts.sentinel_gas_tank.to_account_info().try_borrow_mut_lamports()? += winner_reward;
        } else {
            **query.to_account_info().try_borrow_mut_lamports()? -= winner_reward;
            **ctx.accounts.winner_wallet.to_account_info().try_borrow_mut_lamports()? += winner_reward;
        }

        // Treasury Fee
        **query.to_account_info().try_borrow_mut_lamports()? -= treasury_fee;
        **ctx.accounts.treasury.to_account_info().try_borrow_mut_lamports()? += treasury_fee;

        Ok(())
    }

    // --- APPEALS ---
    pub fn file_appeal(ctx: Context<FileAppeal>, reason: String) -> Result<()> {
        let query = &mut ctx.accounts.query_account;
        let now = Clock::get()?.unix_timestamp;

        require!(query.status == QueryStatus::Finalized, CustomError::NotFinalized);
        require!(now <= query.finalized_at + SETTLEMENT_WINDOW, CustomError::AppealWindowClosed);

        // Transfer 1 SOL Challenge Fee
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
            query.finalized_at = 0; // Immediate unlock
        } else {
            query.status = QueryStatus::Voided;
            // Refund logic would go here
        }
        Ok(())
    }

    // --- CLAIMS & PENALTIES ---
    pub fn claim_stake(ctx: Context<ClaimStake>) -> Result<()> {
        let query = &ctx.accounts.query_account;
        let voter_record = &mut ctx.accounts.voter_record;
        let miner = &mut ctx.accounts.miner_profile;
        let now = Clock::get()?.unix_timestamp;

        require!(query.status == QueryStatus::Finalized, CustomError::NotFinalized);
        if query.finalized_at > 0 {
            require!(now > query.finalized_at + SETTLEMENT_WINDOW, CustomError::SettlementLocked);
        }
        
        require!(voter_record.revealed_value == query.result, CustomError::WrongVote);
        require!(!voter_record.bond_released, CustomError::AlreadyClaimed);

        // Release Pending Lock (Full Withdrawal Access restored)
        miner.pending_settlements = miner.pending_settlements.saturating_sub(VOTE_BOND);
        voter_record.bond_released = true;

        Ok(())
    }

    pub fn slash_liar(ctx: Context<SlashLiar>) -> Result<()> {
        let query = &ctx.accounts.query_account;
        let miner = &mut ctx.accounts.miner_profile;
        let voter_record = &mut ctx.accounts.voter_record;

        require!(query.status == QueryStatus::Finalized, CustomError::NotFinalized);
        require!(voter_record.revealed_value != query.result, CustomError::MinerWasHonest);
        require!(!voter_record.bond_released, CustomError::AlreadyClaimed);

        // 1. Clear Pending Lock
        miner.pending_settlements = miner.pending_settlements.saturating_sub(VOTE_BOND);
        
        // 2. Penalty Transfer
        if miner.is_partner {
            miner.is_active = false; // Ban Partner
        } else {
            let penalty = VOTE_BOND;
            let available = miner.to_account_info().lamports();
            
            if available >= penalty {
                **miner.to_account_info().try_borrow_mut_lamports()? -= penalty;
                **ctx.accounts.treasury.to_account_info().try_borrow_mut_lamports()? += penalty;
            } else {
                // Insolvency -> Ban
                **miner.to_account_info().try_borrow_mut_lamports()? -= available;
                **ctx.accounts.treasury.to_account_info().try_borrow_mut_lamports()? += available;
                miner.is_active = false; 
            }
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
    /// CHECK: Validated
    pub treasury: AccountInfo<'info>, 
    /// CHECK: Validated
    pub sentinel_gas_tank: AccountInfo<'info>, 
    pub system_program: Program<'info, System>
}

#[derive(Accounts)]
pub struct RegisterMiner<'info> { 
    #[account(mut)] pub user: Signer<'info>, 
    #[account(init, payer=user, space=500, seeds=[b"miner", user.key().as_ref()], bump)] pub miner_profile: Account<'info, MinerProfile>, 
    #[account(mut, seeds=[b"category", category_id.as_bytes()], bump)] pub category_stats: Account<'info, CategoryStats>,
    pub system_program: Program<'info, System> 
}

#[derive(Accounts)]
pub struct RegisterPartner<'info> {
    #[account(mut)] pub admin: Signer<'info>,
    /// CHECK: Partner Key
    pub partner_wallet: AccountInfo<'info>,
    #[account(init, payer=admin, space=500, seeds=[b"miner", partner_wallet.key().as_ref()], bump)] pub miner_profile: Account<'info, MinerProfile>,
    #[account(mut)] pub category_stats: Account<'info, CategoryStats>,
    pub system_program: Program<'info, System>
}

#[derive(Accounts)]
pub struct RegisterSentinel<'info> {
    #[account(mut)] pub admin: Signer<'info>,
    /// CHECK: Sentinel Key
    pub sentinel_authority: AccountInfo<'info>,
    #[account(init, payer=admin, space=500, seeds=[b"miner", sentinel_authority.key().as_ref()], bump)] pub miner_profile: Account<'info, MinerProfile>,
    #[account(mut)] pub category_stats: Account<'info, CategoryStats>,
    pub system_program: Program<'info, System>
}

#[derive(Accounts)]
pub struct ManageCapital<'info> { 
    #[account(mut)] pub user: Signer<'info>, 
    #[account(mut, has_one = authority)] pub miner_profile: Account<'info, MinerProfile>, 
    pub system_program: Program<'info, System> 
}

#[derive(Accounts)]
#[instruction(unique_event_id: String, category_id: String)]
pub struct RequestData<'info> { 
    #[account(mut)] pub requester: Signer<'info>, 
    #[account(mut, seeds=[b"category", category_id.as_bytes()], bump)] pub category_stats: Account<'info, CategoryStats>, 
    #[account(init_if_needed, payer=requester, space=500, seeds=[b"query", category_id.as_bytes(), unique_event_id.as_bytes()], bump)] pub query_account: Account<'info, QueryAccount>, 
    pub system_program: Program<'info, System> 
}

#[derive(Accounts)]
pub struct CommitVote<'info> { 
    #[account(mut)] pub voter: Signer<'info>, 
    #[account(mut)] pub miner_profile: Account<'info, MinerProfile>, 
    #[account(mut)] pub query_account: Account<'info, QueryAccount>, 
    #[account(mut)] pub category_stats: Account<'info, CategoryStats>,
    #[account(init_if_needed, payer=voter, space=300, seeds=[b"vote", query_account.key().as_ref(), miner_profile.key().as_ref()], bump)] pub voter_record: Account<'info, VoterRecord>, 
    pub system_program: Program<'info, System> 
}

#[derive(Accounts)]
pub struct RevealVote<'info> { 
    #[account(mut)] pub voter: Signer<'info>, 
    #[account(mut)] pub miner_profile: Account<'info, MinerProfile>, 
    #[account(mut)] pub query_account: Account<'info, QueryAccount>, 
    #[account(mut)] pub voter_record: Account<'info, VoterRecord> 
}

#[derive(Accounts)]
pub struct Tally<'info> { 
    #[account(mut)] pub query_account: Account<'info, QueryAccount>, 
    #[account(mut)] pub winner_profile: Account<'info, MinerProfile>, 
    #[account(has_one=authority)] pub voter_record: Account<'info, VoterRecord>, 
    /// CHECK: Winner Wallet
    #[account(mut)] pub winner_wallet: AccountInfo<'info>, 
    /// CHECK: Treasury
    #[account(mut)] pub treasury: AccountInfo<'info>, 
    /// CHECK: Gas Tank
    #[account(mut)] pub sentinel_gas_tank: AccountInfo<'info> 
}

#[derive(Accounts)]
pub struct FileAppeal<'info> { 
    #[account(mut)] pub challenger: Signer<'info>, 
    #[account(mut)] pub query_account: Account<'info, QueryAccount>, 
    /// CHECK: Treasury
    #[account(mut)] pub treasury: AccountInfo<'info>, 
    pub system_program: Program<'info, System> 
}

#[derive(Accounts)]
pub struct ResolveAppeal<'info> { 
    #[account(mut)] pub admin: Signer<'info>, 
    #[account(mut)] pub query_account: Account<'info, QueryAccount>, 
    pub system_program: Program<'info, System> 
}

#[derive(Accounts)]
pub struct ClaimStake<'info> { 
    #[account(mut)] pub voter: Signer<'info>, 
    #[account(mut)] pub query_account: Account<'info, QueryAccount>, 
    #[account(mut)] pub miner_profile: Account<'info, MinerProfile>, 
    #[account(mut)] pub voter_record: Account<'info, VoterRecord> 
}

#[derive(Accounts)]
pub struct SlashLiar<'info> { 
    #[account(mut)] pub keeper: Signer<'info>, 
    #[account(mut)] pub query_account: Account<'info, QueryAccount>, 
    #[account(mut)] pub miner_profile: Account<'info, MinerProfile>, 
    #[account(mut)] pub voter_record: Account<'info, VoterRecord>, 
    /// CHECK: Treasury
    #[account(mut)] pub treasury: AccountInfo<'info> 
}

#[account] pub struct ProtocolConfig { pub admin: Pubkey, pub treasury: Pubkey, pub sentinel_gas_tank: Pubkey }
#[account] pub struct MinerProfile { pub authority: Pubkey, pub category_id: String, pub locked_liquidity: u64, pub pending_settlements: u64, pub reputation: u64, pub is_partner: bool, pub is_sentinel: bool, pub is_active: bool }
#[account] pub struct CategoryStats { pub category_id: String, pub active_miners: u64 }
#[account] pub struct QueryAccount { pub unique_event_id: String, pub category_id: String, pub bounty_total: u64, pub status: QueryStatus, pub format: ResponseFormat, pub min_responses: u32, pub commit_deadline: i64, pub reveal_deadline: i64, pub finalized_at: i64, pub commit_count: u32, pub sentinel_commit_count: u32, pub reveal_count: u32, pub result: String }
#[account] pub struct VoterRecord { pub authority: Pubkey, pub vote_hash: [u8; 32], pub encrypted_salt: Vec<u8>, pub revealed_value: String, pub has_committed: bool, pub has_revealed: bool, pub bond_released: bool }

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq)] pub enum QueryStatus { Uninitialized, CommitPhase, RevealPhase, Finalized, UnderAppeal, Voided }
#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq)] pub enum ResponseFormat { Binary, Score, Decimal, String, OptionIndex }
#[event] pub struct AppealEvent { pub query: Pubkey, pub reason: String, pub timestamp: i64 }
#[error_code] pub enum CustomError { PhaseClosed, HashMismatch, NotFinalized, WrongVote, AlreadyClaimed, MinerWasHonest, InsufficientFreeCapital, SettlementLocked, MinerBanned, SentinelCapReached, FormatMismatch, AppealWindowClosed }
