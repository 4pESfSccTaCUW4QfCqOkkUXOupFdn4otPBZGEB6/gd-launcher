import { useMemo } from "react";
import { parseTerminalQr } from "../qr";

interface Props {
  lines: string[];
  size?: number;
}

const QUIET = 3;

export default function QrCode({ lines, size = 208 }: Props) {
  const matrix = useMemo(() => parseTerminalQr(lines), [lines]);

  if (!matrix) {
    return <div className="qr-placeholder" style={{ width: size, height: size }} />;
  }

  const rows = matrix.length;
  const cols = matrix[0]?.length ?? rows;
  const path: string[] = [];
  for (let r = 0; r < rows; r += 1) {
    for (let c = 0; c < cols; c += 1) {
      if (matrix[r][c]) path.push(`M${c} ${r}h1v1h-1z`);
    }
  }

  // Square viewBox, square container, 100% fill - no overflow, so the code never
  // bleeds past the white plate.
  const span = Math.max(rows, cols) + QUIET * 2;

  return (
    <div className="qr-frame" style={{ width: size, height: size }}>
      <svg
        viewBox={`${-QUIET} ${-QUIET} ${span} ${span}`}
        width="100%"
        height="100%"
        preserveAspectRatio="xMidYMid meet"
        shapeRendering="crispEdges"
        role="img"
        aria-label="Steam login QR code"
      >
        <rect x={-QUIET} y={-QUIET} width={span} height={span} fill="#ffffff" />
        <path d={path.join("")} fill="#000000" />
      </svg>
    </div>
  );
}
