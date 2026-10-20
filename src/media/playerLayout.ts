export type PlayerLayout = {
  left: number;
  top: number;
  width: number;
  height: number;
  viewportWidth: number;
  viewportHeight: number;
  visible: boolean;
};

export function embeddedPlayerFrame(layout: PlayerLayout | null, width: number, height: number) {
  if (!layout) return { left: 0, top: 0, width, height: Math.min(height, width * 9 / 16) };
  const scale = width / layout.viewportWidth;
  return { left: layout.left * scale, top: layout.top * scale, width: Math.max(1, layout.width * scale), height: Math.max(1, layout.height * scale) };
}
