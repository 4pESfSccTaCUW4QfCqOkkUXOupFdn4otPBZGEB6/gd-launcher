import { invoke } from "@tauri-apps/api/core";
import type {
  AppStateView,
  DepotInfo,
  InstanceDetail,
  InstanceView,
  InstalledMod,
  KnownVersion,
  LoaderVersion,
  MegaHackTarget,
  MhBundle,
  MhInstallReport,
  MhInstalled,
  MhLicense,
  ModSearchPage,
  Settings,
  SteamAccount,
  SteamInstall,
  ToolchainStatus,
  VersionEntry,
} from "./types";

export const api = {
  state: () => invoke<AppStateView>("get_app_state"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),

  depotInfo: () => invoke<DepotInfo>("fetch_depot_info"),
  currentManifest: () =>
    invoke<{ manifestId: string; buildId: string } | null>("current_manifest"),
  steamInstalls: () => invoke<SteamInstall[]>("find_steam_installs"),
  steamAccount: () => invoke<SteamAccount | null>("steam_account"),
  toolchain: () => invoke<ToolchainStatus>("toolchain_status"),
  installToolchain: () => invoke<string>("install_toolchain"),

  addVersion: (entry: VersionEntry) => invoke<VersionEntry[]>("add_version", { entry }),
  removeVersion: (id: string) => invoke<VersionEntry[]>("remove_version", { id }),
  knownVersions: () => invoke<KnownVersion[]>("known_versions"),

  quickInstall: (request: {
    version: string;
    manifest_id: string;
    depot_id: string;
    username: string;
    password: string;
    use_qr: boolean;
    start_download: boolean;
  }) => invoke<InstanceView>("quick_install", { request }),

  createInstance: (request: {
    name: string;
    version: string;
    build_id: string;
    manifest_id: string;
    depot_id: string;
  }) => invoke<InstanceView>("create_instance", { request }),

  importSteam: (request: { index: number; mode: "link" | "copy"; name: string }) =>
    invoke<InstanceView>("import_steam_install", { request }),

  removeInstance: (id: string, deleteFiles: boolean) =>
    invoke<void>("remove_instance", { id, deleteFiles }),

  launch: (id: string) => invoke<void>("launch_instance", { id }),
  detail: (id: string) => invoke<InstanceDetail>("instance_detail", { id }),
  openFolder: (id: string) => invoke<void>("open_instance_folder", { id }),
  openPath: (path: string) => invoke<void>("open_path", { path }),

  startDownload: (request: {
    instance_id: string;
    manifest_id: string;
    depot_id: string;
    username: string;
    password: string;
    use_qr: boolean;
  }) => invoke<void>("start_depot_download", { request }),
  sendInput: (text: string) => invoke<void>("send_depot_input", { text }),
  cancelDownload: () => invoke<void>("cancel_depot_download"),
  downloadRunning: () => invoke<boolean>("download_running"),

  searchMods: (query: string, page: number, perPage: number) =>
    invoke<ModSearchPage>("search_mods", { query, page, perPage }),
  installMod: (instanceId: string, modId: string) =>
    invoke<InstalledMod>("install_mod", { instanceId, modId }),
  uninstallMod: (instanceId: string, modId: string) =>
    invoke<InstalledMod[]>("uninstall_mod", { instanceId, modId }),
  installedMods: (instanceId: string) =>
    invoke<InstalledMod[]>("list_installed_mods", { instanceId }),

  loaderVersions: () => invoke<LoaderVersion[]>("loader_versions"),
  installLoader: (instanceId: string, version: string | null) =>
    invoke<string>("install_loader", { instanceId, version }),

  megaHackTarget: (version: string) =>
    invoke<MegaHackTarget | null>("mega_hack_target", { version }),
  megaHackAvailable: (instanceId: string) =>
    invoke<MhBundle | null>("mega_hack_available", { instanceId }),
  megaHackInstall: (instanceId: string, release: string) =>
    invoke<MhInstallReport>("mega_hack_install", { instanceId, release }),
  megaHackUninstall: (instanceId: string) =>
    invoke<number>("mega_hack_uninstall", { instanceId }),
  megaHackInstalled: (instanceId: string) =>
    invoke<MhInstalled | null>("mega_hack_installed", { instanceId }),
  megaHackLicense: () => invoke<MhLicense>("mega_hack_license"),
  runInstaller: (instanceId: string, path: string) =>
    invoke<void>("run_installer", { instanceId, path }),
  resetSteamSession: () => invoke<number>("reset_steam_session"),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
};

export function errText(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return String(err);
}

export function bytes(n: number): string {
  if (!n) return "—";
  const units = ["B", "KB", "MB", "GB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

export function count(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return String(n);
}
