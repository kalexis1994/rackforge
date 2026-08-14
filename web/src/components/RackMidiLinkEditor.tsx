import { useState, type CSSProperties } from "react";
import type { RackGraphEdge, RackMidiTransform } from "../types";
import { ScrubNumberField } from "./ScrubNumberField";

interface RackMidiLinkEditorProps {
  edge: RackGraphEdge;
  fallback?: RackMidiTransform;
  targetLabel?: string;
  style?: CSSProperties;
  onApply: (transform: RackMidiTransform) => void;
  onClose: () => void;
}

const identityTransform: RackMidiTransform = {
  source_channels: [],
  note_low: 0,
  note_high: 127,
  transpose: 0,
  notes_only: false,
  velocity_input_low: 0,
  velocity_input_high: 127,
  velocity_output_low: 0,
  velocity_output_high: 127,
};

function noteName(note: number) {
  const names = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
  return `${names[note % 12]}${Math.floor(note / 12) - 1}`;
}

function ChannelPicker({
  label,
  value,
  emptyLabel,
  onChange,
}: {
  label: string;
  value?: number;
  emptyLabel: string;
  onChange: (value?: number) => void;
}) {
  return (
    <fieldset className="rack-midi-channel-picker">
      <legend>{label}</legend>
      <div>
        <button
          type="button"
          className={value === undefined ? "active" : ""}
          aria-pressed={value === undefined}
          onClick={() => onChange(undefined)}
        >
          {emptyLabel}
        </button>
        {Array.from({ length: 16 }, (_, index) => index + 1).map((channel) => (
          <button
            type="button"
            key={channel}
            className={value === channel ? "active" : ""}
            aria-pressed={value === channel}
            onClick={() => onChange(channel)}
          >
            {channel}
          </button>
        ))}
      </div>
    </fieldset>
  );
}

function SourceChannelPicker({
  values,
  onChange,
}: {
  values: number[];
  onChange: (values: number[]) => void;
}) {
  const selected = new Set(values);
  return (
    <fieldset className="rack-midi-channel-picker">
      <legend>Source channels</legend>
      <div>
        <button
          type="button"
          className={values.length === 0 ? "active" : ""}
          onClick={() => onChange([])}
        >
          Omni
        </button>
        {Array.from({ length: 16 }, (_, index) => index + 1).map((channel) => (
          <button
            type="button"
            key={channel}
            className={selected.has(channel) ? "active" : ""}
            aria-pressed={selected.has(channel)}
            onClick={() => onChange(
              selected.has(channel)
                ? values.filter((value) => value !== channel)
                : [...values, channel].sort((left, right) => left - right),
            )}
          >
            {channel}
          </button>
        ))}
      </div>
    </fieldset>
  );
}

export function RackMidiLinkEditor({
  edge,
  fallback,
  targetLabel = "Instrument",
  style,
  onApply,
  onClose,
}: RackMidiLinkEditorProps) {
  const [draft, setDraft] = useState<RackMidiTransform>(
    edge.midi_transform ?? fallback ?? identityTransform,
  );

  const patch = (next: Partial<RackMidiTransform>) => setDraft((current) => ({ ...current, ...next }));
  const valid = draft.note_low <= draft.note_high
    && draft.velocity_input_low < draft.velocity_input_high
    && draft.velocity_output_low <= draft.velocity_output_high;
  const curveX1 = (draft.velocity_input_low / 127) * 100;
  const curveX2 = (draft.velocity_input_high / 127) * 100;
  const curveY1 = 100 - (draft.velocity_output_low / 127) * 100;
  const curveY2 = 100 - (draft.velocity_output_high / 127) * 100;

  return (
    <section
      className="rack-midi-link-editor"
      style={style}
      aria-label="MIDI connection settings"
      onPointerDown={(event) => event.stopPropagation()}
    >
      <header>
        <div>
          <span>MIDI CONNECTION</span>
          <strong>Input → {targetLabel}</strong>
        </div>
        <button type="button" className="rack-midi-close" aria-label="Close" onClick={onClose}>×</button>
      </header>
      <div className="rack-midi-link-scroll">
        <section className="rack-midi-section">
          <h3>Channel routing</h3>
          <SourceChannelPicker
            values={draft.source_channels}
            onChange={(source_channels) => patch({ source_channels })}
          />
          <ChannelPicker
            label="Target channel"
            value={draft.target_channel}
            emptyLabel="Same"
            onChange={(target_channel) => patch({ target_channel })}
          />
        </section>

        <section className="rack-midi-section">
          <h3>Key range</h3>
          <div className="rack-midi-field-grid">
            <ScrubNumberField label="Lowest note" value={draft.note_low} minimum={0} maximum={127} suffix={noteName(draft.note_low)} onChange={(note_low) => patch({ note_low })} />
            <ScrubNumberField label="Highest note" value={draft.note_high} minimum={0} maximum={127} suffix={noteName(draft.note_high)} onChange={(note_high) => patch({ note_high })} />
            <ScrubNumberField label="Transpose" value={draft.transpose} minimum={-48} maximum={48} suffix={`${draft.transpose > 0 ? "+" : ""}${draft.transpose} st`} onChange={(transpose) => patch({ transpose })} />
          </div>
          <label className="rack-midi-check">
            <input type="checkbox" checked={draft.notes_only} onChange={(event) => patch({ notes_only: event.target.checked })} />
            <span>Notes only <small>Suppress controllers, pitch bend and aftertouch</small></span>
          </label>
        </section>

        <section className="rack-midi-section rack-midi-velocity-section">
          <div>
            <h3>Velocity curve</h3>
            <div className="rack-midi-field-grid compact">
              <ScrubNumberField label="Min input" value={draft.velocity_input_low} minimum={0} maximum={126} onChange={(velocity_input_low) => patch({ velocity_input_low })} />
              <ScrubNumberField label="Max input" value={draft.velocity_input_high} minimum={1} maximum={127} onChange={(velocity_input_high) => patch({ velocity_input_high })} />
              <ScrubNumberField label="Min output" value={draft.velocity_output_low} minimum={0} maximum={127} onChange={(velocity_output_low) => patch({ velocity_output_low })} />
              <ScrubNumberField label="Max output" value={draft.velocity_output_high} minimum={0} maximum={127} onChange={(velocity_output_high) => patch({ velocity_output_high })} />
            </div>
          </div>
          <svg className="rack-midi-curve" viewBox="0 0 100 100" role="img" aria-label="Velocity curve preview">
            <path d="M0 100H100V0" className="axis" />
            <polyline points={`0,${curveY1} ${curveX1},${curveY1} ${curveX2},${curveY2} 100,${curveY2}`} />
          </svg>
        </section>
      </div>
      <footer>
        <button type="button" onClick={() => setDraft(identityTransform)}>Reset</button>
        <span />
        <button type="button" onClick={onClose}>Cancel</button>
        <button type="button" className="primary" disabled={!valid} onClick={() => onApply(draft)}>Apply</button>
      </footer>
    </section>
  );
}
