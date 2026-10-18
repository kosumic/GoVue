export type GameCue = { id: string; start: number; duration: number; text: string };
export type GameSource = { videoId: string; title: string; start: number | null; trackId?: string; language?: string };
export type LearningGame = {
  id: string;
  definition: { title: string; html: string; css: string; script: string; rules: string; cueIds: string[]; sourceCueId?: string };
  state: Record<string, unknown>;
  cues: GameCue[];
  revision: number;
  source?: GameSource;
};
export type GameAction = (messageId: string, gameId: string, action: string, input: Record<string, unknown>, eventId: string) => Promise<LearningGame>;
