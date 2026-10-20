import { browserRequest } from '../../modules/govue-core';
import type { PlayerLayout } from '../media/playerLayout';

export function acceptPlayerLayout(value: unknown): PlayerLayout | null {
  try { return browserRequest<PlayerLayout | null>({ op: 'browser_layout', layout: value }); }
  catch { return null; }
}
