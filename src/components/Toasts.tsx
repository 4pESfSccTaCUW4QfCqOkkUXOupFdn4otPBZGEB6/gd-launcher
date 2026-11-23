export interface Toast {
  id: number;
  message: string;
  kind: "info" | "ok" | "error";
}

export default function Toasts({ toasts }: { toasts: Toast[] }) {
  if (toasts.length === 0) return null;
  return (
    <div className="toasts">
      {toasts.map((t) => (
        <div key={t.id} className={`toast ${t.kind === "info" ? "" : t.kind}`}>
          {t.message}
        </div>
      ))}
    </div>
  );
}
