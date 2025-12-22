import React, { useState, useEffect, useCallback } from 'react';
import { View, FlatList, Alert, StyleSheet, RefreshControl } from 'react-native';
import {
  Text,
  Card,
  Button,
  Dialog,
  Portal,
  useTheme,
  Chip,
  ProgressBar,
  IconButton,
  Divider,
  ActivityIndicator,
  Surface,
} from 'react-native-paper';
import { Clock, TrendingUp, TrendingDown, Lock, CheckCircle, XCircle, RefreshCw } from 'lucide-react-native';
import { useMarket, MarketWithMeta, UserPosition } from '../hooks/useMarket';
import { MarketStatus, formatSol, calculateOdds, getMarketStatusLabel } from '../types/market';

export default function PredictionMarket() {
  const { colors } = useTheme();
  const {
    markets,
    loading,
    error,
    isConnected,
    fetchMarkets,
    fetchUserPosition,
    buyBet,
    sellBet,
    redeemWinnings,
    claimRefund,
    BET_PRICE_SOL,
    CANCELLATION_FEE_PERCENT,
  } = useMarket();

  const [viewMode, setViewMode] = useState<'Live' | 'History'>('Live');
  const [selectedMarket, setSelectedMarket] = useState<MarketWithMeta | null>(null);
  const [userPosition, setUserPosition] = useState<UserPosition | null>(null);
  const [betDialogVisible, setBetDialogVisible] = useState(false);
  const [betCount, setBetCount] = useState(1);
  const [selectedSide, setSelectedSide] = useState<boolean | null>(null);
  const [refreshing, setRefreshing] = useState(false);

  // Filter markets by status
  const liveMarkets = markets.filter(m => m.status === MarketStatus.Open || m.status === MarketStatus.Locked);
  const historyMarkets = markets.filter(m => m.status === MarketStatus.Resolved || m.status === MarketStatus.Cancelled);

  const displayedMarkets = viewMode === 'Live' ? liveMarkets : historyMarkets;

  // Fetch user position when market is selected
  useEffect(() => {
    if (selectedMarket && isConnected) {
      fetchUserPosition(selectedMarket.publicKey).then(setUserPosition);
    } else {
      setUserPosition(null);
    }
  }, [selectedMarket, isConnected, fetchUserPosition]);

  const onRefresh = useCallback(async () => {
    setRefreshing(true);
    await fetchMarkets();
    setRefreshing(false);
  }, [fetchMarkets]);

  const handleOpenBetDialog = (market: MarketWithMeta) => {
    setSelectedMarket(market);
    setBetDialogVisible(true);
    setBetCount(1);
    setSelectedSide(null);
  };

  const handlePlaceBet = async () => {
    if (!selectedMarket || selectedSide === null) return;

    const signature = await buyBet(selectedMarket.publicKey, betCount, selectedSide);
    if (signature) {
      Alert.alert('Success', `Bet placed! ${betCount} x ${selectedSide ? 'YES' : 'NO'}`);
      setBetDialogVisible(false);
      // Refresh position
      const newPosition = await fetchUserPosition(selectedMarket.publicKey);
      setUserPosition(newPosition);
    }
  };

  const handleSellBet = async (side: boolean, count: number) => {
    if (!selectedMarket) return;

    const signature = await sellBet(selectedMarket.publicKey, count, side);
    if (signature) {
      Alert.alert('Success', `Sold ${count} x ${side ? 'YES' : 'NO'} bets (10% fee applied)`);
      const newPosition = await fetchUserPosition(selectedMarket.publicKey);
      setUserPosition(newPosition);
    }
  };

  const handleRedeem = async () => {
    if (!selectedMarket) return;

    const signature = await redeemWinnings(selectedMarket.publicKey);
    if (signature) {
      Alert.alert('Success', 'Winnings redeemed!');
      const newPosition = await fetchUserPosition(selectedMarket.publicKey);
      setUserPosition(newPosition);
    }
  };

  const handleClaimRefund = async () => {
    if (!selectedMarket) return;

    const signature = await claimRefund(selectedMarket.publicKey);
    if (signature) {
      Alert.alert('Success', 'Refund claimed!');
      const newPosition = await fetchUserPosition(selectedMarket.publicKey);
      setUserPosition(newPosition);
    }
  };

  const getTimeRemaining = (lockTimestamp: number): string => {
    const now = Math.floor(Date.now() / 1000);
    const diff = lockTimestamp - now;
    if (diff <= 0) return 'Locked';
    const hours = Math.floor(diff / 3600);
    const mins = Math.floor((diff % 3600) / 60);
    if (hours > 24) return `${Math.floor(hours / 24)}d ${hours % 24}h`;
    if (hours > 0) return `${hours}h ${mins}m`;
    return `${mins}m`;
  };

  const renderMarketCard = ({ item }: { item: MarketWithMeta }) => {
    const { yesOdds, noOdds } = calculateOdds(item.totalYesBets, item.totalNoBets);
    const totalPool = item.totalYesAmount + item.totalNoAmount;
    const isOpen = item.status === MarketStatus.Open;
    const isResolved = item.status === MarketStatus.Resolved;
    const isCancelled = item.status === MarketStatus.Cancelled;

    return (
      <Card
        style={[styles.card, { backgroundColor: colors.surface }]}
        onPress={() => handleOpenBetDialog(item)}
      >
        <Card.Content>
          <View style={styles.cardHeader}>
            <Chip
              icon={isOpen ? 'circle' : isResolved ? 'check' : 'close'}
              compact
              textStyle={{ fontSize: 10 }}
              style={{
                backgroundColor: isOpen
                  ? colors.primaryContainer
                  : isResolved
                  ? colors.tertiaryContainer
                  : colors.errorContainer,
              }}
            >
              {getMarketStatusLabel(item.status)}
            </Chip>
            {isOpen && (
              <View style={styles.timeContainer}>
                <Clock size={12} color={colors.onSurfaceVariant} />
                <Text variant="labelSmall" style={{ marginLeft: 4 }}>
                  {getTimeRemaining(item.lockTimestamp)}
                </Text>
              </View>
            )}
          </View>

          <Text variant="titleMedium" style={styles.question} numberOfLines={2}>
            {item.question || item.marketId}
          </Text>

          {isResolved && item.winningSide !== null && (
            <View style={[styles.resultBadge, { backgroundColor: colors.primaryContainer }]}>
              <Text style={{ color: colors.primary, fontWeight: 'bold' }}>
                Result: {item.winningSide ? 'YES' : 'NO'}
              </Text>
            </View>
          )}

          {isCancelled && (
            <View style={[styles.resultBadge, { backgroundColor: colors.errorContainer }]}>
              <Text style={{ color: colors.error }}>Cancelled - Refunds Available</Text>
            </View>
          )}

          <View style={styles.oddsContainer}>
            <View style={styles.oddsSide}>
              <TrendingUp size={16} color={colors.primary} />
              <Text variant="labelMedium" style={{ color: colors.primary, marginLeft: 4 }}>
                YES {(yesOdds * 100).toFixed(0)}%
              </Text>
              <Text variant="labelSmall" style={{ color: colors.onSurfaceVariant }}>
                ({item.totalYesBets} bets)
              </Text>
            </View>
            <View style={styles.oddsSide}>
              <TrendingDown size={16} color={colors.error} />
              <Text variant="labelMedium" style={{ color: colors.error, marginLeft: 4 }}>
                NO {(noOdds * 100).toFixed(0)}%
              </Text>
              <Text variant="labelSmall" style={{ color: colors.onSurfaceVariant }}>
                ({item.totalNoBets} bets)
              </Text>
            </View>
          </View>

          <ProgressBar
            progress={yesOdds}
            color={colors.primary}
            style={styles.progressBar}
          />

          <Text variant="labelSmall" style={styles.poolText}>
            Pool: {formatSol(totalPool)} SOL
          </Text>
        </Card.Content>
      </Card>
    );
  };

  const renderBetDialog = () => {
    if (!selectedMarket) return null;

    const { yesOdds, noOdds } = calculateOdds(selectedMarket.totalYesBets, selectedMarket.totalNoBets);
    const totalPool = selectedMarket.totalYesAmount + selectedMarket.totalNoAmount;
    const isOpen = selectedMarket.status === MarketStatus.Open;
    const isResolved = selectedMarket.status === MarketStatus.Resolved;
    const isCancelled = selectedMarket.status === MarketStatus.Cancelled;
    const canBet = isOpen && isConnected;
    const canRedeem = isResolved && userPosition && !userPosition.hasRedeemed;
    const canRefund = isCancelled && userPosition && !userPosition.hasRedeemed;

    // Calculate potential payout for selected bet
    const potentialPayout = selectedSide !== null
      ? ((betCount + (selectedSide ? selectedMarket.totalYesBets : selectedMarket.totalNoBets)) /
         (betCount + (selectedSide ? selectedMarket.totalYesBets : selectedMarket.totalNoBets))) *
        (totalPool + betCount * BET_PRICE_SOL * 1e9)
      : 0;

    return (
      <Portal>
        <Dialog
          visible={betDialogVisible}
          onDismiss={() => setBetDialogVisible(false)}
          style={{ backgroundColor: colors.surface }}
        >
          <Dialog.Title>{selectedMarket.question || selectedMarket.marketId}</Dialog.Title>
          <Dialog.Content>
            {/* Market Stats */}
            <Surface style={styles.statsContainer} elevation={1}>
              <View style={styles.statRow}>
                <Text variant="labelMedium">Status:</Text>
                <Chip compact>{getMarketStatusLabel(selectedMarket.status)}</Chip>
              </View>
              <View style={styles.statRow}>
                <Text variant="labelMedium">Total Pool:</Text>
                <Text variant="bodyMedium" style={{ fontWeight: 'bold' }}>
                  {formatSol(totalPool)} SOL
                </Text>
              </View>
              <View style={styles.statRow}>
                <Text variant="labelMedium">YES Bets:</Text>
                <Text variant="bodyMedium">{selectedMarket.totalYesBets} ({(yesOdds * 100).toFixed(1)}%)</Text>
              </View>
              <View style={styles.statRow}>
                <Text variant="labelMedium">NO Bets:</Text>
                <Text variant="bodyMedium">{selectedMarket.totalNoBets} ({(noOdds * 100).toFixed(1)}%)</Text>
              </View>
              {isOpen && (
                <View style={styles.statRow}>
                  <Text variant="labelMedium">Locks in:</Text>
                  <Text variant="bodyMedium">{getTimeRemaining(selectedMarket.lockTimestamp)}</Text>
                </View>
              )}
            </Surface>

            <Divider style={{ marginVertical: 16 }} />

            {/* User Position */}
            {userPosition && (userPosition.yesBets > 0 || userPosition.noBets > 0) && (
              <>
                <Text variant="titleSmall" style={{ marginBottom: 8 }}>Your Position</Text>
                <Surface style={styles.statsContainer} elevation={1}>
                  {userPosition.yesBets > 0 && (
                    <View style={styles.positionRow}>
                      <View>
                        <Text variant="bodyMedium">YES: {userPosition.yesBets} bets</Text>
                        <Text variant="labelSmall">
                          Potential: {formatSol(userPosition.potentialYesPayout)} SOL
                        </Text>
                      </View>
                      {isOpen && (
                        <Button
                          mode="outlined"
                          compact
                          onPress={() => handleSellBet(true, userPosition.yesBets)}
                        >
                          Sell (-10%)
                        </Button>
                      )}
                    </View>
                  )}
                  {userPosition.noBets > 0 && (
                    <View style={styles.positionRow}>
                      <View>
                        <Text variant="bodyMedium">NO: {userPosition.noBets} bets</Text>
                        <Text variant="labelSmall">
                          Potential: {formatSol(userPosition.potentialNoPayout)} SOL
                        </Text>
                      </View>
                      {isOpen && (
                        <Button
                          mode="outlined"
                          compact
                          onPress={() => handleSellBet(false, userPosition.noBets)}
                        >
                          Sell (-10%)
                        </Button>
                      )}
                    </View>
                  )}
                </Surface>
                <Divider style={{ marginVertical: 16 }} />
              </>
            )}

            {/* Place Bet Section */}
            {canBet && (
              <>
                <Text variant="titleSmall" style={{ marginBottom: 8 }}>Place Bet</Text>
                <Text variant="labelSmall" style={{ marginBottom: 12, color: colors.onSurfaceVariant }}>
                  Each bet costs {BET_PRICE_SOL} SOL. Sell before lock with {CANCELLATION_FEE_PERCENT}% fee.
                </Text>

                <View style={styles.betControls}>
                  <View style={styles.betCountContainer}>
                    <IconButton
                      icon="minus"
                      size={20}
                      onPress={() => setBetCount(Math.max(1, betCount - 1))}
                    />
                    <Text variant="headlineSmall" style={{ marginHorizontal: 16 }}>
                      {betCount}
                    </Text>
                    <IconButton
                      icon="plus"
                      size={20}
                      onPress={() => setBetCount(betCount + 1)}
                    />
                  </View>
                  <Text variant="bodyMedium">
                    = {(betCount * BET_PRICE_SOL).toFixed(2)} SOL
                  </Text>
                </View>

                <View style={styles.sideButtons}>
                  <Button
                    mode={selectedSide === true ? 'contained' : 'outlined'}
                    onPress={() => setSelectedSide(true)}
                    style={[styles.sideButton, { borderColor: colors.primary }]}
                    buttonColor={selectedSide === true ? colors.primary : undefined}
                  >
                    YES
                  </Button>
                  <Button
                    mode={selectedSide === false ? 'contained' : 'outlined'}
                    onPress={() => setSelectedSide(false)}
                    style={[styles.sideButton, { borderColor: colors.error }]}
                    buttonColor={selectedSide === false ? colors.error : undefined}
                  >
                    NO
                  </Button>
                </View>
              </>
            )}

            {/* Redeem/Refund Section */}
            {canRedeem && (
              <Button
                mode="contained"
                onPress={handleRedeem}
                style={{ marginTop: 16 }}
                buttonColor={colors.tertiary}
              >
                Redeem Winnings
              </Button>
            )}

            {canRefund && (
              <Button
                mode="contained"
                onPress={handleClaimRefund}
                style={{ marginTop: 16 }}
              >
                Claim Full Refund
              </Button>
            )}

            {!isConnected && (
              <Text style={{ textAlign: 'center', color: colors.error, marginTop: 16 }}>
                Connect wallet to place bets
              </Text>
            )}
          </Dialog.Content>

          <Dialog.Actions>
            <Button onPress={() => setBetDialogVisible(false)}>Close</Button>
            {canBet && selectedSide !== null && (
              <Button mode="contained" onPress={handlePlaceBet} loading={loading}>
                Place Bet
              </Button>
            )}
          </Dialog.Actions>
        </Dialog>
      </Portal>
    );
  };

  return (
    <View style={[styles.container, { backgroundColor: colors.background }]}>
      {/* Tab Buttons */}
      <View style={styles.tabContainer}>
        <Button
          mode={viewMode === 'Live' ? 'contained' : 'text'}
          onPress={() => setViewMode('Live')}
          style={styles.tabButton}
        >
          Live ({liveMarkets.length})
        </Button>
        <Button
          mode={viewMode === 'History' ? 'contained' : 'text'}
          onPress={() => setViewMode('History')}
          style={styles.tabButton}
        >
          History ({historyMarkets.length})
        </Button>
      </View>

      {/* Error Display */}
      {error && (
        <Surface style={[styles.errorBanner, { backgroundColor: colors.errorContainer }]}>
          <Text style={{ color: colors.error }}>{error}</Text>
        </Surface>
      )}

      {/* Loading State */}
      {loading && markets.length === 0 && (
        <View style={styles.loadingContainer}>
          <ActivityIndicator size="large" />
          <Text style={{ marginTop: 16 }}>Loading markets...</Text>
        </View>
      )}

      {/* Empty State */}
      {!loading && displayedMarkets.length === 0 && (
        <View style={styles.emptyContainer}>
          <Text variant="bodyLarge" style={{ color: colors.onSurfaceVariant }}>
            No {viewMode.toLowerCase()} markets found
          </Text>
          <Button
            mode="outlined"
            onPress={fetchMarkets}
            style={{ marginTop: 16 }}
            icon={() => <RefreshCw size={16} color={colors.primary} />}
          >
            Refresh
          </Button>
        </View>
      )}

      {/* Market List */}
      <FlatList
        data={displayedMarkets}
        renderItem={renderMarketCard}
        keyExtractor={(item) => item.publicKey.toBase58()}
        numColumns={1}
        contentContainerStyle={styles.listContent}
        refreshControl={
          <RefreshControl refreshing={refreshing} onRefresh={onRefresh} />
        }
      />

      {/* Bet Dialog */}
      {renderBetDialog()}
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
  },
  tabContainer: {
    flexDirection: 'row',
    padding: 8,
  },
  tabButton: {
    flex: 1,
    marginHorizontal: 4,
  },
  card: {
    marginHorizontal: 12,
    marginVertical: 6,
  },
  cardHeader: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    marginBottom: 8,
  },
  timeContainer: {
    flexDirection: 'row',
    alignItems: 'center',
  },
  question: {
    fontWeight: 'bold',
    marginBottom: 8,
  },
  resultBadge: {
    padding: 8,
    borderRadius: 8,
    alignItems: 'center',
    marginVertical: 8,
  },
  oddsContainer: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    marginTop: 8,
  },
  oddsSide: {
    flexDirection: 'row',
    alignItems: 'center',
  },
  progressBar: {
    marginTop: 8,
    height: 8,
    borderRadius: 4,
  },
  poolText: {
    textAlign: 'center',
    marginTop: 8,
  },
  statsContainer: {
    padding: 12,
    borderRadius: 8,
  },
  statRow: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    marginVertical: 4,
  },
  positionRow: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    marginVertical: 8,
  },
  betControls: {
    alignItems: 'center',
    marginVertical: 16,
  },
  betCountContainer: {
    flexDirection: 'row',
    alignItems: 'center',
    marginBottom: 8,
  },
  sideButtons: {
    flexDirection: 'row',
    justifyContent: 'space-around',
    marginTop: 8,
  },
  sideButton: {
    flex: 1,
    marginHorizontal: 8,
  },
  loadingContainer: {
    flex: 1,
    justifyContent: 'center',
    alignItems: 'center',
  },
  emptyContainer: {
    flex: 1,
    justifyContent: 'center',
    alignItems: 'center',
  },
  errorBanner: {
    padding: 12,
    marginHorizontal: 12,
    marginBottom: 8,
    borderRadius: 8,
  },
  listContent: {
    paddingBottom: 16,
  },
});
