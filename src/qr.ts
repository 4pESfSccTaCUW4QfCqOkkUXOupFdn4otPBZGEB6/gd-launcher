/**
 * DepotDownloader prints a QR login as two-character-wide block art in the OEM
 * code page. Rendering that in a terminal font squashes it, so it is parsed back
 * into a module matrix and drawn as real squares.
 *
 * Two traps in the real output:
 *
 *   - progress bars are drawn from the same block characters, so a naive scan
 *     welds a strip of bar to the right edge of the code;
 *   - a row of the code can come out entirely light, which breaks a strictly
 *     contiguous run.
 *
 * So a row only counts as part of the code when the line holds nothing but
 * blocks and whitespace, and rows are grouped by proximity instead of adjacency.
 */
const BLOCK_CHAR = /[\u2588\u2580\u2584\u258c\u2590\u2591\u2592\u2593]/;
const BLOCK = new RegExp(BLOCK_CHAR.source);
const ART_LINE = /^[ \t]*[\u2588\u2580\u2584\u258c\u2590\u2591\u2592\u2593][\u2588\u2580\u2584\u258c\u2590\u2591\u2592\u2593 \t]*$/;
const BLANK = /^[ \t]*$/;

/** Rows that could belong to the code, grouped where they sit close together. */
function artRange(rows: string[]): { start: number; end: number } | null {
  const hits: number[] = [];
  for (let i = 0; i < rows.length; i += 1) {
    if (ART_LINE.test(rows[i])) hits.push(i);
  }
  if (hits.length < 9) return null;

  // A gap of up to two lines is tolerated; a bigger one starts a new block, and
  // only the tallest block is the code.
  const GAP = 2;
  let best = { start: hits[0], end: hits[0], rows: 1 };
  let start = hits[0];
  let count = 1;
  for (let i = 1; i < hits.length; i += 1) {
    if (hits[i] - hits[i - 1] <= GAP + 1) {
      count += 1;
    } else {
      if (count > best.rows) best = { start, end: hits[i - 1], rows: count };
      start = hits[i];
      count = 1;
    }
  }
  if (count > best.rows) best = { start, end: hits[hits.length - 1], rows: count };
  return { start: best.start, end: best.end };
}

export function parseTerminalQr(lines: string[]): boolean[][] | null {
  const rows = lines.map((line) => line.replace(/[ \t]+$/, ""));
  const range = artRange(rows);
  if (!range) return null;

  // Text lines inside the span (progress output interleaved with the art) are
  // dropped; blank lines are kept as light rows so the geometry stays honest.
  const art = rows
    .slice(range.start, range.end + 1)
    .filter((line) => ART_LINE.test(line) || BLANK.test(line));
  if (art.length < 9) return null;

  const coded = art.filter((line) => ART_LINE.test(line));
  const leads = coded.map((line) => line.search(BLOCK)).filter((n) => n >= 0);
  if (leads.length === 0) return null;
  const lead = Math.min(...leads);

  const widest = Math.max(...coded.map((line) => line.length));
  const cols = Math.floor((widest - lead) / 2);
  if (cols < 9) return null;

  const matrix = art.map((line) => {
    const padded = line.padEnd(lead + cols * 2, " ");
    const modules: boolean[] = [];
    for (let c = 0; c < cols; c += 1) {
      modules.push(BLOCK.test(padded[lead + c * 2] ?? " "));
    }
    return modules;
  });

  // Crop the quiet zone: the terminal art pads asymmetrically, and a QR has to
  // come out square before it will scan.
  let top = matrix.length;
  let bottom = -1;
  let left = cols;
  let right = -1;
  for (let r = 0; r < matrix.length; r += 1) {
    for (let c = 0; c < cols; c += 1) {
      if (!matrix[r][c]) continue;
      if (r < top) top = r;
      if (r > bottom) bottom = r;
      if (c < left) left = c;
      if (c > right) right = c;
    }
  }
  if (bottom < 0 || right < 0) return null;

  const cropped: boolean[][] = [];
  for (let r = top; r <= bottom; r += 1) {
    cropped.push(matrix[r].slice(left, right + 1));
  }

  const h = cropped.length;
  const w = cropped[0]?.length ?? 0;
  // QR versions are square; a partial frame is not.
  if (w < 9 || Math.abs(h - w) > 1) return null;
  return cropped;
}
