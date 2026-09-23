import { materializeRackGraph, rackGraphNodeName, songPartAsRack } from "./rackGraph";
import type { RackDefinition, RackGraphEdge, SongDefinition } from "./types";

/**
 * Names what changed between two states of a Rack, for its editing history:
 * "Added RF-Comp", "Connected RF-Comp → Audio Output", "Removed RF-EQ ·
 * chain reconnected". It reads the two states rather than being told, so a
 * step the graph's rules took (an effect inserted, a chain closed) is named
 * as plainly as one taken by hand.
 */
export function describeRackChange(before: RackDefinition, after: RackDefinition): string {
  const a = materializeRackGraph(before);
  const b = materializeRackGraph(after);
  const slotName = (rack: RackDefinition, slotId: string) =>
    rack.slots.find((slot) => slot.id === slotId)?.name ?? "a plugin";

  const added = b.slots.filter((slot) => !a.slots.some((one) => one.id === slot.id));
  const removed = a.slots.filter((slot) => !b.slots.some((one) => one.id === slot.id));
  const edgesBefore = new Map(a.graph!.edges.map((edge) => [connectionKey(edge), edge]));
  const edgesAfter = new Map(b.graph!.edges.map((edge) => [connectionKey(edge), edge]));
  const edgesAdded = [...edgesAfter].filter(([key]) => !edgesBefore.has(key)).map(([, edge]) => edge);
  const edgesRemoved = [...edgesBefore].filter(([key]) => !edgesAfter.has(key)).map(([, edge]) => edge);

  if (added.length === 1 && removed.length === 0) {
    const rewired = edgesRemoved.length > 0 ? " · inserted into the chain" : "";
    return `Added ${added[0].name}${rewired}`;
  }
  if (removed.length === 1 && added.length === 0) {
    const healed = edgesAdded.some((edge) => edge.signal === "audio");
    return `Removed ${removed[0].name}${healed ? " · chain reconnected" : ""}`;
  }
  if (added.length > 0 || removed.length > 0) {
    return `Changed ${added.length + removed.length} plugins`;
  }

  const nodesAdded = b.graph!.nodes.filter((node) => !a.graph!.nodes.some((one) => one.id === node.id));
  const nodesRemoved = a.graph!.nodes.filter((node) => !b.graph!.nodes.some((one) => one.id === node.id));
  if (nodesAdded.length === 1 && nodesRemoved.length === 0) {
    return `Added ${rackGraphNodeName(b, nodesAdded[0].id)}`;
  }
  if (nodesRemoved.length === 1 && nodesAdded.length === 0) {
    return `Removed ${rackGraphNodeName(a, nodesRemoved[0].id)}`;
  }

  const describeEdge = (rack: RackDefinition, edge: RackGraphEdge) =>
    `${rackGraphNodeName(rack, edge.source.node_id)} → ${rackGraphNodeName(rack, edge.target.node_id)}`;
  if (edgesAdded.length === 1 && edgesRemoved.length === 1
    && edgesAdded[0].source.node_id === edgesRemoved[0].source.node_id) {
    return `Re-patched ${describeEdge(b, edgesAdded[0])}`;
  }
  if (edgesAdded.length === 1 && edgesRemoved.length === 1) {
    return `Moved a cable to ${describeEdge(b, edgesAdded[0])}`;
  }
  if (edgesAdded.length === 1 && edgesRemoved.length === 0) {
    return `Connected ${describeEdge(b, edgesAdded[0])}`;
  }
  if (edgesRemoved.length === 1 && edgesAdded.length === 0) {
    return `Disconnected ${describeEdge(a, edgesRemoved[0])}`;
  }
  if (edgesRemoved.length === 1 && edgesAdded.length === 2) {
    // A node dropped into a cable: the cable's two ends now meet at it.
    const [into, out] = edgesAdded[0].target.node_id === edgesAdded[1].source.node_id
      ? edgesAdded
      : [edgesAdded[1], edgesAdded[0]];
    if (into.target.node_id === out.source.node_id
      && into.source.node_id === edgesRemoved[0].source.node_id
      && out.target.node_id === edgesRemoved[0].target.node_id) {
      return `Inserted ${rackGraphNodeName(b, into.target.node_id)} between `
        + `${rackGraphNodeName(b, into.source.node_id)} and ${rackGraphNodeName(b, out.target.node_id)}`;
    }
  }
  if (edgesAdded.length + edgesRemoved.length > 0) {
    return "Changed connections";
  }

  const routingChanged = a.graph!.edges.some((edge) => {
    const other = b.graph!.edges.find((one) => one.id === edge.id);
    return other && JSON.stringify(other.midi_transform) !== JSON.stringify(edge.midi_transform);
  });
  if (routingChanged) return "Changed MIDI routing";

  const moved = b.graph!.nodes.filter((node) => {
    const was = a.graph!.nodes.find((one) => one.id === node.id);
    return was && (was.position.x !== node.position.x || was.position.y !== node.position.y);
  });
  if (moved.length === 1) return `Moved ${rackGraphNodeName(b, moved[0].id)}`;
  if (moved.length > 1) return `Moved ${moved.length} nodes`;

  if (JSON.stringify(a.graph!.labels ?? []) !== JSON.stringify(b.graph!.labels ?? [])) {
    return "Edited notes";
  }
  if (a.name !== b.name) return "Renamed Rack";

  const changedSlot = b.slots.find((slot) => {
    const was = a.slots.find((one) => one.id === slot.id);
    return was && JSON.stringify(was) !== JSON.stringify(slot);
  });
  if (changedSlot) {
    const was = a.slots.find((one) => one.id === changedSlot.id)!;
    if (was.enabled !== changedSlot.enabled) {
      return `${changedSlot.enabled ? "Enabled" : "Disabled"} ${changedSlot.name}`;
    }
    if (was.name !== changedSlot.name) return `Renamed ${slotName(a, changedSlot.id)}`;
    return `Edited ${changedSlot.name}`;
  }
  return "Edited Rack";
}

/** The same for a Song: its name, its Parts, or what changed inside one. */
export function describeSongChange(before: SongDefinition, after: SongDefinition): string {
  if (before.name !== after.name) return "Renamed Song";
  if (after.parts.length > before.parts.length) return "Added a Part";
  if (after.parts.length < before.parts.length) return "Removed a Part";
  const changed = after.parts.find((part, index) =>
    JSON.stringify(part) !== JSON.stringify(before.parts[index]));
  if (!changed) return "Edited Song";
  const was = before.parts.find((part) => part.id === changed.id);
  if (!was) return "Reordered Parts";
  if (was.name !== changed.name) return `Renamed ${was.name}`;
  return `${changed.name}: ${describeRackChange(songPartAsRack(was), songPartAsRack(changed))}`;
}

function connectionKey(edge: RackGraphEdge) {
  return `${edge.signal}:${edge.source.node_id}:${edge.source.port_id}>${edge.target.node_id}:${edge.target.port_id}`;
}
