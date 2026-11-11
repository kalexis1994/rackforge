import type { PerformanceSnapshot, SongPart } from "./types";

/** What the header's window says in LIVE, under LIVE MODE: where in the
 *  library the stage is (its second line), and what is playing (its third,
 *  the large one). */
export interface LiveDisplay {
  context: string;
  name: string;
}

const SEPARATOR = " · ";

const BROWSE_LABEL: Record<PerformanceSnapshot["live"]["mode"], string> = {
  rack: "RACK",
  song: "SONG",
  setlist: "SETLIST",
};

/**
 * The window's lines for the active LIVE target:
 *
 *   Rack      RACK                  / the Rack
 *   Song      SONG · the song       / the Rack · the part
 *   Setlist   the song · the setlist / the Rack · the part
 *
 * A part names the Rack it plays; a part that carries its own graph and no
 * Rack of the library is named alone.
 */
export function describeLiveDisplay(performance: PerformanceSnapshot | null): LiveDisplay | null {
  if (!performance) return null;
  const { library, live } = performance;
  const location = live.active;
  if (!location) {
    return { context: BROWSE_LABEL[live.mode], name: "Nothing active" };
  }
  const rackName = (rackId: string | undefined) =>
    rackId ? library.racks.find((rack) => rack.id === rackId)?.name : undefined;
  const partName = (part: SongPart | undefined) => {
    if (!part) return "Missing part";
    const rack = rackName(part.rack_id) ?? rackName(live.active_rack_id);
    return rack ? `${rack}${SEPARATOR}${part.name}` : part.name;
  };

  switch (location.kind) {
    case "rack":
      return { context: "RACK", name: rackName(location.rack_id) ?? "Missing Rack" };
    case "song": {
      const song = library.songs.find((item) => item.id === location.song_id);
      const part = song?.parts.find((item) => item.id === location.part_id);
      return {
        context: `SONG${SEPARATOR}${song?.name ?? "Missing song"}`,
        name: partName(part),
      };
    }
    case "setlist": {
      const setlist = library.setlists.find((item) => item.id === location.setlist_id);
      const entry = setlist?.entries.find((item) => item.id === location.entry_id);
      const song = library.songs.find((item) => item.id === entry?.song_id);
      const part = song?.parts.find((item) => item.id === location.part_id);
      return {
        context: `${song?.name ?? "Missing song"}${SEPARATOR}${setlist?.name ?? "Missing setlist"}`,
        name: partName(part),
      };
    }
  }
}
