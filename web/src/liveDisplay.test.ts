import { describe, expect, it } from "vitest";
import { describeLiveDisplay } from "./liveDisplay";
import type { PerformanceSnapshot } from "./types";

function snapshot(live: PerformanceSnapshot["live"]): PerformanceSnapshot {
  return {
    schema_version: 4,
    revision: 1,
    live,
    library: {
      schema_version: 1,
      racks: [
        { schema_version: 1, id: "rack.piano", name: "Stage Piano", enabled: true, slots: [] },
        { schema_version: 1, id: "rack.pad", name: "Warm Pad", enabled: true, slots: [] },
      ],
      songs: [{
        schema_version: 1,
        id: "song.one",
        name: "Moon River",
        enabled: true,
        parts: [
          { id: "part.intro", name: "Intro", rack_id: "rack.pad" },
          { id: "part.own", name: "Bridge", rack_id: "rack.gone" },
        ],
      }],
      setlists: [{
        schema_version: 1,
        id: "set.friday",
        name: "Friday",
        enabled: true,
        entries: [{ id: "entry.1", song_id: "song.one" }],
      }],
      patterns: [],
      sequencer_tabs: [],
    },
  } as unknown as PerformanceSnapshot;
}

describe("the header's window in LIVE", () => {
  it("names a Rack under RACK", () => {
    expect(describeLiveDisplay(snapshot({
      mode: "rack",
      active: { kind: "rack", rack_id: "rack.piano" },
    }))).toEqual({ context: "RACK", name: "Stage Piano" });
  });

  it("names the song, then the Rack and its part", () => {
    expect(describeLiveDisplay(snapshot({
      mode: "song",
      active: { kind: "song", song_id: "song.one", part_id: "part.intro" },
    }))).toEqual({ context: "SONG · Moon River", name: "Warm Pad · Intro" });
  });

  it("names the song and its setlist, then the Rack and its part", () => {
    expect(describeLiveDisplay(snapshot({
      mode: "setlist",
      active: { kind: "setlist", setlist_id: "set.friday", entry_id: "entry.1", part_id: "part.intro" },
    }))).toEqual({ context: "Moon River · Friday", name: "Warm Pad · Intro" });
  });

  it("names a part alone when it plays no Rack of the library", () => {
    expect(describeLiveDisplay(snapshot({
      mode: "song",
      active: { kind: "song", song_id: "song.one", part_id: "part.own" },
    }))?.name).toBe("Bridge");
  });

  it("says nothing is active, under the mode being browsed", () => {
    expect(describeLiveDisplay(snapshot({ mode: "setlist" }))).toEqual({
      context: "SETLIST",
      name: "Nothing active",
    });
    expect(describeLiveDisplay(null)).toBeNull();
  });
});
