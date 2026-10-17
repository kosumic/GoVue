export type GameCue = { id: string; start: number; duration: number; text: string };
export type LearningGame = {
  id: string;
  definition: { title: string; html: string; css: string; script: string; rules: string; cueIds: string[] };
  state: Record<string, unknown>;
  cues: GameCue[];
  revision: number;
};
export type GameAction = (messageId: string, gameId: string, action: string, input: Record<string, unknown>, eventId: string) => Promise<LearningGame>;
