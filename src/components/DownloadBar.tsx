import { useEffect, useRef, useState } from "react";
import type { JobState } from "../App";
import QrCode from "./QrCode";

interface Props {
  job: JobState;
  onCancel: () => void;
  onSend: (text: string) => void;
  onDismiss: () => void;
}

export default function DownloadBar({ job, onCancel, onSend, onDismiss }: Props) {
  const [line, setLine] = useState("");
  const logRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [job.logs.length]);

  const submit = () => {
    if (!line.trim()) return;
    onSend(line);
    setLine("");
  };

  const hasQr = job.qr !== null && job.qr.some((l) => l.includes("\u2588"));

  return (
    <div className="download-bar">
      <div className="download-top">
        <span className="download-title">
          {job.running ? (
            <>
              <span className="spinner" style={{ marginRight: 8 }} />
              pulling depot · {job.title}
            </>
          ) : job.exit?.ok ? (
            <>depot complete · {job.title}</>
          ) : (
            <>depot stopped · {job.title}</>
          )}
        </span>
        <div className="bar">
          <i style={{ width: `${Math.min(100, Math.max(0, job.percent))}%` }} />
        </div>
        <span className="mono" style={{ flex: "none", minWidth: 46, textAlign: "right" }}>
          {job.percent.toFixed(1)}%
        </span>
        {job.running ? (
          <button className="tiny danger" onClick={onCancel}>
            Cancel
          </button>
        ) : (
          <button className="tiny" onClick={onDismiss}>
            Dismiss
          </button>
        )}
      </div>

      {hasQr && job.running && (
        <div className="qr-panel">
          <QrCode lines={job.qr ?? []} size={196} />
          <div className="qr-copy">
            <b>Scan with the Steam mobile app</b>
            Steam app → Menu → Sign in with QR code. No password, no Steam Guard code. The
            session is remembered, so the next pull runs silent.
            <div className="hint" style={{ marginTop: 8 }}>
              Steam → Confirm Sign In. If the code expires a fresh one appears here.
            </div>
          </div>
        </div>
      )}

      <div className="log" ref={logRef}>
        {job.logs.length === 0 ? (
          <span style={{ color: "var(--dim)" }}>waiting for output…</span>
        ) : (
          job.logs.map((l, i) => (
            <div key={i} className={l.stream === "err" ? "err" : undefined}>
              {l.line}
            </div>
          ))
        )}
      </div>

      {job.running && (
        <div className="stdin-row">
          <input
            value={line}
            placeholder="Steam Guard code / password prompt — type here and press Enter"
            onChange={(e) => setLine(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") submit();
            }}
          />
          <button onClick={submit}>Send</button>
        </div>
      )}
    </div>
  );
}
