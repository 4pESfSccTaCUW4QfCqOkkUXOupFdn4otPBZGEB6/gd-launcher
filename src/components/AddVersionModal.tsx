import { useEffect, useMemo, useState } from "react";
import { api, errText } from "../api";
import type { AppStateView, KnownVersion, SteamAccount, VersionEntry } from "../types";
import type { Toast } from "./Toasts";

interface Props {
  state: AppStateView;
  onClose: () => void;
  notify: (message: string, kind?: Toast["kind"]) => void;
  onCreated: (id: string) => void;
  onQuickInstall: (
    version: string,
    manifestId: string,
    depotId: string,
    useQr: boolean,
  ) => Promise<string>;
}

export default function AddVersionModal({
  state,
  onClose,
  notify,
  onCreated,
  onQuickInstall,
}: Props) {
  const [rows, setRows] = useState<KnownVersion[]>([]);
  const [account, setAccount] = useState<SteamAccount | null>(null);
  const [loading, setLoading] = useState(true);
  const [useQr, setUseQr] = useState(!state.depot_session.signed_in);
  const [busy, setBusy] = useState<string | null>(null);
  const [customManifest, setCustomManifest] = useState("");
  const [customVersion, setCustomVersion] = useState("");

  useEffect(() => {
    let alive = true;
    setLoading(true);
    Promise.all([api.knownVersions(), api.steamAccount()])
      .then(([list, acct]) => {
        if (!alive) return;
        setRows(list);
        setAccount(acct);
      })
      .catch((err) => notify(errText(err), "error"))
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [notify]);

  const installedManifests = useMemo(
    () => new Set(state.instances.map((i) => i.manifest_id)),
    [state.instances],
  );

  // The cached login token is the truth. A detected Steam client is a fallback for
  // the first pull, and the QR scan covers everything else.
  const session = state.depot_session;
  const qrDone = session.signed_in;
  const canPull = Boolean(account?.account_name) || qrDone || useQr;

  const pull = async (version: string, manifestId: string, depotId: string) => {
    if (!canPull) {
      notify("no Steam account detected. pick one in Settings, or use QR sign-in", "error");
      return;
    }
    setBusy(manifestId);
    try {
      const id = await onQuickInstall(version, manifestId, depotId, useQr && !qrDone);
      notify(`pulling Geometry Dash ${version}`, "ok");
      onCreated(id);
    } catch (err) {
      notify(errText(err), "error");
    } finally {
      setBusy(null);
    }
  };

  const pullCustom = async () => {
    const manifest = customManifest.trim();
    if (!manifest) {
      notify("paste a manifest id", "error");
      return;
    }
    const label = customVersion.trim() || "custom";
    const entry: VersionEntry = {
      id: "",
      version: label,
      build_id: "",
      manifest_id: manifest,
      depot_id: "322171",
      notes: "",
      added_at: 0,
    };
    await api.addVersion(entry).catch(() => undefined);
    await pull(label, manifest, "322171");
  };

  return (
    <div className="overlay" onClick={onClose}>
      <div
        className="modal"
        onClick={(e) => e.stopPropagation()}
        style={{ width: "min(880px, 100%)" }}
      >
        <div className="modal-head">
          <h2>Add a Geometry Dash version</h2>
          <button className="ghost tiny" onClick={onClose}>
            Close
          </button>
        </div>

        <div className="modal-body">
          <div className="account-strip">
            <div style={{ flex: 1, minWidth: 0 }}>
              {account?.account_name ? (
                <>
                  <div className="account-name">
                    <span className="badge ok">Steam</span> {account.account_name}
                    {account.persona && account.persona !== account.account_name && (
                      <span className="hint"> ({account.persona})</span>
                    )}
                  </div>
                  <div className="hint">
                    Detected from your Steam client
                    {account.active ? " (currently signed in)" : ""}. No password needed; the
                    session is reused.
                  </div>
                </>
              ) : (
                <>
                  <div className="account-name">
                    <span className="badge warn">no account</span> Steam not detected
                  </div>
                  <div className="hint">
                    Sign in with a QR code below, or set an account name in Settings.
                  </div>
                </>
              )}
            </div>
            {qrDone ? (
              <span className="badge ok">signed in as {session.username || "steam"}</span>
            ) : (
              <label className="toggle">
                <input
                  type="checkbox"
                  checked={useQr}
                  onChange={(e) => setUseQr(e.target.checked)}
                />
                <span>
                  Sign in with QR
                  <small>phone scan, no password</small>
                </span>
              </label>
            )}
          </div>

          {loading ? (
            <div className="hint">
              <span className="spinner" /> reading the manifest table…
            </div>
          ) : (
            <div className="scroll-x" style={{ maxHeight: 380, overflowY: "auto" }}>
              <table>
                <thead>
                  <tr>
                    <th>Version</th>
                    <th>Released</th>
                    <th>Manifest</th>
                    <th>Mega Hack</th>
                    <th>Source</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {rows.map((row) => {
                    const already = installedManifests.has(row.manifest_id);
                    return (
                      <tr key={row.manifest_id}>
                        <td>
                          <span className={`badge ${row.source === "live" ? "info" : "version"}`}>
                            {row.version}
                          </span>
                          {already && (
                            <span className="badge ok" style={{ marginLeft: 6 }}>
                              installed
                            </span>
                          )}
                        </td>
                        <td className="mono">{row.source === "live" ? "now" : row.date || "-"}</td>
                        <td className="mono">{row.manifest_id}</td>
                        <td>
                          {row.mega_hack ? (
                            <span className="badge info">{row.mega_hack}</span>
                          ) : (
                            <span className="hint" style={{ marginTop: 0 }}>
                              -
                            </span>
                          )}
                        </td>
                        <td className="hint" style={{ margin: 0 }}>
                          {row.source === "live"
                            ? "steam, public branch"
                            : row.source === "added"
                              ? "added by you"
                              : "bundled history"}
                        </td>
                        <td style={{ textAlign: "right" }}>
                          <button
                            className={row.source === "live" ? "primary tiny" : "tiny"}
                            disabled={busy !== null || !canPull}
                            onClick={() => void pull(row.version, row.manifest_id, row.depot_id)}
                          >
                            {busy === row.manifest_id ? "Starting…" : "Download"}
                          </button>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}

          <div className="section" style={{ marginTop: 18, marginBottom: 0 }}>
            <h3>Any other manifest</h3>
            <div className="two-col">
              <label className="field">
                <span>Manifest id</span>
                <input
                  value={customManifest}
                  placeholder="641118531549408775"
                  onChange={(e) => setCustomManifest(e.target.value.trim())}
                />
              </label>
              <label className="field">
                <span>Version label</span>
                <input
                  value={customVersion}
                  placeholder="2.113"
                  onChange={(e) => setCustomVersion(e.target.value.trim())}
                />
              </label>
            </div>
            <button
              className="primary"
              disabled={busy !== null || !customManifest.trim() || !canPull}
              onClick={() => void pullCustom()}
            >
              {busy === customManifest.trim() ? "Starting…" : "Download this manifest"}
            </button>
          </div>
        </div>

        <div className="modal-foot">
          <span className="hint" style={{ marginRight: "auto" }}>
            Depot 322171, app 322170, fixed. Every download lands in its own instance folder.
          </span>
          <button onClick={onClose}>Done</button>
        </div>
      </div>
    </div>
  );
}
