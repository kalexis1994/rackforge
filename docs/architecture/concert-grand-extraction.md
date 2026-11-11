# Concert Grand repository boundary

Concert Grand is owned by the public
[rackforge-plugin-concert-grand](https://github.com/kalexis1994/rackforge-plugin-concert-grand)
repository. The extraction starts from RackForge `d0744691`, after fetching
`origin/main` and confirming it is already an ancestor of this work branch.
The plugin repository preserves 218 filtered commits, with its original
authors and dates, plus the laboratory tools and reports. Its migration
directory records the old-to-new commit map and exact DSP source hashes.

RackForge retains the portable SDK, loader, routing, controllers, generic
render infrastructure and host integration tests. The piano's Rust source,
Web assets, voicings, artwork, native unit tests and model research move out
of this workspace. The SDK dependency in the plugin is pinned to the published
RackForge revision `91eb43f88495f0485e7a7ed61718280ddce23a8d`.

## Releases and builds

The first independent release is
[0.171.37](https://github.com/kalexis1994/rackforge-plugin-concert-grand/releases/tag/v0.171.37).
It changes packaging and ownership, retaining the accepted model and its
compiled defaults. The package includes WASM and the verified ARM64 portable
native library. No deployment or audio configuration is changed by this
repository extraction.

`tools/fetch-official-plugins.py` is the single pin list for every instrument,
including the default one. The piano's pin specifies its version, immutable
release URL and SHA256. A pin marked `default` is downloaded beside the
`official/` directory because existing native hosts embed a default package
plus that directory. This keeps the instrument included exactly once. The
browser uses the same archive and its declared `concert-grand` extraction
directory, preserving existing storage URLs.

Windows, Linux, Flatpak, Raspberry Pi, Android and browser builds fetch the
package; none compiles the piano from RackForge source. Package-set validation
and release notes derive the expected version from the same pin list.

## Validation

- The independent crate passes 82 native model tests; 69 explicit measurements
  remain ignored. Both exact audio fingerprints remain
  `e4abcfd02a5757c1` and `dfb7f193962235e0`.
- The independently built and optimized WASM matches the attested ARM64
  library for all 16 programs, serial and in units, including saved state.
- The plugin-owned package builder runs successfully on the Pi, and the final
  package's component and native bytes equal the versions just verified.
- RackForge downloads the published release, verifies its SHA256, validates
  its contents, extracts the browser/test fixture and reuses its verified
  cache successfully. The downloaded archive is 4,277,636 bytes, SHA256
  `f9cf0df2cabc8cd73209175b517052021b06615a126507ba6abbd63f2eaffc8c`.
- All 40 host tool tests pass. Shell and PowerShell build entry points parse,
  the host workflow parses, and the locked SDK build passes after removing
  the piano from the workspace and lockfile.

The host still tests the actual Concert Grand through `RenderPool`, using a
released fixture rather than plugin source. To prepare it:

```sh
python tools/fetch-official-plugins.py --plugin-id org.rackforge.concert-grand --output-directory target/test-packages/official --extract-directory target/test-plugins
cargo test -p rackforge-core --test parallel_render -- --ignored
```

Plugin CI independently tests the native model on Linux x86-64, ARM64 and
Windows, and builds and exercises the WASM startup and worker-instance paths.
Moving source does not rewrite RackForge's published Git history; historical
revisions retain their original files and measurements.

[Verification record](concert-grand-extraction-verification.json).
