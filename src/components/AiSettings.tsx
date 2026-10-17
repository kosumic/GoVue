import { useState } from 'react';
import {
  ActivityIndicator,
  KeyboardAvoidingView,
  Platform,
  StyleSheet,
  Text,
  View,
} from 'react-native';
import { useAi } from '../ai/useAi';
import { useThemeColors } from '../theme';
import { SheetRow } from '../player/PlayerSheet';
import { AiProviders } from './AiProviders';
import { AiUsageSheet } from './AiUsageSheet';

export function AiSettings() {
  const [attempt, setAttempt] = useState(0);
  return (
    <KeyboardAvoidingView
      style={styles.page}
      behavior={Platform.OS === 'ios' ? 'padding' : 'height'}
    >
      <Content key={attempt} onRetry={() => setAttempt(value => value + 1)} />
    </KeyboardAvoidingView>
  );
}

function Content({ onRetry }: { onRetry: () => void }) {
  const [usageVisible, setUsageVisible] = useState(false);
  const ai = useAi(true, 'settings');
  const colors = useThemeColors();
  if (!ai.ready) return (
    <View style={styles.status}>
      {ai.error ? <>
        <Text accessibilityRole="alert" style={{ color: colors.error }}>
          {ai.error}
        </Text>
        <SheetRow title="Try again" onPress={onRetry} />
      </> : <ActivityIndicator accessibilityLabel="Loading AI settings" color={colors.ink} />}
    </View>
  );
  return <><AiProviders ai={ai} onUsage={() => setUsageVisible(true)} /><AiUsageSheet visible={usageVisible} onClose={() => setUsageVisible(false)} /></>;
}

const styles = StyleSheet.create({
  page: { flex: 1 },
  status: { flex: 1, justifyContent: 'center', alignItems: 'center', padding: 20, gap: 16 },
});
