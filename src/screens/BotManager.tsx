import React, { useState } from 'react';
import { View, FlatList } from 'react-native';
import { Text, Card, Button, FAB, Dialog, Portal, TextInput, useTheme, Chip, SegmentedButtons } from 'react-native-paper';
import { Shield, Fuel } from 'lucide-react-native';

const INITIAL_BOTS = [
  { id: '1', name: 'NBA Sniper', type: 'Miner', stake: 1.0, activeVotes: 0, maxVotes: 2, status: 'Running' },
];

export default function BotManager() {
  const { colors } = useTheme();
  const [bots, setBots] = useState(INITIAL_BOTS);
  const [visible, setVisible] = useState(false);
  const [config, setConfig] = useState({ name: '', type: 'Miner', stake: '1' });

  const handleDeploy = () => {
    setBots([...bots, { id: Math.random().toString(), name: config.name, type: config.type, stake: parseFloat(config.stake), activeVotes: 0, maxVotes: parseFloat(config.stake)/0.5, status: 'Starting' }]);
    setVisible(false);
  };

  return (
    <View style={{ flex: 1, backgroundColor: colors.background }}>
      <FlatList
        data={bots}
        renderItem={({ item }) => (
            <Card style={{ margin: 10, backgroundColor: colors.surface }}>
                <Card.Content>
                    <View style={{ flexDirection: 'row', justifyContent: 'space-between' }}>
                        <Text style={{ fontWeight: 'bold' }}>{item.name}</Text>
                        <Chip>{item.type}</Chip>
                    </View>
                    <Text style={{ marginTop: 5 }}>Stake: {item.stake} SOL</Text>
                </Card.Content>
            </Card>
        )}
      />
      <FAB icon="robot" style={{ position: 'absolute', margin: 16, right: 0, bottom: 0, backgroundColor: colors.primary }} onPress={() => setVisible(true)} />
      <Portal>
        <Dialog visible={visible} onDismiss={() => setVisible(false)} style={{ backgroundColor: colors.surface }}>
            <Dialog.Title>Deploy Bot</Dialog.Title>
            <Dialog.Content>
                <TextInput label="Name" value={config.name} onChangeText={t => setConfig({...config, name: t})} mode="outlined" style={{marginBottom: 10}} />
                <SegmentedButtons value={config.type} onValueChange={v => setConfig({...config, type: v})} buttons={[{ value: 'Miner', label: 'Miner' }, { value: 'Whale', label: 'Guardian' }]} />
                <TextInput label="Stake" value={config.stake} onChangeText={t => setConfig({...config, stake: t})} mode="outlined" style={{marginTop: 10}} />
            </Dialog.Content>
            <Dialog.Actions><Button onPress={handleDeploy}>Deploy</Button></Dialog.Actions>
        </Dialog>
      </Portal>
    </View>
  );
}
