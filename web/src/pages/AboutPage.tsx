import { Play, Plug } from "lucide-react";
import { BrandMark } from "../components/BrandMark";
import { PageHeading } from "../components/PageHeading";
import { IS_BROWSER_HOST } from "../host";
import {
  ANDROID_PATH,
  LINUX_PATH,
  RASPBERRY_PI_PATH,
  WINDOWS_PATH,
} from "../components/platformGlyphs";

/** The project's home, and RackForge Web, the whole interface running in a
 *  browser. The desktop app and the VST3 editor open only these two in the
 *  system browser (desktop_webview.rs, view.rs). */
const PROJECT_URL = "https://github.com/kalexis1994/rackforge";
const WEB_DEMO_URL = "https://kalexis1994.github.io/rackforge/";
/** Always the newest release: GitHub resolves `latest` itself, so none of
 *  these links goes stale the way a pinned version would. */
const LATEST_RELEASE_URL = `${PROJECT_URL}/releases/latest`;
const latestAsset = (name: string) => `${LATEST_RELEASE_URL}/download/${name}`;

/** One key per platform, with the processor it is built for. Checksums are
 *  on the release page. */
const DOWNLOADS: Array<{
  platform: string;
  file: string;
  arch: string;
  asset: string;
  glyph: string | "plug";
}> = [
  { platform: "Windows", file: ".exe", arch: "x86-64", asset: "RackForge-Windows-x86_64.exe", glyph: WINDOWS_PATH },
  { platform: "VST3 · Windows", file: ".zip", arch: "x86-64", asset: "RackForge-VST3-Windows-x86_64.zip", glyph: "plug" },
  { platform: "Linux", file: ".tar.gz", arch: "x86-64", asset: "RackForge-Linux-x86_64.tar.gz", glyph: LINUX_PATH },
  { platform: "Raspberry Pi", file: ".tar.gz", arch: "ARM64", asset: "RackForge-RaspberryPi-arm64.tar.gz", glyph: RASPBERRY_PI_PATH },
  { platform: "Android", file: ".apk", arch: "ARM64", asset: "RackForge-Android-arm64.apk", glyph: ANDROID_PATH },
];

/** What comes installed, in a line each: the Concert Grand built here, and
 *  the official plugins tools/fetch-official-plugins.py pins -- a change
 *  there is a reviewed edit, and so is this list. Instruments first. */
const INCLUDED_PLUGINS: Array<{ name: string; kind: "instrument" | "effect"; about: string }> = [
  { name: "RF - Concert Grand", kind: "instrument", about: "A grand piano modelled from its physics — strings, hammers and soundboard — with no samples." },
  { name: "RF-Tines", kind: "instrument", about: "An electric piano modelled from hammer, tine and pickup, from bell-like to barking." },
  { name: "RF-Organ", kind: "instrument", about: "A tonewheel organ with its rotary speaker, modelled rather than sampled." },
  { name: "RF-7", kind: "instrument", about: "A six-operator FM synthesizer: all 32 algorithms, and the cartridges you already own." },
  { name: "RF-106", kind: "instrument", about: "A six-voice analog polysynth with its lush stereo chorus and 128 factory programs." },
  { name: "RF-5", kind: "instrument", about: "A five-voice analog polysynth: two oscillators a voice, hard sync and a four-pole filter." },
  { name: "RF-Comp", kind: "effect", about: "A stereo compressor with a soft knee, parallel mix and live gain-reduction meters." },
  { name: "RF-EQ", kind: "effect", about: "An eight-band parametric equaliser with high- and low-pass filters and shelves." },
  { name: "RF-Limiter", kind: "effect", about: "A true-peak limiter that holds the output under its ceiling, whatever reaches it." },
];

/* About says what RackForge is, what it comes with, where the project lives
   and where to get it. Links open in a new window, so the interface stays
   where it is. */

export function AboutPage() {
  return (
    <>
      <PageHeading
        title="About RackForge"
        detail="A portable instrument host built around one shared interface and native real-time runtimes."
      />
      <section className="settings-grid">
        {/* Where the project lives: its source, releases and issues. It
            used to name the runtime protocol, which says nothing to a
            player. */}
        <article className="settings-card about-card">
          <BrandMark />
          {/* What RackForge is, in a few lines, for whoever meets it here
              first -- the same promise the README opens with. */}
          <p className="about-synopsis">
            RackForge turns a computer, a phone or a Raspberry Pi into an
            instrument you can play. Connect a MIDI keyboard, pick a sound,
            layer sounds into Racks and build a show from Songs and Setlists,
            with no DAW session to set up first. The same instruments and the
            same interface on every platform.
          </p>
          {/* The one lit key on the page: the quickest way to know RackForge
              is to play it. RackForge Web does not offer itself. */}
          {IS_BROWSER_HOST ? null : (
            <a
              className="about-web-key"
              href={WEB_DEMO_URL}
              target="_blank"
              rel="noopener noreferrer"
            >
              <Play aria-hidden="true" />
              <span>
                <strong>Try RackForge Web</strong>
                <small>Nothing to install</small>
              </span>
            </a>
          )}
          <div className="settings-copy">
            <span className="card-kicker">Project</span>
            <h2>RackForge on GitHub</h2>
            <p>Source code, releases and issues.</p>
            <a
              className="about-project-link"
              href={PROJECT_URL}
              target="_blank"
              rel="noopener noreferrer"
            >
              github.com/kalexis1994/rackforge
            </a>
          </div>
        </article>

        <article className="settings-card about-plugins">
          <div className="settings-copy">
            <span className="card-kicker">Plugins</span>
            <h2>Ready to play</h2>
            <p>
              Installed with RackForge: every one made of code rather than
              samples, and the same on every platform.
            </p>
          </div>
          <ul className="about-plugin-list">
            {INCLUDED_PLUGINS.map((plugin) => (
              <li key={plugin.name}>
                <span className="about-plugin-name">
                  <strong>{plugin.name}</strong>
                  <span className={`plugin-kind-tag ${plugin.kind}`}>
                    {plugin.kind === "instrument" ? "Instrument" : "Effect"}
                  </span>
                </span>
                <p>{plugin.about}</p>
              </li>
            ))}
          </ul>
        </article>

        <article className="settings-card about-downloads">
          <div className="settings-copy">
            <span className="card-kicker">Downloads</span>
            <h2>Take RackForge with you</h2>
            <p>
              Pick your platform and get the newest release: on your computer,
              in your DAW, on a Raspberry Pi on stage, or in your pocket.
            </p>
          </div>
          <ul className="about-download-keys">
            {DOWNLOADS.map((download) => (
              <li key={download.asset}>
                <a
                  className="about-download-key"
                  href={latestAsset(download.asset)}
                  target="_blank"
                  rel="noopener noreferrer"
                  title={download.asset}
                >
                  {download.glyph === "plug" ? (
                    <Plug aria-hidden="true" />
                  ) : (
                    <svg viewBox="0 0 24 24" aria-hidden="true">
                      <path d={download.glyph} />
                    </svg>
                  )}
                  <span>
                    <strong>{download.platform}</strong>
                    <small>{download.file} · {download.arch}</small>
                  </span>
                </a>
              </li>
            ))}
          </ul>
          <a
            className="about-project-link"
            href={LATEST_RELEASE_URL}
            target="_blank"
            rel="noopener noreferrer"
          >
            Every download and its checksums
          </a>
        </article>

        <article className="settings-card">
          <div className="settings-copy">
            <span className="card-kicker">Typefaces</span>
            <h2>Set in three</h2>
            <p>
              Chakra Petch for headings, Barlow Semi Condensed for the panel
              legends, JetBrains Mono for anything that must line up in a
              column. All three under the SIL Open Font License, whose text
              ships beside the fonts.
            </p>
          </div>
        </article>
      </section>
    </>
  );
}
