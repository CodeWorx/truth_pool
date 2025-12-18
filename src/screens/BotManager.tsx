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
