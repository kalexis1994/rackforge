import {
  Background,
  BackgroundVariant,
  Controls,
  Handle,
  MarkerType,
  MiniMap,
  Position,
  ReactFlow,
  applyNodeChanges,
  type Connection,
  type Edge,
  type Node,
  type NodeChange,
  type NodeProps,
  type NodeTypes,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { memo, useCallback, useMemo, useState } from "react";
import {
  materializeRackGraph,
  rackGraphId,
  removeSlotFromRack,
} from "../rackGraph";
import type {
  RackDefinition,
  RackGraphEdge,
  RackGraphLabel,
  RackGraphLabelTone,
  RackGraphNode,
  RackGraphSignal,
} from "../types";

type CanvasNodeData = {
  title: string;
  subtitle: string;
  kind: RackGraphNode["kind"]["kind"] | "label";
  labelKind?: RackGraphLabel["kind"];
  tone?: RackGraphLabelTone;
};

type CanvasNode = Node<CanvasNodeData>;

const RackNodeCard = memo(function RackNodeCard({ data, selected }: NodeProps<CanvasNode>) {
  const acceptsMidi = data.kind === "plugin" || data.kind === "rack";
  const emitsAudio = acceptsMidi;
  return (
    <div className={`rack-flow-node ${data.kind} ${selected ? "selected" : ""}`}>
      {acceptsMidi ? (
        <Handle
          id="midi:midi_in"
          type="target"
          position={Position.Left}
          className="midi-handle"
        />
      ) : null}
      {data.kind === "audio_output" ? (
        <Handle
          id="audio:in"
          type="target"
          position={Position.Left}
          className="audio-handle"
        />
      ) : null}
      <span className="rack-flow-node-icon">
        {data.kind === "midi_input"
          ? "MIDI"
          : data.kind === "audio_output"
            ? "OUT"
            : data.kind === "rack"
              ? "RACK"
              : "RF"}
      </span>
      <div>
        <strong>{data.title}</strong>
        <small>{data.subtitle}</small>
      </div>
      {data.kind === "midi_input" ? (
        <Handle
          id="midi:out"
          type="source"
          position={Position.Right}
          className="midi-handle"
        />
      ) : null}
      {emitsAudio ? (
        <Handle
          id="audio:audio_out"
          type="source"
          position={Position.Right}
          className="audio-handle"
        />
      ) : null}
    </div>
  );
});

const LabelCard = memo(function LabelCard({ data, selected }: NodeProps<CanvasNode>) {
  return (
    <div
      className={`rack-flow-label ${data.labelKind ?? "note"} tone-${data.tone ?? "neutral"} ${selected ? "selected" : ""}`}
    >
      <span>{data.labelKind === "section" ? "SECTION" : "NOTE"}</span>
      <strong>{data.title}</strong>
    </div>
  );
});

const nodeTypes: NodeTypes = {
  rackNode: RackNodeCard,
  labelNode: LabelCard,
};

const labelTones: RackGraphLabelTone[] = [
  "neutral",
  "cyan",
  "green",
  "amber",
  "violet",
  "red",
];

function nodeTitle(node: RackGraphNode, rack: RackDefinition, racks: RackDefinition[]) {
  switch (node.kind.kind) {
    case "midi_input":
      return ["MIDI Input", node.kind.bus_id];
    case "audio_input":
      return ["Audio Input", node.kind.bus_id];
    case "audio_output":
      return ["Audio Output", node.kind.bus_id];
    case "midi_output":
      return ["MIDI Output", node.kind.bus_id];
    case "plugin": {
      const slotId = node.kind.slot_id;
      const slot = rack.slots.find((candidate) => candidate.id === slotId);
      return [slot?.name ?? "Missing Slot", slot?.plugin_id ?? slotId];
    }
    case "rack": {
      const rackId = node.kind.rack_id;
      const child = racks.find((candidate) => candidate.id === rackId);
      return [child?.name ?? "Missing Rack", "Child Rack"];
    }
  }
}

function toCanvasNodes(rack: RackDefinition, racks: RackDefinition[]): CanvasNode[] {
  const graph = materializeRackGraph(rack).graph!;
  const nodes = graph.nodes.map((node): CanvasNode => {
    const [title, subtitle] = nodeTitle(node, rack, racks);
    return {
      id: node.id,
      type: "rackNode",
      position: node.position,
      data: { title, subtitle, kind: node.kind.kind },
      deletable: false,
    };
  });
  for (const label of graph.labels ?? []) {
    nodes.push({
      id: `label:${label.id}`,
      type: "labelNode",
      position: label.position,
      data: {
        title: label.text,
        subtitle: "",
        kind: "label",
        labelKind: label.kind,
        tone: label.tone,
      },
      style: { width: label.width, height: label.height },
      zIndex: label.kind === "section" ? -10 : 10,
      deletable: true,
    });
  }
  return nodes;
}

function toCanvasEdges(edges: RackGraphEdge[]): Edge[] {
  return edges.map((edge) => ({
    id: edge.id,
    source: edge.source.node_id,
    target: edge.target.node_id,
    sourceHandle: `${edge.signal}:${edge.source.port_id}`,
    targetHandle: `${edge.signal}:${edge.target.port_id}`,
    className: `rack-flow-edge ${edge.signal}`,
    animated: edge.signal === "midi",
    markerEnd: { type: MarkerType.ArrowClosed },
    style: {
      stroke: edge.signal === "midi" ? "#62dff1" : "#f4b860",
      strokeWidth: 2,
    },
  }));
}

function decodeHandle(handle: string | null | undefined) {
  if (!handle) return undefined;
  const separator = handle.indexOf(":");
  if (separator < 1) return undefined;
  return {
    signal: handle.slice(0, separator) as RackGraphSignal,
    portId: handle.slice(separator + 1),
  };
}

function dependsOn(
  sourceId: string,
  targetId: string,
  racks: Map<string, RackDefinition>,
  visited = new Set<string>(),
): boolean {
  if (sourceId === targetId) return true;
  if (visited.has(sourceId)) return false;
  visited.add(sourceId);
  const source = racks.get(sourceId);
  if (!source?.graph) return false;
  return source.graph.nodes.some(
    (node) =>
      node.kind.kind === "rack" &&
      dependsOn(node.kind.rack_id, targetId, racks, visited),
  );
}

interface RackGraphEditorProps {
  rack: RackDefinition;
  racks: RackDefinition[];
  onChange: (rack: RackDefinition) => void;
}

export default function RackGraphEditor({ rack, racks, onChange }: RackGraphEditorProps) {
  const materialized = useMemo(() => materializeRackGraph(rack), [rack]);
  const mappedNodes = useMemo(
    () => toCanvasNodes(materialized, racks),
    [materialized, racks],
  );
  const [selectedId, setSelectedId] = useState<string>();
  const nodes = useMemo(
    () => mappedNodes.map((node) => ({ ...node, selected: node.id === selectedId })),
    [mappedNodes, selectedId],
  );
  const edges = useMemo(
    () => toCanvasEdges(materialized.graph!.edges),
    [materialized.graph],
  );
  const rackMap = useMemo(() => new Map(racks.map((item) => [item.id, item])), [racks]);
  const childOptions = useMemo(
    () =>
      racks.filter(
        (candidate) =>
          candidate.id !== rack.id && !dependsOn(candidate.id, rack.id, rackMap),
      ),
    [rack.id, rackMap, racks],
  );
  const [childRackId, setChildRackId] = useState("");
  const activeChildRackId = childOptions.some(
    (candidate) => candidate.id === childRackId,
  )
    ? childRackId
    : (childOptions[0]?.id ?? "");

  const updateGraph = useCallback(
    (next: RackDefinition["graph"]) => onChange({ ...materialized, graph: next }),
    [materialized, onChange],
  );

  const handleNodesChange = useCallback(
    (changes: NodeChange<CanvasNode>[]) => {
      if (!changes.some((change) => change.type === "position" && change.position)) return;
      const nextNodes = applyNodeChanges(changes, nodes);
      const positions = new Map(nextNodes.map((node) => [node.id, node.position]));
      const graph = materialized.graph!;
      updateGraph({
        ...graph,
        nodes: graph.nodes.map((node) =>
          positions.has(node.id) ? { ...node, position: positions.get(node.id)! } : node,
        ),
        labels: (graph.labels ?? []).map((label) =>
          positions.has(`label:${label.id}`)
            ? { ...label, position: positions.get(`label:${label.id}`)! }
            : label,
        ),
      });
    },
    [materialized.graph, nodes, updateGraph],
  );

  const connect = useCallback(
    (connection: Connection) => {
      const source = decodeHandle(connection.sourceHandle);
      const target = decodeHandle(connection.targetHandle);
      if (!connection.source || !connection.target || !source || !target) return;
      if (source.signal !== target.signal) return;
      const graph = materialized.graph!;
      const duplicate = graph.edges.some(
        (edge) =>
          edge.signal === source.signal &&
          edge.source.node_id === connection.source &&
          edge.source.port_id === source.portId &&
          edge.target.node_id === connection.target &&
          edge.target.port_id === target.portId,
      );
      if (duplicate) return;
      updateGraph({
        ...graph,
        edges: [
          ...graph.edges,
          {
            id: rackGraphId("edge"),
            signal: source.signal,
            source: { node_id: connection.source, port_id: source.portId },
            target: { node_id: connection.target, port_id: target.portId },
          },
        ],
      });
    },
    [materialized.graph, updateGraph],
  );

  const removeEdges = useCallback(
    (removed: Edge[]) => {
      const removedIds = new Set(removed.map((edge) => edge.id));
      const graph = materialized.graph!;
      updateGraph({
        ...graph,
        edges: graph.edges.filter((edge) => !removedIds.has(edge.id)),
      });
    },
    [materialized.graph, updateGraph],
  );

  const addLabel = useCallback(
    (kind: RackGraphLabel["kind"]) => {
      const graph = materialized.graph!;
      const label: RackGraphLabel = {
        id: rackGraphId("label"),
        text: kind === "section" ? "New section" : "New note",
        kind,
        tone: kind === "section" ? "cyan" : "neutral",
        position: { x: 120, y: kind === "section" ? -80 : 280 },
        width: kind === "section" ? 640 : 240,
        height: kind === "section" ? 320 : 100,
      };
      updateGraph({ ...graph, labels: [...(graph.labels ?? []), label] });
      setSelectedId(`label:${label.id}`);
    },
    [materialized.graph, updateGraph],
  );

  const addChildRack = useCallback(() => {
    if (!activeChildRackId) return;
    const graph = materialized.graph!;
    const midiInput = graph.nodes.find((node) => node.kind.kind === "midi_input");
    const audioOutput = graph.nodes.find(
      (node) => node.kind.kind === "audio_output" && node.kind.bus_id === "main",
    );
    if (!midiInput || !audioOutput) return;
    const nodeId = rackGraphId("rack");
    updateGraph({
      ...graph,
      nodes: [
        ...graph.nodes,
        {
          id: nodeId,
          kind: { kind: "rack", rack_id: activeChildRackId },
          position: { x: 360, y: graph.nodes.length * 90 },
        },
      ],
      edges: [
        ...graph.edges,
        {
          id: rackGraphId("edge.midi"),
          signal: "midi",
          source: { node_id: midiInput.id, port_id: "out" },
          target: { node_id: nodeId, port_id: "midi_in" },
        },
        {
          id: rackGraphId("edge.audio"),
          signal: "audio",
          source: { node_id: nodeId, port_id: "audio_out" },
          target: { node_id: audioOutput.id, port_id: "in" },
        },
      ],
    });
  }, [activeChildRackId, materialized.graph, updateGraph]);

  const removeSelected = useCallback(() => {
    if (!selectedId) return;
    if (selectedId.startsWith("label:")) {
      const graph = materialized.graph!;
      const labelId = selectedId.slice("label:".length);
      updateGraph({
        ...graph,
        labels: (graph.labels ?? []).filter((label) => label.id !== labelId),
      });
      setSelectedId(undefined);
      return;
    }
    const node = materialized.graph!.nodes.find((candidate) => candidate.id === selectedId);
    if (!node || node.kind.kind === "midi_input" || node.kind.kind.endsWith("output")) return;
    if (node.kind.kind === "plugin") {
      onChange(removeSlotFromRack(materialized, node.kind.slot_id));
    } else {
      const graph = materialized.graph!;
      updateGraph({
        ...graph,
        nodes: graph.nodes.filter((candidate) => candidate.id !== selectedId),
        edges: graph.edges.filter(
          (edge) =>
            edge.source.node_id !== selectedId && edge.target.node_id !== selectedId,
        ),
      });
    }
    setSelectedId(undefined);
  }, [materialized, onChange, selectedId, updateGraph]);

  const selectedLabel = selectedId?.startsWith("label:")
    ? materialized.graph!.labels?.find(
        (label) => label.id === selectedId.slice("label:".length),
      )
    : undefined;

  const updateSelectedLabel = useCallback(
    (patch: Partial<RackGraphLabel>) => {
      if (!selectedLabel) return;
      const graph = materialized.graph!;
      updateGraph({
        ...graph,
        labels: (graph.labels ?? []).map((label) =>
          label.id === selectedLabel.id ? { ...label, ...patch } : label,
        ),
      });
    },
    [materialized.graph, selectedLabel, updateGraph],
  );

  return (
    <div className="rack-graph-editor">
      <div className="rack-graph-toolbar">
        <div>
          <button type="button" onClick={() => addLabel("note")}>
            ＋ Note
          </button>
          <button type="button" onClick={() => addLabel("section")}>
            ＋ Section
          </button>
        </div>
        <div className="rack-graph-child-tools">
          <select
            aria-label="Child Rack"
            value={activeChildRackId}
            onChange={(event) => setChildRackId(event.target.value)}
            disabled={childOptions.length === 0}
          >
            {childOptions.length === 0 ? <option>No child Racks available</option> : null}
            {childOptions.map((candidate) => (
              <option key={candidate.id} value={candidate.id}>
                {candidate.name}
              </option>
            ))}
          </select>
          <button type="button" onClick={addChildRack} disabled={!activeChildRackId}>
            ＋ Child Rack
          </button>
          <button type="button" className="danger" onClick={removeSelected} disabled={!selectedId}>
            Remove selected
          </button>
        </div>
      </div>
      {selectedLabel ? (
        <div className="rack-graph-label-tools">
          <label>
            Label
            <input
              value={selectedLabel.text}
              maxLength={512}
              onChange={(event) => updateSelectedLabel({ text: event.target.value })}
            />
          </label>
          <label>
            Tone
            <select
              value={selectedLabel.tone}
              onChange={(event) =>
                updateSelectedLabel({ tone: event.target.value as RackGraphLabelTone })
              }
            >
              {labelTones.map((tone) => (
                <option key={tone} value={tone}>
                  {tone}
                </option>
              ))}
            </select>
          </label>
        </div>
      ) : null}
      <div className="rack-graph-canvas">
        <ReactFlow
          nodes={nodes}
          edges={edges}
          nodeTypes={nodeTypes}
          onNodesChange={handleNodesChange}
          onConnect={connect}
          onEdgesDelete={removeEdges}
          onNodeClick={(_event, node) => setSelectedId(node.id)}
          onPaneClick={() => setSelectedId(undefined)}
          deleteKeyCode={["Backspace", "Delete"]}
          minZoom={0.2}
          maxZoom={2.5}
          zoomOnScroll
          panOnScroll={false}
          fitView
          fitViewOptions={{ padding: 0.18, maxZoom: 1.15 }}
          colorMode="dark"
          proOptions={{ hideAttribution: true }}
        >
          <Background variant={BackgroundVariant.Dots} gap={22} size={1.2} />
          <MiniMap pannable zoomable nodeStrokeWidth={3} />
          <Controls
            position="top-right"
            orientation="horizontal"
            showInteractive={false}
          />
        </ReactFlow>
      </div>
      <p className="rack-graph-hint">
        Mouse wheel zooms · drag empty space to pan · drag ports to connect · Delete removes a
        selected connection.
      </p>
    </div>
  );
}
