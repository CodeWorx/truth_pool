#!/bin/bash

# TruthPool Supplemental Installer
# Fills in the missing UI/Theme files for the Mobile App
# Usage: chmod +x finish_setup.sh && ./finish_setup.sh

echo "🎨 Installing UI Components & Themes..."

cd truth-pool

# ==========================================
# 1. APP THEME (Dark Mode)
# ==========================================
echo "Writing src/theme/solanaTheme.ts..."
cat << 'EOF' > src/theme/solanaTheme.ts
import { MD3DarkTheme as PaperDarkTheme } from 'react-native-paper';
import { DarkTheme as NavigationDarkTheme } from '@react-navigation/native';

const SOLANA_PURPLE = '#9945FF';
const SOLANA_GREEN = '#14F195';
const BACKGROUND_BLACK = '#000000';
const SURFACE_DARK = '#121212';
const SURFACE_VARIANT = '#1E1E1E';

export const SolanaTheme = {
  ...PaperDarkTheme,
  ...NavigationDarkTheme,
  colors: {
    ...PaperDarkTheme.colors,
    ...NavigationDarkTheme.colors,
    primary: SOLANA_PURPLE,
    onPrimary: '#FFFFFF',
    primaryContainer: '#3B1F64',
    onPrimaryContainer: '#EADDFF',
    secondary: SOLANA_GREEN,
    onSecondary: '#003921',
    secondaryContainer: '#005231',
    onSecondaryContainer: '#93F8C5',
    background: BACKGROUND_BLACK,
    surface: SURFACE_DARK,
    surfaceVariant: SURFACE_VARIANT,
    onSurface: '#E6E1E5',
    onSurfaceVariant: '#CAC4D0',
    error: '#FFB4AB',
    onError: '#690005',
    elevation: {
        level0: 'transparent',
        level1: '#1E1E1E',
        level2: '#222222',
        level3: '#252525',
        level4: '#272727',
        level5: '#2C2C2C',
    }
  },
  roundness: 4,
};
EOF

# ==========================================
# 2. WALLET CONTEXT (Session Caching)
# ==========================================
echo "Writing src/context/SolanaContext.tsx..."
cat << 'EOF' > src/context/SolanaContext.tsx
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
EOF

# ==========================================
# 3. DASHBOARD SCREEN
# ==========================================
echo "Writing src/screens/Dashboard.tsx..."
cat << 'EOF' > src/screens/Dashboard.tsx
import React from 'react';
import { View, ScrollView, RefreshControl } from 'react-native';
import { Text, Card, Button, Avatar, useTheme, Surface } from 'react-native-paper';
import { useSolana } from '../context/SolanaContext';
import { Bot, Trophy, Wallet, Activity } from 'lucide-react-native';

export default function Dashboard() {
  const { colors } = useTheme();
  const { publicKey, connect, isConnected } = useSolana();
  const [refreshing, setRefreshing] = React.useState(false);

  const stats = { activeBots: 3, totalEarnings: 12.54, winRate: '68%' };

  const onRefresh = React.useCallback(() => {
    setRefreshing(true);
    setTimeout(() => setRefreshing(false), 2000);
  }, []);

  if (!isConnected) {
    return (
      <View style={{ flex: 1, justifyContent: 'center', alignItems: 'center', backgroundColor: colors.background }}>
        <Wallet size={64} color={colors.primary} />
        <Text variant="headlineMedium" style={{ marginTop: 20, color: colors.onBackground, fontWeight: 'bold' }}>TruthPool Seeker</Text>
        <Text variant="bodyMedium" style={{ color: colors.onSurfaceVariant, marginBottom: 30 }}>Connect Seed Vault</Text>
        <Button mode="contained" onPress={connect} style={{ width: 200 }}>Connect</Button>
      </View>
    );
  }

  return (
    <ScrollView 
      style={{ flex: 1, backgroundColor: colors.background }}
      refreshControl={<RefreshControl refreshing={refreshing} onRefresh={onRefresh} tintColor={colors.primary} />}
    >
      <View style={{ padding: 20 }}>
        <Text variant="headlineSmall" style={{ color: colors.onBackground, fontWeight: 'bold' }}>Command Center</Text>
        <Text variant="labelLarge" style={{ color: colors.secondary }}>{publicKey?.toBase58().slice(0, 8)}...</Text>
        
        <Surface style={{ marginTop: 20, padding: 20, borderRadius: 16, backgroundColor: colors.primaryContainer }} elevation={2}>
          <View style={{ flexDirection: 'row', justifyContent: 'space-between', alignItems: 'center' }}>
            <View>
              <Text variant="labelMedium" style={{ color: colors.onPrimaryContainer }}>Total Earnings</Text>
              <Text variant="displayMedium" style={{ color: colors.primary, fontWeight: 'bold' }}>{stats.totalEarnings} SOL</Text>
            </View>
            <Trophy size={40} color={colors.primary} />
          </View>
        </Surface>

        <View style={{ flexDirection: 'row', gap: 12, marginTop: 12 }}>
          <Card style={{ flex: 1, backgroundColor: colors.surfaceVariant }}>
            <Card.Content>
              <Activity size={24} color={colors.secondary} />
              <Text variant="titleLarge" style={{ marginTop: 8 }}>{stats.activeBots}</Text>
              <Text variant="labelSmall">Active Bots</Text>
            </Card.Content>
          </Card>
          <Card style={{ flex: 1, backgroundColor: colors.surfaceVariant }}>
            <Card.Content>
              <Bot size={24} color={colors.secondary} />
              <Text variant="titleLarge" style={{ marginTop: 8 }}>{stats.winRate}</Text>
              <Text variant="labelSmall">Win Rate</Text>
            </Card.Content>
          </Card>
        </View>
      </View>
    </ScrollView>
  );
}
EOF

# ==========================================
# 4. MAIN APP ENTRY (Navigation)
# ==========================================
echo "Writing App.tsx..."
cat << 'EOF' > App.tsx
import 'react-native-get-random-values';
import 'text-encoding-polyfill';
import { Buffer } from 'buffer';
global.Buffer = Buffer;

import React from 'react';
import { PaperProvider } from 'react-native-paper';
import { NavigationContainer } from '@react-navigation/native';
import { createBottomTabNavigator } from '@react-navigation/bottom-tabs';
import { SafeAreaProvider } from 'react-native-safe-area-context';
import { Activity, LayoutDashboard, Server, TrendingUp } from 'lucide-react-native';

import { SolanaTheme } from './src/theme/solanaTheme';
import { SolanaProvider } from './src/context/SolanaContext';

import Dashboard from './src/screens/Dashboard';
import BotManager from './src/screens/BotManager';
import PredictionMarket from './src/screens/PredictionMarket';

const Tab = createBottomTabNavigator();

export default function App() {
  return (
    <SafeAreaProvider>
      <SolanaProvider>
        <PaperProvider theme={SolanaTheme}>
          <NavigationContainer theme={SolanaTheme}>
            <Tab.Navigator
              screenOptions={({ theme }) => ({
                headerShown: false,
                tabBarStyle: {
                  backgroundColor: theme.colors.surface,
                  borderTopColor: theme.colors.surfaceVariant,
                  height: 60,
                  paddingBottom: 8
                },
                tabBarActiveTintColor: theme.colors.primary,
                tabBarInactiveTintColor: theme.colors.onSurfaceVariant,
              })}
            >
              <Tab.Screen 
                name="Dashboard" 
                component={Dashboard} 
                options={{ tabBarIcon: ({ color, size }) => <LayoutDashboard color={color} size={size} /> }}
              />
              <Tab.Screen 
                name="Bots" 
                component={BotManager} 
                options={{ tabBarIcon: ({ color, size }) => <Server color={color} size={size} /> }}
              />
              <Tab.Screen 
                name="Markets" 
                component={PredictionMarket} 
                options={{ tabBarIcon: ({ color, size }) => <TrendingUp color={color} size={size} /> }}
              />
            </Tab.Navigator>
          </NavigationContainer>
        </PaperProvider>
      </SolanaProvider>
    </SafeAreaProvider>
  );
}
EOF

echo "✅ Supplemental Install Complete. All files are now present."