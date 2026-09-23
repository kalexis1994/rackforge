import { Plug } from "lucide-react";
import { BrandMark } from "../components/BrandMark";
import { PageHeading } from "../components/PageHeading";
import {
  ANDROID_PATH,
  LINUX_PATH,
  RASPBERRY_PI_PATH,
  WINDOWS_PATH,
} from "../components/platformGlyphs";

/** The project's home. The desktop app and the VST3 editor open only links
 *  under this owner in the system browser (desktop_webview.rs, view.rs). */
const PROJECT_URL = "https://github.com/kalexis1994/rackforge";
/** Always the newest release: GitHub resolves `latest` itself, so none of
 *  these links goes stale the way a pinned version would. */
const LATEST_RELEASE_URL = `${PROJECT_URL}/releases/latest`;
const latestAsset = (name: string) => `${LATEST_RELEASE_URL}/download/${name}`;

/** One key per platform, each the Standard build as the release names it,
 *  with the processor it is built for. The Minimal builds and the checksums
 *  are on the release page. */
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

/* About says where the project lives and where to get it. Links open in a
   new window, so the interface stays where it is. */

export function AboutPage() {
  return (
    <>
      <PageHeading
        eyebrow="RackForge"
        title="About"
        detail="A portable instrument host built around one shared interface and native real-time runtimes."
      />
      <section className="settings-grid">
        {/* Where the project lives: its source, releases and issues. It
            used to name the runtime protocol, which says nothing to a
            player. */}
        <article className="settings-card about-card">
          <BrandMark />
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
            Every download, Minimal builds and checksums
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
