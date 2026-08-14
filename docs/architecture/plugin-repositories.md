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

Artifact URLs are resolved relative to `v1/index.json`. Cross-origin artifact
URLs are rejected in v1, keeping credentials, redirects and trust boundaries
simple.

## Product integration status

RackForge hosts do not expose repository configuration, remote catalog
browsing, or remote package installation in LITTLE, Web, Desktop, or Android.
Plugins are installed from user-selected local `.rfplugin` packages. The
protocol and signing tools in this document are retained for publisher tooling
and possible future distribution work, but are disconnected from the runtime
product surfaces.
