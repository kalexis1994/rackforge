# Software information and signed releases

Managed Linux and Raspberry Pi installations expose the running host version
and a manual signed-release check in LITTLE (**SYSTEM → ABOUT**) and on the
Web **About RackForge** page. The host supplies the version and build, so a
cached interface cannot substitute its own version for the running service.

The first implementation checks releases and prepares verified archives. It
does not replace running binaries, execute an installer, or restart audio.
An installation with no signed release feed says so explicitly; it does not
claim to be up to date from unverified GitHub metadata.

## Components

`rackforge-updater.service` runs as the installation's user, with a low CPU
and I/O weight and write access limited to `state/updates`. Its control socket
is `/run/rackforge-updater/control.sock`, accessible to the RackForge group.
Both Web and LITTLE use that service. Checking, downloading and hashing run
outside Core and the controller input loop.

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

The Web routes use the host's existing authentication; mutations also require
the matching browser origin. LITTLE and the local CLI use the Unix socket.

```sh
rackforge-updater status
rackforge-updater check
rackforge-updater stage 0.1.40
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

## Activation boundary

Automatic activation needs a separate transaction: stop the managed services,
replace a complete runtime, restart it, verify its version and health, and keep
a recoverable previous installation until those checks pass. Data migrations
and recovery after a power loss belong to that transaction too. It must survive
the restart of Web and the controller. This first service deliberately has no
privileged installation operation or **Install and restart** control.
