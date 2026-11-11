import { useCallback, useState, type ImgHTMLAttributes } from "react";

/**
 * An image that appears when it is whole, not while it arrives.
 *
 * Plugin artwork is large -- banners 1600 px wide and up to 700 KB, icons
 * 512 px and up to 300 KB -- and over a Raspberry Pi's Wi-Fi a card used to
 * paint its banner in strips and its icon a moment later. This keeps an image
 * out of sight until it has loaded and decoded, then fades it in; one already
 * in the cache is shown at once, with no flash. Offscreen images wait to be
 * scrolled to. An image that fails is dropped, and `onFailed` lets the caller
 * show what stands in for it.
 */
export function FadeImage({
  src,
  className = "",
  alt = "",
  onFailed,
  ...rest
}: Omit<ImgHTMLAttributes<HTMLImageElement>, "onLoad" | "onError"> & {
  src: string;
  onFailed?: () => void;
}) {
  // Loaded is kept per source, so a new source fades in again rather than
  // inheriting the previous image's state.
  const [loadedSrc, setLoadedSrc] = useState<string | null>(null);
  const [failedSrc, setFailedSrc] = useState<string | null>(null);
  const loaded = loadedSrc === src;
  // A cached image can be complete before React attaches onLoad. Recorded by
  // the `src` it was given -- the browser's `currentSrc` is absolute, and
  // would never match a relative source.
  const measure = useCallback((image: HTMLImageElement | null) => {
    if (image?.complete && image.naturalWidth > 0) setLoadedSrc(src);
  }, [src]);
  if (failedSrc === src) return null;
  return (
    <img
      {...rest}
      key={src}
      ref={measure}
      src={src}
      alt={alt}
      loading="lazy"
      decoding="async"
      className={`${className} rf-fade-image${loaded ? " is-loaded" : ""}`.trim()}
      onLoad={() => setLoadedSrc(src)}
      onError={() => {
        setFailedSrc(src);
        onFailed?.();
      }}
    />
  );
}
