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
