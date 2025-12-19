import React, {
  createContext,
  useContext,
  useState,
  useMemo,
  useCallback,
  useEffect,
  ReactNode,
} from "react";
import {
  Connection,
  PublicKey,
  clusterApiUrl,
  Transaction,
  VersionedTransaction,
} from "@solana/web3.js";
import { transact } from "@solana-mobile/mobile-wallet-adapter-protocol-web3js";
import AsyncStorage from "@react-native-async-storage/async-storage";

// ============================================
// TYPES
// ============================================

interface SolanaContextState {
  connection: Connection;
  publicKey: PublicKey | null;
  connect: () => Promise<void>;
  disconnect: () => Promise<void>;
  signAndSend: (
    transaction: Transaction | VersionedTransaction
  ) => Promise<string | null>;
  signTransaction: (
    transaction: Transaction | VersionedTransaction
  ) => Promise<Transaction | VersionedTransaction | null>;
  isConnected: boolean;
  isConnecting: boolean;
  error: string | null;
}

interface SolanaProviderProps {
  children: ReactNode;
  cluster?: "devnet" | "mainnet-beta" | "testnet";
  customRpcUrl?: string;
}

// ============================================
// CONSTANTS
// ============================================

const AUTH_TOKEN_KEY = "truthpool_auth_token";
const PUBKEY_CACHE_KEY = "truthpool_cached_pubkey";

const APP_IDENTITY = {
  name: "TruthPool Seeker",
  uri: "https://truthpool.io",
  icon: "favicon.ico",
};

// ============================================
// CONTEXT
// ============================================

const SolanaContext = createContext<SolanaContextState | null>(null);

export const SolanaProvider: React.FC<SolanaProviderProps> = ({
  children,
  cluster = "devnet",
  customRpcUrl,
}) => {
  const [publicKey, setPublicKey] = useState<PublicKey | null>(null);
  const [authToken, setAuthToken] = useState<string | null>(null);
  const [isConnecting, setIsConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Initialize connection
  const connection = useMemo(() => {
    const url = customRpcUrl || clusterApiUrl(cluster);
    return new Connection(url, "confirmed");
  }, [cluster, customRpcUrl]);

  // Load cached auth on mount
  useEffect(() => {
    const loadCachedAuth = async () => {
      try {
        const [token, cachedPubkey] = await Promise.all([
          AsyncStorage.getItem(AUTH_TOKEN_KEY),
          AsyncStorage.getItem(PUBKEY_CACHE_KEY),
        ]);

        if (token) {
          setAuthToken(token);
        }

        // Restore cached pubkey for UI (will revalidate on next transaction)
        if (cachedPubkey) {
          try {
            setPublicKey(new PublicKey(cachedPubkey));
          } catch {
            // Invalid cached pubkey, clear it
            await AsyncStorage.removeItem(PUBKEY_CACHE_KEY);
          }
        }
      } catch (err) {
        console.error("Failed to load cached auth:", err);
      }
    };

    loadCachedAuth();
  }, []);

  // Connect wallet
  const connect = useCallback(async () => {
    if (isConnecting) return;

    setIsConnecting(true);
    setError(null);

    try {
      await transact(async (wallet) => {
        const auth = await wallet.authorize({
          cluster,
          identity: APP_IDENTITY,
          auth_token: authToken || undefined,
        });

        if (!auth.accounts || auth.accounts.length === 0) {
          throw new Error("No accounts returned from wallet");
        }

        const newPubkey = new PublicKey(auth.accounts[0].address);
        setPublicKey(newPubkey);
        setAuthToken(auth.auth_token);

        // Cache for persistence
        await Promise.all([
          AsyncStorage.setItem(AUTH_TOKEN_KEY, auth.auth_token),
          AsyncStorage.setItem(PUBKEY_CACHE_KEY, newPubkey.toBase58()),
        ]);
      });
    } catch (err: any) {
      console.error("Wallet connection error:", err);
      setError(err.message || "Failed to connect wallet");
      setPublicKey(null);
    } finally {
      setIsConnecting(false);
    }
  }, [authToken, cluster, isConnecting]);

  // Disconnect wallet
  const disconnect = useCallback(async () => {
    setPublicKey(null);
    setAuthToken(null);
    setError(null);

    await Promise.all([
      AsyncStorage.removeItem(AUTH_TOKEN_KEY),
      AsyncStorage.removeItem(PUBKEY_CACHE_KEY),
    ]).catch(console.error);
  }, []);

  // Sign and send transaction
  const signAndSend = useCallback(
    async (
      transaction: Transaction | VersionedTransaction
    ): Promise<string | null> => {
      setError(null);

      try {
        return await transact(async (wallet) => {
          // Reauthorize to ensure session is valid
          const auth = await wallet.authorize({
            cluster,
            identity: APP_IDENTITY,
            auth_token: authToken || undefined,
          });

          // Update auth token if changed
          if (auth.auth_token !== authToken) {
            setAuthToken(auth.auth_token);
            await AsyncStorage.setItem(AUTH_TOKEN_KEY, auth.auth_token);
          }

          // Sign and send
          const signatures = await wallet.signAndSendTransactions({
            transactions: [transaction],
          });

          if (!signatures || signatures.length === 0) {
            throw new Error("No signature returned");
          }

          return signatures[0];
        });
      } catch (err: any) {
        console.error("Transaction error:", err);
        setError(err.message || "Transaction failed");
        return null;
      }
    },
    [authToken, cluster]
  );

  // Sign transaction (without sending)
  const signTransaction = useCallback(
    async (
      transaction: Transaction | VersionedTransaction
    ): Promise<Transaction | VersionedTransaction | null> => {
      setError(null);

      try {
        return await transact(async (wallet) => {
          const auth = await wallet.authorize({
            cluster,
            identity: APP_IDENTITY,
            auth_token: authToken || undefined,
          });

          if (auth.auth_token !== authToken) {
            setAuthToken(auth.auth_token);
            await AsyncStorage.setItem(AUTH_TOKEN_KEY, auth.auth_token);
          }

          const signedTransactions = await wallet.signTransactions({
            transactions: [transaction],
          });

          if (!signedTransactions || signedTransactions.length === 0) {
            throw new Error("No signed transaction returned");
          }

          return signedTransactions[0];
        });
      } catch (err: any) {
        console.error("Signing error:", err);
        setError(err.message || "Signing failed");
        return null;
      }
    },
    [authToken, cluster]
  );

  const value: SolanaContextState = useMemo(
    () => ({
      connection,
      publicKey,
      connect,
      disconnect,
      signAndSend,
      signTransaction,
      isConnected: !!publicKey,
      isConnecting,
      error,
    }),
    [
      connection,
      publicKey,
      connect,
      disconnect,
      signAndSend,
      signTransaction,
      isConnecting,
      error,
    ]
  );

  return (
    <SolanaContext.Provider value={value}>{children}</SolanaContext.Provider>
  );
};

// ============================================
// HOOK
// ============================================

export const useSolana = (): SolanaContextState => {
  const context = useContext(SolanaContext);

  if (!context) {
    throw new Error("useSolana must be used within a SolanaProvider");
  }

  return context;
};

export default SolanaContext;
