# GD Launcher

Geometry Dash only keeps one build installed. Steam overwrites it every time RobTop pushes an
update, so if you want to play an old version you are stuck hunting down a copy somewhere. This
keeps as many builds as you want, side by side, each in its own folder with its own mods and
saves.

Builds come straight from Steam by depot manifest, so you get the real thing and not a repack.

There is a window and there is a terminal command. Same config file, same instances, same
downloads. Use whichever you feel like.

Windows only. Rust and Tauri for the window, one small Rust binary for the terminal.

## Install

Grab what you need from [Releases](../../releases/latest).

`GD.Launcher_1.2.0_x64-setup.exe` is the window. It installs per user, so there is no admin
prompt, and WebView2 is already on Windows 11.

`gd.exe` is the terminal version. Put it in any folder that is on your `PATH` and run `gd help`.

## Builds

25 builds are in the table, 2.0 through 2.2081, each with its Steam depot manifest id.

```
gd versions
```

Anything else goes in by manifest id:

```
gd install 2.2099 --manifest 1234567890123456789
```

Valve does not publish old manifests, so the table in `src-tauri/src/manifests.rs` is the offline
copy everyone shares. The current public manifest is fetched from `api.steamcmd.net`.

## Terminal

`gd install` asks what it needs before it starts downloading, so you set it up and walk away
instead of coming back to a finished pull and three questions.

```
> gd install 2.113
name the instance [Geometry Dash 2.113]: pre 2.2
install mega hack for 2.113? (y/N): y
  1) v7.1.1-beta.4    standalone paid  <- default
  2) v7.1.0           standalone paid
  3) v7.0.0           standalone paid
release [v7.1.1-beta.4]: 2
instance  f0a0ede6  2.113  ...\instances\pre-2-2-2-113
...
megahack  v7.1.0  4 files  9.9 MB
```

The Mega Hack question only appears below 2.2, which is the last build before Geode existed. The
list is whatever Absolute actually serves for that exact build. If they have nothing for it, you
get told that and a link to the store instead of the wrong DLL.

If the release list has a Geode package it is hidden here, because a fresh instance has no loader
on disk yet. Install the loader first and `gd mh` will offer it.

Flags to skip the questions:

```
--name <text>     name it and move on
--mh <release>    install that Mega Hack release
--no-mh           never ask about Mega Hack
-y, --yes         take every default
```

Everything else:

```
gd list                        instances you have
gd versions                    builds in the table
gd install <version>           make an instance and pull it
gd launch <instance>           start the game
gd remove <instance>           forget an instance  [--delete-files]
gd path <instance>             print the instance folder

gd steam                       Steam installs, accounts and login state
gd import <index>              register a Steam install  [--link|--copy]
gd login                       QR sign in, cached for later pulls  [--force]

gd mh <instance>               Mega Hack releases for that build
gd mh <instance> install [r]   install one

gd mods <instance> list        installed Geode mods
gd mods <instance> search <q>  search the Geode index
gd mods <instance> install <id>
gd mods <instance> remove <id>

gd loader <instance> [status]  Geode loader state
gd loader <instance> install [version]

gd doctor                      paths, toolchain and login state
```

An instance is matched by full id, id prefix, version or name, so `gd launch 2.2074` works.
`--data-dir` points the whole tool at a different folder. `-v` prints every line DepotDownloader
writes.

Progress is only drawn when stderr is a terminal, so `gd install 2.0 > log` gives you two lines
and nothing else.

## Window

Open it and you get the list of instances with their build, size and mod count. Add a build from
the version table, pull it, and the instance shows up as a card with the version on it.

Each instance page has the folder, the launch button, Geode or Mega Hack for that build, and
remove. Settings has the depot backend, the instance folder, the theme and the login reset.

## Steam login

Steam stopped storing passwords years ago. The client keeps a refresh token and nothing else, so
no tool can read a password and this one does not ask for one.

What happens instead:

| What | Where it comes from |
| --- | --- |
| Account name | `config/loginusers.vdf`, active account first |
| Signed in | `HKCU\Software\Valve\Steam\ActiveProcess\ActiveUser` |
| Session token | DepotDownloader's own store |
| First sign in | `-qr`, scan with the Steam mobile app |

Scan the code once and the token is cached. Every pull after that reuses it. The code only comes
back when there is genuinely no usable token, or when Steam rejects the one that is there, or when
you reset the session yourself.

DepotDownloader writes its token into .NET isolated storage, and the folder names are hashes of
the assembly identity, so they change with every DepotDownloader release. `depot_session.rs` walks
the store, inflates the protobuf and reads the login token out of it. That is what both front ends
believe when they decide whether to show you a QR code.

The token store sits next to the tool's own exe. Move the data folder and the old token is
orphaned, which is why a pull that fails that way retries once as a QR login instead of dying.

## Mega Hack

Geode did not exist before 2.2, so old builds get the tool of their era. `src-tauri/src/mh.rs`
holds the public Windows release history and resolves a release for any build.

| Release | Build | Cost | Channel |
| --- | --- | --- | --- |
| v9.1.3 | 2.2081 | paid | Geode |
| v9.0.11 | 2.2074 | paid | Geode |
| v8.0.3 | 2.206 | paid | Geode and standalone |
| v8.0.0-alpha.13 | 2.204 | paid | Geode and standalone |
| v8.0.0-alpha.5 | 2.2 | paid | standalone |
| v7.1.1 | 2.113 | paid | Geode and standalone |
| v6.3.0 | 2.113 | paid | standalone |
| v5.4 | 2.113 and below | free | standalone |

How it picks:

- Newest stable release for that build, one per major. A beta is skipped when that major has a
  finished release.
- The 2.2 line was alpha only until v8.0.2, so builds below 2.206 fall back to the newest alpha of
  their major rather than showing an empty panel.
- 2.113 offers v7, v6 and v5. Anything older gets v5, the free one.
- A build with no release of its own takes the newest release for an older build. Never a newer
  one.
- Geode packages are hidden below 2.2 and hidden anywhere the loader is not on disk.

Version ordering follows RobTop's own scheme, `2.<minor digit><sub-build>`, where the tail is a
digit string and not a number. `2.2074` is 2.2 build 074 and shipped before `2.208`, which is
build 08. Compare those as integers and you pick the wrong tool, so the tail is compared digit by
digit with the shorter one first.

Nothing is downloaded from a public direct link because there is not one. The launcher reads
Absolute's own catalogue:

```
GET https://absolllute.com/api/mega_hack/v9/install.json
GET https://absolllute.com/api/mega_hack/v9/files/{version}/{file}
```

Each entry names the release, the build it targets, Geode or standalone, and the SHA-256 of the
archive. That hash is checked before anything is written.

| Shape | Payload | Goes to |
| --- | --- | --- |
| Geode | `absolllute.megahack.geode` | `<instance>\geode\mods` |
| Standalone | `hackpro.dll`, `hackproldr.dll`, `discord_game_sdk.dll`, `XInput9_1_0.dll` | instance root |

A `.mh_install_data` marker records the release and every file written, so remove deletes exactly
those and nothing else. Your Steam copy is never touched.

Paid releases need an Absolute account. The launcher reads the licence the official installer
writes to `%LOCALAPPDATA%\absolllute.megahack\license` and never sees your password. Sign in once
there and every paid release installs from here.

## Geode

2.2 and up. The loader button drops `Geode.dll` and the XInput proxy into the instance, stable or
beta. Below 2.2 the button is gone and the backend refuses the call anyway.

Mods install from the Geode index into `<instance>\geode\mods`, checked against the index hash.
Search, install, remove.

## Where files go

```
%APPDATA%\org.gdlauncher.desktop\
  config.json                     settings, instances, version catalogue
  instances\<name>-<version>\     one folder per build
  tools\depotdownloader\          managed DepotDownloader
  cache\                          loader bundles and Mega Hack archives
```

An instance is a normal folder with `GeometryDash.exe` in it. Delete it by hand and the launcher
shows it as missing instead of falling over.

## Build

Rust with the MSVC toolchain, Node 18 or newer, WebView2 (already on Windows 11).

```powershell
npm install
npm run tauri build
```

Output:

```
src-tauri\target\release\gd-launcher.exe
src-tauri\target\release\gd.exe
src-tauri\target\release\bundle\nsis\GD Launcher_1.2.0_x64-setup.exe
```

`gd.exe` is a separate binary in the same package, so `cargo build --release --bin gd` builds just
the terminal one without touching the window.

While working on it:

```powershell
npm run tauri dev
cargo run --manifest-path src-tauri/Cargo.toml --bin gd -- list
```

The shared core is Tauri free. Anything that does real work takes a path or a progress callback,
and the window is a thin wrapper that turns those callbacks into events. That is why both front
ends behave the same.

```
src/                     React and TypeScript
  App.tsx                routing, events, job state
  qr.ts                  CP437 terminal QR to module matrix
  components/            Library, InstancePage, BrowseMods, SettingsPage, AddVersionModal,
                         DownloadBar, QrCode, Toasts
src-tauri/src/
  lib.rs                 Tauri commands
  bin/gd.rs              the terminal front end
  config.rs              settings, instances, version catalogue
  steam.rs               Steam libraries and live depot info
  depot.rs               DepotDownloader and steamcmd driver
  depot_session.rs       reads DepotDownloader's cached token
  geode.rs               Geode index, mods, loader
  mh.rs                  Mega Hack release table
  mh_install.rs          Mega Hack download, verify, install, remove
  instance.rs            create, copy, launch, delete
  util.rs                HTTP, downloads, zip
```

The QR is not an image. DepotDownloader prints it as terminal block characters in the Windows OEM
code page, `qr.ts` decodes those into a module matrix, and `QrCode.tsx` draws it as one SVG path.

## Releasing

```powershell
git tag v1.2.0
git push origin v1.2.0
```

`release.yml` builds `gd.exe`, runs tauri-action on `windows-latest`, and attaches both files to a
draft release. GitHub turns the space in the installer name into a dot, so the asset lands as
`GD.Launcher_1.2.0_x64-setup.exe`. `check.yml` runs the typecheck, the Rust tests and a `gd` build
on every push.

## Licence

MIT, see [LICENSE](LICENSE).

Not affiliated with RobTop Games, Geode or Absolute. Geometry Dash is RobTop's, Geode is the Geode
team's, Mega Hack is Absolute's. This only drives their public endpoints.
