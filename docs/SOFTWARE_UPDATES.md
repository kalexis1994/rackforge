# Software information and signed releases

Managed Linux and Raspberry Pi installations expose the running host version
and a manual signed-release check in LITTLE (**SYSTEM → ABOUT**) and on the
Web **About RackForge** page. The host supplies the version and build, so a
cached interface cannot substitute its own version for the running service.

The interface can download, verify and install a newer signed runtime after
the player confirms that audio and the controller will restart briefly.
An installation with no signed release feed says so explicitly; it does not
claim to be up to date from unverified GitHub metadata.

## Components

`rackforge-updater.service` runs as the installation's user, with a low CPU
and I/O weight and write access limited to `state/updates`. Its control socket
is `/run/rackforge-updater/control.sock`, accessible to the RackForge group.
Both Web and LITTLE use that service. Checking, downloading and hashing run
outside Core and the controller input loop.

`rackforge-installer.service` survives the restart of all runtime services.
Its root-owned executable lives at `/usr/libexec/rackforge-update-agent`, outside
the runtime being replaced. A root supervisor only controls the fixed units
in `/etc/rackforge-updater/agent.json`; a child running as the installation's
ordinary user verifies, extracts and replaces files. No package installer or
downloaded executable ever runs as root. The agent and its protocol are a
stable bootstrap component, installed by the Linux/Pi service installer.
The supervisor retains only UID/GID transition capabilities. Before launching
the worker it clears all inheritable, ambient and bounding capabilities; the
worker verifies that every capability set is empty before touching runtime files.

The stable feed is the `updates.json` asset of the latest public RackForge
release. Its detached Ed25519 signature is fetched from the version-specific
release, avoiding a mismatch when `latest` changes between the two requests.
The public verification key is compiled into the updater from
`config/update-public-key.txt`.

A manifest binds the product (`org.rackforge.host`), stable semantic version,
release notes, platform, exact GitHub asset URL, byte count and SHA-256. Supported
targets are `linux-x86_64-services` and `linux-aarch64-services`. Only a newer
verified release becomes an offer. Neither the controller nor a browser can
provide an installer URL, shell command or destination path.

## Interfaces

- `GET /api/v1/host/software` reads the installed version and current check.
- `POST /api/v1/host/updates/check` starts a check and returns immediately.
- `POST /api/v1/host/updates/stage` with `{"version":"0.1.40"}` prepares that
  previously verified offer. It does not activate it.
- `POST /api/v1/host/updates/install` with the same body downloads and installs
  that verified offer. It returns immediately; poll software information.
- `POST /api/v1/host/updates/recover` retries an unsuccessful recovery.

The Web routes use the host's existing authentication; mutations also require
the matching browser origin. LITTLE and the local CLI use the Unix socket.

```sh
rackforge-updater status
rackforge-updater check
rackforge-updater stage 0.1.40
rackforge-updater install 0.1.40
rackforge-updater recover
```

The CLI prints the shared state. Poll `status` for a background operation's
result. `ready` means that the archive and its signed metadata are stored in
`state/updates/VERSION/`, after the size and digest were checked. It does not
mean that the installed host has changed. A failed download removes its partial
file. Invalid signatures, absent platform assets and network failures never
become update offers.

## Signing and publishing

The private host-update key stays outside the repository and is separate from
Deskflow and plugin repository keys. Keep a private backup of it: existing
installations trust the committed public key. Do not replace that public key
as part of an ordinary release.

For a new deployment of the updater itself, a key pair can be created with:

```sh
rackforge-updater keygen /private/host-updates.key /private/host-updates.pub
```

The current RackForge deployment already has its public key committed. Its
matching private key must be supplied by the release operator, rather than
generating a new one. It is never included in a package or an artifact.

The Linux and Raspberry Pi builders put `version`, `architecture` and `edition`
in `build-info.txt`. `tools/sign-host-update.py` checks those values against the
proposed release before signing the packages' sizes and hashes. After signing,
the native verifier checks the metadata and both packages against RackForge's
compiled public key. A mismatched signing key or package aborts publication.

```sh
export RACKFORGE_UPDATE_SIGNING_KEY=/private/host-updates.key
python tools/assemble-release.py RUN_ID v0.1.40 notes.md --notes-header --publish
```

The assembly step adds `updates.json` and `updates.json.sig` to the release and
its checksums. Publishing now requires a signing key; assembling unsigned
artifacts for inspection still works without one. The existing public release
does not acquire a signature automatically: the next signed release enables
live discovery for installations carrying this updater.

The signature and package can also be verified offline:

```sh
rackforge-updater verify updates.json updates.json.sig \
  RackForge-RaspberryPi-arm64.tar.gz linux-aarch64-services
```

## Installation and recovery

Web offers **Install and restart** followed by **Install now** or **Cancel**.
LITTLE asks **RESTART AUDIO?**, defaults to **NO - CANCEL**, and requires the
player to choose **YES - INSTALL**. Both use the same transaction service.
Duplicate requests cannot start another operation. Web reconnects while the
services restart and reloads the new interface after a successful update.
LITTLE remembers to reopen the update result after its driver restarts.

Before stopping audio, the agent copies the archive into its transaction,
verifies its signature and digest again, and checks the packaged version,
architecture, update key, required binaries, driver and interface build.
Extraction rejects links, devices, duplicate paths, traversal and excessive
expansion. Free space must allow a candidate and a recoverable backup.

The durable journal is `state/updates/activation.json`. Runtime candidates and
the previous installation live in `.host-updates/TRANSACTION/`. A journal is
flushed before any service stops or live component moves. Each rename and
its parent directory are flushed. Once services are stopped, the agent copies
the controller and plugin stores, installs the release's bundled packages in
those copies, and preserves the player's enablement choices. The startup
instrument follows its updated bundled package. Old plugin versions remain
available for existing sessions. User presets, data, PIN, network and audio
device settings are not replaced by release defaults.

Core, Web, platform host, updater and controller are then started if they were
active before installation. Success requires five consecutive healthy checks:
the units are active, Web identifies the expected version and revision, and
Core responds to a live session request when audio was running. A process that
briefly starts and crashes cannot commit the update.

On failure the agent stops the candidate services, restores every previous
component and checks the restored services. The result is **UPDATE RESTORED**
in LITTLE and an explicit rollback message on Web. If recovery itself cannot
start the services, the result is **RECOVERY FAILED**, with a retry action and
the error preserved. It never reports an unsuccessful recovery as installed.

Managed units depend on the independent agent. On boot after an interrupted
transaction, it restores files before releasing those units to start; it then
checks recovery. Restoration is idempotent across another interruption.
The previous complete runtime is retained until another update succeeds.
This protocol does not run data-format migrations; incompatible migrations
need a separate, explicitly recoverable protocol before they can be shipped.

For an offline installation, place an official archive, `updates.json` and
`updates.json.sig` in `state/updates/VERSION/`, then run:

```sh
rackforge-updater offer-staged 0.1.40
```

This rechecks the signature, platform, size, hash and newer-version rule before
making the package available through the same UI confirmation and installer.
It does not trust an arbitrary URL, extracted directory or unsigned package.

## Isolated integration tests

The `test-support` feature is exclusively for isolated managed-host tests. It
uses the published RFC 8032 test key and can fetch fixtures from an explicit
loopback address. It also supports a process-exit failpoint after a component
swap. Normal release builds enable none of these hooks and only trust the
production public key. Never deploy a `test-support` binary to a real runtime.
