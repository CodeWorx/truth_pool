use anchor_lang::prelude::*;
use anchor_lang::solana_program::keccak;

declare_id!("TrutH6qfNhnAiVwMz2gxBkqGKxCrHZaQBFSTewxVV1j");

// --- CONSTANTS ---
const VOTE_BOND: u64 = 500_000_000; // 0.5 SOL
const PARTNER_VIRTUAL_CAPACITY: u64 = 500_000_000_000; // 500 SOL equivalent
const SENTINEL_VIRTUAL_CAPACITY: u64 = 500_000_000_000; // 500 SOL equivalent
const APPEAL_BOND: u64 = 1_000_000_000; // 1 SOL
const SETTLEMENT_WINDOW: i64 = 43200; // 12 Hours
const MAX_SENTINELS: u32 = 100; // Hard cap on protocol nodes
const COMMIT_DURATION: i64 = 600; // 10 mins
const REVEAL_DURATION: i64 = 600; // 10 mins after commit ends
const DISPUTE_ESCALATION_WINDOW: i64 = 86400; // 24 hours to resolve before escalation

// --- PREDICTION MARKET CONSTANTS ---
const BET_PRICE_LAMPORTS: u64 = 1_000_000_000; // 1 SOL = $1 equivalent (adjust based on SOL price)
const CANCELLATION_FEE_BPS: u64 = 1000; // 10% = 1000 basis points

#[program]
pub mod truth_pool {
    use super::*;

    // --- CONFIGURATION ---
    /// Initialize protocol config
    /// NOTE: The admin account should be a multi-sig (Squads/Realms) for production.
    /// The arbiter_authority is the capital/reputation bot authority for Level 1 disputes.
    pub fn initialize_config(ctx: Context<InitConfig>) -> Result<()> {
        let config = &mut ctx.accounts.config;
        config.admin = ctx.accounts.admin.key();
        config.treasury = ctx.accounts.treasury.key();
        config.sentinel_gas_tank = ctx.accounts.sentinel_gas_tank.key();
        config.sentinel_count = 0;
        config.arbiter_authority = ctx.accounts.arbiter_authority.key();
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
                query.dispute_level = 1; // Level 1: Arbiter bots
                query.dispute_initiated_at = now;
                msg!("Sentinel Dominance (>50%). Escalated to arbiter bots (Level 1)");
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
            query.dispute_level = 1;
            query.dispute_initiated_at = now;
            msg!("Insufficient responses. Escalated to arbiter bots (Level 1)");
            return Ok(());
        }

        let consensus_pct = (max_votes as u64 * 100) / (total_valid as u64);
        if consensus_pct < 66 {
            query.status = QueryStatus::InDispute;
            query.dispute_level = 1;
            query.dispute_initiated_at = now;
            msg!("No supermajority. Escalated to arbiter bots (Level 1)");
            return Ok(());
        }

        // FIXED: Trustless Lottery using XOR Accumulator
        // Guard against division by zero
        require!(max_votes > 0, CustomError::NoValidVotes);

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

    // --- ARBITER RESOLVE DISPUTE (Level 1) ---
    /// Capital/Reputation bots resolve disputes at Level 1
    /// This is the first line of automated dispute resolution
    pub fn arbiter_resolve_dispute(ctx: Context<ArbiterResolveDispute>, new_result: Option<String>) -> Result<()> {
        let query = &mut ctx.accounts.query_account;
        let config = &ctx.accounts.config;

        require!(
            ctx.accounts.arbiter.key() == config.arbiter_authority,
            CustomError::Unauthorized
        );
        require!(query.status == QueryStatus::InDispute, CustomError::NotInDispute);
        require!(query.dispute_level == 1, CustomError::WrongDisputeLevel);

        if let Some(result) = new_result {
            query.result = result;
            query.status = QueryStatus::Finalized;
            query.finalized_at = Clock::get()?.unix_timestamp;
            query.dispute_level = 0;
            msg!("Dispute resolved by arbiter bots (Level 1)");
        } else {
            query.status = QueryStatus::Voided;
            query.dispute_level = 0;
            msg!("Dispute voided by arbiter bots (Level 1)");
        }

        Ok(())
    }

    // --- ESCALATE TO DAO (Level 2) ---
    /// Escalate unresolved dispute to DAO for human review
    /// Can be called by arbiter if they cannot resolve, or automatically after timeout
    pub fn escalate_to_dao(ctx: Context<EscalateDispute>) -> Result<()> {
        let query = &mut ctx.accounts.query_account;
        let config = &ctx.accounts.config;
        let now = Clock::get()?.unix_timestamp;

        require!(query.status == QueryStatus::InDispute, CustomError::NotInDispute);
        require!(query.dispute_level == 1, CustomError::WrongDisputeLevel);

        // Either arbiter explicitly escalates, or timeout has passed
        let is_arbiter = ctx.accounts.escalator.key() == config.arbiter_authority;
        let timeout_passed = now > query.dispute_initiated_at + DISPUTE_ESCALATION_WINDOW;

        require!(is_arbiter || timeout_passed, CustomError::EscalationNotAllowed);

        query.dispute_level = 2; // Level 2: DAO human review
        query.dispute_initiated_at = now; // Reset timer for DAO review

        msg!("Dispute escalated to DAO (Level 2) for human review");
        emit!(DisputeEscalatedEvent {
            query: query.key(),
            from_level: 1,
            to_level: 2,
            timestamp: now
        });

        Ok(())
    }

    // --- DAO RESOLVE DISPUTE (Level 2) ---
    /// DAO multi-sig resolves disputes at Level 2 (final human review)
    /// This is the final escalation point - requires multi-sig admin
    pub fn dao_resolve_dispute(ctx: Context<DaoResolveDispute>, new_result: Option<String>) -> Result<()> {
        let query = &mut ctx.accounts.query_account;
        let config = &ctx.accounts.config;

        // Must be signed by DAO multi-sig admin
        require!(ctx.accounts.admin.key() == config.admin, CustomError::Unauthorized);
        require!(query.status == QueryStatus::InDispute, CustomError::NotInDispute);
        require!(query.dispute_level == 2, CustomError::WrongDisputeLevel);

        if let Some(result) = new_result {
            query.result = result;
            query.status = QueryStatus::Finalized;
            query.finalized_at = Clock::get()?.unix_timestamp;
            query.dispute_level = 0;
            msg!("Dispute resolved by DAO multi-sig (Level 2)");
        } else {
            query.status = QueryStatus::Voided;
            query.dispute_level = 0;
            msg!("Dispute voided by DAO multi-sig (Level 2)");
        }

        emit!(DisputeResolvedEvent {
            query: query.key(),
            level: 2,
            result: query.result.clone(),
            timestamp: Clock::get()?.unix_timestamp
        });

        Ok(())
    }

    // --- UPDATE CONFIG ---
    /// Update protocol configuration (DAO multi-sig only)
    pub fn update_config(
        ctx: Context<UpdateConfig>,
        new_admin: Option<Pubkey>,
        new_treasury: Option<Pubkey>,
        new_gas_tank: Option<Pubkey>,
        new_arbiter: Option<Pubkey>,
    ) -> Result<()> {
        let config = &mut ctx.accounts.config;

        require!(ctx.accounts.admin.key() == config.admin, CustomError::Unauthorized);

        if let Some(admin) = new_admin {
            config.admin = admin;
            msg!("Admin (multi-sig) updated");
        }
        if let Some(treasury) = new_treasury {
            config.treasury = treasury;
            msg!("Treasury updated");
        }
        if let Some(gas_tank) = new_gas_tank {
            config.sentinel_gas_tank = gas_tank;
            msg!("Gas tank updated");
        }
        if let Some(arbiter) = new_arbiter {
            config.arbiter_authority = arbiter;
            msg!("Arbiter authority updated");
        }

        Ok(())
    }

    // --- DEACTIVATE SENTINEL (NEW) ---
    pub fn deactivate_sentinel(ctx: Context<DeactivateSentinel>) -> Result<()> {
        let config = &mut ctx.accounts.config;
        let miner = &mut ctx.accounts.miner_profile;

        require!(ctx.accounts.admin.key() == config.admin, CustomError::Unauthorized);
        require!(miner.is_sentinel, CustomError::NotASentinel);
        require!(miner.is_active, CustomError::MinerBanned);

        // Check no locked funds
        require!(
            miner.locked_liquidity == 0 && miner.pending_settlements == 0,
            CustomError::HasLockedFunds
        );

        miner.is_active = false;
        config.sentinel_count = config.sentinel_count.saturating_sub(1);

        msg!("Sentinel deactivated");
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

    // ============================================
    // PREDICTION MARKET INSTRUCTIONS
    // ============================================

    /// Create a new prediction market linked to an oracle query
    /// Markets use fixed $1 bets with parimutuel payout
    pub fn create_bet_market(
        ctx: Context<CreateBetMarket>,
        market_id: String,
        lock_timestamp: i64,
    ) -> Result<()> {
        require!(market_id.len() <= 64, CustomError::InvalidMarketId);

        let market = &mut ctx.accounts.bet_market;
        let query = &ctx.accounts.query_account;
        let now = Clock::get()?.unix_timestamp;

        require!(lock_timestamp > now, CustomError::MarketLocked);

        market.market_id = market_id;
        market.oracle_query = query.key();
        market.creator = ctx.accounts.creator.key();
        market.lock_timestamp = lock_timestamp;
        market.total_yes_bets = 0;
        market.total_no_bets = 0;
        market.total_yes_amount = 0;
        market.total_no_amount = 0;
        market.status = MarketStatus::Open;
        market.winning_side = None;
        market.created_at = now;

        emit!(MarketCreatedEvent {
            market: market.key(),
            oracle_query: query.key(),
            lock_timestamp,
            creator: ctx.accounts.creator.key(),
        });

        Ok(())
    }

    /// Buy a bet on YES or NO outcome
    /// Each bet costs exactly 1 unit (BET_PRICE_LAMPORTS)
    /// bet_count: number of $1 bets to place
    /// side: true = YES, false = NO
    pub fn buy_bet(
        ctx: Context<BuyBet>,
        bet_count: u64,
        side: bool,
    ) -> Result<()> {
        require!(bet_count > 0, CustomError::InsufficientBetAmount);

        let market = &mut ctx.accounts.bet_market;
        let user_bet = &mut ctx.accounts.user_bet;
        let now = Clock::get()?.unix_timestamp;

        require!(market.status == MarketStatus::Open, CustomError::MarketLocked);
        require!(now < market.lock_timestamp, CustomError::MarketLocked);

        let total_cost = bet_count.checked_mul(BET_PRICE_LAMPORTS)
            .ok_or(CustomError::InsufficientBetAmount)?;

        // Transfer SOL from bettor to market
        let cpi_context = CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            anchor_lang::system_program::Transfer {
                from: ctx.accounts.bettor.to_account_info(),
                to: market.to_account_info(),
            },
        );
        anchor_lang::system_program::transfer(cpi_context, total_cost)?;

        // Update market totals
        if side {
            market.total_yes_bets += bet_count;
            market.total_yes_amount += total_cost;
        } else {
            market.total_no_bets += bet_count;
            market.total_no_amount += total_cost;
        }

        // Update user bet record
        if user_bet.market == Pubkey::default() {
            // First bet - initialize
            user_bet.market = market.key();
            user_bet.bettor = ctx.accounts.bettor.key();
            user_bet.yes_bets = 0;
            user_bet.no_bets = 0;
            user_bet.yes_amount = 0;
            user_bet.no_amount = 0;
            user_bet.has_redeemed = false;
        }

        if side {
            user_bet.yes_bets += bet_count;
            user_bet.yes_amount += total_cost;
        } else {
            user_bet.no_bets += bet_count;
            user_bet.no_amount += total_cost;
        }

        emit!(BetPlacedEvent {
            market: market.key(),
            bettor: ctx.accounts.bettor.key(),
            side,
            bet_count,
            amount: total_cost,
        });

        Ok(())
    }

    /// Sell back bets before lock with 10% cancellation fee
    /// sell_count: number of bets to sell back
    /// side: true = YES, false = NO
    pub fn sell_bet(
        ctx: Context<SellBet>,
        sell_count: u64,
        side: bool,
    ) -> Result<()> {
        require!(sell_count > 0, CustomError::InsufficientBetAmount);

        let market = &mut ctx.accounts.bet_market;
        let user_bet = &mut ctx.accounts.user_bet;
        let now = Clock::get()?.unix_timestamp;

        require!(market.status == MarketStatus::Open, CustomError::MarketLocked);
        require!(now < market.lock_timestamp, CustomError::MarketLocked);

        // Check user has enough bets to sell
        if side {
            require!(user_bet.yes_bets >= sell_count, CustomError::InsufficientBetAmount);
        } else {
            require!(user_bet.no_bets >= sell_count, CustomError::InsufficientBetAmount);
        }

        let gross_refund = sell_count.checked_mul(BET_PRICE_LAMPORTS)
            .ok_or(CustomError::InsufficientBetAmount)?;

        // Calculate 10% cancellation fee
        let fee = gross_refund.checked_mul(CANCELLATION_FEE_BPS)
            .ok_or(CustomError::InsufficientBetAmount)?
            .checked_div(10000)
            .ok_or(CustomError::InsufficientBetAmount)?;

        let net_refund = gross_refund.saturating_sub(fee);

        // Transfer refund from market to bettor
        **market.to_account_info().try_borrow_mut_lamports()? -= net_refund;
        **ctx.accounts.bettor.to_account_info().try_borrow_mut_lamports()? += net_refund;

        // Cancellation fee stays in the market pool (redistributed to remaining bettors)
        // Update market totals
        if side {
            market.total_yes_bets -= sell_count;
            market.total_yes_amount = market.total_yes_amount.saturating_sub(gross_refund);
            // Fee goes to the pool (stays in market)
            user_bet.yes_bets -= sell_count;
            user_bet.yes_amount = user_bet.yes_amount.saturating_sub(gross_refund);
        } else {
            market.total_no_bets -= sell_count;
            market.total_no_amount = market.total_no_amount.saturating_sub(gross_refund);
            user_bet.no_bets -= sell_count;
            user_bet.no_amount = user_bet.no_amount.saturating_sub(gross_refund);
        }

        emit!(BetSoldEvent {
            market: market.key(),
            bettor: ctx.accounts.bettor.key(),
            side,
            sell_count,
            net_refund,
            fee,
        });

        Ok(())
    }

    /// Lock the market when lock_timestamp is reached
    /// If no opposing bets exist, market is cancelled and refunds issued
    pub fn lock_market(ctx: Context<LockMarket>) -> Result<()> {
        let market = &mut ctx.accounts.bet_market;
        let now = Clock::get()?.unix_timestamp;

        require!(market.status == MarketStatus::Open, CustomError::MarketAlreadyResolved);
        require!(now >= market.lock_timestamp, CustomError::MarketNotLocked);

        // Check if opposing bets exist
        if market.total_yes_bets == 0 || market.total_no_bets == 0 {
            // No opposing bets - mark for cancellation
            market.status = MarketStatus::Cancelled;
            msg!("Market cancelled - no opposing bets");
        } else {
            market.status = MarketStatus::Locked;
            msg!("Market locked for betting");
        }

        emit!(MarketLockedEvent {
            market: market.key(),
            status: market.status.clone(),
            total_yes_bets: market.total_yes_bets,
            total_no_bets: market.total_no_bets,
        });

        Ok(())
    }

    /// Resolve the market using the oracle result
    /// Can only be called after oracle query is finalized
    pub fn resolve_market(ctx: Context<ResolveMarket>) -> Result<()> {
        let market = &mut ctx.accounts.bet_market;
        let query = &ctx.accounts.query_account;

        require!(market.status == MarketStatus::Locked, CustomError::MarketNotLocked);
        require!(query.status == QueryStatus::Finalized, CustomError::OracleNotFinalized);

        // Determine winning side based on oracle result
        // For binary outcomes: "yes", "true", "1" = YES wins, anything else = NO wins
        let result_lower = query.result.to_lowercase();
        let yes_wins = result_lower == "yes" || result_lower == "true" || result_lower == "1";

        market.status = MarketStatus::Resolved;
        market.winning_side = Some(yes_wins);

        emit!(MarketResolvedEvent {
            market: market.key(),
            oracle_query: query.key(),
            oracle_result: query.result.clone(),
            winning_side: yes_wins,
            total_pool: market.total_yes_amount + market.total_no_amount,
        });

        Ok(())
    }

    /// Redeem winnings for a resolved market
    /// Winners split the losers' pool proportionally (parimutuel)
    pub fn redeem_winnings(ctx: Context<RedeemWinnings>) -> Result<()> {
        let market = &ctx.accounts.bet_market;
        let user_bet = &mut ctx.accounts.user_bet;

        require!(market.status == MarketStatus::Resolved, CustomError::MarketNotResolved);
        require!(!user_bet.has_redeemed, CustomError::AlreadyRedeemed);

        let winning_side = market.winning_side.ok_or(CustomError::MarketNotResolved)?;

        // Get user's winning bet count
        let user_winning_bets = if winning_side {
            user_bet.yes_bets
        } else {
            user_bet.no_bets
        };

        require!(user_winning_bets > 0, CustomError::NotAWinner);

        // Calculate payout using parimutuel formula
        // Payout = (user_winning_bets / total_winning_bets) * total_pool
        let total_winning_bets = if winning_side {
            market.total_yes_bets
        } else {
            market.total_no_bets
        };

        let total_pool = market.total_yes_amount + market.total_no_amount;

        // Calculate proportional share: (user_bets * total_pool) / total_winning_bets
        let payout = (user_winning_bets as u128)
            .checked_mul(total_pool as u128)
            .ok_or(CustomError::InsufficientBetAmount)?
            .checked_div(total_winning_bets as u128)
            .ok_or(CustomError::InsufficientBetAmount)? as u64;

        // Transfer payout from market to winner
        **ctx.accounts.bet_market.to_account_info().try_borrow_mut_lamports()? -= payout;
        **ctx.accounts.bettor.to_account_info().try_borrow_mut_lamports()? += payout;

        user_bet.has_redeemed = true;

        emit!(WinningsRedeemedEvent {
            market: market.key(),
            bettor: ctx.accounts.bettor.key(),
            winning_bets: user_winning_bets,
            payout,
        });

        Ok(())
    }

    /// Claim refund for a cancelled market (no opposing bets)
    pub fn claim_refund(ctx: Context<ClaimRefund>) -> Result<()> {
        let market = &ctx.accounts.bet_market;
        let user_bet = &mut ctx.accounts.user_bet;

        require!(market.status == MarketStatus::Cancelled, CustomError::MarketNotCancelled);
        require!(!user_bet.has_redeemed, CustomError::AlreadyRedeemed);

        // Refund full amount (no fee for cancellation due to no opposing bets)
        let total_refund = user_bet.yes_amount + user_bet.no_amount;

        require!(total_refund > 0, CustomError::InsufficientBetAmount);

        // Transfer refund from market to bettor
        **ctx.accounts.bet_market.to_account_info().try_borrow_mut_lamports()? -= total_refund;
        **ctx.accounts.bettor.to_account_info().try_borrow_mut_lamports()? += total_refund;

        user_bet.has_redeemed = true;

        emit!(RefundClaimedEvent {
            market: market.key(),
            bettor: ctx.accounts.bettor.key(),
            refund_amount: total_refund,
        });

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
    /// CHECK: Arbiter authority (capital/reputation bots) for Level 1 dispute resolution
    pub arbiter_authority: AccountInfo<'info>,
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
        seeds = [b"miner", user.key().as_ref()],
        bump,
        constraint = miner_profile.authority == user.key() @ CustomError::Unauthorized
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
        seeds = [b"miner", voter.key().as_ref()],
        bump,
        constraint = miner_profile.authority == voter.key() @ CustomError::Unauthorized
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
        seeds = [b"miner", voter.key().as_ref()],
        bump,
        constraint = miner_profile.authority == voter.key() @ CustomError::Unauthorized
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
        seeds = [b"miner", voter.key().as_ref()],
        bump,
        constraint = miner_profile.authority == voter.key() @ CustomError::Unauthorized
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
        seeds = [b"miner", voter.key().as_ref()],
        bump,
        constraint = miner_profile.authority == voter.key() @ CustomError::Unauthorized
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

/// Level 1: Arbiter bots (capital/reputation) resolve disputes
#[derive(Accounts)]
pub struct ArbiterResolveDispute<'info> {
    #[account(mut)]
    pub arbiter: Signer<'info>,
    #[account(seeds = [b"config"], bump)]
    pub config: Account<'info, ProtocolConfig>,
    #[account(mut)]
    pub query_account: Account<'info, QueryAccount>,
}

/// Escalate dispute from Level 1 to Level 2 (DAO)
#[derive(Accounts)]
pub struct EscalateDispute<'info> {
    #[account(mut)]
    pub escalator: Signer<'info>,
    #[account(seeds = [b"config"], bump)]
    pub config: Account<'info, ProtocolConfig>,
    #[account(mut)]
    pub query_account: Account<'info, QueryAccount>,
}

/// Level 2: DAO multi-sig resolves final disputes
#[derive(Accounts)]
pub struct DaoResolveDispute<'info> {
    /// NOTE: This should be a multi-sig account (Squads, Realms, etc.)
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(seeds = [b"config"], bump)]
    pub config: Account<'info, ProtocolConfig>,
    #[account(mut)]
    pub query_account: Account<'info, QueryAccount>,
}

#[derive(Accounts)]
pub struct UpdateConfig<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(mut, seeds = [b"config"], bump)]
    pub config: Account<'info, ProtocolConfig>,
}

#[derive(Accounts)]
pub struct DeactivateSentinel<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(mut, seeds = [b"config"], bump)]
    pub config: Account<'info, ProtocolConfig>,
    #[account(mut)]
    pub miner_profile: Account<'info, MinerProfile>,
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
// PREDICTION MARKET ACCOUNT CONTEXTS
// ============================================

#[derive(Accounts)]
#[instruction(market_id: String)]
pub struct CreateBetMarket<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    pub query_account: Account<'info, QueryAccount>,
    #[account(
        init,
        payer = creator,
        space = 8 + BetMarket::INIT_SPACE,
        seeds = [b"market", query_account.key().as_ref(), market_id.as_bytes()],
        bump
    )]
    pub bet_market: Account<'info, BetMarket>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct BuyBet<'info> {
    #[account(mut)]
    pub bettor: Signer<'info>,
    #[account(mut)]
    pub bet_market: Account<'info, BetMarket>,
    #[account(
        init_if_needed,
        payer = bettor,
        space = 8 + UserBet::INIT_SPACE,
        seeds = [b"user_bet", bet_market.key().as_ref(), bettor.key().as_ref()],
        bump
    )]
    pub user_bet: Account<'info, UserBet>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SellBet<'info> {
    #[account(mut)]
    pub bettor: Signer<'info>,
    #[account(mut)]
    pub bet_market: Account<'info, BetMarket>,
    #[account(
        mut,
        seeds = [b"user_bet", bet_market.key().as_ref(), bettor.key().as_ref()],
        bump
    )]
    pub user_bet: Account<'info, UserBet>,
}

#[derive(Accounts)]
pub struct LockMarket<'info> {
    #[account(mut)]
    pub bet_market: Account<'info, BetMarket>,
}

#[derive(Accounts)]
pub struct ResolveMarket<'info> {
    #[account(
        mut,
        constraint = bet_market.oracle_query == query_account.key()
    )]
    pub bet_market: Account<'info, BetMarket>,
    pub query_account: Account<'info, QueryAccount>,
}

#[derive(Accounts)]
pub struct RedeemWinnings<'info> {
    #[account(mut)]
    pub bettor: Signer<'info>,
    #[account(mut)]
    pub bet_market: Account<'info, BetMarket>,
    #[account(
        mut,
        seeds = [b"user_bet", bet_market.key().as_ref(), bettor.key().as_ref()],
        bump,
        constraint = user_bet.bettor == bettor.key()
    )]
    pub user_bet: Account<'info, UserBet>,
}

#[derive(Accounts)]
pub struct ClaimRefund<'info> {
    #[account(mut)]
    pub bettor: Signer<'info>,
    #[account(mut)]
    pub bet_market: Account<'info, BetMarket>,
    #[account(
        mut,
        seeds = [b"user_bet", bet_market.key().as_ref(), bettor.key().as_ref()],
        bump,
        constraint = user_bet.bettor == bettor.key()
    )]
    pub user_bet: Account<'info, UserBet>,
}

// ============================================
// DATA STRUCTURES
// ============================================

/// Protocol configuration account
/// NOTE: The `admin` field should be a multi-sig account (e.g., Squads, Realms DAO)
/// for production deployments. This ensures DAO governance for critical operations
/// like dispute resolution and configuration updates.
#[account]
#[derive(InitSpace)]
pub struct ProtocolConfig {
    /// Multi-sig admin account for DAO governance (final dispute resolution)
    pub admin: Pubkey,
    pub treasury: Pubkey,
    pub sentinel_gas_tank: Pubkey,
    pub sentinel_count: u32,
    /// Capital/Reputation bot authority for Level 1 dispute resolution
    pub arbiter_authority: Pubkey,
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
    /// Dispute escalation level (0 = none, 1 = arbiter bots, 2 = DAO)
    pub dispute_level: u8,
    /// Timestamp when dispute was initiated (for escalation timing)
    pub dispute_initiated_at: i64,
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
#[derive(InitSpace)]
pub struct VoteStatsSafe {
    pub query_key: Pubkey,
    #[max_len(50)] // 50 options max
    pub options: Vec<VoteOptionSimple>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct VoteOptionSimple {
    #[max_len(64)]
    pub value: String,
    pub count: u32,
}

// ============================================
// PREDICTION MARKET DATA STRUCTURES
// ============================================

/// Prediction market linked to an oracle query
/// Uses fixed $1 bets with parimutuel payout (100% to winners, 0% protocol fee)
#[account]
#[derive(InitSpace)]
pub struct BetMarket {
    /// Unique market identifier
    #[max_len(64)]
    pub market_id: String,
    /// The oracle query this market is linked to
    pub oracle_query: Pubkey,
    /// Creator of the market
    pub creator: Pubkey,
    /// Timestamp when betting locks (no more bets after this)
    pub lock_timestamp: i64,
    /// Total number of YES bets (each bet = 1 unit)
    pub total_yes_bets: u64,
    /// Total number of NO bets (each bet = 1 unit)
    pub total_no_bets: u64,
    /// Total SOL wagered on YES
    pub total_yes_amount: u64,
    /// Total SOL wagered on NO
    pub total_no_amount: u64,
    /// Market status
    pub status: MarketStatus,
    /// Winning side (true = YES, false = NO), None if not resolved
    pub winning_side: Option<bool>,
    /// When market was created
    pub created_at: i64,
}

/// User's bet position in a market
#[account]
#[derive(InitSpace)]
pub struct UserBet {
    /// Market this bet belongs to
    pub market: Pubkey,
    /// User who placed the bet
    pub bettor: Pubkey,
    /// Number of YES bets
    pub yes_bets: u64,
    /// Number of NO bets
    pub no_bets: u64,
    /// Total amount wagered on YES
    pub yes_amount: u64,
    /// Total amount wagered on NO
    pub no_amount: u64,
    /// Whether winnings/refund has been claimed
    pub has_redeemed: bool,
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

/// Prediction market status
#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq, InitSpace, Default)]
pub enum MarketStatus {
    /// Market is open for betting
    #[default]
    Open,
    /// Betting is locked, awaiting resolution
    Locked,
    /// Market resolved, winners can claim
    Resolved,
    /// Market cancelled (no opposing bets), refunds available
    Cancelled,
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

#[event]
pub struct DisputeEscalatedEvent {
    pub query: Pubkey,
    pub from_level: u8,
    pub to_level: u8,
    pub timestamp: i64,
}

#[event]
pub struct DisputeResolvedEvent {
    pub query: Pubkey,
    pub level: u8,
    pub result: String,
    pub timestamp: i64,
}

// ============================================
// PREDICTION MARKET EVENTS
// ============================================

#[event]
pub struct MarketCreatedEvent {
    pub market: Pubkey,
    pub oracle_query: Pubkey,
    pub lock_timestamp: i64,
    pub creator: Pubkey,
}

#[event]
pub struct BetPlacedEvent {
    pub market: Pubkey,
    pub bettor: Pubkey,
    pub side: bool,
    pub bet_count: u64,
    pub amount: u64,
}

#[event]
pub struct BetSoldEvent {
    pub market: Pubkey,
    pub bettor: Pubkey,
    pub side: bool,
    pub sell_count: u64,
    pub net_refund: u64,
    pub fee: u64,
}

#[event]
pub struct MarketLockedEvent {
    pub market: Pubkey,
    pub status: MarketStatus,
    pub total_yes_bets: u64,
    pub total_no_bets: u64,
}

#[event]
pub struct MarketResolvedEvent {
    pub market: Pubkey,
    pub oracle_query: Pubkey,
    pub oracle_result: String,
    pub winning_side: bool,
    pub total_pool: u64,
}

#[event]
pub struct WinningsRedeemedEvent {
    pub market: Pubkey,
    pub bettor: Pubkey,
    pub winning_bets: u64,
    pub payout: u64,
}

#[event]
pub struct RefundClaimedEvent {
    pub market: Pubkey,
    pub bettor: Pubkey,
    pub refund_amount: u64,
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
    #[msg("Query not in dispute")]
    NotInDispute,
    #[msg("Account is not a sentinel")]
    NotASentinel,
    #[msg("Cannot deactivate: has locked funds")]
    HasLockedFunds,
    #[msg("No valid votes to tally")]
    NoValidVotes,
    #[msg("Wrong dispute level for this operation")]
    WrongDisputeLevel,
    #[msg("Escalation not allowed (not arbiter or timeout not passed)")]
    EscalationNotAllowed,
    // Prediction Market Errors
    #[msg("Market is locked")]
    MarketLocked,
    #[msg("Market is not locked")]
    MarketNotLocked,
    #[msg("Market already resolved")]
    MarketAlreadyResolved,
    #[msg("Market not resolved")]
    MarketNotResolved,
    #[msg("Invalid bet side")]
    InvalidBetSide,
    #[msg("No opposing bets")]
    NoOpposingBets,
    #[msg("Already redeemed")]
    AlreadyRedeemed,
    #[msg("Not a winner")]
    NotAWinner,
    #[msg("Insufficient bet amount")]
    InsufficientBetAmount,
    #[msg("Market not cancelled")]
    MarketNotCancelled,
    #[msg("Oracle not finalized")]
    OracleNotFinalized,
    #[msg("Invalid market ID")]
    InvalidMarketId,
}
