import { useState, type RefObject } from "react";

/**
 * A plugin's splash and its loading mark, each arriving once it has loaded.
 *
 * Both used to appear the moment their image landed -- the photograph cut in
 * over a black plate, the icon a beat later wherever it happened to finish.
 * Now each waits, invisible, until its image is in, and then comes up: the
 * splash settling out of a slight zoom, the mark rising into place just
 * after it. The mark's fill from the bottom is still driven from the frame
 * through `litRef`.
 */
export function BrandSplashArtwork({
  splashUrl,
  iconUrl,
  litRef,
}: {
  splashUrl: string;
  iconUrl: string;
  litRef: RefObject<HTMLImageElement | null>;
}) {
  const [splashLoaded, setSplashLoaded] = useState(false);
  const [iconLoaded, setIconLoaded] = useState(false);
  return (
    <>
      <img
        className={`splash-bg${splashLoaded ? " is-loaded" : ""}`}
        src={splashUrl}
        alt=""
        // One that will not load stays hidden: the plate, not a broken image.
        onLoad={() => setSplashLoaded(true)}
      />
      <div className={`splash-icon${iconLoaded ? " is-loaded" : ""}`} aria-hidden="true">
        <img
          className="splash-icon-dim"
          src={iconUrl}
          alt=""
          onLoad={() => setIconLoaded(true)}
        />
        <img ref={litRef} className="splash-icon-lit" src={iconUrl} alt="" />
      </div>
    </>
  );
}
