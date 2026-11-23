import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, bytes, errText } from "../api";
import type {
  DepotSession,
  DownloadProgress,
  InstanceDetail,
  LoaderVersion,
  MegaHackTarget,
  MhBundle,
  MhInstalled,
  MhLicense,
} from "../types";
import type { Toast } from "./Toasts";

interface Props {
  instanceId: string;
  session: DepotSession;
  notify: (message: string, kind?: Toast["kind"]) => void;
  onBack: () => void;
  onReload: () => Promise<void>;
  onBrowseMods: () => void;
  onStartDownload: (
    instanceId: string,
    manifestId: string,
    depotId: string,
    useQr?: boolean,
  ) => Promise<void>;
}

export default function InstancePage({
  instanceId,
  session,
  notify,
  onBack,
  onReload,
  onBrowseMods,
  onStartDownload,
}: Props) {
  const [detail, setDetail] = useState<InstanceDetail | null>(null);
  const [loaders, setLoaders] = useState<LoaderVersion[]>([]);
  const [loaderPick, setLoaderPick] = useState<string>("");
  const [mh, setMh] = useState<MegaHackTarget | null>(null);
  const [mhPick, setMhPick] = useState("");
  const [mhAvail, setMhAvail] = useState<MhBundle | null>(null);
  const [mhHave, setMhHave] = useState<MhInstalled | null>(null);
  const [mhLicense, setMhLicense] = useState<MhLicense | null>(null);
  const [mhProgress, setMhProgress] = useState<DownloadProgress | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [manifest, setManifest] = useState("");
  const [depot, setDepot] = useState("322171");
  const [useQr, setUseQr] = useState(!session.signed_in);

  const version = detail?.view.version ?? "";

  const load = useCallback(async () => {
    try {
      const next = await api.detail(instanceId);
      setDetail(next);
      setManifest(next.view.manifest_id);
      setDepot(next.view.depot_id || "322171");
    } catch (err) {
      notify(errText(err), "error");
    }
  }, [instanceId, notify]);

  useEffect(() => {
    void load();
  }, [load]);

  // Loader list is only relevant from 2.2 up; Mega Hack targeting is always useful.
  useEffect(() => {
    let alive = true;
    api
      .loaderVersions()
      .then((list) => {
        if (!alive) return;
        setLoaders(list);
        setLoaderPick(list.find((l) => !l.prerelease)?.version ?? list[0]?.version ?? "");
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    let alive = true;
    if (!version) {
      setMh(null);
      return;
    }
    api
      .megaHackTarget(version)
      .then((target) => {
        if (!alive) return;
        setMh(target);
        // Start on the newest stable release that runs this build.
        setMhPick(target?.releases[0]?.version ?? "");
      })
      .catch(() => {
        if (!alive) return;
        setMh(null);
        setMhPick("");
      });
    return () => {
      alive = false;
    };
  }, [version]);

  // What the catalogue can install for this instance, what is already in the
  // folder, and whether a paid license is on disk.
  useEffect(() => {
    let alive = true;
    if (!version) {
      setMhAvail(null);
      setMhHave(null);
      return;
    }
    api
      .megaHackInstalled(instanceId)
      .then((have) => {
        if (alive) setMhHave(have);
      })
      .catch(() => undefined);
    api
      .megaHackLicense()
      .then((lic) => {
        if (alive) setMhLicense(lic);
      })
      .catch(() => undefined);
    if (!mhPick) {
      setMhAvail(null);
      return;
    }
    api
      .megaHackAvailable(instanceId)
      .then((bundle) => {
        if (alive) setMhAvail(bundle);
      })
      .catch(() => {
        if (alive) setMhAvail(null);
      });
    return () => {
      alive = false;
    };
  }, [instanceId, version, mhPick]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let gone = false;
    void listen<DownloadProgress>("mh://progress", (e) => setMhProgress(e.payload)).then((un) => {
      if (gone) un();
      else unlisten = un;
    });
    return () => {
      gone = true;
      unlisten?.();
    };
  }, []);

  if (!detail) {
    return (
      <div className="empty">
        <b>loading instance</b>
        <span>reading the folder…</span>
      </div>
    );
  }

  const inst = detail.view;
  const below22 = mh?.below_2_2 ?? false;
  // The picker starts on the newest stable release; the table and the buttons all
  // follow whatever is selected.
  const mhRelease = mh
    ? (mh.releases.find((r) => r.version === mhPick) ?? mh.recommended)
    : null;

  const launch = async () => {
    try {
      await api.launch(instanceId);
      notify("game started", "ok");
    } catch (err) {
      notify(errText(err), "error");
    }
  };

  const installLoader = async () => {
    setBusy("loader");
    try {
      const installed = await api.installLoader(instanceId, loaderPick || null);
      notify(`Geode ${installed} installed`, "ok");
      await load();
      await onReload();
    } catch (err) {
      notify(errText(err), "error");
    } finally {
      setBusy(null);
    }
  };

  const pullDepot = async () => {
    if (!manifest.trim()) {
      notify("a manifest id is required", "error");
      return;
    }
    setBusy("depot");
    try {
      await onStartDownload(instanceId, manifest.trim(), depot.trim() || "322171", useQr);
      notify("depot pull started", "ok");
    } finally {
      setBusy(null);
    }
  };

  const getInstaller = async () => {
    if (!mh || !mhRelease) return;
    try {
      await api.openUrl(mh.store_url);
      notify(`opened the store — pick ${mhRelease.version}`, "ok");
    } catch (err) {
      notify(errText(err), "error");
    }
  };

  const installMegaHack = async () => {
    if (!mhRelease) return;
    setBusy("mh");
    setMhProgress(null);
    try {
      const report = await api.megaHackInstall(instanceId, mhRelease.version);
      notify(
        `Mega Hack ${report.bundle.version} installed — ${report.geode ? "geode package" : `${report.files.length} files`}`,
        "ok",
      );
      setMhHave(await api.megaHackInstalled(instanceId));
      await load();
      await onReload();
    } catch (err) {
      notify(errText(err), "error");
    } finally {
      setBusy(null);
      setMhProgress(null);
    }
  };

  const removeMegaHack = async () => {
    if (!window.confirm("Remove Mega Hack files this launcher installed?")) return;
    setBusy("mh");
    try {
      const removed = await api.megaHackUninstall(instanceId);
      notify(`${removed} file${removed === 1 ? "" : "s"} removed`, "ok");
      setMhHave(null);
      await load();
      await onReload();
    } catch (err) {
      notify(errText(err), "error");
    } finally {
      setBusy(null);
    }
  };

  const removeInstance = async () => {
    if (!inst) return;
    const message = inst.linked
      ? `Unregister "${inst.name}" from the launcher? The Steam folder is left untouched.`
      : `Delete "${inst.name}" and its files from disk? This cannot be undone.`;
    if (!window.confirm(message)) return;
    try {
      await api.removeInstance(instanceId, !inst.linked);
      await onReload();
      notify("instance removed", "ok");
      onBack();
    } catch (err) {
      notify(errText(err), "error");
    }
  };

  const uninstall = async (modId: string, name: string) => {
    if (!window.confirm(`Remove ${name} from this instance?`)) return;
    try {
      const mods = await api.uninstallMod(instanceId, modId);
      setDetail({ ...detail, mods });
      await onReload();
      notify(`${name} removed`, "ok");
    } catch (err) {
      notify(errText(err), "error");
    }
  };

  const compat = (modGd: string) =>
    !modGd || !inst.version || modGd === inst.version ? "ok" : "warn";

  return (
    <>
      <div className="page-head">
        <div>
          <button className="ghost tiny" onClick={onBack} style={{ marginBottom: 6 }}>
            ← library
          </button>
          <h1>
            {inst.name}
            <span className="badge version">{inst.version || "unlabelled"}</span>
            {inst.geode && <span className="badge ok">geode</span>}
            {mh && mhRelease && <span className="badge info">{mhRelease.major_label}</span>}
          </h1>
          <p>{inst.path}</p>
        </div>
        <div className="head-actions">
          <button onClick={() => void api.openFolder(instanceId)}>Folder</button>
          {!below22 && <button onClick={onBrowseMods}>Mods</button>}
          <button className="danger" onClick={() => void removeInstance()}>
            Remove
          </button>
          <button className="primary" disabled={!inst.has_exe} onClick={() => void launch()}>
            Launch
          </button>
        </div>
      </div>

      <div className="section">
        <h3>build</h3>
        <div className="panel">
          <dl className="kv">
            <dt>version</dt>
            <dd>{inst.version || "—"}</dd>
            <dt>steam build</dt>
            <dd>{inst.build_id || "—"}</dd>
            <dt>manifest</dt>
            <dd>{inst.manifest_id || "—"}</dd>
            <dt>depot</dt>
            <dd>{inst.depot_id || "—"}</dd>
            <dt>on disk</dt>
            <dd>
              {inst.size_human} · {inst.has_exe ? "executable present" : "executable missing"}
            </dd>
            <dt>mods</dt>
            <dd>{inst.mod_count}</dd>
            {inst.linked && (
              <>
                <dt>source</dt>
                <dd>linked live Steam install (not managed)</dd>
              </>
            )}
          </dl>

          {!inst.linked && (
            <details className="disclosure">
              <summary>advanced — pull or repair this build by manifest</summary>
              <div className="two-col" style={{ marginTop: 14 }}>
                <label className="field">
                  <span>manifest gid</span>
                  <input
                    value={manifest}
                    onChange={(e) => setManifest(e.target.value.trim())}
                    placeholder="manifest gid"
                  />
                </label>
                <label className="field">
                  <span>depot</span>
                  <input value={depot} onChange={(e) => setDepot(e.target.value.trim())} />
                </label>
              </div>
              {session.signed_in ? (
                <div className="hint" style={{ marginBottom: 10 }}>
                  steam session cached for{" "}
                  <b style={{ color: "var(--text)" }}>{session.username || "this account"}</b> — the
                  pull authenticates on its own.
                </div>
              ) : (
                <label className="toggle" style={{ marginBottom: 10 }}>
                  <input
                    type="checkbox"
                    checked={useQr}
                    onChange={(e) => setUseQr(e.target.checked)}
                  />
                  <span>
                    sign in with QR
                    <small>phone scan, no password</small>
                  </span>
                </label>
              )}
              <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
                <button disabled={busy === "depot"} onClick={() => void pullDepot()}>
                  {busy === "depot" ? "starting" : "pull depot"}
                </button>
                <span className="hint" style={{ marginTop: 0 }}>
                  re-running a manifest validates and repairs in place
                </span>
              </div>
            </details>
          )}
        </div>
      </div>

      {mh && mhRelease && (
        <div className="section">
          <h3>mega hack</h3>
          <div className="panel">
            <div style={{ display: "flex", gap: 10, alignItems: "flex-end", flexWrap: "wrap" }}>
              <label className="field" style={{ marginBottom: 0, minWidth: 300, flex: "0 1 340px" }}>
                <span>release to install</span>
                <select value={mhPick} onChange={(e) => setMhPick(e.target.value)}>
                  {mh.releases.map((r) => (
                    <option key={r.version} value={r.version}>
                      {r.version} · gd {r.game} · {r.paid ? "paid" : "free"}
                    </option>
                  ))}
                </select>
              </label>
              <span className={`badge ${mhRelease.paid ? "warn" : "ok"}`}>
                {mhRelease.paid ? "paid" : "free"}
              </span>
              <span className="badge info">{mhRelease.major_label}</span>
              {mhRelease.version === mh.recommended.version && (
                <span className="badge ok">default</span>
              )}
            </div>

            <div className="meta-row" style={{ marginTop: 12 }}>
              <span>
                for gd <b>{mhRelease.game}</b>
              </span>
              <span>
                released <b>{mhRelease.date}</b>
              </span>
              <span>
                channel{" "}
                <b>
                  {[mhRelease.geode ? "geode" : "", mhRelease.standalone ? "standalone" : ""]
                    .filter(Boolean)
                    .join(" · ") || "—"}
                </b>
              </span>
              <span>
                {mhRelease.platforms}
              </span>
            </div>

            {mhHave && (
              <div className="account-strip" style={{ marginTop: 14, marginBottom: 0 }}>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div className="account-name">
                    <span className="badge ok">installed</span> Mega Hack {mhHave.bundle}
                  </div>
                  <div className="hint">
                    {mhHave.geode
                      ? "geode package in geode/mods"
                      : `${mhHave.files.length} files at the instance root`}
                  </div>
                </div>
                <button className="tiny danger" onClick={() => void removeMegaHack()}>
                  remove
                </button>
              </div>
            )}

            <div style={{ display: "flex", gap: 8, flexWrap: "wrap", marginTop: 14 }}>
              {mhAvail ? (
                mhRelease.paid && !mhLicense?.signed_in ? (
                  <button className="primary" onClick={() => void getInstaller()}>
                    sign in to install {mhAvail.version}
                  </button>
                ) : (
                  <button
                    className="primary"
                    disabled={busy === "mh"}
                    onClick={() => void installMegaHack()}
                  >
                    {busy === "mh" ? "installing" : `install ${mhAvail.version}`}
                  </button>
                )
              ) : (
                <button className="primary" onClick={() => void getInstaller()}>
                  get {mhRelease.version} from the store
                </button>
              )}
            </div>

            {busy === "mh" && mhProgress && (
              <div className="download-top" style={{ marginTop: 12 }}>
                <span className="download-title">{mhProgress.label}</span>
                <div className="bar">
                  <i style={{ width: `${mhProgress.percent}%` }} />
                </div>
                <span className="mono">{mhProgress.percent.toFixed(0)}%</span>
              </div>
            )}

            <div className="hint" style={{ marginTop: 10 }}>
              {mhAvail
                ? mhRelease.paid && !mhLicense?.signed_in
                  ? "This release is paid. Sign in once in Absolute's installer — the license it writes lives in %LOCALAPPDATA%\\absolllute.megahack and the launcher picks it up from there."
                  : `Downloads ${mhAvail.geode ? "the Geode package" : "the standalone DLLs"} straight into this instance — no folder picker, no Detect Geometry Dash, and never the Steam copy unless this instance points at it.`
                : "The catalogue does not serve this release, so there is nothing to download here. Open the store and grab it there."}
              {below22 && " Geode-only releases are not in the list: there is no Geode below 2.2."}
            </div>
          </div>
        </div>
      )}

      {!below22 && (
        <div className="section">
          <h3>geode loader</h3>
          <div className="panel">
            <div style={{ display: "flex", gap: 10, alignItems: "flex-end", flexWrap: "wrap" }}>
              <label className="field" style={{ marginBottom: 0, minWidth: 220 }}>
                <span>loader build</span>
                <select value={loaderPick} onChange={(e) => setLoaderPick(e.target.value)}>
                  {loaders.length === 0 && <option value="">index offline</option>}
                  {loaders.map((l) => (
                    <option key={l.version} value={l.version}>
                      {l.version} · gd {l.gd_win || "?"} {l.prerelease ? "(beta)" : ""}
                    </option>
                  ))}
                </select>
              </label>
              <button
                className={inst.geode ? "" : "primary"}
                disabled={busy === "loader" || loaders.length === 0}
                onClick={() => void installLoader()}
              >
                {busy === "loader"
                  ? "installing"
                  : inst.geode
                    ? "reinstall / update"
                    : "install geode"}
              </button>
            </div>
            <div className="hint" style={{ marginTop: 10 }}>
              Geode needs 2.2 or newer. Below that the launcher offers Mega Hack instead.
            </div>
          </div>
        </div>
      )}

      {below22 && (
        <div className="section">
          <h3>geode loader</h3>
          <div className="panel">
            <div className="hint" style={{ marginTop: 0 }}>
              Unavailable on {inst.version || "this build"} — Geode requires 2.2 or newer. Use
              Mega Hack {mh?.major ?? "v5"} above.
            </div>
          </div>
        </div>
      )}

      {!below22 && (
        <div className="section">
          <h3>installed mods ({detail.mods.length})</h3>
          {detail.mods.length === 0 ? (
            <div className="empty">
              <b>no mods in this instance</b>
              <span>browse the index and install straight into this folder</span>
            </div>
          ) : (
            <div className="list">
              {detail.mods.map((mod) => (
                <div className="row" key={mod.file}>
                  <div className="row-main">
                    <div className="row-title">
                      {mod.name}
                      <span className="badge">v{mod.version}</span>
                      {mod.gd_win && (
                        <span className={`badge ${compat(mod.gd_win) === "ok" ? "ok" : "warn"}`}>
                          gd {mod.gd_win}
                        </span>
                      )}
                    </div>
                    <div className="row-desc">
                      {mod.developer ? `${mod.developer} — ` : ""}
                      {mod.description || mod.id}
                    </div>
                  </div>
                  <div className="row-side">
                    <span className="mono">{bytes(mod.size)}</span>
                    <button
                      className="tiny danger"
                      onClick={() => void uninstall(mod.id, mod.name)}
                    >
                      remove
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </>
  );
}
