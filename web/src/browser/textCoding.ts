/**
 * UTF-8 coding for the audio worklet.
 *
 * `AudioWorkletGlobalScope` is deliberately minimal: it has no `fetch`, no
 * timers and — the part that matters here — no `TextEncoder` or `TextDecoder`.
 * The engine and the WASI shim both need them, so this installs a small
 * implementation when the scope is missing one. Everywhere else the browser's
 * own is left alone.
 */

class Utf8Encoder {
  readonly encoding = "utf-8";

  encode(input = ""): Uint8Array {
    const bytes: number[] = [];
    for (const character of input) {
      let point = character.codePointAt(0) ?? 0;
      if (point < 0x80) {
        bytes.push(point);
      } else if (point < 0x800) {
        bytes.push(0xc0 | (point >> 6), 0x80 | (point & 0x3f));
      } else if (point < 0x10000) {
        bytes.push(0xe0 | (point >> 12), 0x80 | ((point >> 6) & 0x3f), 0x80 | (point & 0x3f));
      } else {
        bytes.push(
          0xf0 | (point >> 18),
          0x80 | ((point >> 12) & 0x3f),
          0x80 | ((point >> 6) & 0x3f),
          0x80 | (point & 0x3f),
        );
        point = 0;
      }
    }
    return new Uint8Array(bytes);
  }

  encodeInto(input: string, destination: Uint8Array) {
    const encoded = this.encode(input);
    const written = Math.min(encoded.length, destination.length);
    destination.set(encoded.subarray(0, written));
    return { read: input.length, written };
  }
}

class Utf8Decoder {
  readonly encoding = "utf-8";

  decode(input?: ArrayBuffer | ArrayBufferView): string {
    if (!input) return "";
    const bytes =
      input instanceof Uint8Array
        ? input
        : ArrayBuffer.isView(input)
          ? new Uint8Array(input.buffer, input.byteOffset, input.byteLength)
          : new Uint8Array(input);

    let result = "";
    for (let index = 0; index < bytes.length; ) {
      const first = bytes[index];
      let point: number;
      let width: number;
      if (first < 0x80) {
        point = first;
        width = 1;
      } else if ((first & 0xe0) === 0xc0) {
        point = first & 0x1f;
        width = 2;
      } else if ((first & 0xf0) === 0xe0) {
        point = first & 0x0f;
        width = 3;
      } else {
        point = first & 0x07;
        width = 4;
      }
      if (index + width > bytes.length) {
        // Truncated sequence: emit the replacement character rather than
        // reading past the end.
        result += "�";
        break;
      }
      for (let offset = 1; offset < width; offset += 1) {
        point = (point << 6) | (bytes[index + offset] & 0x3f);
      }
      result += String.fromCodePoint(point);
      index += width;
    }
    return result;
  }
}

/** Installs the fallbacks when the surrounding scope has none. */
export function installTextCoding() {
  const scope = globalThis as typeof globalThis & {
    TextEncoder?: unknown;
    TextDecoder?: unknown;
  };
  // Cast through `unknown`: these implement the parts of the standard
  // interfaces the engine and the WASI shim use, not every optional member.
  if (typeof scope.TextEncoder === "undefined") {
    scope.TextEncoder = Utf8Encoder as unknown as typeof TextEncoder;
  }
  if (typeof scope.TextDecoder === "undefined") {
    scope.TextDecoder = Utf8Decoder as unknown as typeof TextDecoder;
  }
}
