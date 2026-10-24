// Presentation only: Rust supplies the encoded matrix, including error correction.
export function qrArtwork(modules: string[]) {
  const size = modules.length;
  const quiet = 4;
  const eyes: [number, number][] = [[quiet, quiet], [quiet + size - 7, quiet], [quiet, quiet + size - 7]];
  const paths: string[] = [];
  for (let row = 0; row < size; row++) {
    for (let col = 0; col < size; col++) {
      if (modules[row][col] !== '1') continue;
      if ((row < 7 && (col < 7 || col >= size - 7)) || (col < 7 && row >= size - 7)) continue;
      const x = col + quiet, y = row + quiet;
      const up = modules[row - 1]?.[col] === '1', down = modules[row + 1]?.[col] === '1';
      const left = modules[row][col - 1] === '1', right = modules[row][col + 1] === '1';
      // Round exposed corners and join adjacent modules into smooth, solid traces.
      const tl = !up && !left ? 0.4 : 0, tr = !up && !right ? 0.4 : 0;
      const bl = !down && !left ? 0.4 : 0, br = !down && !right ? 0.4 : 0;
      paths.push(`M${x + tl} ${y}H${x + 1 - tr}Q${x + 1} ${y} ${x + 1} ${y + tr}V${y + 1 - br}Q${x + 1} ${y + 1} ${x + 1 - br} ${y + 1}H${x + bl}Q${x} ${y + 1} ${x} ${y + 1 - bl}V${y + tl}Q${x} ${y} ${x + tl} ${y}z`);
    }
  }
  return { path: paths.join(''), eyes, extent: size + quiet * 2 };
}
