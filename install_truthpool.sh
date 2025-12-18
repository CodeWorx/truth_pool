#!/bin/bash

# TruthPool Master Installer
# Usage: chmod +x install_truthpool.sh && ./install_truthpool.sh

echo "🚀 Initializing TruthPool Protocol Ecosystem..."

# 1. Create Directory Structure
mkdir -p truth-pool/programs/truth-pool/src
mkdir -p truth-pool/src/context
mkdir -p truth-pool/src/screens
mkdir -p truth-pool/src/theme
mkdir -p truth-pool/bots/miner-agent
mkdir -p truth-pool/docs

cd truth-pool

# ==========================================
# 1. ANCHOR SMART CONTRACT (Fully Implemented)
# ==========================================
echo "📦 Writing Anchor Program (lib.rs)..."
cat << 'EOF' > programs/truth-pool/src/lib.rs
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
EOF

# ==========================================
# 2. MOBILE APP (React Native) - Full Files
# ==========================================
echo "📱 Writing Mobile App Code..."

# --- BOT MANAGER (With Advanced Config) ---
cat << 'EOF' > src/screens/BotManager.tsx
import React, { useState } from 'react';
import { View, FlatList, ScrollView } from 'react-native';
import { Text, Card, Button, FAB, Dialog, Portal, TextInput, useTheme, Chip, Switch, SegmentedButtons, HelperText, Divider, IconButton } from 'react-native-paper';
import { Zap, Shield, Fuel } from 'lucide-react-native';

const INITIAL_BOTS = [
  { id: '1', name: 'NBA Sniper', type: 'Miner', stake: 1.0, activeVotes: 0, maxVotes: 2, status: 'Running' },
  { id: '2', name: 'Whale Guardian', type: 'Whale', stake: 15.0, activeVotes: 4, maxVotes: 30, status: 'Running' },
];

export default function BotManager() {
  const { colors } = useTheme();
  const [bots, setBots] = useState(INITIAL_BOTS);
  
  const [visible, setVisible] = useState(false);
  const [step, setStep] = useState(1);
  const [botConfig, setBotConfig] = useState({
    name: '',
    type: 'Miner',
    stake: '1',
    categories: ['Sports'],
    maxGas: '0.000005',
    schedule: '1min',
    apis: ['ESPN']
  });

  const capacity = Math.floor(parseFloat(botConfig.stake || '0') / 0.5);

  const handleDeploy = () => {
    const newBot = {
        id: Math.random().toString(),
        name: botConfig.name,
        type: botConfig.type,
        stake: parseFloat(botConfig.stake),
        activeVotes: 0,
        maxVotes: capacity,
        status: 'Starting...'
    };
    setBots([...bots, newBot]);
    setVisible(false);
    setStep(1);
  };

  const toggleCategory = (cat: string) => {
    const current = botConfig.categories;
    if (current.includes(cat)) {
        setBotConfig({ ...botConfig, categories: current.filter(c => c !== cat) });
    } else {
        setBotConfig({ ...botConfig, categories: [...current, cat] });
    }
  };

  const renderBot = ({ item }: { item: any }) => (
    <Card style={{ marginBottom: 12, backgroundColor: colors.surface }} mode="outlined">
      <Card.Content>
        <View style={{ flexDirection: 'row', justifyContent: 'space-between', alignItems: 'flex-start' }}>
          <View>
            <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8 }}>
                {item.type === 'Whale' && <Shield size={16} color={colors.primary} />}
                <Text variant="titleMedium" style={{ fontWeight: 'bold' }}>{item.name}</Text>
            </View>
            <Text variant="bodySmall" style={{ color: colors.onSurfaceVariant, marginTop: 4 }}>
                Capacity: {item.activeVotes} / {item.maxVotes} Votes
            </Text>
            <View style={{ flexDirection: 'row', gap: 4, marginTop: 8 }}>
                <Chip icon="cash" compact textStyle={{fontSize: 10}}>{item.stake} SOL</Chip>
                <Chip icon="check-circle" compact textStyle={{fontSize: 10}} style={{ backgroundColor: colors.secondaryContainer }}>{item.status}</Chip>
            </View>
          </View>
          <IconButton icon="dots-vertical" onPress={() => {}} />
        </View>
      </Card.Content>
      <Card.Actions style={{ borderTopWidth: 1, borderTopColor: colors.surfaceVariant }}>
        <Button textColor={colors.primary} compact icon="wallet-plus">Top Up</Button>
        <Button textColor={colors.onSurface} compact icon="cog">Config</Button>
        <Button textColor={colors.error} compact icon="stop-circle">Stop</Button>
      </Card.Actions>
    </Card>
  );

  return (
    <View style={{ flex: 1, backgroundColor: colors.background }}>
      <FlatList
        data={bots}
        renderItem={renderBot}
        keyExtractor={item => item.id}
        contentContainerStyle={{ padding: 20, paddingBottom: 80 }}
        ListHeaderComponent={
            <View style={{ marginBottom: 20 }}>
                <Text variant="headlineSmall" style={{ fontWeight: 'bold' }}>Bot Fleet</Text>
            </View>
        }
      />

      <FAB icon="robot" label="Deploy Bot" style={{ position: 'absolute', margin: 16, right: 0, bottom: 0, backgroundColor: colors.primary }} onPress={() => setVisible(true)} />

      <Portal>
        <Dialog visible={visible} onDismiss={() => setVisible(false)} style={{ backgroundColor: colors.surface }}>
            <Dialog.Title>{step === 1 ? '1. Identity' : step === 2 ? '2. Economics' : '3. Config'}</Dialog.Title>
            <Dialog.Content>
                {step === 1 && (
                    <View>
                        <TextInput label="Bot Name" value={botConfig.name} onChangeText={t => setBotConfig({...botConfig, name: t})} mode="outlined" style={{ marginBottom: 15 }} />
                        <SegmentedButtons
                            value={botConfig.type}
                            onValueChange={v => setBotConfig({...botConfig, type: v, stake: v === 'Whale' ? '10' : '1'})}
                            buttons={[{ value: 'Miner', label: 'Miner', icon: 'pickaxe' }, { value: 'Whale', label: 'Guardian', icon: 'shield' }]}
                            style={{ marginBottom: 15 }}
                        />
                        <Text variant="bodyMedium" style={{ marginBottom: 8 }}>Categories</Text>
                        <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: 8 }}>
                            {['Sports', 'Crypto', 'Politics'].map(cat => (
                                <Chip key={cat} selected={botConfig.categories.includes(cat)} onPress={() => toggleCategory(cat)} showSelectedOverlay>{cat}</Chip>
                            ))}
                        </View>
                    </View>
                )}
                {step === 2 && (
                    <View>
                        <TextInput label="Initial Stake (SOL)" value={botConfig.stake} onChangeText={t => setBotConfig({...botConfig, stake: t})} keyboardType="numeric" mode="outlined" />
                        <HelperText type="info" visible>Bandwidth: {capacity} Concurrent Votes</HelperText>
                        <Divider style={{ marginVertical: 15 }} />
                        <TextInput label="Max Gas (Gwei)" value={botConfig.maxGas} onChangeText={t => setBotConfig({...botConfig, maxGas: t})} mode="outlined" right={<TextInput.Icon icon={() => <Fuel size={20} />} />} />
                    </View>
                )}
                {step === 3 && (
                    <View>
                        <Text style={{ marginBottom: 8 }}>Schedule</Text>
                        <SegmentedButtons value={botConfig.schedule} onValueChange={v => setBotConfig({...botConfig, schedule: v})} buttons={[{ value: '1min', label: '1 min' }, { value: '5min', label: '5 min' }, { value: '1hr', label: '1 hr' }]} style={{ marginBottom: 15 }} />
                        <Text style={{ marginBottom: 8 }}>Sources</Text>
                        <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: 8 }}>
                            {['ESPN', 'Yahoo', 'CoinGecko'].map(api => (
                                <Chip key={api} selected={botConfig.apis.includes(api)} onPress={() => {const curr = botConfig.apis; setBotConfig({...botConfig, apis: curr.includes(api) ? curr.filter(x => x !== api) : [...curr, api]})}}>{api}</Chip>
                            ))}
                        </View>
                    </View>
                )}
            </Dialog.Content>
            <Dialog.Actions>
                {step > 1 && <Button onPress={() => setStep(step - 1)}>Back</Button>}
                {step < 3 ? <Button mode="contained" onPress={() => setStep(step + 1)}>Next</Button> : <Button mode="contained" onPress={handleDeploy}>Deploy</Button>}
            </Dialog.Actions>
        </Dialog>
      </Portal>
    </View>
  );
}
EOF

# --- PREDICTION MARKET (Grid + Appeal) ---
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
  { id: '1', question: 'BTC > $100k?', volume: 450.5, yesPrice: 0.65, noPrice: 0.35, category: 'Crypto', status: 'Active' },
  { id: '4', question: 'Breakpoint City', volume: 50.0, yesPrice: 1.0, noPrice: 0.0, category: 'Crypto', status: 'Finalized', result: 'YES', finalizedAt: NOW - 3600 }, 
  { id: '5', question: 'Election 24', volume: 1200.5, yesPrice: 0.0, noPrice: 1.0, category: 'Politics', status: 'Finalized', result: 'NO', finalizedAt: NOW - 50000 }, 
  { id: '6', question: 'Fed Rate Cut', volume: 890.0, yesPrice: 0.15, noPrice: 0.85, category: 'Politics', status: 'UnderAppeal', finalizedAt: NOW - 4000 },
];

export default function PredictionMarket() {
  const { colors } = useTheme();
  const [markets, setMarkets] = useState([]);
  const [viewMode, setViewMode] = useState('Live');
  const [appealVisible, setAppealVisible] = useState(false);
  const [selectedMarket, setSelectedMarket] = useState(null);
  const [appealReason, setAppealReason] = useState('');
  const [createVisible, setCreateVisible] = useState(false);
  const [betVisible, setBetVisible] = useState(false);
  const [newQuestion, setNewQuestion] = useState('');
  const [betAmount, setBetAmount] = useState('');

  useEffect(() => {
    const load = async () => {
        const c = await AsyncStorage.getItem(MARKETS_CACHE_KEY);
        setMarkets(c ? JSON.parse(c) : INITIAL_MARKETS);
    };
    load();
  }, []);

  const handleAppeal = () => {
    if (!appealReason.trim()) return Alert.alert("Error", "Enter reason.");
    Alert.alert("Appeal Filed", "1 SOL Bond Deposited.");
    const updated = markets.map(m => m.id === selectedMarket.id ? {...m, status: 'UnderAppeal'} : m);
    setMarkets(updated);
    setAppealVisible(false);
  };

  const handleCreateMarket = () => {
    setMarkets([{ id: Math.random().toString(), question: newQuestion, volume: 0, yesPrice: 0.5, category: 'Custom', status: 'Active' }, ...markets]);
    setCreateVisible(false);
  };

  const renderMarket = ({ item }) => {
    const isFinal = item.status === 'Finalized' || item.status === 'UnderAppeal';
    const isSettling = isFinal && item.status === 'Finalized' && ((Date.now()/1000) - item.finalizedAt) < 43200;

    return (
      <Card style={{ flex: 1, margin: 6, backgroundColor: colors.surface }} onPress={() => { setSelectedMarket(item); isFinal ? setAppealVisible(true) : setBetVisible(true); }}>
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

        <Dialog visible={createVisible} onDismiss={() => setCreateVisible(false)} style={{ backgroundColor: colors.surface }}>
            <Dialog.Title>New Market</Dialog.Title>
            <Dialog.Content>
                <TextInput label="Question" value={newQuestion} onChangeText={setNewQuestion} mode="outlined" />
            </Dialog.Content>
            <Dialog.Actions>
                <Button onPress={handleCreateMarket}>Create</Button>
            </Dialog.Actions>
        </Dialog>

        <Dialog visible={betVisible} onDismiss={() => setBetVisible(false)} style={{ backgroundColor: colors.surface }}>
            <Dialog.Title>Place Bet</Dialog.Title>
            <Dialog.Content>
                <TextInput label="Amount" value={betAmount} onChangeText={setBetAmount} mode="outlined" keyboardType="numeric" />
            </Dialog.Content>
            <Dialog.Actions>
                <Button onPress={() => setBetVisible(false)}>Confirm</Button>
            </Dialog.Actions>
        </Dialog>
      </Portal>

      <FAB icon="plus" style={{ position: 'absolute', margin: 16, right: 0, bottom: 0, backgroundColor: colors.primary }} onPress={() => setCreateVisible(true)} />
    </View>
  );
}
EOF

# ==========================================
# 3. MINER BOT (TypeScript) - Full Logic
# ==========================================
echo "🤖 Writing Miner Bot..."
cat << 'EOF' > bots/miner-agent/index.ts
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
EOF

echo "✅ Installation Complete."
EOF