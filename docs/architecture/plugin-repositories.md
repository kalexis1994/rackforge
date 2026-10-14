# Plugin repositories

RackForge plugin distribution uses signed, static HTTP repositories. A
repository can be hosted on a regular web server or CDN; it does not need a
custom application server.

## Trust model

Repository identity is a local configuration composed of an ID, base URL and
an Ed25519 public key. The public key is never learned from the repository it
authenticates. The official RackForge release will ship with the official
repository identity, while users may explicitly add other identities.

Every catalog is downloaded as two files:

```text
<base-url>/v1/index.json
<base-url>/v1/index.json.sig
```

The detached signature covers the exact `index.json` bytes. Each artifact in
that signed catalog carries its byte length and SHA-256 digest. Installation
requires all of the following:

1. strict Ed25519 catalog signature verification;
2. repository ID equality between local configuration and catalog;
3. HTTPS, unless `allow_insecure_http` was explicitly enabled for a LAN;
4. exact package length and SHA-256 equality;
5. safe ZIP extraction with path, symlink, entry-count and expanded-size
   limits;
6. a valid RackForge plugin manifest whose ID and version equal the signed
   catalog entry;
7. a native binary for the current platform.

Changing a repository URL does not grant trust to a new signing key. A mirror
signed by the same key is safe. Adding a different key is a separate, explicit
trust decision.

## Package transport

`.rfplugin` is a ZIP archive with the package contents at its root:

```text
rackforge-plugin.toml
lib/<native-library>
web/<optional-static-assets>
```

The extracted directory is immutable. User banks, presets, state, credentials
and caches continue to live under `data/plugins/<plugin-id>` and are never
replaced by installation or update.

Versions are installed side by side under a package store. Activation is a
small atomic metadata change, so a failed update cannot destroy the previously
working version. A running audio graph is never hot-swapped by the downloader;
the host changes versions only at an explicit safe lifecycle boundary.

## Official packages a release carries

Every host and platform installer lays down the official set its release
carries, Concert Grand included, on each start or install through
`install_bundled_official_archive` (`rackforge-store install-local
--official` in the Linux and Raspberry Pi installers). The same rules hold
everywhere:

- The release is the authority on what its versions contain: a same-version
  copy with other bytes is replaced, and the record says `official`.
- A plugin the player uninstalled stays out. `uninstall_plugin` records it in
  `removed.json`; installing the plugin again from a file or a repository
  takes the uninstall back; no release ever does.
- A plugin the store has never held is enabled. One it already knows keeps
  whatever the player chose.
- The store keeps the two newest versions of the plugin, never dropping the
  one just installed. Every host plays the newest, so older versions are
  never run again; the previous one stays so going back has somewhere to go.
  A version a running process still has mapped is left in place and pruned
  on a later start.

The VST3 plug-in lays down what its DLL carries by the same rules, then offers
the DAW every enabled instrument in the store that has a portable component,
at its newest version, Concert Grand first and the rest by name: an
instrument installed from a file or a repository appears there too, and one
the player uninstalled or switched off does not.

## Catalog v1

The signed JSON document uses this shape:

```json
{
  "schema_version": 1,
  "repository_id": "org.rackforge.official",
  "name": "RackForge Official",
  "generated_at": "2026-07-31T00:00:00Z",
  "plugins": [
    {
      "id": "org.rackforge.rf-kr106",
      "name": "RF-KR106",
      "summary": "Juno-inspired virtual analog synthesizer",
      "license": "GPL-3.0-or-later",
      "homepage": "https://example.com/rf-kr106",
      "releases": [
        {
          "version": "0.1.0",
          "published_at": "2026-07-31T00:00:00Z",
          "artifacts": [
            {
              "platform": "linux-aarch64",
              "url": "packages/rf-kr106-0.1.0-linux-aarch64.rfplugin",
              "size": 123456,
              "sha256": "64 lowercase hexadecimal characters"
            }
          ]
        }
      ]
    }
  ]
}
```

Artifact URLs are resolved relative to `v1/index.json`. An artifact on
another origin is rejected unless the repository configuration lists that
origin in `artifact_origins`: a catalog on a static site can then point at
packages on a release host. The signed catalog fixes each artifact's length
and SHA-256, so the origin decides only which servers a client asks.
Downloads follow redirects but never to plain HTTP unless
`allow_insecure_http` is set, and a server that stops answering fails the
request.

Without a version asked for, a client installs the newest stable release; a
prerelease is installed only by naming its version.

## The official catalog

RackForge's own catalog, `org.rackforge.official`, is built into RackForge
(`official_repository()`) and published with the project's GitHub Pages site:

```text
https://kalexis1994.github.io/rackforge/repository/v1/index.json
https://kalexis1994.github.io/rackforge/repository/v1/index.json.sig
```

Its packages stay where their plugins release them, on GitHub
(`artifact_origins = ["https://github.com"]`). Packages installed from it are
trusted like the ones a release carries: their record names the official
catalog, which lets a host run a package's native build. Only RackForge's key
may use the id; a configuration that claims it with another key is refused.

`config/plugin-catalog.toml` lists the plugins, each by its GitHub repository
and release asset. `tools/build-plugin-catalog.py` reads the newest stable
releases of each (three by default), downloads and checks every package -- a
safe archive, the plugin it claims to be, a portable component -- and writes
`v1/index.json`. Names, versions and summaries come from the packages'
manifests; a license from their `SPDX-License-Identifier` line unless the
configuration names one. A plugin that cannot be read or checked is left out
with a warning. Concert Grand is built here, so `tools/assemble-release.py`
attaches `RF-Concert-Grand.rfplugin` to every RackForge release and the
catalog reads it from there.

`.github/workflows/pages.yml` builds the catalog, signs it with
`rackforge-store sign` and verifies the signature against the key the hosts
trust before deploying, on every push to `main`, daily, and on a
`plugin-released` repository_dispatch. When nothing can be built or signed,
the catalog already published is kept.

Setting up the key, once:

1. On a trusted machine, `rackforge-store keygen catalog.key catalog.pub`.
2. Store the contents of `catalog.key` as the `RACKFORGE_CATALOG_SIGNING_KEY`
   Actions secret of the RackForge repository, then delete the file. Pull
   requests never see it: only the push, schedule and dispatch runs sign.
3. Put the contents of `catalog.pub` in `OFFICIAL_CATALOG_PUBLIC_KEY` in
   `crates/rackforge-repository/src/lib.rs`.

A plugin repository can announce a release so the catalog does not wait for
the daily run, with a token that may dispatch to the RackForge repository:

```sh
gh api repos/kalexis1994/rackforge/dispatches -f event_type=plugin-released
```

`rackforge-store` drives the catalog headlessly; `official` stands for the
built-in identity wherever a repositories file is expected:

```sh
rackforge-store list official
rackforge-store outdated official STORE_ROOT
rackforge-store update official STORE_ROOT [PLUGIN_ID]
```

An update installs the new version beside the old one and keeps the two
newest, as a release's install does; whether the plugin is enabled does not
change.

## Product integration status

The official catalog is published and `rackforge-store` lists, installs and
updates from it. The hosts' own surfaces -- LITTLE, Web, Desktop and Android
-- do not browse it or offer its updates yet; there, plugins are installed
from user-selected local `.rfplugin` packages and from the set each release
carries.
