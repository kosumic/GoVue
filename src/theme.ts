import { createContext, useContext, useMemo } from 'react';

export const colors = {
  background: '#0F0F0F',
  surface: '#0F0F0F',
  dialog: '#212121',
  action: '#3EA6FF',
  ink: '#FFFFFF',
  muted: '#AAAAAA',
  accent: '#FFFFFF',
  accentSoft: '#272727',
  line: '#3A3A3A',
  error: '#FF0000',
  inverted: '#0F0F0F',
  raised: '#272727',
  separator: '#3A3A3A',
  field: '#0F0F0F',
  outline: '#3A3A3A',
};
export type ThemeColors = typeof colors;
export const lightColors: ThemeColors = {
  background: '#FFFFFF',
  surface: '#FFFFFF',
  dialog: '#FFFFFF',
  action: '#065FD4',
  ink: '#0F0F0F',
  muted: '#606060',
  accent: '#0F0F0F',
  accentSoft: '#F2F2F2',
  line: '#E5E5E5',
  error: '#FF0000',
  inverted: '#FFFFFF',
  raised: '#F2F2F2',
  separator: '#E5E5E5',
  field: '#F9F9F9',
  outline: '#888888',
};
export const ThemeContext = createContext<ThemeColors>(colors);
export const useThemeColors = () => useContext(ThemeContext);
export function useThemeStyles<T>(factory: (colors: ThemeColors) => T): T {
  const palette = useThemeColors();
  return useMemo(() => factory(palette), [factory, palette]);
}
