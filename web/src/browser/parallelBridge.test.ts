import { describe, expect, it, vi } from "vitest";
import { ParallelBridge } from "./parallelBridge";
import { HEADER, poolLayout, type PoolRequest } from "./renderPool";

function fixture() {
  const bridge = new ParallelBridge();
  const memory = new WebAssembly.Memory({ initial: 1 });
  const requests: PoolRequest[] = [];
  const retire = vi.fn();
  bridge.attach(memory);
  bridge.configure(r => requests.push(r), () => new Uint8Array([1]),
    instance => new Uint8Array([instance]), retire);
  const api = bridge.imports;
  const prepare = (instance: number) => {
    api.rf_par_request(instance, 9, 2, 8, 40, 8, 4, 48000, 2, 0, 2);
    bridge.flushRequests();
    return requests.at(-1)!;
  };
  const attach = (request: PoolRequest, ready = true) => {
    const layout = poolLayout(request.geometry);
    const buffer = new SharedArrayBuffer(layout.totalBytes);
    const control = new Int32Array(buffer, 0, HEADER.WORDS);
    control[HEADER.EPOCH] = request.epoch;
    control[HEADER.READY_MASK] = ready ? 3 : 0;
    bridge.adopt(request.instance, buffer, 2, request.epoch);
    return { buffer, control, layout };
  };
  return { bridge, api, memory, requests, prepare, attach, retire };
}

describe("instance-owned parallel workers", () => {
  it("keeps pools and restored control state distinct even for the same module", () => {
    const f = fixture();
    const first = f.prepare(1), second = f.prepare(2);
    const a = f.attach(first), b = f.attach(second);
    expect([...new Uint8Array(first.state)]).toEqual([1]);
    expect([...new Uint8Array(second.state)]).toEqual([2]);
    expect(f.bridge.initialized).toBe(true);
    expect(f.api.rf_par_ready(1)).toBe(2);
    f.api.rf_par_commit(1);
    expect(a.control[HEADER.SEQ]).toBe(1);
    expect(b.control[HEADER.SEQ]).toBe(0);
    f.bridge.release(2);
    a.control[HEADER.ACK_MASK] = 3;
    expect(f.api.rf_par_ready(1)).toBe(2);
    expect(f.api.rf_par_ready(2)).toBe(0);
  });

  it("holds startup until every worker is ready and never promotes a started classic instance", () => {
    const f = fixture();
    const request = f.prepare(1);
    const { control } = f.attach(request, false);
    expect(f.bridge.initialized).toBe(false);
    control[HEADER.READY_MASK] = 1;
    expect(f.bridge.initialized).toBe(false);
    expect(f.api.rf_par_ready(1)).toBe(0);
    control[HEADER.READY_MASK] = 3;
    expect(f.api.rf_par_ready(1)).toBe(0);
    expect(f.retire).toHaveBeenCalledWith(1, request.epoch);
  });

  it("ignores retired attachments and failure messages after reprepare", () => {
    const f = fixture();
    const old = f.prepare(1), current = f.prepare(1);
    f.attach(old);
    f.bridge.unavailable(1, old.epoch);
    expect(f.bridge.initialized).toBe(false);
    f.attach(current);
    expect(f.bridge.initialized).toBe(true);
    expect(f.api.rf_par_ready(1)).toBe(2);
  });

  it("waits for every worker acknowledgement before allowing arena reuse", () => {
    const f = fixture();
    const { control } = f.attach(f.prepare(1));
    expect(f.api.rf_par_ready(1)).toBe(2);
    f.api.rf_par_commit(1);
    control[HEADER.DONE] = 1;
    control[HEADER.ACK_MASK] = 1;
    expect(f.api.rf_par_ready(1)).toBe(-1);
    expect(f.api.rf_par_begin(1, 2, 1, 0, 0)).toBe(-1);
    expect(control[HEADER.SEQ]).toBe(1);
    control[HEADER.ACK_MASK] = 3;
    expect(f.api.rf_par_ready(1)).toBe(2);
    expect(control[HEADER.DEFERRED]).toBe(1);
  });

  it("returns the entire wide mix and the matching unit report from its own arena", () => {
    const f = fixture();
    const a = f.attach(f.prepare(1));
    const b = f.attach(f.prepare(2));
    new Float32Array(a.buffer, a.layout.mixOffset + 160, 40).fill(0.25);
    new Uint8Array(a.buffer, a.layout.reportOffset + 4, 4).set([4, 3, 2, 1]);
    new Uint8Array(b.buffer, b.layout.reportOffset + 4, 4).fill(99);
    expect(f.api.rf_par_mix_read(1, 1, 1024, 40)).toBe(0);
    expect([...new Float32Array(f.memory.buffer, 1024, 40)]).toEqual(Array(40).fill(0.25));
    expect(f.api.rf_par_report_read(1, 1, 2048, 4)).toBe(0);
    expect([...new Uint8Array(f.memory.buffer, 2048, 4)]).toEqual([4, 3, 2, 1]);
    expect(f.api.rf_par_report_read(1, 1, 2048, 5)).toBe(-1);
  });
});
