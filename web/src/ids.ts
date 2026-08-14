let fallbackCounter = 0;

function bytesToHex(bytes: Uint8Array) {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

export function randomIdToken() {
  const cryptoApi = globalThis.crypto;
  if (typeof cryptoApi?.randomUUID === "function") {
    return cryptoApi.randomUUID().replaceAll("-", "");
  }
  if (typeof cryptoApi?.getRandomValues === "function") {
    const bytes = new Uint8Array(16);
    cryptoApi.getRandomValues(bytes);
    return bytesToHex(bytes);
  }

  fallbackCounter = (fallbackCounter + 1) >>> 0;
  const timestamp = Date.now().toString(16).padStart(12, "0");
  const counter = fallbackCounter.toString(16).padStart(8, "0");
  const random = Math.floor(Math.random() * 0x1_0000_0000)
    .toString(16)
    .padStart(8, "0");
  return `${timestamp}${counter}${random}`;
}

export function scopedId(prefix: string) {
  return `${prefix}.${randomIdToken()}`;
}
