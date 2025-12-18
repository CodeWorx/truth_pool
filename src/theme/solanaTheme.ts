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
