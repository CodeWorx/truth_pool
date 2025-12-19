use anchor_lang::prelude::*;
use anchor_lang::solana_program::keccak;

declare_id!("TrutHPooL11111111111111111111111111111111");

// --- CONSTANTS ---
const VOTE_BOND: u64 = 500_000_000; // 0.5 SOL
const PARTNER_VIRTUAL_CAPACITY: u64 = 500_000_000_000; // 500 SOL equivalent
const SENTINEL_VIRTUAL_CAPACITY: u64 = 500_000_000_000; // 500 SOL equivalent
const APPEAL_BOND: u64 = 1_000_000_000; // 1 SOL
const SETTLEMENT_WINDOW: i64 = 43200; // 12 Hours
const MAX_SENTINELS: u32 = 100; // Hard cap on protocol nodes
const COMMIT_DURATION: i64 = 600; // 10 mins
const REVEAL_DURATION: i64 = 600; // 10 mins after commit ends

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

    // --- CATEGORY INITIALIZATION (NEW) ---
    pub fn initialize_category(ctx: Context<InitCategory>, category_id: String) -> Result<()> {
        require!(category_id.len() <= 32, CustomError::CategoryIdTooLong);

        let category = &mut ctx.accounts.category_stats;
        category.category_id = category_id;
        category.active_miners = 0;
        Ok(())
    }

    // --- REGISTRATION ---
    pub fn register_miner(ctx: Context<RegisterMiner>, category_id: String) -> Result<()> {
        require!(category_id.len() <= 32, CustomError::CategoryIdTooLong);

        let miner = &mut ctx.accounts.miner_profile;
        let category = &mut ctx.accounts.category_stats;

        // Verify category matches
        require!(category.category_id == category_id, CustomError::CategoryMismatch);

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
        require!(category_id.len() <= 32, CustomError::CategoryIdTooLong);

        let config = &ctx.accounts.config;
        require!(ctx.accounts.admin.key() == config.admin, CustomError::Unauthorized);

        let miner = &mut ctx.accounts.miner_profile;
        let category = &mut ctx.accounts.category_stats;

        miner.authority = ctx.accounts.partner_wallet.key();
        miner.category_id = category_id;
        miner.locked_liquidity = 0;
        miner.pending_settlements = 0;
        miner.reputation = 100;
        miner.is_partner = true;
        miner.is_sentinel = false;
        miner.is_active = true;

        category.active_miners += 1;

        Ok(())
    }

    pub fn register_sentinel(ctx: Context<RegisterSentinel>, category_id: String) -> Result<()> {
        require!(category_id.len() <= 32, CustomError::CategoryIdTooLong);

        let config = &mut ctx.accounts.config;
        require!(ctx.accounts.admin.key() == config.admin, CustomError::Unauthorized);
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

        Ok(())
    }

    // --- CAPITAL MANAGEMENT ---
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
        require!(unique_event_id.len() <= 64, CustomError::EventIdTooLong);
        require!(category_id.len() <= 32, CustomError::CategoryIdTooLong);

        let query = &mut ctx.accounts.query_account;
        let category = &ctx.accounts.category_stats;

        // Check if query is already resolved
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
            let network_floor: u32 = 100;
            let active_floor = ((category.active_miners + 1) / 2) as u32;
            query.min_responses = if active_floor > network_floor {
                active_floor
            } else {
                network_floor
            };

            let now = Clock::get()?.unix_timestamp;
            query.commit_deadline = now + COMMIT_DURATION;
            query.reveal_deadline = now + COMMIT_DURATION + REVEAL_DURATION;
            query.commit_count = 0;
            query.reveal_count = 0;
            query.sentinel_commit_count = 0;
            query.sentinel_reveal_count = 0;
            query.random_accumulator = [0u8; 32];
            query.finalized_at = 0;
            query.result = String::new();
            query.winning_ticket_id = 0;

            // Init VoteStats
            let stats = &mut ctx.accounts.vote_stats;
            stats.query_key = query.key();
            stats.options = Vec::new();
        } else {
            // Deduplication - adding to existing bounty
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

    // --- PHASE TRANSITION (NEW) ---
    pub fn advance_to_reveal(ctx: Context<AdvancePhase>) -> Result<()> {
        let query = &mut ctx.accounts.query_account;
        let now = Clock::get()?.unix_timestamp;

        require!(query.status == QueryStatus::CommitPhase, CustomError::WrongPhase);
        require!(now > query.commit_deadline, CustomError::CommitWindowOpen);

        query.status = QueryStatus::RevealPhase;
        msg!("Advanced to RevealPhase");
        Ok(())
    }

    // --- VOTING (Commit) ---
    // FIXED: vote_hash is now [u8; 32] raw bytes from keccak256
    pub fn commit_vote(
        ctx: Context<CommitVote>,
        vote_hash: [u8; 32],
        encrypted_salt: Vec<u8>,
    ) -> Result<()> {
        let miner = &mut ctx.accounts.miner_profile;
        let query = &mut ctx.accounts.query_account;
        let now = Clock::get()?.unix_timestamp;

        require!(miner.is_active, CustomError::MinerBanned);
        require!(query.status == QueryStatus::CommitPhase, CustomError::WrongPhase);
        require!(now <= query.commit_deadline, CustomError::PhaseClosed);

        // FIXED: Sentinel cap check BEFORE incrementing
        if miner.is_sentinel {
            // Cap Sentinels at 49% of TOTAL commits (checked before adding)
            if query.commit_count > 2 {
                let max_allowed = (query.commit_count - 1) / 2; // -1 because we haven't added yet
                require!(
                    query.sentinel_commit_count < max_allowed,
                    CustomError::SentinelCapReached
                );
            }
            // Virtual capacity for sentinels
            require!(
                miner.locked_liquidity < SENTINEL_VIRTUAL_CAPACITY,
                CustomError::InsufficientFreeCapital
            );
        } else if miner.is_partner {
            // Virtual capacity for partners
            require!(
                miner.locked_liquidity < PARTNER_VIRTUAL_CAPACITY,
                CustomError::InsufficientFreeCapital
            );
        } else {
            // Standard miner - real capital check
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
        voter_record.authority = miner.authority; // Store the actual authority, not miner PDA
        voter_record.miner_profile = miner.key();
        voter_record.has_committed = true;
        voter_record.has_revealed = false;
        voter_record.bond_released = false;
        voter_record.ticket_id = 0;
        voter_record.revealed_value = String::new();

        query.commit_count += 1;
        if miner.is_sentinel {
            query.sentinel_commit_count += 1;
        }

        // REMOVED: Speed trigger that caused race conditions
        // Phase transition now handled by advance_to_reveal or deadline

        emit!(VoteEvent {
            query: query.key(),
            voter: miner.key(),
            phase: VotePhase::Commit
        });
        Ok(())
    }

    // --- VOTING (Reveal) ---
    // FIXED: Uses keccak256 with raw bytes for hash verification
    pub fn reveal_vote(ctx: Context<RevealVote>, value: String, salt: String) -> Result<()> {
        let miner = &mut ctx.accounts.miner_profile;
        let voter_record = &mut ctx.accounts.voter_record;
        let query = &mut ctx.accounts.query_account;
        let stats = &mut ctx.accounts.vote_stats;
        let now = Clock::get()?.unix_timestamp;

        require!(query.status == QueryStatus::RevealPhase, CustomError::WrongPhase);
        require!(now <= query.reveal_deadline, CustomError::PhaseClosed);
        require!(voter_record.has_committed, CustomError::NotCommitted);
        require!(!voter_record.has_revealed, CustomError::AlreadyRevealed);

        // FIXED: Hash verification using keccak256 with consistent encoding
        // Format: keccak256(value_bytes || salt_bytes)
        let mut preimage = Vec::new();
        preimage.extend_from_slice(value.as_bytes());
        preimage.extend_from_slice(salt.as_bytes());
        let calculated_hash = keccak::hash(&preimage).to_bytes();

        require!(calculated_hash == voter_record.vote_hash, CustomError::HashMismatch);

        // XOR Accumulator for trustless randomness
        let salt_hash = keccak::hash(salt.as_bytes()).to_bytes();
        for i in 0..32 {
            query.random_accumulator[i] ^= salt_hash[i];
        }

        // Update Vote Statistics
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
            require!(stats.options.len() < 50, CustomError::TooManyOptions);
            stats.options.push(VoteOptionSimple {
                value: value.clone(),
                count: 1,
            });
            voter_record.ticket_id = 1;
        }

        voter_record.revealed_value = value;
        voter_record.has_revealed = true;
        query.reveal_count += 1;

        if miner.is_sentinel {
            query.sentinel_reveal_count += 1;
        }

        // Capital reuse: Unlock Active -> Move to Pending
        miner.locked_liquidity = miner.locked_liquidity.saturating_sub(VOTE_BOND);
        miner.pending_settlements += VOTE_BOND;

        emit!(VoteEvent {
            query: query.key(),
            voter: miner.key(),
            phase: VotePhase::Reveal
        });
        Ok(())
    }

    // --- TALLY ---
    pub fn tally_votes(ctx: Context<Tally>) -> Result<()> {
        let query = &mut ctx.accounts.query_account;
        let stats = &ctx.accounts.vote_stats;
        let now = Clock::get()?.unix_timestamp;

        require!(query.status == QueryStatus::RevealPhase, CustomError::WrongPhase);
        require!(now > query.reveal_deadline, CustomError::RevealWindowOpen);

        // Sentinel Reveal Cap Check
        if query.reveal_count > 0 {
            let max_sentinel_ratio = query.reveal_count / 2;
            if query.sentinel_reveal_count > max_sentinel_ratio {
                query.status = QueryStatus::InDispute;
                msg!("Sentinel Dominance Detected (>50%). Sent to Dispute.");
                return Ok(());
            }
        }

        // Universal Forgiveness Check
        if query.reveal_count < (query.commit_count / 2) {
            query.status = QueryStatus::Voided;
            msg!("Network Outage. Round Voided.");
            return Ok(());
        }

        // Determine Winner
        let mut winner_value = String::new();
        let mut max_votes: u32 = 0;
        let mut total_valid: u32 = 0;

        for opt in stats.options.iter() {
            total_valid += opt.count;
            if opt.count > max_votes {
                max_votes = opt.count;
                winner_value = opt.value.clone();
            }
        }

        // Consensus Checks
        if total_valid < query.min_responses {
            query.status = QueryStatus::InDispute;
            msg!("Insufficient responses");
            return Ok(());
        }

        let consensus_pct = (max_votes as u64 * 100) / (total_valid as u64);
        if consensus_pct < 66 {
            query.status = QueryStatus::InDispute;
            msg!("No supermajority");
            return Ok(());
        }

        // FIXED: Trustless Lottery using XOR Accumulator
        let final_entropy = query.random_accumulator;
        let random_u64 = u64::from_le_bytes(final_entropy[0..8].try_into().unwrap());
        let winning_ticket = (random_u64 % (max_votes as u64)) + 1;

        query.result = winner_value;
        query.winning_ticket_id = winning_ticket as u32;
        query.status = QueryStatus::Finalized;
        query.finalized_at = now;

        msg!("Winner: {}, Ticket: {}", query.result, query.winning_ticket_id);
        Ok(())
    }

    // --- CLAIMS ---
    pub fn claim_stake(ctx: Context<ClaimStake>) -> Result<()> {
        let query = &ctx.accounts.query_account;
        let voter_record = &mut ctx.accounts.voter_record;
        let miner = &mut ctx.accounts.miner_profile;
        let config = &ctx.accounts.config;
        let now = Clock::get()?.unix_timestamp;

        require!(
            query.status == QueryStatus::Finalized,
            CustomError::NotFinalized
        );
        if query.finalized_at > 0 {
            require!(
                now > query.finalized_at + SETTLEMENT_WINDOW,
                CustomError::SettlementLocked
            );
        }

        require!(voter_record.revealed_value == query.result, CustomError::WrongVote);
        require!(!voter_record.bond_released, CustomError::AlreadyClaimed);

        // Release pending settlement
        miner.pending_settlements = miner.pending_settlements.saturating_sub(VOTE_BOND);
        voter_record.bond_released = true;

        // Check if this voter won the lottery
        if voter_record.ticket_id == query.winning_ticket_id {
            let bounty = query.bounty_total;
            let treasury_fee = bounty / 10; // 10% fee
            let winner_share = bounty - treasury_fee;

            // FIXED: Verify treasury matches config
            require!(
                ctx.accounts.treasury.key() == config.treasury,
                CustomError::InvalidTreasury
            );

            if miner.is_sentinel {
                // Sentinel winnings go to gas tank
                require!(
                    ctx.accounts.sentinel_gas_tank.key() == config.sentinel_gas_tank,
                    CustomError::InvalidGasTank
                );
                **ctx.accounts.query_account.to_account_info().try_borrow_mut_lamports()? -= winner_share;
                **ctx.accounts.sentinel_gas_tank.to_account_info().try_borrow_mut_lamports()? += winner_share;
            } else {
                // FIXED: Winner wallet must match voter's authority
                require!(
                    ctx.accounts.winner_wallet.key() == voter_record.authority,
                    CustomError::InvalidWinnerWallet
                );
                **ctx.accounts.query_account.to_account_info().try_borrow_mut_lamports()? -= winner_share;
                **ctx.accounts.winner_wallet.to_account_info().try_borrow_mut_lamports()? += winner_share;
            }

            // Treasury fee
            **ctx.accounts.query_account.to_account_info().try_borrow_mut_lamports()? -= treasury_fee;
            **ctx.accounts.treasury.to_account_info().try_borrow_mut_lamports()? += treasury_fee;

            emit!(ClaimEvent {
                query: query.key(),
                winner: miner.key(),
                amount: winner_share
            });
        }

        Ok(())
    }

    // --- VOIDED ROUND RECOVERY (NEW) ---
    pub fn recover_from_void(ctx: Context<RecoverVoid>) -> Result<()> {
        let query = &ctx.accounts.query_account;
        let voter_record = &mut ctx.accounts.voter_record;
        let miner = &mut ctx.accounts.miner_profile;

        require!(query.status == QueryStatus::Voided, CustomError::NotVoided);
        require!(!voter_record.bond_released, CustomError::AlreadyClaimed);

        // Release all locked funds
        if voter_record.has_revealed {
            miner.pending_settlements = miner.pending_settlements.saturating_sub(VOTE_BOND);
        } else if voter_record.has_committed {
            miner.locked_liquidity = miner.locked_liquidity.saturating_sub(VOTE_BOND);
        }

        voter_record.bond_released = true;
        Ok(())
    }

    // --- APPEALS ---
    pub fn file_appeal(ctx: Context<FileAppeal>, reason: String) -> Result<()> {
        let query = &mut ctx.accounts.query_account;
        let config = &ctx.accounts.config;
        let now = Clock::get()?.unix_timestamp;

        require!(query.status == QueryStatus::Finalized, CustomError::NotFinalized);
        require!(now <= query.finalized_at + SETTLEMENT_WINDOW, CustomError::AppealWindowClosed);

        // FIXED: Verify treasury
        require!(
            ctx.accounts.treasury.key() == config.treasury,
            CustomError::InvalidTreasury
        );

        let cpi_context = CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            anchor_lang::system_program::Transfer {
                from: ctx.accounts.challenger.to_account_info(),
                to: ctx.accounts.treasury.to_account_info(),
            },
        );
        anchor_lang::system_program::transfer(cpi_context, APPEAL_BOND)?;

        query.status = QueryStatus::UnderAppeal;
        emit!(AppealEvent {
            query: query.key(),
            reason,
            timestamp: now
        });
        Ok(())
    }

    pub fn resolve_appeal(ctx: Context<ResolveAppeal>, uphold_result: bool) -> Result<()> {
        let query = &mut ctx.accounts.query_account;
        let config = &ctx.accounts.config;

        require!(ctx.accounts.admin.key() == config.admin, CustomError::Unauthorized);

        if uphold_result {
            query.status = QueryStatus::Finalized;
            query.finalized_at = Clock::get()?.unix_timestamp; // Reset settlement window
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
        let config = &ctx.accounts.config;

        require!(query.status == QueryStatus::Finalized, CustomError::NotFinalized);
        require!(voter_record.has_revealed, CustomError::NotRevealed);
        require!(voter_record.revealed_value != query.result, CustomError::MinerWasHonest);
        require!(!voter_record.bond_released, CustomError::AlreadyClaimed);

        // FIXED: Verify treasury
        require!(
            ctx.accounts.treasury.key() == config.treasury,
            CustomError::InvalidTreasury
        );

        miner.pending_settlements = miner.pending_settlements.saturating_sub(VOTE_BOND);

        if !miner.is_partner && !miner.is_sentinel {
            let available = miner.to_account_info().lamports();
            let rent = Rent::get()?.minimum_balance(miner.to_account_info().data_len());

            if available > rent + VOTE_BOND {
                **miner.to_account_info().try_borrow_mut_lamports()? -= VOTE_BOND;
                **ctx.accounts.treasury.to_account_info().try_borrow_mut_lamports()? += VOTE_BOND;
                emit!(CapitalEvent {
                    user: miner.key(),
                    amount: VOTE_BOND,
                    action: CapitalAction::Slash
                });
            } else {
                miner.is_active = false; // Banned - insufficient funds
            }
        } else {
            // Partners/Sentinels get banned, not slashed
            miner.is_active = false;
        }

        voter_record.bond_released = true;
        miner.reputation = miner.reputation.saturating_sub(10);

        Ok(())
    }

    // --- SLASH NON-REVEALER (NEW) ---
    pub fn slash_non_revealer(ctx: Context<SlashNonRevealer>) -> Result<()> {
        let miner = &mut ctx.accounts.miner_profile;
        let voter_record = &mut ctx.accounts.voter_record;
        let query = &ctx.accounts.query_account;
        let config = &ctx.accounts.config;
        let now = Clock::get()?.unix_timestamp;

        // Can only slash after reveal deadline
        require!(now > query.reveal_deadline, CustomError::RevealWindowOpen);
        require!(voter_record.has_committed, CustomError::NotCommitted);
        require!(!voter_record.has_revealed, CustomError::AlreadyRevealed);
        require!(!voter_record.bond_released, CustomError::AlreadyClaimed);

        require!(
            ctx.accounts.treasury.key() == config.treasury,
            CustomError::InvalidTreasury
        );

        // Release from locked (they never moved to pending since they didn't reveal)
        miner.locked_liquidity = miner.locked_liquidity.saturating_sub(VOTE_BOND);

        if !miner.is_partner && !miner.is_sentinel {
            let available = miner.to_account_info().lamports();
            let rent = Rent::get()?.minimum_balance(miner.to_account_info().data_len());

            if available > rent + VOTE_BOND {
                **miner.to_account_info().try_borrow_mut_lamports()? -= VOTE_BOND;
                **ctx.accounts.treasury.to_account_info().try_borrow_mut_lamports()? += VOTE_BOND;
            } else {
                miner.is_active = false;
            }
        }

        voter_record.bond_released = true;
        Ok(())
    }
}

// ============================================
// ACCOUNT CONTEXTS
// ============================================

#[derive(Accounts)]
pub struct InitConfig<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(
        init,
        payer = admin,
        space = 8 + ProtocolConfig::INIT_SPACE,
        seeds = [b"config"],
        bump
    )]
    pub config: Account<'info, ProtocolConfig>,
    /// CHECK: Treasury address, validated by admin
    pub treasury: AccountInfo<'info>,
    /// CHECK: Sentinel gas tank address, validated by admin
    pub sentinel_gas_tank: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(category_id: String)]
pub struct InitCategory<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(seeds = [b"config"], bump)]
    pub config: Account<'info, ProtocolConfig>,
    #[account(
        init,
        payer = admin,
        space = 8 + CategoryStats::INIT_SPACE,
        seeds = [b"category", category_id.as_bytes()],
        bump
    )]
    pub category_stats: Account<'info, CategoryStats>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(category_id: String)]
pub struct RegisterMiner<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(
        init,
        payer = user,
        space = 8 + MinerProfile::INIT_SPACE,
        seeds = [b"miner", user.key().as_ref()],
        bump
    )]
    pub miner_profile: Account<'info, MinerProfile>,
    #[account(
        mut,
        seeds = [b"category", category_id.as_bytes()],
        bump
    )]
    pub category_stats: Account<'info, CategoryStats>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(category_id: String)]
pub struct RegisterPartner<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(seeds = [b"config"], bump)]
    pub config: Account<'info, ProtocolConfig>,
    /// CHECK: Partner wallet address
    pub partner_wallet: AccountInfo<'info>,
    #[account(
        init,
        payer = admin,
        space = 8 + MinerProfile::INIT_SPACE,
        seeds = [b"miner", partner_wallet.key().as_ref()],
        bump
    )]
    pub miner_profile: Account<'info, MinerProfile>,
    #[account(
        mut,
        seeds = [b"category", category_id.as_bytes()],
        bump
    )]
    pub category_stats: Account<'info, CategoryStats>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(category_id: String)]
pub struct RegisterSentinel<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(mut, seeds = [b"config"], bump)]
    pub config: Account<'info, ProtocolConfig>,
    /// CHECK: Sentinel authority address
    pub sentinel_authority: AccountInfo<'info>,
    #[account(
        init,
        payer = admin,
        space = 8 + MinerProfile::INIT_SPACE,
        seeds = [b"miner", sentinel_authority.key().as_ref()],
        bump
    )]
    pub miner_profile: Account<'info, MinerProfile>,
    #[account(
        mut,
        seeds = [b"category", category_id.as_bytes()],
        bump
    )]
    pub category_stats: Account<'info, CategoryStats>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ManageCapital<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(
        mut,
        has_one = authority,
        seeds = [b"miner", user.key().as_ref()],
        bump
    )]
    pub miner_profile: Account<'info, MinerProfile>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(unique_event_id: String, category_id: String)]
pub struct RequestData<'info> {
    #[account(mut)]
    pub requester: Signer<'info>,
    #[account(
        seeds = [b"category", category_id.as_bytes()],
        bump
    )]
    pub category_stats: Account<'info, CategoryStats>,
    #[account(
        init_if_needed,
        payer = requester,
        space = 8 + QueryAccount::INIT_SPACE,
        seeds = [b"query", category_id.as_bytes(), unique_event_id.as_bytes()],
        bump
    )]
    pub query_account: Account<'info, QueryAccount>,
    #[account(
        init_if_needed,
        payer = requester,
        space = 8 + 32 + 4 + (4 + 64 + 4) * 50, // ~3500 bytes for 50 options
        seeds = [b"stats", query_account.key().as_ref()],
        bump
    )]
    pub vote_stats: Account<'info, VoteStatsSafe>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct AdvancePhase<'info> {
    #[account(mut)]
    pub query_account: Account<'info, QueryAccount>,
}

#[derive(Accounts)]
pub struct CommitVote<'info> {
    #[account(mut)]
    pub voter: Signer<'info>,
    #[account(
        mut,
        has_one = authority,  // FIXED: Now requires authority match
        seeds = [b"miner", voter.key().as_ref()],
        bump
    )]
    pub miner_profile: Account<'info, MinerProfile>,
    #[account(mut)]
    pub query_account: Account<'info, QueryAccount>,
    #[account(
        seeds = [b"category", query_account.category_id.as_bytes()],
        bump
    )]
    pub category_stats: Account<'info, CategoryStats>,
    #[account(
        init_if_needed,
        payer = voter,
        space = 8 + VoterRecord::INIT_SPACE,
        seeds = [b"vote", query_account.key().as_ref(), miner_profile.key().as_ref()],
        bump
    )]
    pub voter_record: Account<'info, VoterRecord>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RevealVote<'info> {
    #[account(mut)]
    pub voter: Signer<'info>,
    #[account(
        mut,
        has_one = authority,  // FIXED: Now requires authority match
        seeds = [b"miner", voter.key().as_ref()],
        bump
    )]
    pub miner_profile: Account<'info, MinerProfile>,
    #[account(mut)]
    pub query_account: Account<'info, QueryAccount>,
    #[account(
        mut,
        seeds = [b"vote", query_account.key().as_ref(), miner_profile.key().as_ref()],
        bump
    )]
    pub voter_record: Account<'info, VoterRecord>,
    #[account(
        mut,
        seeds = [b"stats", query_account.key().as_ref()],
        bump
    )]
    pub vote_stats: Account<'info, VoteStatsSafe>,
}

#[derive(Accounts)]
pub struct Tally<'info> {
    #[account(mut)]
    pub query_account: Account<'info, QueryAccount>,
    #[account(
        seeds = [b"stats", query_account.key().as_ref()],
        bump
    )]
    pub vote_stats: Account<'info, VoteStatsSafe>,
}

#[derive(Accounts)]
pub struct ClaimStake<'info> {
    #[account(mut)]
    pub voter: Signer<'info>,
    #[account(seeds = [b"config"], bump)]
    pub config: Account<'info, ProtocolConfig>,
    #[account(mut)]
    pub query_account: Account<'info, QueryAccount>,
    #[account(
        mut,
        has_one = authority,
        seeds = [b"miner", voter.key().as_ref()],
        bump
    )]
    pub miner_profile: Account<'info, MinerProfile>,
    #[account(
        mut,
        seeds = [b"vote", query_account.key().as_ref(), miner_profile.key().as_ref()],
        bump
    )]
    pub voter_record: Account<'info, VoterRecord>,
    /// CHECK: Validated against config.treasury
    #[account(mut)]
    pub treasury: AccountInfo<'info>,
    /// CHECK: Validated against config.sentinel_gas_tank
    #[account(mut)]
    pub sentinel_gas_tank: AccountInfo<'info>,
    /// CHECK: Validated against voter_record.authority
    #[account(mut)]
    pub winner_wallet: AccountInfo<'info>,
}

#[derive(Accounts)]
pub struct RecoverVoid<'info> {
    #[account(mut)]
    pub voter: Signer<'info>,
    pub query_account: Account<'info, QueryAccount>,
    #[account(
        mut,
        has_one = authority,
        seeds = [b"miner", voter.key().as_ref()],
        bump
    )]
    pub miner_profile: Account<'info, MinerProfile>,
    #[account(
        mut,
        seeds = [b"vote", query_account.key().as_ref(), miner_profile.key().as_ref()],
        bump
    )]
    pub voter_record: Account<'info, VoterRecord>,
}

#[derive(Accounts)]
pub struct FileAppeal<'info> {
    #[account(mut)]
    pub challenger: Signer<'info>,
    #[account(seeds = [b"config"], bump)]
    pub config: Account<'info, ProtocolConfig>,
    #[account(mut)]
    pub query_account: Account<'info, QueryAccount>,
    /// CHECK: Validated against config.treasury
    #[account(mut)]
    pub treasury: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ResolveAppeal<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(seeds = [b"config"], bump)]
    pub config: Account<'info, ProtocolConfig>,
    #[account(mut)]
    pub query_account: Account<'info, QueryAccount>,
}

#[derive(Accounts)]
pub struct SlashLiar<'info> {
    #[account(mut)]
    pub keeper: Signer<'info>,
    #[account(seeds = [b"config"], bump)]
    pub config: Account<'info, ProtocolConfig>,
    pub query_account: Account<'info, QueryAccount>,
    #[account(mut)]
    pub miner_profile: Account<'info, MinerProfile>,
    #[account(
        mut,
        constraint = voter_record.miner_profile == miner_profile.key()
    )]
    pub voter_record: Account<'info, VoterRecord>,
    /// CHECK: Validated against config.treasury
    #[account(mut)]
    pub treasury: AccountInfo<'info>,
}

#[derive(Accounts)]
pub struct SlashNonRevealer<'info> {
    #[account(mut)]
    pub keeper: Signer<'info>,
    #[account(seeds = [b"config"], bump)]
    pub config: Account<'info, ProtocolConfig>,
    pub query_account: Account<'info, QueryAccount>,
    #[account(mut)]
    pub miner_profile: Account<'info, MinerProfile>,
    #[account(
        mut,
        constraint = voter_record.miner_profile == miner_profile.key()
    )]
    pub voter_record: Account<'info, VoterRecord>,
    /// CHECK: Validated against config.treasury
    #[account(mut)]
    pub treasury: AccountInfo<'info>,
}

// ============================================
// DATA STRUCTURES
// ============================================

#[account]
#[derive(InitSpace)]
pub struct ProtocolConfig {
    pub admin: Pubkey,
    pub treasury: Pubkey,
    pub sentinel_gas_tank: Pubkey,
    pub sentinel_count: u32,
}

#[account]
#[derive(InitSpace)]
pub struct MinerProfile {
    pub authority: Pubkey,
    #[max_len(32)]
    pub category_id: String,
    pub locked_liquidity: u64,
    pub pending_settlements: u64,
    pub reputation: u64,
    pub is_partner: bool,
    pub is_sentinel: bool,
    pub is_active: bool,
}

#[account]
#[derive(InitSpace)]
pub struct CategoryStats {
    #[max_len(32)]
    pub category_id: String,
    pub active_miners: u64,
}

#[account]
#[derive(InitSpace)]
pub struct QueryAccount {
    #[max_len(64)]
    pub unique_event_id: String,
    #[max_len(32)]
    pub category_id: String,
    pub bounty_total: u64,
    pub status: QueryStatus,
    pub format: ResponseFormat,
    pub min_responses: u32,
    pub commit_deadline: i64,
    pub reveal_deadline: i64,
    pub finalized_at: i64,
    pub commit_count: u32,
    pub sentinel_commit_count: u32,
    pub sentinel_reveal_count: u32,
    pub reveal_count: u32,
    #[max_len(64)]
    pub result: String,
    pub winning_ticket_id: u32,
    pub random_accumulator: [u8; 32],
}

#[account]
#[derive(InitSpace)]
pub struct VoterRecord {
    pub authority: Pubkey,
    pub miner_profile: Pubkey,
    pub vote_hash: [u8; 32],
    #[max_len(256)]
    pub encrypted_salt: Vec<u8>,
    #[max_len(64)]
    pub revealed_value: String,
    pub ticket_id: u32,
    pub has_committed: bool,
    pub has_revealed: bool,
    pub bond_released: bool,
}

#[account]
pub struct VoteStatsSafe {
    pub query_key: Pubkey,
    pub options: Vec<VoteOptionSimple>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct VoteOptionSimple {
    pub value: String,
    pub count: u32,
}

// ============================================
// ENUMS
// ============================================

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq, InitSpace, Default)]
pub enum QueryStatus {
    #[default]
    Uninitialized,
    CommitPhase,
    RevealPhase,
    Finalized,
    UnderAppeal,
    Voided,
    InDispute,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq, InitSpace, Default)]
pub enum ResponseFormat {
    #[default]
    Binary,
    Score,
    Decimal,
    String,
    OptionIndex,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
pub enum CapitalAction {
    Deposit,
    Withdraw,
    Slash,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
pub enum VotePhase {
    Commit,
    Reveal,
}

// ============================================
// EVENTS
// ============================================

#[event]
pub struct CapitalEvent {
    pub user: Pubkey,
    pub amount: u64,
    pub action: CapitalAction,
}

#[event]
pub struct VoteEvent {
    pub query: Pubkey,
    pub voter: Pubkey,
    pub phase: VotePhase,
}

#[event]
pub struct AppealEvent {
    pub query: Pubkey,
    pub reason: String,
    pub timestamp: i64,
}

#[event]
pub struct ClaimEvent {
    pub query: Pubkey,
    pub winner: Pubkey,
    pub amount: u64,
}

// ============================================
// ERRORS
// ============================================

#[error_code]
pub enum CustomError {
    #[msg("Phase is closed")]
    PhaseClosed,
    #[msg("Hash does not match commitment")]
    HashMismatch,
    #[msg("Query not finalized")]
    NotFinalized,
    #[msg("Voted for wrong answer")]
    WrongVote,
    #[msg("Already claimed")]
    AlreadyClaimed,
    #[msg("Miner voted correctly")]
    MinerWasHonest,
    #[msg("Insufficient free capital")]
    InsufficientFreeCapital,
    #[msg("Settlement window not passed")]
    SettlementLocked,
    #[msg("Miner is banned")]
    MinerBanned,
    #[msg("Sentinel cap reached")]
    SentinelCapReached,
    #[msg("Format mismatch")]
    FormatMismatch,
    #[msg("Appeal window closed")]
    AppealWindowClosed,
    #[msg("Query already resolved")]
    QueryAlreadyResolved,
    #[msg("Too many vote options")]
    TooManyOptions,
    #[msg("Unauthorized")]
    Unauthorized,
    #[msg("Reveal window still open")]
    RevealWindowOpen,
    #[msg("Commit window still open")]
    CommitWindowOpen,
    #[msg("Max sentinels reached")]
    MaxSentinelsReached,
    #[msg("Wrong phase")]
    WrongPhase,
    #[msg("Not committed")]
    NotCommitted,
    #[msg("Already revealed")]
    AlreadyRevealed,
    #[msg("Not revealed")]
    NotRevealed,
    #[msg("Query not voided")]
    NotVoided,
    #[msg("Invalid treasury account")]
    InvalidTreasury,
    #[msg("Invalid gas tank account")]
    InvalidGasTank,
    #[msg("Invalid winner wallet")]
    InvalidWinnerWallet,
    #[msg("Category ID too long")]
    CategoryIdTooLong,
    #[msg("Event ID too long")]
    EventIdTooLong,
    #[msg("Category mismatch")]
    CategoryMismatch,
}
