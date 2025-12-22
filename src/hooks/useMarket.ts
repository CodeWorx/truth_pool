import { useState, useCallback, useEffect } from 'react';
import {
  PublicKey,
  Transaction,
  TransactionInstruction,
  SystemProgram,
  LAMPORTS_PER_SOL,
} from '@solana/web3.js';
import { useSolana } from '../context/SolanaContext';
import {
  PROGRAM_ID,
  BET_PRICE_LAMPORTS,
  MarketStatus,
  BetMarket,
  UserBet,
  getMarketPDA,
  getUserBetPDA,
} from '../types/market';

// Instruction discriminators (first 8 bytes of sha256 hash of instruction name)
const DISCRIMINATORS = {
  buyBet: Buffer.from([139, 124, 73, 18, 21, 56, 156, 116]),
  sellBet: Buffer.from([168, 167, 172, 216, 137, 27, 174, 55]),
  redeemWinnings: Buffer.from([149, 95, 181, 242, 94, 90, 158, 162]),
  claimRefund: Buffer.from([132, 225, 180, 57, 16, 252, 229, 52]),
};

export interface MarketWithMeta extends BetMarket {
  publicKey: PublicKey;
  question?: string;
}

export interface UserPosition {
  yesBets: number;
  noBets: number;
  yesAmount: number;
  noAmount: number;
  hasRedeemed: boolean;
  potentialYesPayout: number;
  potentialNoPayout: number;
}

export function useMarket() {
  const { connection, publicKey, signAndSend, isConnected } = useSolana();
  const [markets, setMarkets] = useState<MarketWithMeta[]>([]);
  const [userPositions, setUserPositions] = useState<Map<string, UserPosition>>(new Map());
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Fetch all markets from chain
  const fetchMarkets = useCallback(async () => {
    if (!connection) return;

    setLoading(true);
    setError(null);

    try {
      // Fetch all BetMarket accounts
      const accounts = await connection.getProgramAccounts(PROGRAM_ID, {
        filters: [
          { dataSize: 8 + 68 + 32 + 32 + 8 + 8 + 8 + 8 + 8 + 1 + 2 + 8 }, // Approximate BetMarket size
        ],
      });

      const parsedMarkets: MarketWithMeta[] = [];

      for (const { pubkey, account } of accounts) {
        try {
          const data = account.data;
          // Skip discriminator (8 bytes)
          let offset = 8;

          // Parse market_id (string with 4-byte length prefix + max 64 bytes)
          const marketIdLen = data.readUInt32LE(offset);
          offset += 4;
          const marketId = data.slice(offset, offset + marketIdLen).toString('utf8');
          offset += 64; // max_len

          // Parse oracle_query (32 bytes)
          const oracleQuery = new PublicKey(data.slice(offset, offset + 32));
          offset += 32;

          // Parse creator (32 bytes)
          const creator = new PublicKey(data.slice(offset, offset + 32));
          offset += 32;

          // Parse timestamps and amounts (all i64/u64 = 8 bytes each)
          const lockTimestamp = Number(data.readBigInt64LE(offset));
          offset += 8;
          const totalYesBets = Number(data.readBigUInt64LE(offset));
          offset += 8;
          const totalNoBets = Number(data.readBigUInt64LE(offset));
          offset += 8;
          const totalYesAmount = Number(data.readBigUInt64LE(offset));
          offset += 8;
          const totalNoAmount = Number(data.readBigUInt64LE(offset));
          offset += 8;

          // Parse status (1 byte enum)
          const status = data.readUInt8(offset) as MarketStatus;
          offset += 1;

          // Parse winning_side (Option<bool> = 1 byte for Some/None + 1 byte for value)
          const hasWinningSide = data.readUInt8(offset) === 1;
          offset += 1;
          const winningSide = hasWinningSide ? data.readUInt8(offset) === 1 : null;
          offset += 1;

          // Parse created_at
          const createdAt = Number(data.readBigInt64LE(offset));

          parsedMarkets.push({
            publicKey: pubkey,
            marketId,
            oracleQuery,
            creator,
            lockTimestamp,
            totalYesBets,
            totalNoBets,
            totalYesAmount,
            totalNoAmount,
            status,
            winningSide,
            createdAt,
            question: marketId, // Use marketId as question for now
          });
        } catch (parseErr) {
          console.warn('Failed to parse market account:', pubkey.toBase58(), parseErr);
        }
      }

      setMarkets(parsedMarkets);
    } catch (err: any) {
      console.error('Failed to fetch markets:', err);
      setError(err.message || 'Failed to fetch markets');
    } finally {
      setLoading(false);
    }
  }, [connection]);

  // Fetch user's position in a market
  const fetchUserPosition = useCallback(async (marketPubkey: PublicKey): Promise<UserPosition | null> => {
    if (!connection || !publicKey) return null;

    try {
      const [userBetPDA] = getUserBetPDA(marketPubkey, publicKey);
      const accountInfo = await connection.getAccountInfo(userBetPDA);

      if (!accountInfo) return null;

      const data = accountInfo.data;
      let offset = 8; // Skip discriminator

      // Skip market pubkey (32 bytes)
      offset += 32;
      // Skip bettor pubkey (32 bytes)
      offset += 32;

      const yesBets = Number(data.readBigUInt64LE(offset));
      offset += 8;
      const noBets = Number(data.readBigUInt64LE(offset));
      offset += 8;
      const yesAmount = Number(data.readBigUInt64LE(offset));
      offset += 8;
      const noAmount = Number(data.readBigUInt64LE(offset));
      offset += 8;
      const hasRedeemed = data.readUInt8(offset) === 1;

      // Find market to calculate potential payouts
      const market = markets.find(m => m.publicKey.equals(marketPubkey));
      const totalPool = market ? market.totalYesAmount + market.totalNoAmount : 0;

      return {
        yesBets,
        noBets,
        yesAmount,
        noAmount,
        hasRedeemed,
        potentialYesPayout: market && market.totalYesBets > 0
          ? (yesBets / market.totalYesBets) * totalPool
          : 0,
        potentialNoPayout: market && market.totalNoBets > 0
          ? (noBets / market.totalNoBets) * totalPool
          : 0,
      };
    } catch (err) {
      console.error('Failed to fetch user position:', err);
      return null;
    }
  }, [connection, publicKey, markets]);

  // Buy bet instruction
  const buyBet = useCallback(async (
    marketPubkey: PublicKey,
    betCount: number,
    side: boolean // true = YES, false = NO
  ): Promise<string | null> => {
    if (!publicKey || !connection) {
      setError('Wallet not connected');
      return null;
    }

    setLoading(true);
    setError(null);

    try {
      const [userBetPDA] = getUserBetPDA(marketPubkey, publicKey);

      // Build instruction data
      const data = Buffer.alloc(8 + 8 + 1);
      DISCRIMINATORS.buyBet.copy(data, 0);
      data.writeBigUInt64LE(BigInt(betCount), 8);
      data.writeUInt8(side ? 1 : 0, 16);

      const instruction = new TransactionInstruction({
        keys: [
          { pubkey: publicKey, isSigner: true, isWritable: true },
          { pubkey: marketPubkey, isSigner: false, isWritable: true },
          { pubkey: userBetPDA, isSigner: false, isWritable: true },
          { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
        ],
        programId: PROGRAM_ID,
        data,
      });

      const transaction = new Transaction().add(instruction);
      transaction.feePayer = publicKey;
      transaction.recentBlockhash = (await connection.getLatestBlockhash()).blockhash;

      const signature = await signAndSend(transaction);

      if (signature) {
        // Refresh markets after bet
        await fetchMarkets();
      }

      return signature;
    } catch (err: any) {
      console.error('Buy bet failed:', err);
      setError(err.message || 'Failed to place bet');
      return null;
    } finally {
      setLoading(false);
    }
  }, [publicKey, connection, signAndSend, fetchMarkets]);

  // Sell bet instruction
  const sellBet = useCallback(async (
    marketPubkey: PublicKey,
    sellCount: number,
    side: boolean
  ): Promise<string | null> => {
    if (!publicKey || !connection) {
      setError('Wallet not connected');
      return null;
    }

    setLoading(true);
    setError(null);

    try {
      const [userBetPDA] = getUserBetPDA(marketPubkey, publicKey);

      const data = Buffer.alloc(8 + 8 + 1);
      DISCRIMINATORS.sellBet.copy(data, 0);
      data.writeBigUInt64LE(BigInt(sellCount), 8);
      data.writeUInt8(side ? 1 : 0, 16);

      const instruction = new TransactionInstruction({
        keys: [
          { pubkey: publicKey, isSigner: true, isWritable: true },
          { pubkey: marketPubkey, isSigner: false, isWritable: true },
          { pubkey: userBetPDA, isSigner: false, isWritable: true },
        ],
        programId: PROGRAM_ID,
        data,
      });

      const transaction = new Transaction().add(instruction);
      transaction.feePayer = publicKey;
      transaction.recentBlockhash = (await connection.getLatestBlockhash()).blockhash;

      const signature = await signAndSend(transaction);

      if (signature) {
        await fetchMarkets();
      }

      return signature;
    } catch (err: any) {
      console.error('Sell bet failed:', err);
      setError(err.message || 'Failed to sell bet');
      return null;
    } finally {
      setLoading(false);
    }
  }, [publicKey, connection, signAndSend, fetchMarkets]);

  // Redeem winnings instruction
  const redeemWinnings = useCallback(async (
    marketPubkey: PublicKey
  ): Promise<string | null> => {
    if (!publicKey || !connection) {
      setError('Wallet not connected');
      return null;
    }

    setLoading(true);
    setError(null);

    try {
      const [userBetPDA] = getUserBetPDA(marketPubkey, publicKey);

      const data = Buffer.alloc(8);
      DISCRIMINATORS.redeemWinnings.copy(data, 0);

      const instruction = new TransactionInstruction({
        keys: [
          { pubkey: publicKey, isSigner: true, isWritable: true },
          { pubkey: marketPubkey, isSigner: false, isWritable: true },
          { pubkey: userBetPDA, isSigner: false, isWritable: true },
        ],
        programId: PROGRAM_ID,
        data,
      });

      const transaction = new Transaction().add(instruction);
      transaction.feePayer = publicKey;
      transaction.recentBlockhash = (await connection.getLatestBlockhash()).blockhash;

      const signature = await signAndSend(transaction);

      if (signature) {
        await fetchMarkets();
      }

      return signature;
    } catch (err: any) {
      console.error('Redeem winnings failed:', err);
      setError(err.message || 'Failed to redeem winnings');
      return null;
    } finally {
      setLoading(false);
    }
  }, [publicKey, connection, signAndSend, fetchMarkets]);

  // Claim refund instruction (for cancelled markets)
  const claimRefund = useCallback(async (
    marketPubkey: PublicKey
  ): Promise<string | null> => {
    if (!publicKey || !connection) {
      setError('Wallet not connected');
      return null;
    }

    setLoading(true);
    setError(null);

    try {
      const [userBetPDA] = getUserBetPDA(marketPubkey, publicKey);

      const data = Buffer.alloc(8);
      DISCRIMINATORS.claimRefund.copy(data, 0);

      const instruction = new TransactionInstruction({
        keys: [
          { pubkey: publicKey, isSigner: true, isWritable: true },
          { pubkey: marketPubkey, isSigner: false, isWritable: true },
          { pubkey: userBetPDA, isSigner: false, isWritable: true },
        ],
        programId: PROGRAM_ID,
        data,
      });

      const transaction = new Transaction().add(instruction);
      transaction.feePayer = publicKey;
      transaction.recentBlockhash = (await connection.getLatestBlockhash()).blockhash;

      const signature = await signAndSend(transaction);

      if (signature) {
        await fetchMarkets();
      }

      return signature;
    } catch (err: any) {
      console.error('Claim refund failed:', err);
      setError(err.message || 'Failed to claim refund');
      return null;
    } finally {
      setLoading(false);
    }
  }, [publicKey, connection, signAndSend, fetchMarkets]);

  // Auto-fetch markets on mount and when connection changes
  useEffect(() => {
    if (connection) {
      fetchMarkets();
    }
  }, [connection, fetchMarkets]);

  return {
    markets,
    userPositions,
    loading,
    error,
    isConnected,
    fetchMarkets,
    fetchUserPosition,
    buyBet,
    sellBet,
    redeemWinnings,
    claimRefund,
    BET_PRICE_SOL: BET_PRICE_LAMPORTS / LAMPORTS_PER_SOL,
    CANCELLATION_FEE_PERCENT: 10,
  };
}

export default useMarket;
