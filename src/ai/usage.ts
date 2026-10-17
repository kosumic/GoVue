import { useEffect, useState } from 'react';
import { aiRequest } from '../../modules/lingoplay-core';

export type TokenUsage = {
  inputTokens?: number; outputTokens?: number; uncachedInputTokens?: number;
  cacheReadTokens?: number; cacheCreationTokens?: number; reasoningTokens?: number;
  latestContextTokens?: number; streamMs?: number;
};
export type UsageStats = { [K in keyof TokenUsage]: number | null } & {
  replyCount: number; unreportedReplies: number; chatCount: number; activeDays: number | null;
  averagePerChat: number | null; averagePerDay: number | null;
  cacheHitRate: number | null; outputTokensPerSecond: number | null; legacyUsage: boolean;
};
export type ModelUsage = {
  key: string; model: string; modelName: string; providerLabel: string;
  estimated: boolean; removed: boolean; stats: UsageStats;
};
export type UsageReport = {
  chatId: string | null; title: string | null; stats: UsageStats;
  providers: { id: string; label: string; models: ModelUsage[] }[];
  chats: { id: string; title: string; model: string; providerLabel: string }[];
  context: null | { used: number; window: number | null; maxOutputTokens: number | null; reasoningEffort: string; model: string; providerLabel: string };
};

export function useUsage(visible: boolean, chatId: string | null) {
  const [report, setReport] = useState<UsageReport | null>(null);
  const [error, setError] = useState('');
  const [version, setVersion] = useState(0);
  useEffect(() => {
    let active = true;
    setReport(null); setError('');
    if (visible) void aiRequest<UsageReport>({ op: 'ai_usage', ...(chatId ? { chatId } : {}) })
      .then(value => { if (active) setReport(value); })
      .catch(reason => { if (active) setError(reason instanceof Error ? reason.message : 'Could not load token usage.'); });
    return () => { active = false; };
  }, [visible, chatId, version]);
  return { report, error, refresh: () => setVersion(value => value + 1) };
}

export function formatTokens(value: number | null | undefined): string {
  if (value == null) return '—';
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return Math.round(value).toLocaleString();
}
