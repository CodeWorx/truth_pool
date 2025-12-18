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
