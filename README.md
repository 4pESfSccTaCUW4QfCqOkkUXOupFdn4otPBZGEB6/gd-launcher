# gd

Geometry Dash only keeps one build installed. Steam overwrites it every time RobTop pushes an
update, so if you want to play an old version you are stuck hunting down a copy somewhere. This
keeps as many builds as you want, side by side, each in its own folder with its own mods and
saves.

Builds come straight from Steam by depot manifest, so you get the real thing and not a repack.

One command, no window. It is a single Rust binary that runs in the terminal. There is a full
screen interface for the same thing when you would rather browse than type.

Windows only.

## Install

Download `gd.exe` from [Releases](../../releases/latest) and put it in any folder that is on your
`PATH`. Then:

```
gd help
```

If you would rather not touch `PATH`, run it by path. Nothing else is required.

## Full screen interface

```
gd -tui
```

That opens the interface and remembers the choice, so after the first time bare `gd` opens it too.
`gd --no-tui` puts it back to help. The same switch is in the Settings screen.

Six screens, `1` through `6` or `tab`:

| Screen | What it does |
| --- | --- |
| Instances | launch, remove, install the Geode loader, open the folder |
| Versions | the 25 bundled builds, `enter` installs one |
| Mods | installed Geode mods, search the index, install, remove |
| Mega Hack | releases for the selected build, install and remove |
| Steam | Steam copies, accounts, the cached login, copy or link one in |
| Settings | palette, defaults, paths |

`enter` is the main action on each screen. `?` lists every key. `L` hides the activity panel,
which is where download progress and QR codes go. `q` quits and asks first when a job is running.

Downloads run on a worker thread, so the screen keeps drawing while a build pulls. Jobs run one
at a time, in the order you started them.

The Mega Hack question during an install is only asked when Absolute actually publishes a package
for that build. If there is nothing to install, the build pulls on its own and the activity panel
says so.

### Settings

Every row is saved to `config.json` as soon as it changes.

| Setting | What it does |
| --- | --- |
| theme | amber, phosphor, ice, violet, ember or paper, applied live |
| tui by default | bare `gd` opens the interface instead of help |
| confirm remove | ask before a remove that deletes files |
| depot backend | DepotDownloader or steamcmd |
| max downloads | parallel chunk connections, 1 to 32 |
| purge depot cache | delete `.DepotDownloader` after a good pull |
| remember username | prefill the cached Steam account name |
| instances folder | where instance folders are created, empty for the default |
| game arguments | extra flags appended to `GeometryDash.exe` |
| data folder | read only, set it with `--data-dir` |

`left` and `right` change a value, `enter` edits text or flips a toggle, `R` resets everything to
defaults. Instances are never touched by a reset.

## Builds

25 builds are in the table, 2.0 through 2.2081, each with its Steam depot manifest id.

```
gd versions
```

Anything else goes in by manifest id:

```
gd install 2.2099 --manifest 1234567890123456789
```

Valve does not publish old manifests, so the table in `src/manifests.rs` is the offline copy
everyone shares. The current public manifest is fetched from `api.steamcmd.net`.

## Install a build

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
done      f0a0ede6  178.5 MB  778 files
megahack  v7.1.0  4 files  9.9 MB
launch    gd launch f0a0ede6
```

The Mega Hack question only appears below 2.2, which is the last build before Geode existed. The
list is whatever Absolute actually serves for that exact build. If they have nothing for it, you
get told that and a link to the store instead of the wrong DLL.

Geode packages are hidden here, because a fresh instance has no loader on disk yet. Install the
loader first and `gd mh` will offer them.

Flags to skip the questions:

```
--name <text>     name it and move on
--mh <release>    install that Mega Hack release
--no-mh           never ask about Mega Hack
-y, --yes         take every default
```

## Commands

```
gd list                        instances you have
gd versions                    builds in the table
gd install <version>           make an instance and pull it
gd launch [instance]           start the game
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

gd -tui                        open the full screen interface, and remember it
gd --no-tui                    stop bare gd opening it
```

`gd launch` with no name, or the name `steam` or `default`, starts the copy Steam installed. Any
other name is matched against the managed instances by full id, id prefix, version or name, so
`gd launch 2.2074` works. Those three words are reserved and cannot be reached as instance names.

Global flags:

```
--data-dir <path>              use a different data folder
-v, --verbose                  show every line DepotDownloader writes
```

Progress is only drawn when stderr is a terminal, so `gd install 2.0 > log` gives you two lines
and nothing else. `-v` also lets the game's own log through on launch, which is otherwise
swallowed.

## Steam login

Steam stopped storing passwords years ago. The client keeps a refresh token and nothing else, so
no tool can read a password and this one does not ask for one.

| What | Where it comes from |
| --- | --- |
| Account name | `config/loginusers.vdf`, active account first |
| Signed in | `HKCU\Software\Valve\Steam\ActiveProcess\ActiveUser` |
| Session token | DepotDownloader's own store |
| First sign in | `-qr`, scan with the Steam mobile app |

Scan the code once and the token is cached. Every pull after that reuses it. The code only comes
back when there is genuinely no usable token, when Steam rejects the one that is there, or when
you run `gd login --force`. In the full screen interface, `s` on the Steam screen starts the same
sign in, and the code is drawn in the activity panel.

DepotDownloader writes its token into .NET isolated storage, and the folder names are hashes of
the assembly identity, so they change with every DepotDownloader release. `depot_session.rs` walks
the store, inflates the protobuf and reads the login token out of it.

The token store sits next to the tool's own exe. Move the data folder and the old token is
orphaned, which is why a pull that fails that way retries once as a QR login instead of dying.

`gd login` has to pull something, because DepotDownloader only answers `-qr` during a download. It
takes the oldest bundled build, about 40 MB, into a scratch folder and deletes it.

## Mega Hack

Geode did not exist before 2.2, so old builds get the tool of their era. `src/mh.rs` holds the
public Windows release history and resolves a release for any build.

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
  their major rather than showing an empty list.
- 2.113 offers v7, v6 and v5. Anything older gets v5, the free one.
- A build with no release of its own takes the newest release for an older build. Never a newer
  one.
- Geode packages are hidden below 2.2 and hidden anywhere the loader is not on disk.

Version ordering follows RobTop's own scheme, `2.<minor digit><sub-build>`, where the tail is a
digit string and not a number. `2.2074` is 2.2 build 074 and shipped before `2.208`, which is
build 08. Compare those as integers and you pick the wrong tool, so the tail is compared digit by
digit with the shorter one first.

Nothing is downloaded from a public direct link because there is not one. The catalogue is
Absolute's own:

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

Paid releases need an Absolute account. The licence the official installer writes to
`%LOCALAPPDATA%\absolllute.megahack\license` is what gets read, and your password is never seen.
Sign in once there and every paid release installs from here.

## Geode

2.2 and up. The loader command drops `Geode.dll` and the XInput proxy into the instance, stable or
beta. Below 2.2 it refuses.

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

An instance is a normal folder with `GeometryDash.exe` in it. Delete it by hand and `gd list`
shows it as missing instead of falling over.

## Build

Rust with the MSVC toolchain. Nothing else.

```powershell
cargo build --release
```

Output:

```
target\release\gd.exe
```

Run the tests:

```powershell
cargo test --lib
```

Layout:

```
src/
  main.rs                the command line front end
  lib.rs                 module list
  config.rs              settings, instances, version catalogue
  paths.rs               every folder this tool owns, resolved once
  steam.rs               Steam libraries and live depot info
  depot.rs               DepotDownloader and steamcmd argv and line decoder
  depot_session.rs       reads DepotDownloader's cached token
  pull.rs                one pull, backend lookup to child teardown
  geode.rs               Geode index, mods, loader
  mh.rs                  Mega Hack release table
  mh_install.rs          Mega Hack download, verify, install, remove
  instance.rs            create, copy, launch, delete
  util.rs                HTTP, downloads, zip
  tui/
    mod.rs               screen state, keys, event loop
    jobs.rs              the worker thread and every job it runs
    view.rs              drawing
    theme.rs             the six palettes
    settings.rs          the settings rows as data
```

The QR is not an image. DepotDownloader prints it as terminal block characters in the Windows OEM
code page, `depot::LinePump` decodes those bytes, and both front ends draw the lines untouched.
They are already square in a terminal cell grid.

## Releasing

```powershell
git tag v2.1.0
git push origin v2.1.0
```

`release.yml` builds `gd.exe` on `windows-latest` and attaches it to a GitHub release. `check.yml`
runs the tests and a release build on every push.

## Licence

MIT, see [LICENSE](LICENSE).

Not affiliated with RobTop Games, Geode or Absolute. Geometry Dash is RobTop's, Geode is the Geode
team's, Mega Hack is Absolute's. This only drives their public endpoints.
