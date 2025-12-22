import { PublicKey } from '@solana/web3.js';

// Program ID
export const PROGRAM_ID = new PublicKey('TrutH6qfNhnAiVwMz2gxBkqGKxCrHZaQBFSTewxVV1j');

// Constants from smart contract
export const BET_PRICE_LAMPORTS = 1_000_000_000; // 1 SOL
export const CANCELLATION_FEE_BPS = 1000; // 10%

// Market status enum matching Rust
export enum MarketStatus {
  Open = 0,
  Locked = 1,
  Resolved = 2,
  Cancelled = 3,
}

// BetMarket account structure
export interface BetMarket {
  marketId: string;
  oracleQuery: PublicKey;
  creator: PublicKey;
  lockTimestamp: number;
  totalYesBets: number;
  totalNoBets: number;
  totalYesAmount: number;
  totalNoAmount: number;
  status: MarketStatus;
  winningSide: boolean | null;
  createdAt: number;
}

// UserBet account structure
export interface UserBet {
  market: PublicKey;
  bettor: PublicKey;
  yesBets: number;
  noBets: number;
  yesAmount: number;
  noAmount: number;
  hasRedeemed: boolean;
}

// Query account structure (for oracle)
export interface QueryAccount {
  uniqueEventId: string;
  categoryId: string;
  status: number;
  result: string;
}

// PDA seed constants
export const MARKET_SEED = 'market';
export const USER_BET_SEED = 'user_bet';
export const QUERY_SEED = 'query';

// Helper to derive market PDA
export function getMarketPDA(
  queryAccount: PublicKey,
  marketId: string,
  programId: PublicKey = PROGRAM_ID
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [
      Buffer.from(MARKET_SEED),
      queryAccount.toBuffer(),
      Buffer.from(marketId),
    ],
    programId
  );
}

// Helper to derive user bet PDA
export function getUserBetPDA(
  marketAccount: PublicKey,
  userPubkey: PublicKey,
  programId: PublicKey = PROGRAM_ID
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [
      Buffer.from(USER_BET_SEED),
      marketAccount.toBuffer(),
      userPubkey.toBuffer(),
    ],
    programId
  );
}

// Display helpers
export function formatSol(lamports: number): string {
  return (lamports / 1_000_000_000).toFixed(2);
}

export function getMarketStatusLabel(status: MarketStatus): string {
  switch (status) {
    case MarketStatus.Open:
      return 'Open';
    case MarketStatus.Locked:
      return 'Locked';
    case MarketStatus.Resolved:
      return 'Resolved';
    case MarketStatus.Cancelled:
      return 'Cancelled';
    default:
      return 'Unknown';
  }
}

export function calculateOdds(yesBets: number, noBets: number): { yesOdds: number; noOdds: number } {
  const total = yesBets + noBets;
  if (total === 0) {
    return { yesOdds: 0.5, noOdds: 0.5 };
  }
  return {
    yesOdds: yesBets / total,
    noOdds: noBets / total,
  };
}

export function calculatePotentialPayout(
  userBets: number,
  totalWinningBets: number,
  totalPool: number
): number {
  if (totalWinningBets === 0) return 0;
  return (userBets / totalWinningBets) * totalPool;
}
