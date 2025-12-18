import React, { createContext, useContext, useState, useMemo, useCallback, useEffect } from 'react';
import { Connection, PublicKey, clusterApiUrl } from '@solana/web3.js';
import { transact } from '@solana-mobile/mobile-wallet-adapter-protocol-web3js';
import AsyncStorage from '@react-native-async-storage/async-storage';

interface SolanaContextState {
  connection: Connection;
  publicKey: PublicKey | null;
  connect: () => Promise<void>;
  disconnect: () => void;
  signAndSend: (transaction: any) => Promise<string | null>;
  isConnected: boolean;
}

const SolanaContext = createContext<SolanaContextState>({} as SolanaContextState);
const AUTH_TOKEN_KEY = 'truthpool_auth_token';

export const SolanaProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [publicKey, setPublicKey] = useState<PublicKey | null>(null);
  const [authToken, setAuthToken] = useState<string | null>(null);
  
  // Use Helius/Triton in prod
  const connection = useMemo(() => new Connection(clusterApiUrl('devnet'), 'confirmed'), []);

  useEffect(() => {
    AsyncStorage.getItem(AUTH_TOKEN_KEY).then(token => {
        if (token) setAuthToken(token);
    });
  }, []);

  const connect = useCallback(async () => {
    try {
      await transact(async (wallet) => {
        const auth = await wallet.authorize({
          cluster: 'devnet',
          identity: { name: 'TruthPool Seeker', uri: 'https://truthpool.io', icon: 'favicon.ico' },
          auth_token: authToken || undefined,
        });
        setPublicKey(new PublicKey(auth.accounts[0].address));
        setAuthToken(auth.auth_token);
        await AsyncStorage.setItem(AUTH_TOKEN_KEY, auth.auth_token);
      });
    } catch (err) {
      console.error('Wallet Error:', err);
      setPublicKey(null);
    }
  }, [authToken]);

  const disconnect = useCallback(async () => {
    setPublicKey(null);
    setAuthToken(null);
    await AsyncStorage.removeItem(AUTH_TOKEN_KEY);
  }, []);

  const signAndSend = useCallback(async (transaction: any) => {
    try {
      return await transact(async (wallet) => {
        const auth = await wallet.authorize({ 
            cluster: 'devnet', 
            identity: { name: 'TruthPool Seeker', uri: 'https://truthpool.io', icon: 'favicon.ico' },
            auth_token: authToken || undefined 
        });
        if (auth.auth_token !== authToken) {
            setAuthToken(auth.auth_token);
            await AsyncStorage.setItem(AUTH_TOKEN_KEY, auth.auth_token);
        }
        const sigs = await wallet.signAndSendTransactions({ transactions: [transaction] });
        return sigs[0];
      });
    } catch (err) {
      console.error('Signing Error:', err);
      return null;
    }
  }, [authToken]);

  return (
    <SolanaContext.Provider value={{ connection, publicKey, connect, disconnect, signAndSend, isConnected: !!publicKey }}>
      {children}
    </SolanaContext.Provider>
  );
};

export const useSolana = () => useContext(SolanaContext);
