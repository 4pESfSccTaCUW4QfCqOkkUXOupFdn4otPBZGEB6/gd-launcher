# GD Launcher

A Geometry Dash version manager and Geode mod launcher for Windows, built with Rust + Tauri v2.

Same shape as a Minecraft launcher: every Geometry Dash build is its own instance, you pick the
build by Steam depot manifest, and mods install straight into the instance you selected.

## Install

Grab the installer from [Releases](../../releases/latest):

```
GD.Launcher_1.1.0_x64-setup.exe
```

It installs per-user (no admin prompt) and needs nothing else — WebView2 ships with Windows 11.
Building from source instead: see [Build](#build).

The same release carries `gd.exe`, the terminal front end. Drop it anywhere on `PATH` and run
`gd help`; see [Terminal](#terminal).

## What it does

- **Any build, by manifest.** Paste a Steam depot manifest id (or take the live one from
  `api.steamcmd.net`) and the launcher pulls exactly that build into its own folder.
  The version label sits on the card next to the instance.
- **Real depot pulls.** Content comes from Steam's CDN through SteamRE/DepotDownloader
  (auto-fetched, ~32 MB) or Valve's `steamcmd` if you already have it. Steam Guard prompts
  are typed into the launcher's download console.
- **Geode mods.** Search the Geode index, install the newest build of any mod into a chosen
  instance, remove it again. Downloads are SHA-256 checked against the index hash.
- **Loader install.** Drops `Geode.dll` + the XInput proxy into the instance, with a picker
  for stable or beta loader builds. Gated at 2.2: the button does not exist below that, and the
  backend refuses the call even if the UI is bypassed.
- **Mega Hack below 2.2.** Anything older than 2.2 has no Geode, so the instance page offers
  the Mega Hack major that matches the build instead — v5 free for 2.1.13 and below, v8 for
  2.2 through 2.2074, v9 for 2.2081 and up. Paid majors read the license Absolute's installer
  writes and download straight into the instance; there is no folder picker and no separate
  installer step.
- **Import what you own.** Point at a Steam install (link in place) or duplicate it into a
  managed folder.
- **Launch.** Writes `steam_appid.txt` and starts `GeometryDash.exe` from the instance folder,
  so each build runs with its own save/mod state.
- **A terminal front end.** `gd.exe` does all of the above without a window — see
  [Terminal](#terminal).

## Look

Terminal skin, themeable. Every theme is two colours — a ground and an accent — with the greys
derived from the ground, so nothing fights. No radius, no gradients, no shadows, panels sharing
edges. Default is **amber** (`#000` + `#ffb000`); the rest are one click away in
**Settings → appearance**:

`amber` · `phosphor` · `ice` · `violet` · `ember` · `rose` · `slate` · `paper` (the one light theme)

The choice is a `data-theme` attribute on `<html>` backed by CSS variables and persisted in
`localStorage`, so it survives a restart and costs nothing at runtime.

Every theme carries a third derived value beyond the two: `--fill`, the raised surface a neutral
button sits on. **Buttons are filled, never outlined** — a neutral action is `--fill`, a primary
one is the accent, a destructive hover flips to the accent. There is no `1px solid` button
anywhere in the sheet. Badges follow the same rule: chips are filled, not boxed.

## Terminal

`gd.exe` is the same launcher without a window. Same data folder, same `config.json`, same
manifest table, same DepotDownloader invocation — an instance created in one shows up in the
other.

```
gd list                        managed instances
gd versions                    bundled game builds
gd install <version>           create an instance and pull its depot
gd launch <instance>           start Geometry Dash
gd remove <instance>           forget an instance  [--delete-files]
gd path <instance>             print the instance folder

gd steam                       Steam installs, accounts and the depot session
gd import <index>              register a Steam install  [--link|--copy]
gd login                       QR sign-in, cached for later pulls  [--force]

gd mh <instance>               Mega Hack releases for that build
gd mh <instance> install [r]   install one  (default: recommended)

gd mods <instance> list        installed Geode mods
gd mods <instance> search <q>  search the Geode index
gd mods <instance> install <id>
gd mods <instance> remove <id>

gd loader <instance> [status]  Geode loader state
gd loader <instance> install [version]

gd doctor                      paths, toolchain and session
```

An instance is matched by full id, id prefix, version or name, so `gd launch 2.2074` works.
`--data-dir` points the whole tool at a different folder; `-v` shows every line
DepotDownloader prints.

Two things behave differently to the window, both because a terminal is line-based:

- **The QR prints as block art.** DepotDownloader writes it to stdout in the Windows OEM code
  page; `depot::LinePump` decodes those bytes and the CLI prints the lines untouched, which is
  already square in a terminal cell grid. Scan it with the Steam app.
- **`gd login` pulls once.** DepotDownloader has no login-only mode — `-qr` is answered during a
  download — so a sign-in pulls the oldest bundled build into a scratch folder and deletes it.
  About 40 MB, once, and every later pull reuses the token.

A cached token that DepotDownloader cannot read is retried as a QR login rather than failing:
the token store is keyed by the tool's own path, so it does not survive the data folder moving.

## Build

Requires Rust (MSVC toolchain), Node 18+, and the WebView2 runtime (already present on
Windows 11).

```powershell
npm install
npm run tauri build
```

Artifacts:

```
src-tauri\target\release\gd-launcher.exe
src-tauri\target\release\gd.exe
src-tauri\target\release\bundle\nsis\GD Launcher_1.1.0_x64-setup.exe
```

`gd.exe` is a separate binary in the same package, so `cargo build --release --bin gd` builds it
on its own without touching the window.

Dev loop:

```powershell
npm run tauri dev
cargo run --manifest-path src-tauri/Cargo.toml --bin gd -- list
```

## First run

1. **Settings → Install / update DepotDownloader.** One click, managed under app data.
2. **Add version.** Pick a row from the manifest table — every Geometry Dash build from `2.0`
   through `2.2081` ships with its Steam depot manifest id and release date. Click
   **Download**. That's it: the launcher creates an instance folder and runs
   `-app 322170 -depot 322171 -manifest <id>`.
3. **Sign in — once.** No typing. The launcher reads your account straight out of the local
   Steam client (`config/loginusers.vdf` + the active account in the registry) and reuses its
   session. With no stored token, **Sign in with QR** is already ticked: a real QR appears in the
   download console, you scan it with the Steam mobile app, and the login token is written.
   Every later launch reads that token back, so the QR is only ever offered when there genuinely
   is no session. Settings → **reset session** deletes the token and brings the code back.
4. When the pull finishes, open the instance. On 2.2 and up, **Install Geode**. Below 2.2 the
   Geode panel is replaced by the Mega Hack panel for that build.
5. **Browse mods**, pick the instance in the top-right selector, install.

Anything not in the table — a brand-new public build, a beta branch, a manifest someone
linked you — goes in the *Any other manifest* box at the bottom with its version label. It is
added to the catalogue so it shows up in the table next time.

## Steam sign-in

Steam stopped storing passwords years ago — the client keeps a refresh token instead — so no
tool can "read the password". What the launcher does instead:

| What | Where it comes from |
| --- | --- |
| Account name | `config/loginusers.vdf`, active account first |
| Signed-in state | `HKCU\Software\Valve\Steam\ActiveProcess\ActiveUser` |
| Session token | DepotDownloader's own store, reused via `-remember-password` |
| First auth | `-qr` — phone scan, no password, no Steam Guard code |

The QR flow is a bootstrap, not a login screen — and it comes back by itself when it should.
DepotDownloader writes its token into .NET isolated storage:

```
%LOCALAPPDATA%\IsolatedStorage\<user>\<assembly>\<domain>\AssemFiles\account.config
```

The assembly and domain segments are hashes of the assembly identity, so they move with every
DepotDownloader release. `src-tauri/src/depot_session.rs` walks the store, inflates the
deflate-wrapped protobuf, and reads field 4 — the `LoginTokens` map — for an account name and a
non-empty token. Nothing else in that file is touched and the token itself never leaves the
module.

That read is what the UI believes. A launch with a token shows `session · <your account>` and no QR
toggle; a launch without one ticks **Sign in with QR** for you. When Steam rejects a stale key
the backend deletes it and fires `steam://auth-needed`, so the next pull shows a fresh code
instead of failing quietly. **Settings → reset session** deletes it on demand.

Password entry still exists as a fallback: if DepotDownloader prompts, the download console at
the bottom of the window takes the line. It is never written to disk.

## Mega Hack

Geode did not exist before 2.2, so old builds get the tool of their era. Every build in the
manifest table resolves to a Mega Hack release — the full public Windows history lives in
`src-tauri/src/mh.rs` (112 rows, newest first) and nothing about it is hard-coded in the front
end.

| Release | Game build | Cost | Channel |
| --- | --- | --- | --- |
| v9.1.3 | 2.2081 | paid | Geode |
| v9.0.11 | 2.2074 | paid | Geode |
| v8.0.3 | 2.206 | paid | Geode + standalone |
| v8.0.0-alpha.13d | 2.204 | paid | Geode + standalone |
| v8.0.0-alpha.5a | 2.2 | paid | standalone |
| v7.1.1.3 | 2.113 | paid | Geode + standalone |
| v6.3.0-GM1 | 2.113 | paid | standalone |
| v5.4 | 2.113 and below | free | standalone |

Rules the resolver enforces, all covered by unit tests:

- **Stable only, one per major.** A release with `alpha` or `beta` in its name is skipped whenever that major shipped a finished build — so 2.2081 gets `v9.1.3`, never `v9.1.0-beta.8`, and 2.2074 gets `v8.0.3`, never `v8.0.6-alpha.1`.
- **One exception:** the 2.2 line was alpha-only until v8.0.2, so builds below 2.206 fall back to the newest alpha of that major instead of showing an empty panel.
- **Newest first, newest selected.** The picker opens on the newest stable release that runs the build, and every option is selectable — the buttons and the detail block follow the selection.
- **2.1 ladder is v7 → v6 → v5.** 2.113 offers all three; anything older falls back to v5 alone, the free one. Geode-only releases are hidden below 2.2 because there is no Geode there (`v7.1.1.4` is skipped in favour of the standalone `v7.1.1.3`).
- **2.2 and up is v9 → v8.**
- **No guessing forward.** A build with no row of its own (`2.208`, `2.2071`, `2.201`) takes the newest release targeting an *older* build, never a newer one.

Version ordering follows Geometry Dash's own scheme, `2.<minor digit><sub-build>`, where the
tail is a digit string and not a number — `2.2074` is 2.2 build 074 and shipped *before*
`2.208`, which is build 08. Comparing those as integers picks the wrong tool, so the tail is
compared digit by digit with shorter first (`2.2` < `2.201`).

There is no public direct download for any release and v6+ is account-gated, so the launcher
resolves the release from Absolute's own catalogue and installs it itself — see
[Native Mega Hack install](#native-mega-hack-install). The instance page is a release selector
and one install button.

## Native Mega Hack install

The installer is a Qt wrapper around a public catalogue, so the launcher skips it:

```
GET https://absolllute.com/api/mega_hack/v9/install.json
GET https://absolllute.com/api/mega_hack/v9/files/{version}/{file}
```

Each entry names the release, the game build it targets, Geode or standalone, and the SHA-256
of the archive. `src-tauri/src/mh_install.rs` resolves the entry for the instance's exact build,
downloads it, verifies the hash, and drops it where the game looks:

| Shape | Payload | Lands in |
| --- | --- | --- |
| Geode | `absolllute.megahack.geode` | `<instance>\geode\mods` |
| Standalone | 4 DLLs (`hackpro.dll`, `hackproldr.dll`, `discord_game_sdk.dll`, `XInput9_1_0.dll`) | instance root |

No installer window, no folder picker, and nothing written to your Steam copy — the instance
you picked is the only folder touched. A `.mh_install_data` marker records the release and every
file written, so **remove** on the instance page deletes exactly those files and nothing else.
There is no separate "run installer" step: one button, start to finish.

Only exact game builds are served: a build the catalogue does not list (2.2, 2.202, 2.203) has
no native install, so the button opens the store instead of handing over the wrong package.

Paid releases (v6 and up) need an Absolute account. The launcher reads the license the official
installer writes to `%LOCALAPPDATA%\absolllute.megahack\license` — it never handles your
password. No license on disk means the button says `sign in to install` and opens the store;
once you have signed in there once, every paid release installs straight from the instance page.
v5 is free and not in the catalogue, so its button opens the store too.

## Where things live

```
%APPDATA%\org.gdlauncher.desktop\
  config.json                     settings, instances, version catalogue
  instances\<slug>-<version>\     one folder per managed build
  tools\depotdownloader\          managed DepotDownloader
  cache\                          loader bundles
```

Instances are plain folders — `GeometryDash.exe`, `geode\mods`, saves, everything. Delete the
folder by hand and the launcher will show the instance as missing rather than crash.

## Steam depot reference

| Field | Value |
| --- | --- |
| App id | `322170` |
| Windows content depot | `322171` |
| macOS content depot | `322172` |
| Shared Steamworks depot | `228989` |

Historical manifest ids are not published by Valve; the launcher ships a bundled table
(`src-tauri/src/manifests.rs`) covering every release from `2.0` to `2.2081`, plus the live
public manifest fetched from `api.steamcmd.net`. Add anything newer by hand and it is stored
in the catalogue.

## Notes

- Passwords reach DepotDownloader on its command line, which is visible to other processes on
  the machine for the duration of the run. That is the tool's interface, not a launcher choice.
- `-validate` is always passed, so re-running a manifest repairs a broken install in place.
- The depot chunk cache (`.DepotDownloader` inside the instance) is deleted after a successful
  pull unless you turn that off in Settings.
- Two depot pulls cannot run at once; the second returns an error instead of racing the same
  CDN cache.

## Layout

```
src/                     React + TypeScript front end
  App.tsx                routing, event wiring, job state
  qr.ts                  CP437 terminal QR → module matrix
  components/            Library, InstancePage, BrowseMods, SettingsPage, AddVersionModal,
                         DownloadBar, QrCode, Toasts
src-tauri/src/
  lib.rs                 Tauri command surface
  bin/gd.rs              `gd`, the terminal front end
  config.rs              settings / instances / version catalogue persistence
  steam.rs               Steam library discovery + live depot metadata
  depot.rs               DepotDownloader & steamcmd process driver
  depot_session.rs       reads DepotDownloader's cached Steam login token
  geode.rs               Geode index, mod install/remove, loader install
  mh.rs                  Mega Hack release table + targeting
  mh_install.rs          native Mega Hack download, verify, install, remove
  instance.rs            instance lifecycle, copy, launch
  util.rs                HTTP, verified downloads, zip extraction
```

Both front ends sit on the same core. The Tauri-only pieces are the event emitters and the app
handle; every module that does real work exposes a `*_in` / plain function taking a path or a
progress callback, and the window is a thin wrapper that maps those callbacks onto `emit`.

The QR is rendered from DepotDownloader's terminal output, not an image: `depot.rs` keeps the
raw bytes, `qr.ts` decodes CP437 block characters (`0xDB`, not UTF-8) into a square module
matrix, and `QrCode.tsx` draws it as one SVG path inside a fixed square viewBox so nothing
bleeds past the white plate.

## Releases

Pushing a tag builds and publishes the installer:

```powershell
git tag v1.1.0
git push origin v1.1.0
```

`.github/workflows/release.yml` runs `npm ci`, builds `gd.exe`, then `tauri-action` on
`windows-latest` and attaches the installer to a draft release (GitHub rewrites the space in the
filename to a dot, so the asset lands as `GD.Launcher_1.1.0_x64-setup.exe`). A final step uploads
`gd.exe` beside it, because tauri-action only bundles the window.
`.github/workflows/check.yml` runs the typecheck, the Rust tests and a `gd` build on every push
and pull request.

## Licence

MIT — see [LICENSE](LICENSE).

Not affiliated with RobTop Games, Geode, or Absolute. Geometry Dash is RobTop's; Geode is the
Geode SDK team's; Mega Hack is Absolute's. This launcher only drives their public endpoints.
