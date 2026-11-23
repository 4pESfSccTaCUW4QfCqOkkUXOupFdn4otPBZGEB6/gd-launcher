import { useCallback, useEffect, useMemo, useState } from "react";
import { api, bytes, count, errText } from "../api";
import type { AppStateView, ModCard } from "../types";
import type { Toast } from "./Toasts";

interface Props {
  state: AppStateView;
  initialInstanceId: string | null;
  notify: (message: string, kind?: Toast["kind"]) => void;
  onReload: () => Promise<void>;
  onOpenInstance: (id: string) => void;
}

const PER_PAGE = 20;

export default function BrowseMods({
  state,
  initialInstanceId,
  notify,
  onReload,
  onOpenInstance,
}: Props) {
  const [instanceId, setInstanceId] = useState<string | null>(
    initialInstanceId ?? state.instances[0]?.id ?? null,
  );
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(1);
  const [mods, setMods] = useState<ModCard[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const [installing, setInstalling] = useState<string | null>(null);
  const [installed, setInstalled] = useState<Set<string>>(new Set());

  const instance = useMemo(
    () => state.instances.find((i) => i.id === instanceId) ?? null,
    [state.instances, instanceId],
  );

  const search = useCallback(
    async (q: string, p: number) => {
      setLoading(true);
      try {
        const result = await api.searchMods(q, p, PER_PAGE);
        setMods(result.mods);
        setTotal(result.total);
      } catch (err) {
        notify(errText(err), "error");
      } finally {
        setLoading(false);
      }
    },
    [notify],
  );

  useEffect(() => {
    void search("", 1);
  }, [search]);

  const refreshInstalled = useCallback(async () => {
    if (!instanceId) {
      setInstalled(new Set());
      return;
    }
    try {
      const mods = await api.installedMods(instanceId);
      setInstalled(new Set(mods.map((m) => m.id)));
    } catch {
      setInstalled(new Set());
    }
  }, [instanceId]);

  useEffect(() => {
    void refreshInstalled();
  }, [refreshInstalled]);

  const install = async (mod: ModCard) => {
    if (!instanceId) {
      notify("pick an instance first", "error");
      return;
    }
    setInstalling(mod.id);
    try {
      await api.installMod(instanceId, mod.id);
      notify(`${mod.name} installed`, "ok");
      await refreshInstalled();
      await onReload();
    } catch (err) {
      notify(errText(err), "error");
    } finally {
      setInstalling(null);
    }
  };

  const pages = Math.max(1, Math.ceil(total / PER_PAGE));

  return (
    <>
      <div className="page-head">
        <div>
          <h1>Browse mods</h1>
          <p>
            Geode index · {count(total)} mods · newest build of each is installed with its hash
            checked
          </p>
        </div>
        <div className="head-actions">
          <select
            value={instanceId ?? ""}
            onChange={(e) => setInstanceId(e.target.value || null)}
            style={{ minWidth: 240 }}
          >
            {state.instances.length === 0 && <option value="">no instances</option>}
            {state.instances.map((i) => (
              <option key={i.id} value={i.id}>
                {i.name} · {i.version || "unlabelled"}
                {i.geode ? "" : " (no loader)"}
              </option>
            ))}
          </select>
        </div>
      </div>

      {instance && !instance.geode && (
        <div className="panel" style={{ marginBottom: 16, borderColor: "var(--accent)" }}>
          <div style={{ display: "flex", gap: 14, alignItems: "center" }}>
            <div style={{ flex: 1 }}>
              <b>This instance has no Geode loader</b>
              <div className="hint" style={{ marginTop: 4 }}>
                Mods can be dropped in, but the game will not load them until the loader is
                installed.
              </div>
            </div>
            <button className="tiny" onClick={() => onOpenInstance(instance.id)}>
              Open instance
            </button>
          </div>
        </div>
      )}

      <div className="toolbar">
        <div className="grow">
          <input
            value={query}
            placeholder="search the index — betterinfo, globed, texture loader…"
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                setPage(1);
                void search(query, 1);
              }
            }}
          />
        </div>
        <button
          className="primary"
          disabled={loading}
          onClick={() => {
            setPage(1);
            void search(query, 1);
          }}
        >
          {loading ? "Searching…" : "Search"}
        </button>
        {query && (
          <button
            onClick={() => {
              setQuery("");
              setPage(1);
              void search("", 1);
            }}
          >
            Clear
          </button>
        )}
      </div>

      {loading && mods.length === 0 ? (
        <div className="empty">
          <b>
            <span className="spinner" /> hitting the index
          </b>
          <span>geode-sdk.org</span>
        </div>
      ) : mods.length === 0 ? (
        <div className="empty">
          <b>Nothing matched</b>
          <span>try a different term</span>
        </div>
      ) : (
        <div className="list">
          {mods.map((mod) => {
            const already = installed.has(mod.id);
            return (
              <div className="row" key={mod.id}>
                <div className="row-main">
                  <div className="row-title">
                    {mod.name}
                    <span className="badge">v{mod.version}</span>
                    {mod.featured && <span className="badge info">featured</span>}
                    {mod.gd_win && <span className="badge">GD {mod.gd_win}</span>}
                    {mod.geode && <span className="badge">Geode {mod.geode}</span>}
                    {already && <span className="badge ok">installed</span>}
                  </div>
                  <div className="row-desc">{mod.description || mod.id}</div>
                  <div className="hint" style={{ marginTop: 4 }}>
                    {mod.developers.join(", ") || "unknown author"} · {count(mod.downloads)}{" "}
                    downloads
                  </div>
                </div>
                <div className="row-side">
                  <button
                    className={already ? "tiny" : "primary tiny"}
                    disabled={installing === mod.id || !instanceId}
                    onClick={() => void install(mod)}
                  >
                    {installing === mod.id ? "Installing…" : already ? "Reinstall" : "Install"}
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      <div className="toolbar" style={{ marginTop: 16, justifyContent: "center" }}>
        <button
          className="tiny"
          disabled={page <= 1 || loading}
          onClick={() => {
            const next = page - 1;
            setPage(next);
            void search(query, next);
          }}
        >
          Previous
        </button>
        <span className="mono">
          page {page} / {pages}
        </span>
        <button
          className="tiny"
          disabled={page >= pages || loading}
          onClick={() => {
            const next = page + 1;
            setPage(next);
            void search(query, next);
          }}
        >
          Next
        </button>
      </div>

      <div className="hint" style={{ textAlign: "center" }}>
        {instance ? `installing into ${instance.name}` : "no target instance selected"}
        {instance && ` · ${bytes(instance.size_bytes)} on disk`}
      </div>
    </>
  );
}
