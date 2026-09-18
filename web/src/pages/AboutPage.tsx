import { BrandMark } from "../components/BrandMark";
import { PageHeading } from "../components/PageHeading";
import { HostHealth, useHostHealth } from "../hooks/useHostHealth";
import { IS_BROWSER_HOST, isDesktopHost, isVstHost } from "../host";

/** Which RackForge the player is looking at, in their words rather than the
 *  wire's. Only ever what this build can actually know about itself. */
function hostShellName(health: HostHealth | null): string {
  if (IS_BROWSER_HOST) return "This browser";
  if (isVstHost()) return "VST3 plug-in";
  if (isDesktopHost()) return "Desktop app";
  if (health?.host === "desktop") return "Desktop app";
  if (health?.host === "vst3") return "VST3 plug-in";
  return "Web interface";
}

/* About says what this build is, where it is running and what it speaks.
   Everything on it is read from the host or stamped in at build time — a
   version panel that guesses is worse than none. */

export function AboutPage() {
  const health = useHostHealth();
  const hostRevision = health?.revision;
  const drift = hostRevision !== undefined && hostRevision !== __UI_REVISION__;
  const shell = hostShellName(health);
  return (
    <>
      <PageHeading
        eyebrow="RackForge"
        title="About"
        detail="A portable instrument host built around one shared interface and native real-time runtimes."
      />
      <section className="settings-grid">
        <article className="settings-card about-card">
          <BrandMark />
          <div className="settings-copy">
            <span className="card-kicker">Runtime protocol</span>
            <h2>rackforge.host@1</h2>
            <p>Portable .rfplugin runtime · Rust core · native audio and MIDI.</p>
          </div>
        </article>

        <article className="settings-card">
          <div className="settings-copy">
            <span className="card-kicker">This build</span>
            <h2>{shell}</h2>
            <p>
              The interface and the host binary are stamped separately, so a
              half-finished deploy shows here instead of as a behaviour you
              cannot explain.
            </p>
          </div>
          <dl className="about-facts">
            <div>
              <dt>Interface</dt>
              <dd>{__UI_REVISION__}</dd>
            </div>
            <div>
              <dt>Host</dt>
              <dd>{hostRevision ?? "—"}</dd>
            </div>
          </dl>
          {drift ? (
            <p className="about-drift">
              These disagree. The interface and the host came from different
              builds; reinstall the one that is behind.
            </p>
          ) : null}
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
