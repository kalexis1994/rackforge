/**
 * Files a plugin page saves for the player -- RF-5's tapes, a SysEx dump, an
 * exported preset -- are links it makes itself: `<a download>` to a `blob:`
 * or `data:` URL. A browser saves them; Android's WebView drops them. In the
 * Android app, the kit hands each such file to RackForge, which asks where
 * to save it with the system's own dialog. Elsewhere this does nothing.
 */

interface AndroidDownloads {
  saveDownload?: (fileName: string, mediaType: string, base64: string) => void;
}

const MAX_BYTES = 64 * 1024 * 1024;

function bridge(): AndroidDownloads | undefined {
  if (typeof window === "undefined") return undefined;
  return (window as unknown as { RackForgeAndroid?: AndroidDownloads }).RackForgeAndroid;
}

/** A link this kit saves for the page: a named download of the page's own bytes. */
export function isPageDownload(anchor: { download: string; href: string }): boolean {
  return anchor.download !== "" && (anchor.href.startsWith("blob:") || anchor.href.startsWith("data:"));
}

function base64Of(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const url = String(reader.result);
      resolve(url.slice(url.indexOf(",") + 1));
    };
    reader.onerror = () => reject(reader.error ?? new Error("The file could not be read."));
    reader.readAsDataURL(blob);
  });
}

async function save(save: NonNullable<AndroidDownloads["saveDownload"]>, name: string, href: string) {
  const blob = await (await fetch(href)).blob();
  if (blob.size > MAX_BYTES) throw new Error("The file is too large to save.");
  save(name, blob.type || "application/octet-stream", await base64Of(blob));
}

function install() {
  const saveDownload = bridge()?.saveDownload;
  if (!saveDownload) return;
  const marked = window as unknown as { __rackforgeDownloads?: boolean };
  if (marked.__rackforgeDownloads) return;
  marked.__rackforgeDownloads = true;
  const handOver = (anchor: HTMLAnchorElement) => {
    void save(saveDownload.bind(bridge()), anchor.download, anchor.href).catch((error: unknown) => {
      console.error("RackForge could not save the file", error);
    });
  };
  // A link the player taps.
  document.addEventListener(
    "click",
    (event) => {
      const anchor = (event.target as Element | null)?.closest?.("a[download]");
      if (!(anchor instanceof HTMLAnchorElement) || !isPageDownload(anchor)) return;
      event.preventDefault();
      handOver(anchor);
    },
    true,
  );
  // A link a script makes and clicks, perhaps never in the document.
  const click = HTMLAnchorElement.prototype.click;
  HTMLAnchorElement.prototype.click = function clickOrSave(this: HTMLAnchorElement) {
    if (!this.isConnected && isPageDownload(this)) {
      handOver(this);
      return;
    }
    click.call(this);
  };
}

install();
