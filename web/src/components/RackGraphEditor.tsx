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
  type Viewport,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import {
  memo,
  useCallback,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import {
  materializeRackGraph,
  rackGraphId,
  removeSlotFromRack,
} from "../rackGraph";
import { RackSlotPopover } from "./RackSlotPopover";
import type {
  PluginInstance,
  RackDefinition,
  RackGraphEdge,
  RackGraphLabel,
  RackGraphLabelTone,
  RackGraphNode,
  RackGraphPosition,
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

type GraphMenuAnchor = {
  position: RackGraphPosition;
  offset: RackGraphPosition;
};

type CanvasBounds = {
  left: number;
  top: number;
  width: number;
  height: number;
};

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

function toCanvasNodes(
  rack: RackDefinition,
  racks: RackDefinition[],
): CanvasNode[] {
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
  onChange: (
    update: RackDefinition | ((current: RackDefinition) => RackDefinition),
  ) => void;
  canAddInstrument: boolean;
  onAddInstrument: (position: RackGraphPosition) => void;
  instances: PluginInstance[];
  renderPluginSurface: (options: {
    instance: PluginInstance;
    onSelectSound: (soundId: string) => Promise<unknown>;
  }) => ReactNode;
}

export default function RackGraphEditor({
  rack,
  racks,
  onChange,
  canAddInstrument,
  onAddInstrument,
  instances,
  renderPluginSurface,
}: RackGraphEditorProps) {
  const materialized = useMemo(() => materializeRackGraph(rack), [rack]);
  const [selectedId, setSelectedId] = useState<string>();
  const [editorSlotId, setEditorSlotId] = useState<string>();
  const canvasRef = useRef<HTMLDivElement | null>(null);
  const suppressPaneClickRef = useRef(false);
  const [viewport, setViewport] = useState<Viewport>({ x: 0, y: 0, zoom: 1 });
  const [canvasBounds, setCanvasBounds] = useState<CanvasBounds>({
    left: 0,
    top: 0,
    width: 0,
    height: 0,
  });
  useLayoutEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const measure = () => {
      const bounds = canvas.getBoundingClientRect();
      setCanvasBounds({
        left: bounds.left,
        top: bounds.top,
        width: bounds.width,
        height: bounds.height,
      });
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(canvas);
    window.addEventListener("resize", measure);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, []);
  const paneGestureRef = useRef<{
    pointerId: number;
    x: number;
    y: number;
    timer: number;
    nodeId?: string;
  } | null>(null);
  const [nodeMenu, setNodeMenu] = useState<{
    nodeId: string;
    anchor: GraphMenuAnchor;
  } | null>(null);
  const [paneMenu, setPaneMenu] = useState<GraphMenuAnchor | null>(null);
  const createMenuAnchor = useCallback(
    (clientX: number, clientY: number, width: number, height: number) => {
      if (!canvasBounds.width || !canvasBounds.height) return null;
      const gap = 8;
      const localX = clientX - canvasBounds.left;
      const localY = clientY - canvasBounds.top;
      const position = {
        x: (localX - viewport.x) / viewport.zoom,
        y: (localY - viewport.y) / viewport.zoom,
      };
      const x = Math.max(gap, Math.min(canvasBounds.width - width - gap, localX + gap));
      const y = Math.max(gap, Math.min(canvasBounds.height - height - gap, localY + gap));
      return {
        position,
        offset: { x: x - localX, y: y - localY },
      };
    },
    [canvasBounds, viewport],
  );
  const menuStyle = useCallback(
    (anchor: GraphMenuAnchor) => {
      return {
        left: viewport.x + anchor.position.x * viewport.zoom + anchor.offset.x,
        top: viewport.y + anchor.position.y * viewport.zoom + anchor.offset.y,
      };
    },
    [viewport],
  );
  const openNodeMenu = useCallback((
    nodeId: string,
    clientX: number,
    clientY: number,
  ) => {
    const anchor = createMenuAnchor(clientX, clientY, 182, 170);
    if (!anchor) return;
    setSelectedId(nodeId);
    setPaneMenu(null);
    setNodeMenu({ nodeId, anchor });
  }, [createMenuAnchor]);
  const openPaneMenu = useCallback((clientX: number, clientY: number) => {
    const anchor = createMenuAnchor(clientX, clientY, 218, 286);
    if (!anchor) return;
    setSelectedId(undefined);
    setNodeMenu(null);
    setPaneMenu(anchor);
  }, [createMenuAnchor]);
  const finishPaneGesture = useCallback(() => {
    const gesture = paneGestureRef.current;
    if (!gesture) return;
    window.clearTimeout(gesture.timer);
    paneGestureRef.current = null;
  }, []);
  const beginPaneGesture = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      const origin = event.target instanceof Element ? event.target : null;
      const nodeElement = origin?.closest<HTMLElement>(".react-flow__node");
      const isPane = origin?.classList.contains("react-flow__pane");
      if (
        !event.isPrimary ||
        event.button !== 0 ||
        (!isPane && !nodeElement)
      ) {
        return;
      }
      finishPaneGesture();
      const { clientX: x, clientY: y, pointerId } = event;
      paneGestureRef.current = {
        pointerId,
        x,
        y,
        nodeId: nodeElement?.dataset.id,
        timer: window.setTimeout(() => {
          if (paneGestureRef.current?.pointerId !== pointerId) return;
          const gesture = paneGestureRef.current;
          paneGestureRef.current = null;
          suppressPaneClickRef.current = true;
          if (gesture.nodeId) {
            openNodeMenu(gesture.nodeId, x, y);
          } else {
            openPaneMenu(x, y);
          }
        }, 520),
      };
    },
    [finishPaneGesture, openNodeMenu, openPaneMenu],
  );
  const movePaneGesture = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      const gesture = paneGestureRef.current;
      if (
        gesture?.pointerId === event.pointerId &&
        Math.hypot(event.clientX - gesture.x, event.clientY - gesture.y) > 20
      ) {
        finishPaneGesture();
      }
    },
    [finishPaneGesture],
  );
  const mappedNodes = useMemo(
    () => toCanvasNodes(materialized, racks),
    [materialized, racks],
  );
  const [interactiveState, setInteractiveState] = useState(() => ({
    source: mappedNodes,
    nodes: mappedNodes,
  }));
  let interactiveNodes = interactiveState.nodes;
  if (interactiveState.source !== mappedNodes) {
    interactiveNodes = mappedNodes;
    setInteractiveState({ source: mappedNodes, nodes: mappedNodes });
  }
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

  const updateGraph = useCallback((
    update: (
      graph: NonNullable<RackDefinition["graph"]>,
    ) => NonNullable<RackDefinition["graph"]>,
  ) => {
    onChange((currentRack) => {
      const current = materializeRackGraph(currentRack);
      return { ...current, graph: update(current.graph!) };
    });
  }, [onChange]);
  const updateSlot = useCallback((nextSlot: RackDefinition["slots"][number]) => {
    onChange((currentRack) => {
      const current = materializeRackGraph(currentRack);
      return {
        ...current,
        slots: current.slots.map((slot) =>
          slot.id === nextSlot.id ? nextSlot : slot,
        ),
      };
    });
  }, [onChange]);

  const handleNodesChange = useCallback(
    (changes: NodeChange<CanvasNode>[]) => {
      setInteractiveState((current) => ({
        ...current,
        nodes: applyNodeChanges(changes, current.nodes),
      }));
    },
    [],
  );

  const persistNodePosition = useCallback(
    (nodeId: string, position: RackGraphPosition) => {
      updateGraph((graph) => ({
        ...graph,
        nodes: graph.nodes.map((node) =>
          node.id === nodeId ? { ...node, position } : node,
        ),
        labels: (graph.labels ?? []).map((label) =>
          `label:${label.id}` === nodeId
            ? { ...label, position }
            : label,
        ),
      }));
    },
    [updateGraph],
  );

  const connect = useCallback(
    (connection: Connection) => {
      const source = decodeHandle(connection.sourceHandle);
      const target = decodeHandle(connection.targetHandle);
      if (!connection.source || !connection.target || !source || !target) return;
      if (source.signal !== target.signal) return;
      updateGraph((graph) => {
        const duplicate = graph.edges.some(
          (edge) =>
            edge.signal === source.signal &&
            edge.source.node_id === connection.source &&
            edge.source.port_id === source.portId &&
            edge.target.node_id === connection.target &&
            edge.target.port_id === target.portId,
        );
        if (duplicate) return graph;
        return {
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
        };
      });
    },
    [updateGraph],
  );

  const removeEdges = useCallback(
    (removed: Edge[]) => {
      const removedIds = new Set(removed.map((edge) => edge.id));
      updateGraph((graph) => ({
        ...graph,
        edges: graph.edges.filter((edge) => !removedIds.has(edge.id)),
      }));
    },
    [updateGraph],
  );

  const addLabel = useCallback(
    (kind: RackGraphLabel["kind"], position: RackGraphPosition) => {
      const label: RackGraphLabel = {
        id: rackGraphId("label"),
        text: kind === "section" ? "New section" : "New note",
        kind,
        tone: kind === "section" ? "cyan" : "neutral",
        position,
        width: kind === "section" ? 640 : 240,
        height: kind === "section" ? 320 : 100,
      };
      updateGraph((graph) => ({
        ...graph,
        labels: [...(graph.labels ?? []), label],
      }));
      setSelectedId(`label:${label.id}`);
    },
    [updateGraph],
  );

  const addChildRack = useCallback((position: RackGraphPosition) => {
    if (!activeChildRackId) return;
    const nodeId = rackGraphId("rack");
    updateGraph((graph) => {
      const midiInput = graph.nodes.find((node) => node.kind.kind === "midi_input");
      const audioOutput = graph.nodes.find(
        (node) => node.kind.kind === "audio_output" && node.kind.bus_id === "main",
      );
      if (!midiInput || !audioOutput) return graph;
      return {
        ...graph,
        nodes: [
          ...graph.nodes,
          {
            id: nodeId,
            kind: { kind: "rack", rack_id: activeChildRackId },
            position,
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
      };
    });
  }, [activeChildRackId, updateGraph]);

  const removeSelected = useCallback(() => {
    if (!selectedId) return;
    if (selectedId.startsWith("label:")) {
      const labelId = selectedId.slice("label:".length);
      updateGraph((graph) => ({
        ...graph,
        labels: (graph.labels ?? []).filter((label) => label.id !== labelId),
      }));
      setSelectedId(undefined);
      return;
    }
    const node = materialized.graph!.nodes.find((candidate) => candidate.id === selectedId);
    if (!node || node.kind.kind === "midi_input" || node.kind.kind.endsWith("output")) return;
    if (node.kind.kind === "plugin") {
      const slotId = node.kind.slot_id;
      onChange((current) => removeSlotFromRack(current, slotId));
    } else {
      updateGraph((graph) => ({
        ...graph,
        nodes: graph.nodes.filter((candidate) => candidate.id !== selectedId),
        edges: graph.edges.filter(
          (edge) =>
            edge.source.node_id !== selectedId && edge.target.node_id !== selectedId,
        ),
      }));
    }
    setSelectedId(undefined);
  }, [materialized, onChange, selectedId, setSelectedId, updateGraph]);

  const selectedLabel = selectedId?.startsWith("label:")
    ? materialized.graph!.labels?.find(
        (label) => label.id === selectedId.slice("label:".length),
      )
    : undefined;
  const menuNode = nodeMenu
    ? materialized.graph!.nodes.find((node) => node.id === nodeMenu.nodeId)
    : undefined;
  const menuSlotId = menuNode?.kind.kind === "plugin" ? menuNode.kind.slot_id : undefined;
  const menuSlot = menuSlotId
    ? materialized.slots.find((slot) => slot.id === menuSlotId)
    : undefined;
  const editorSlot = editorSlotId
    ? materialized.slots.find((slot) => slot.id === editorSlotId)
    : undefined;
  const editorNode = editorSlotId
    ? materialized.graph!.nodes.find(
        (node) => node.kind.kind === "plugin" && node.kind.slot_id === editorSlotId,
      )
    : undefined;
  const editorInstance = editorSlot
    ? instances.find((instance) => instance.plugin_id === editorSlot.plugin_id)
    : undefined;

  const updateSelectedLabel = useCallback(
    (patch: Partial<RackGraphLabel>) => {
      if (!selectedLabel) return;
      updateGraph((graph) => ({
        ...graph,
        labels: (graph.labels ?? []).map((label) =>
          label.id === selectedLabel.id ? { ...label, ...patch } : label,
        ),
      }));
    },
    [selectedLabel, updateGraph],
  );

  return (
    <div className="rack-graph-editor">
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
      <div
        ref={canvasRef}
        className="rack-graph-canvas"
        onPointerDown={beginPaneGesture}
        onPointerMove={movePaneGesture}
        onPointerUp={finishPaneGesture}
        onPointerCancel={finishPaneGesture}
      >
        <ReactFlow<CanvasNode, Edge>
          nodes={interactiveNodes}
          edges={edges}
          nodeTypes={nodeTypes}
          onNodesChange={handleNodesChange}
          onNodeDragStop={(_event, node) => persistNodePosition(node.id, node.position)}
          onConnect={connect}
          onEdgesDelete={removeEdges}
          onViewportChange={setViewport}
          onNodeClick={(_event, node) => setSelectedId(node.id)}
          onNodeDoubleClick={(event, node) => {
            const graphNode = materialized.graph!.nodes.find(
              (candidate) => candidate.id === node.id,
            );
            if (graphNode?.kind.kind === "plugin") {
              setSelectedId(node.id);
              setNodeMenu(null);
              setPaneMenu(null);
              setEditorSlotId(graphNode.kind.slot_id);
            } else {
              openNodeMenu(node.id, event.clientX, event.clientY);
            }
          }}
          onNodeContextMenu={(event, node) => {
            event.preventDefault();
            openNodeMenu(node.id, event.clientX, event.clientY);
          }}
          onPaneClick={() => {
            if (suppressPaneClickRef.current) {
              suppressPaneClickRef.current = false;
              return;
            }
            setSelectedId(undefined);
            setNodeMenu(null);
            setPaneMenu(null);
          }}
          onPaneContextMenu={(event) => {
            event.preventDefault();
            openPaneMenu(event.clientX, event.clientY);
          }}
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
        <div className="rack-graph-menu-layer">
          {paneMenu ? (
            <div
              className="rack-node-menu rack-pane-menu"
              style={menuStyle(paneMenu)}
              role="menu"
              aria-label="Add to Rack"
            >
              <header>
                <span>Add to Rack</span>
                <strong>{materialized.name}</strong>
              </header>
              <button type="button" role="menuitem" disabled={!canAddInstrument} onClick={() => {
                onAddInstrument(paneMenu.position);
                setPaneMenu(null);
              }}>Instrument</button>
              <button type="button" role="menuitem" onClick={() => {
                addLabel("note", paneMenu.position);
                setPaneMenu(null);
              }}>Note</button>
              <button type="button" role="menuitem" onClick={() => {
                addLabel("section", paneMenu.position);
                setPaneMenu(null);
              }}>Section</button>
              <label className="rack-pane-child-picker">
                <span>Child Rack</span>
                <select
                  aria-label="Child Rack"
                  value={activeChildRackId}
                  onChange={(event) => setChildRackId(event.target.value)}
                  disabled={childOptions.length === 0}
                >
                  {childOptions.length === 0 ? <option>No Racks available</option> : null}
                  {childOptions.map((candidate) => (
                    <option key={candidate.id} value={candidate.id}>{candidate.name}</option>
                  ))}
                </select>
              </label>
              <button type="button" role="menuitem" disabled={!activeChildRackId} onClick={() => {
                addChildRack(paneMenu.position);
                setPaneMenu(null);
              }}>Add Child Rack</button>
              <button type="button" role="menuitem" onClick={() => setPaneMenu(null)}>Close</button>
            </div>
          ) : null}
          {nodeMenu && menuNode ? (
            <div
              className="rack-node-menu"
              style={menuStyle(nodeMenu.anchor)}
              role="menu"
              aria-label="Node actions"
            >
              <header>
                <span>{menuNode.kind.kind === "plugin" ? "Instrument" : "Node"}</span>
                <strong>{menuSlot?.name ?? nodeTitle(menuNode, materialized, racks)[0]}</strong>
              </header>
              {menuSlot ? (
                <>
                  <button type="button" role="menuitem" onClick={() => {
                    setNodeMenu(null);
                    setEditorSlotId(menuSlot.id);
                  }}>Edit instrument</button>
                </>
              ) : null}
              <button type="button" role="menuitem" onClick={() => {
                setNodeMenu(null);
                removeSelected();
              }} disabled={menuNode.kind.kind === "midi_input" || menuNode.kind.kind.endsWith("output")}>Remove node</button>
              <button type="button" role="menuitem" onClick={() => setNodeMenu(null)}>Close</button>
            </div>
          ) : null}
          {editorSlot && editorNode && editorInstance ? (
            <RackSlotPopover
              key={`${editorSlot.id}:${editorSlot.plugin_id}`}
              slot={editorSlot}
              instance={editorInstance}
              nodePosition={editorNode.position}
              viewport={viewport}
              onChange={updateSlot}
              onClose={() => setEditorSlotId(undefined)}
              renderSurface={renderPluginSurface}
            />
          ) : null}
        </div>
      </div>
      <p className="rack-graph-hint">
        Mouse wheel zooms · drag empty space to pan · drag ports to connect · Delete removes a
        selected connection.
      </p>
    </div>
  );
}
