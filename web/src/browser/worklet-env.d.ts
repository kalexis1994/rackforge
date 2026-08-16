/**
 * Types for the audio worklet global scope.
 *
 * TypeScript's DOM library describes the page, not the audio thread, so the
 * handful of globals a processor runs against are declared here.
 */

declare const sampleRate: number;
declare const currentFrame: number;
declare const currentTime: number;

declare abstract class AudioWorkletProcessor {
  readonly port: MessagePort;
  constructor();
  abstract process(
    inputs: Float32Array[][],
    outputs: Float32Array[][],
    parameters: Record<string, Float32Array>,
  ): boolean;
}

declare function registerProcessor(
  name: string,
  processor: new () => AudioWorkletProcessor,
): void;
