import type { ControlMapping, ParameterLink, ParameterLinkMessage } from "./types";

/**
 * Everything that drives one plugin parameter from MIDI: this session's
 * links and the controller maps' mappings. A parameter can gather many --
 * several controllers, each with a base and an Fn layer, and session links
 * besides -- so the control's menu names at most one and opens the rest in a
 * dialog.
 */
export type ParameterAssociation =
  | { kind: "session"; key: string; link: ParameterLink }
  | {
      kind: "controller";
      key: string;
      controller_id: string;
      controller_name: string;
      mapping: ControlMapping;
    };

/** How many associations the control's menu lists itself; more open the dialog. */
export const MENU_ASSOCIATIONS = 1;

export function parameterAssociations(
  sessionLinks: readonly ParameterLink[],
  mapped: ReadonlyArray<{ controller_id: string; controller_name: string; mapping: ControlMapping }>,
): ParameterAssociation[] {
  return [
    ...sessionLinks.map((link): ParameterAssociation => ({ kind: "session", key: `session:${link.id}`, link })),
    ...mapped.map((entry): ParameterAssociation => ({
      kind: "controller",
      key: `controller:${entry.controller_id}:${entry.mapping.id}`,
      ...entry,
    })),
  ];
}

export function messageLabel(message: ParameterLinkMessage): string {
  switch (message.type) {
    case "control_change":
      return `CC ${message.controller}`;
    case "note":
      return `Note ${message.note}`;
    case "pitch_bend":
      return "Pitch wheel";
    case "channel_pressure":
      return "Pressure";
    case "poly_pressure":
      return `Pressure ${message.note}`;
  }
}

export function channelLabel(channel: ParameterLink["channel"]): string {
  return channel.mode === "omni" ? "any channel" : `channel ${channel.channel}`;
}

/** Where it comes from: the session's MIDI input, or the controller. */
export function associationSource(association: ParameterAssociation): string {
  return association.kind === "session" ? association.link.source.display_name : association.controller_name;
}

/** Which control: "CC 74 · channel 1", or the controller's own name for it. */
export function associationControl(association: ParameterAssociation): string {
  if (association.kind === "session") {
    return `${messageLabel(association.link.message)} · ${channelLabel(association.link.channel)}`;
  }
  const { input } = association.mapping;
  const label = messageLabel(input.message);
  return input.name && input.name !== label
    ? `${input.name} · ${label} · ${channelLabel(input.channel)}`
    : `${label} · ${channelLabel(input.channel)}`;
}

export function associationLayer(association: ParameterAssociation): "base" | "fn" {
  const layer = association.kind === "session" ? association.link.layer : association.mapping.layer;
  return layer === "fn" ? "fn" : "base";
}

/** What the control's menu offers for a parameter with this many associations. */
export function menuAssociationItems(count: number): "link" | "one" | "dialog" {
  if (count === 0) return "link";
  return count <= MENU_ASSOCIATIONS ? "one" : "dialog";
}
