import type { WordTranslationPlan } from '../core/vocabulary';

// Apple's translation session allows one task at a time. Skip dismissed work
// before it reaches the native module, and let an active task finish safely.
let pending: Promise<unknown> = Promise.resolve();

export function translateWord(plan: WordTranslationPlan, isCurrent: () => boolean): Promise<string | null> {
  const task = pending.then(async () => {
    if (!isCurrent()) return null;
    if (plan.sameLanguage) return plan.input;
    // Opt-in fixture for simulator recordings where native translation is unavailable.
    if (process.env.EXPO_PUBLIC_GOVUE_MOCK_TRANSLATION === '1') {
      if (plan.input === 'apprendre' && plan.sourceLangCode === 'fr' && plan.targetLangCode === 'en') return 'learn';
      throw Error('No mock translation is configured for this word.');
    }
    const { onTranslateTask } = await import('expo-translate-text').catch(() => {
      throw Error('Install an updated GoVue build to translate words.');
    });
    if (!isCurrent()) return null;
    // 0.3.0 forwards every optional field into a non-null Kotlin map. Supply
    // defaults explicitly because Expo 57 rejects undefined map values.
    const result = await onTranslateTask({ input: plan.input, sourceLangCode: plan.sourceLangCode, targetLangCode: plan.targetLangCode,
      preferredStrategy: 'lowLatency', requiresWifi: false, requireCharging: false }).catch((error: unknown) => {
      const code = error && typeof error === 'object' && 'code' in error ? error.code : undefined;
      if (code === 'MODEL_DOWNLOAD_FAILED') throw Error('Could not download the language model. Check your connection and try again.');
      if (code === 'INVALID_PARAMETER') throw Error('Translation is unavailable for this language pair.');
      if (code === 'ERR_UNSUPPORTED_OS_VERSION') throw Error('Word translation requires iOS 18 or later.');
      throw Error('Could not translate this word. Please try again.');
    });
    if (typeof result.translatedTexts !== 'string' || !result.translatedTexts.trim()) throw Error('No translation was returned. Please try again.');
    return result.translatedTexts;
  });
  pending = task.catch(() => null);
  return task;
}
