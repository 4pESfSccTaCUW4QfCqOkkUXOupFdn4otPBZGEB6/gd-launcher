import { useEffect, useState } from "react";
import { api, errText } from "../api";
import { THEMES } from "../theme";
import type {
  AppStateView,
  DownloadProgress,
  Settings,
  SteamAccount,
  ToolchainStatus,
} from "../types";
import type { Toast } from "./Toasts";

interface Props {
  state: AppStateView;
  tool: ToolchainStatus | null;
  toolProgress: DownloadProgress | null;
  busy: string | null;
  theme: string;
  onTheme: (id: string) => void;
  onInstallToolchain: () => Promise<void>;
  onSave: (settings: Settings) => Promise<void>;
  onReload: () => Promise<void>;
  notify: (message: string, kind?: Toast["kind"]) => void;
}

export default function SettingsPage({
  state,
  tool,
  toolProgress,
  busy,
  theme,
  onTheme,
  onInstallToolchain,
  onSave,
  onReload,
  notify,
}: Props) {
  const [draft, setDraft] = useState<Settings>(state.settings);
  const [saving, setSaving] = useState(false);
  const [account, setAccount] = useState<SteamAccount | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .steamAccount()
      .then((acct) => {
        if (alive) setAccount(acct);
      })
      .catch(() => {
        if (alive) setAccount(null);
      });
    return () => {
      alive = false;
    };
  }, []);

  // Keep the draft in step with the store after a reload from outside.
  useEffect(() => {
    setDraft(state.settings);
  }, [state.settings]);

  const patch = <K extends keyof Settings>(key: K, value: Settings[K]) =>
    setDraft((d) => ({ ...d, [key]: value }));

  const save = async () => {
    setSaving(true);
    try {
      await onSave(draft);
    } finally {
      setSaving(false);
    }
  };

  const resetSession = async () => {
    try {
      const removed = await api.resetSteamSession();
      await onReload();
      notify(
        removed > 0
          ? `cached login token deleted. QR sign-in is back`
          : "no token on disk. QR sign-in is already available",
        "ok",
      );
    } catch (err) {
      notify(errText(err), "error");
    }
  };

  const session = state.depot_session;
  const qrDone = session.signed_in;

  return (
    <>
      <div className="page-head">
        <div>
          <h1>settings</h1>
          <p>depot backend · steam session · storage</p>
        </div>
        <div className="head-actions">
          <button className="primary" disabled={saving} onClick={() => void save()}>
            {saving ? "saving" : "save settings"}
          </button>
        </div>
      </div>

      <div className="section">
        <h3>appearance</h3>
        <div className="panel">
          <div className="theme-grid">
            {THEMES.map((t) => (
              <button
                key={t.id}
                className={`theme-card ${theme === t.id ? "active" : ""}`}
                onClick={() => onTheme(t.id)}
                title={t.light ? "light theme" : "dark theme"}
              >
                <span className="theme-pair">
                  <i style={{ background: t.ground }} />
                  <i style={{ background: t.accent }} />
                </span>
                {t.name}
                {theme === t.id && <small>active</small>}
              </button>
            ))}
          </div>
          <div className="hint" style={{ marginTop: 10 }}>
            ground + accent, nothing else. the choice sticks between launches.
          </div>
        </div>
      </div>

      <div className="section">
        <h3>steam session</h3>
        <div className="panel">
          <div className="account-strip" style={{ marginBottom: 12 }}>
            <div style={{ flex: 1, minWidth: 0 }}>
              {account?.account_name ? (
                <>
                  <div className="account-name">
                    <span className="badge ok">steam</span> {account.account_name}
                    {account.persona && account.persona !== account.account_name && (
                      <span className="hint"> ({account.persona})</span>
                    )}
                  </div>
                  <div className="hint">
                    detected from the local client
                    {account.active ? " (signed in now)" : ""}
                    {account.remember_password ? " (login key cached)" : ""}
                  </div>
                </>
              ) : (
                <>
                  <div className="account-name">
                    <span className="badge warn">none</span> no steam account detected
                  </div>
                  <div className="hint">
                    install and sign in to Steam once, or use the QR flow on a download.
                  </div>
                </>
              )}
            </div>
            <span className={`badge ${qrDone ? "ok" : "warn"}`}>
              {qrDone ? `session · ${session.username || "cached"}` : "qr available"}
            </span>
          </div>

          <dl className="kv" style={{ marginBottom: 12 }}>
            <dt>steam id</dt>
            <dd>{account?.steam_id || "-"}</dd>
            <dt>depot session</dt>
            <dd>
              {qrDone
                ? `cached by DepotDownloader${session.username ? ` for ${session.username}` : ""}`
                : "none. the next pull shows a QR code"}
            </dd>
            <dt>token store</dt>
            <dd>{session.path || "not written yet"}</dd>
          </dl>

          <div style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center" }}>
            <button disabled={!qrDone} onClick={() => void resetSession()}>
              reset session
            </button>
            <span className="hint" style={{ marginTop: 0 }}>
              {qrDone
                ? "deletes the cached login token. The QR code comes back"
                : "nothing cached, so the QR code already shows on the next pull"}
            </span>
          </div>

          <label className="field" style={{ marginTop: 14 }}>
            <span>account name override</span>
            <input
              value={draft.steam_username}
              autoComplete="off"
              placeholder={account?.account_name || "auto-detect"}
              onChange={(e) => patch("steam_username", e.target.value)}
            />
            <div className="hint">
              blank uses whatever the Steam client reports. no password is ever stored. The depot
              tool reuses the cached login key.
            </div>
          </label>
        </div>
      </div>

      <div className="section">
        <h3>depot backend</h3>
        <div className="panel">
          <div className="two-col">
            <label className="field">
              <span>backend</span>
              <select
                value={draft.depot_backend}
                onChange={(e) =>
                  patch("depot_backend", e.target.value as Settings["depot_backend"])
                }
              >
                <option value="depot_downloader">DepotDownloader (managed)</option>
                <option value="steam_cmd">steamcmd</option>
              </select>
            </label>
            <label className="field">
              <span>parallel downloads</span>
              <input
                type="number"
                min={1}
                max={32}
                value={draft.max_downloads}
                onChange={(e) => patch("max_downloads", Number(e.target.value) || 8)}
              />
            </label>
          </div>

          <label className="field">
            <span>DepotDownloader.exe path</span>
            <input
              value={draft.depotdownloader_path}
              placeholder={tool?.depotdownloader ?? "not installed. use the button below"}
              onChange={(e) => patch("depotdownloader_path", e.target.value)}
            />
            <div className="hint">
              blank uses the managed copy at{" "}
              <span className="mono">{tool?.depotdownloader ?? "…/tools/depotdownloader"}</span>
            </div>
          </label>

          <label className="field">
            <span>steamcmd.exe path</span>
            <input
              value={draft.steamcmd_path}
              placeholder={tool?.steamcmd ?? "C:\\steamcmd\\steamcmd.exe"}
              onChange={(e) => patch("steamcmd_path", e.target.value)}
            />
          </label>

          <div style={{ display: "flex", gap: 10, alignItems: "center", flexWrap: "wrap" }}>
            <button
              className="primary"
              disabled={busy === "toolchain"}
              onClick={() => void onInstallToolchain()}
            >
              {busy === "toolchain" ? "downloading" : "install / update depotdownloader"}
            </button>
            <span className={`badge ${tool?.ready ? "ok" : "bad"}`}>
              {tool?.ready ? "backend ready" : "backend missing"}
            </span>
          </div>

          {toolProgress && (
            <div className="download-top" style={{ marginTop: 12 }}>
              <span className="download-title">{toolProgress.label}</span>
              <div className="bar">
                <i style={{ width: `${toolProgress.percent}%` }} />
              </div>
              <span className="mono">{toolProgress.percent.toFixed(0)}%</span>
            </div>
          )}
        </div>
      </div>

      <div className="section">
        <h3>storage</h3>
        <div className="panel">
          <label className="field">
            <span>instances folder</span>
            <input
              value={draft.instances_dir}
              placeholder={state.instances_dir}
              onChange={(e) => patch("instances_dir", e.target.value)}
            />
            <div className="hint">
              blank uses the app data folder. changing this only affects instances created
              afterwards. Existing ones keep their absolute paths.
            </div>
          </label>

          <div className="two-col">
            <label className="field">
              <span>purge depot cache after pull</span>
              <select
                value={draft.purge_depot_cache ? "yes" : "no"}
                onChange={(e) => patch("purge_depot_cache", e.target.value === "yes")}
              >
                <option value="yes">yes</option>
                <option value="no">no</option>
              </select>
            </label>
            <label className="field">
              <span>close launcher on game start</span>
              <select
                value={draft.close_on_launch ? "yes" : "no"}
                onChange={(e) => patch("close_on_launch", e.target.value === "yes")}
              >
                <option value="no">no</option>
                <option value="yes">yes</option>
              </select>
            </label>
          </div>

          <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
            <button onClick={() => void api.openPath(state.data_dir)}>open app data</button>
            <button
              onClick={() => {
                void navigator.clipboard
                  .writeText(state.data_dir)
                  .then(() => notify("path copied", "ok"))
                  .catch((err) => notify(errText(err), "error"));
              }}
            >
              copy data path
            </button>
          </div>
          <div className="hint mono" style={{ marginTop: 8 }}>
            {state.data_dir}
          </div>
        </div>
      </div>

      <div className="section">
        <h3>version catalogue ({state.versions.length})</h3>
        <div className="panel">
          {state.versions.length === 0 ? (
            <div className="hint">
              empty. every manifest added through “add version” lands here with its label, so it can
              be reused for another instance.
            </div>
          ) : (
            <div className="scroll-x">
              <table>
                <thead>
                  <tr>
                    <th>version</th>
                    <th>build</th>
                    <th>manifest</th>
                    <th>depot</th>
                    <th>notes</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {state.versions.map((v) => (
                    <tr key={v.id}>
                      <td>
                        <span className="badge version">{v.version}</span>
                      </td>
                      <td className="mono">{v.build_id || "-"}</td>
                      <td className="mono">{v.manifest_id}</td>
                      <td className="mono">{v.depot_id}</td>
                      <td>{v.notes || "-"}</td>
                      <td style={{ textAlign: "right" }}>
                        <button
                          className="tiny danger"
                          onClick={() => {
                            void api
                              .removeVersion(v.id)
                              .then(() => onReload())
                              .catch((err) => notify(errText(err), "error"));
                          }}
                        >
                          drop
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      </div>
    </>
  );
}
