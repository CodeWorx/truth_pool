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
