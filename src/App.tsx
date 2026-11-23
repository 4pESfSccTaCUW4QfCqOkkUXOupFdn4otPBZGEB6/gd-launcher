import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, errText } from "./api";
import type {
  AppStateView,
  DepotExit,
  DepotLog,
  DepotProgress,
  DownloadProgress,
  QrFrame,
  Settings,
  ToolchainStatus,
} from "./types";
import Library from "./components/Library";
import InstancePage from "./components/InstancePage";
import BrowseMods from "./components/BrowseMods";
import SettingsPage from "./components/SettingsPage";
import DownloadBar from "./components/DownloadBar";
import Toasts, { type Toast } from "./components/Toasts";
import { applyTheme, storedTheme } from "./theme";

export type View =
  | { kind: "library" }
  | { kind: "instance"; id: string }
  | { kind: "mods"; id: string | null }
  | { kind: "settings" };

export interface JobState {
  running: boolean;
  percent: number;
  detail: string;
  logs: DepotLog[];
  exit: DepotExit | null;
  title: string;
  qr: string[] | null;
}

const emptyJob: JobState = {
  running: false,
  percent: 0,
  detail: "",
  logs: [],
  exit: null,
  title: "",
  qr: null,
};

export default function App() {
  const [state, setState] = useState<AppStateView | null>(null);
  const [view, setView] = useState<View>({ kind: "library" });
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [job, setJob] = useState<JobState>(emptyJob);
  const [tool, setTool] = useState<ToolchainStatus | null>(null);
  const [toolProgress, setToolProgress] = useState<DownloadProgress | null>(null);
  const [copyProgress, setCopyProgress] = useState<DownloadProgress | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [theme, setTheme] = useState<string>(() => storedTheme());
  const toastSeq = useRef(0);

  // Themes are pure CSS variables on <html>; the pick persists in localStorage.
  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

  const notify = useCallback((message: string, kind: Toast["kind"] = "info") => {
    const id = ++toastSeq.current;
    setToasts((t) => [...t, { id, message, kind }]);
    window.setTimeout(() => {
      setToasts((t) => t.filter((x) => x.id !== id));
    }, kind === "error" ? 8000 : 4200);
  }, []);

  const reload = useCallback(async () => {
    try {
      const next = await api.state();
      setState(next);
      setTool(await api.toolchain());
    } catch (err) {
      notify(errText(err), "error");
    }
  }, [notify]);

  useEffect(() => {
    void reload();
  }, [reload]);

  // --- live event wiring -----------------------------------------------------
  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    let disposed = false;

    const wire = async () => {
      const subs = await Promise.all([
        listen<DepotLog>("depot://log", (e) => {
          setJob((j) => ({ ...j, logs: [...j.logs.slice(-400), e.payload] }));
        }),
        listen<DepotProgress>("depot://progress", (e) => {
          setJob((j) => ({ ...j, percent: e.payload.percent, detail: e.payload.detail }));
        }),
        listen<DepotExit>("depot://exit", (e) => {
          setJob((j) => ({ ...j, running: false, exit: e.payload, percent: e.payload.ok ? 100 : j.percent }));
          if (e.payload.ok) {
            notify("depot download finished", "ok");
            void reload();
          } else if (e.payload.code !== -2) {
            notify(`depot download exited with code ${e.payload.code}`, "error");
          }
        }),
        listen<DownloadProgress>("tool://progress", (e) => setToolProgress(e.payload)),
        listen<DownloadProgress>("copy://progress", (e) => setCopyProgress(e.payload)),
        listen<QrFrame>("depot://qr", (e) => {
          setJob((j) => ({ ...j, qr: e.payload.lines }));
        }),
        // Fired once by the backend when a QR run exits cleanly. From here the
        // login token is in DepotDownloader's store, so the scan is not offered.
        listen<boolean>("steam://qr-done", () => {
          setJob((j) => ({ ...j, qr: null }));
          notify("steam session stored. next pull signs in on its own", "ok");
          void reload();
        }),
        // The cached key was rejected. The backend already deleted it, so a
        // reload brings the QR flow back.
        listen<string>("steam://auth-needed", () => {
          notify("steam session expired. sign in with QR again", "error");
          void reload();
        }),
      ]);
      if (disposed) {
        subs.forEach((u) => u());
      } else {
        unlisteners.push(...subs);
      }
    };
    void wire();

    return () => {
      disposed = true;
      unlisteners.forEach((u) => u());
    };
  }, [notify, reload]);

  const saveSettings = useCallback(
    async (settings: Settings) => {
      try {
        await api.saveSettings(settings);
        await reload();
        notify("settings saved", "ok");
      } catch (err) {
        notify(errText(err), "error");
      }
    },
    [notify, reload],
  );

  const startDownload = useCallback(
    async (instanceId: string, manifestId: string, depotId: string, useQr = false) => {
      if (!state) return;
      setJob({
        ...emptyJob,
        running: true,
        title: state.instances.find((i) => i.id === instanceId)?.name ?? "instance",
      });
      try {
        await api.startDownload({
          instance_id: instanceId,
          manifest_id: manifestId,
          depot_id: depotId,
          username: credsRef.current.username || state.settings.steam_username,
          password: credsRef.current.password,
          use_qr: useQr,
        });
      } catch (err) {
        setJob((j) => ({ ...j, running: false }));
        notify(errText(err), "error");
      }
    },
    [notify, state],
  );

  /// One click from the version table: create the instance and pull it.
  const quickInstall = useCallback(
    async (version: string, manifestId: string, depotId: string, useQr = false): Promise<string> => {
      setJob({ ...emptyJob, running: true, title: `Geometry Dash ${version}` });
      try {
        const created = await api.quickInstall({
          version,
          manifest_id: manifestId,
          depot_id: depotId,
          username: credsRef.current.username || (state?.settings.steam_username ?? ""),
          password: credsRef.current.password,
          use_qr: useQr,
          start_download: true,
        });
        await reload();
        return created.id;
      } catch (err) {
        setJob((j) => ({ ...j, running: false }));
        throw err;
      }
    },
    [reload, state],
  );

  // Steam credentials live only in memory for the session; never persisted.
  const credsRef = useRef({ username: "", password: "" });

  const sendInput = useCallback(
    async (text: string) => {
      try {
        await api.sendInput(text);
      } catch (err) {
        notify(errText(err), "error");
      }
    },
    [notify],
  );

  const cancelJob = useCallback(async () => {
    try {
      await api.cancelDownload();
    } catch (err) {
      notify(errText(err), "error");
    }
  }, [notify]);

  const installToolchain = useCallback(async () => {
    setBusy("toolchain");
    setToolProgress(null);
    try {
      const path = await api.installToolchain();
      notify(`DepotDownloader ready at ${path}`, "ok");
      setTool(await api.toolchain());
    } catch (err) {
      notify(errText(err), "error");
    } finally {
      setBusy(null);
      setToolProgress(null);
    }
  }, [notify]);

  const navTo = useCallback((next: View) => setView(next), []);

  if (!state) {
    return (
      <div className="shell">
        <div />
        <div className="main">
          <div className="empty">
            <b>wiring the bench</b>
            <span>loading launcher state…</span>
          </div>
        </div>
      </div>
    );
  }

  const selected =
    view.kind === "instance" ? state.instances.find((i) => i.id === view.id) ?? null : null;

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark">GD</div>
          <div className="brand-text">
            <b>GD Launcher</b>
            <small>version manager + Geode</small>
          </div>
        </div>

        <button
          className={`nav-item ${view.kind === "library" || view.kind === "instance" ? "active" : ""}`}
          onClick={() => navTo({ kind: "library" })}
        >
          Library
        </button>
        <button
          className={`nav-item ${view.kind === "mods" ? "active" : ""}`}
          onClick={() => navTo({ kind: "mods", id: selected?.id ?? null })}
        >
          Browse mods
        </button>
        <button
          className={`nav-item ${view.kind === "settings" ? "active" : ""}`}
          onClick={() => navTo({ kind: "settings" })}
        >
          Settings
        </button>

        <div className="sidebar-foot">
          <div style={{ marginBottom: 6 }}>
            <b style={{ color: "var(--text)" }}>{state.instances.length}</b> instance
            {state.instances.length === 1 ? "" : "s"}
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <span
              style={{
                width: 5,
                height: 5,
                background: tool?.ready ? "var(--accent)" : "var(--dim)",
              }}
            />
            {tool?.ready ? "depot backend ready" : "depot backend missing"}
          </div>
          <div style={{ marginTop: 4, fontFamily: "var(--mono)", fontSize: 10.5 }}>
            {tool?.backend === "steam_cmd" ? "steamcmd" : "DepotDownloader"}
          </div>
        </div>
      </aside>

      <main className="main" style={{ paddingBottom: job.running || job.logs.length ? 190 : 30 }}>
        {view.kind === "library" && (
          <Library
            state={state}
            tool={tool}
            busy={busy}
            copyProgress={copyProgress}
            onReload={reload}
            notify={notify}
            onOpenInstance={(id) => navTo({ kind: "instance", id })}
            onBrowseMods={(id) => navTo({ kind: "mods", id })}
            onQuickInstall={quickInstall}
            onInstallToolchain={installToolchain}
            onBusy={setBusy}
          />
        )}

        {view.kind === "instance" && selected && (
          <InstancePage
            instanceId={selected.id}
            session={state.depot_session}
            notify={notify}
            onBack={() => navTo({ kind: "library" })}
            onReload={reload}
            onBrowseMods={() => navTo({ kind: "mods", id: selected.id })}
            onStartDownload={startDownload}
          />
        )}

        {view.kind === "mods" && (
          <BrowseMods
            state={state}
            initialInstanceId={view.id}
            notify={notify}
            onReload={reload}
            onOpenInstance={(id) => navTo({ kind: "instance", id })}
          />
        )}
        {/* end view switch */}

        {view.kind === "settings" && (
          <SettingsPage
            state={state}
            tool={tool}
            toolProgress={toolProgress}
            busy={busy}
            onInstallToolchain={installToolchain}
            onSave={saveSettings}
            onReload={reload}
            theme={theme}
            onTheme={setTheme}
            notify={notify}
          />
        )}
      </main>

      {(job.running || job.logs.length > 0) && (
        <DownloadBar
          job={job}
          onCancel={cancelJob}
          onSend={sendInput}
          onDismiss={() => setJob(emptyJob)}
        />
      )}

      <Toasts toasts={toasts} />
    </div>
  );
}
