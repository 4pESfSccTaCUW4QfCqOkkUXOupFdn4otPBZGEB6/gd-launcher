import { useState } from "react";
import { api, bytes, errText } from "../api";
import type { AppStateView, DownloadProgress, ToolchainStatus } from "../types";
import AddVersionModal from "./AddVersionModal";
import type { Toast } from "./Toasts";

interface Props {
  state: AppStateView;
  tool: ToolchainStatus | null;
  busy: string | null;
  copyProgress: DownloadProgress | null;
  onReload: () => Promise<void>;
  notify: (message: string, kind?: Toast["kind"]) => void;
  onOpenInstance: (id: string) => void;
  onBrowseMods: (id: string) => void;
  onQuickInstall: (
    version: string,
    manifestId: string,
    depotId: string,
    useQr: boolean,
  ) => Promise<string>;
  onInstallToolchain: () => Promise<void>;
  onBusy: (value: string | null) => void;
}

export default function Library(props: Props) {
  const { state, tool, notify, onReload, onOpenInstance, onBusy } = props;
  const [showAdd, setShowAdd] = useState(false);
  const [showImport, setShowImport] = useState(false);
  const [mode, setMode] = useState<"link" | "copy">("copy");

  const launch = async (id: string) => {
    try {
      await api.launch(id);
      notify("game started", "ok");
    } catch (err) {
      notify(errText(err), "error");
    }
  };

  const importInstall = async (index: number) => {
    onBusy("import");
    try {
      const install = state.steam_installs[index];
      const created = await api.importSteam({
        index,
        mode,
        name: `Steam ${install.build_id || "copy"}`,
      });
      await onReload();
      setShowImport(false);
      notify(`imported ${created.name}`, "ok");
      onOpenInstance(created.id);
    } catch (err) {
      notify(errText(err), "error");
    } finally {
      onBusy(null);
    }
  };

  return (
    <>
      <div className="page-head">
        <div>
          <h1>Library</h1>
          <p>
            {state.instances.length} instance{state.instances.length === 1 ? "" : "s"}
            {state.depot_session.signed_in && ` · steam ${state.depot_session.username}`}
          </p>
        </div>
        <div className="head-actions">
          {state.steam_installs.length > 0 && (
            <button onClick={() => setShowImport(true)}>Import from Steam</button>
          )}
          <button className="primary" onClick={() => setShowAdd(true)}>
            Add version
          </button>
        </div>
      </div>

      {!tool?.ready && (
        <div className="section">
          <div className="panel">
            <div style={{ display: "flex", alignItems: "center", gap: 16, flexWrap: "wrap" }}>
              <div style={{ flex: 1, minWidth: 220 }}>
                <b>Depot toolchain is missing</b>
                <div className="hint" style={{ marginTop: 4 }}>
                  Pulling a specific Steam manifest needs DepotDownloader or steamcmd. One click,
                  ~32 MB, no Steam install required.
                </div>
              </div>
              <button
                className="primary"
                disabled={props.busy === "toolchain"}
                onClick={() => void props.onInstallToolchain()}
              >
                {props.busy === "toolchain" ? "Installing…" : "Install toolchain"}
              </button>
            </div>
          </div>
        </div>
      )}

      {state.instances.length === 0 ? (
        <div className="empty">
          <b>No instances yet</b>
          <span>
            Add a Steam depot manifest to pull a specific Geometry Dash build, or import the copy
            you already own.
          </span>
        </div>
      ) : (
        <div className="grid" style={{ marginTop: 20 }}>
          {state.instances.map((inst) => (
            <div className="card" key={inst.id}>
              <div className="card-top">
                <div style={{ minWidth: 0 }}>
                  <div className="card-title">{inst.name}</div>
                  <div className="card-sub">{inst.path}</div>
                </div>
                <span className="badge version">{inst.version || "unlabelled"}</span>
              </div>

              <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                {inst.geode ? (
                  <span className="badge ok">Geode · {inst.mod_count} mods</span>
                ) : (
                  <span className="badge warn">no loader</span>
                )}
                {inst.linked && <span className="badge info">linked steam copy</span>}
                {!inst.has_exe && <span className="badge bad">not downloaded</span>}
                <span className="badge">{inst.size_human}</span>
              </div>

              <div className="card-actions">
                <button
                  className="primary tiny"
                  disabled={!inst.has_exe}
                  onClick={() => void launch(inst.id)}
                >
                  Launch
                </button>
                <button className="tiny" onClick={() => onOpenInstance(inst.id)}>
                  Manage
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {showAdd && (
        <AddVersionModal
          state={state}
          onClose={() => setShowAdd(false)}
          notify={notify}
          onCreated={(id) => {
            setShowAdd(false);
            onOpenInstance(id);
          }}
          onQuickInstall={props.onQuickInstall}
        />
      )}

      {showImport && (
        <div className="overlay" onClick={() => setShowImport(false)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <h2>Import a Steam install</h2>
              <button className="ghost tiny" onClick={() => setShowImport(false)}>
                Close
              </button>
            </div>
            <div className="modal-body">
              <div className="seg" style={{ marginBottom: 14 }}>
                <button
                  className={mode === "copy" ? "active" : ""}
                  onClick={() => setMode("copy")}
                >
                  Copy files
                </button>
                <button
                  className={mode === "link" ? "active" : ""}
                  onClick={() => setMode("link")}
                >
                  Link in place
                </button>
              </div>
              <div className="hint" style={{ marginBottom: 12 }}>
                {mode === "copy"
                  ? "Duplicates the install into the launcher folder. Steam keeps its own copy; mods stay separate."
                  : "Points the launcher at the live Steam folder. Geode mods land in your real install. No duplicate disk use."}
              </div>

              <div className="list">
                {state.steam_installs.map((install, index) => (
                  <div className="row" key={install.path}>
                    <div className="row-main">
                      <div className="row-title">
                        Geometry Dash
                        <span className="badge">build {install.build_id || "?"}</span>
                      </div>
                      <div className="row-desc">{install.path}</div>
                    </div>
                    <div className="row-side">
                      <span className="mono">{bytes(install.size_on_disk)}</span>
                      <button
                        className="primary tiny"
                        disabled={props.busy === "import"}
                        onClick={() => void importInstall(index)}
                      >
                        {props.busy === "import" ? "Importing…" : "Import"}
                      </button>
                    </div>
                  </div>
                ))}
              </div>

              {props.copyProgress && props.busy === "import" && (
                <div style={{ marginTop: 14 }}>
                  <div className="download-top">
                    <span className="download-title">copying {props.copyProgress.label}</span>
                    <div className="bar">
                      <i style={{ width: `${props.copyProgress.percent}%` }} />
                    </div>
                    <span className="mono">{props.copyProgress.percent.toFixed(0)}%</span>
                  </div>
                </div>
              )}
            </div>
            <div className="modal-foot">
              <button onClick={() => setShowImport(false)}>Done</button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
