export type DepotBackend = "depot_downloader" | "steam_cmd";

export interface Settings {
  depot_backend: DepotBackend;
  depotdownloader_path: string;
  steamcmd_path: string;
  steam_username: string;
  remember_username: boolean;
  instances_dir: string;
  max_downloads: number;
  purge_depot_cache: boolean;
  close_on_launch: boolean;
  steam_qr_done: boolean;
}

export interface InstanceView {
  id: string;
  name: string;
  version: string;
  build_id: string;
  manifest_id: string;
  depot_id: string;
  path: string;
  created_at: number;
  linked: boolean;
  exists: boolean;
  has_exe: boolean;
  geode: boolean;
  mod_count: number;
  size_bytes: number;
  size_human: string;
  catalog_version: string | null;
}

export interface VersionEntry {
  id: string;
  version: string;
  build_id: string;
  manifest_id: string;
  depot_id: string;
  notes: string;
  added_at: number;
}

export interface KnownVersion {
  version: string;
  date: string;
  manifest_id: string;
  depot_id: string;
  source: "live" | "bundled" | "added";
  note: string;
  mega_hack: string;
}

export interface SteamAccount {
  steam_id: string;
  account_name: string;
  persona: string;
  remember_password: boolean;
  auto_login: boolean;
  active: boolean;
}

export interface MhRelease {
  version: string;
  major: number;
  major_label: string;
  game: string;
  platforms: string;
  date: string;
  geode: boolean;
  standalone: boolean;
  paid: boolean;
}

export interface MhBundle {
  name: string;
  group: string;
  version: string;
  file: string;
  platform: string[];
  gjver: string;
  auth: string;
  geode: boolean;
  notice: string;
  hash: string;
}

export interface MhInstallReport {
  bundle: MhBundle;
  geode: boolean;
  files: string[];
  bytes: number;
}

export interface MhInstalled {
  release: string;
  bundle: string;
  geode: boolean;
  files: string[];
}

export interface MhLicense {
  signed_in: boolean;
  path: string;
  masked: string;
}

export interface MegaHackTarget {
  major: string;
  label: string;
  paid: boolean;
  below_2_2: boolean;
  covers: string;
  store_url: string;
  note: string;
  recommended: MhRelease;
  releases: MhRelease[];
}

export interface SteamInstall {
  library: string;
  path: string;
  build_id: string;
  manifest_id: string;
  size_on_disk: number;
}

/// The cached Steam login DepotDownloader is holding, if any.
export interface DepotSession {
  signed_in: boolean;
  username: string;
  path: string;
  updated_at: number;
}

export interface AppStateView {
  settings: Settings;
  instances: InstanceView[];
  versions: VersionEntry[];
  steam_installs: SteamInstall[];
  data_dir: string;
  instances_dir: string;
  depot_session: DepotSession;
}

export interface DepotRow {
  depot_id: string;
  os: string;
  manifest_id: string;
  branch: string;
  max_size: number;
}

export interface DepotBranch {
  branch: string;
  manifest_id: string;
  build_id: string;
  updated_at: string;
  description: string;
}

export interface DepotInfo {
  app_id: string;
  name: string;
  depots: DepotRow[];
  branches: DepotBranch[];
}

export interface ToolchainStatus {
  backend: DepotBackend;
  depotdownloader: string | null;
  steamcmd: string | null;
  ready: boolean;
}

export interface ModCard {
  id: string;
  name: string;
  description: string;
  version: string;
  downloads: number;
  featured: boolean;
  developers: string[];
  gd_win: string;
  geode: string;
  download_link: string;
  hash: string;
}

export interface ModSearchPage {
  mods: ModCard[];
  total: number;
  page: number;
  per_page: number;
}

export interface InstalledMod {
  id: string;
  name: string;
  version: string;
  developer: string;
  description: string;
  gd_win: string;
  file: string;
  size: number;
}

export interface LoaderVersion {
  version: string;
  gd_win: string;
  prerelease: boolean;
  url: string;
  hash: string;
  created_at: string;
}

export interface InstanceDetail {
  view: InstanceView;
  mods: InstalledMod[];
}

export interface DownloadProgress {
  label: string;
  received: number;
  total: number;
  percent: number;
}

export interface QrFrame {
  lines: string[];
}

export interface DepotLog {
  stream: string;
  line: string;
}

export interface DepotProgress {
  percent: number;
  detail: string;
}

export interface DepotExit {
  code: number;
  ok: boolean;
}
