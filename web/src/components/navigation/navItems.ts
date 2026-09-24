import { Blocks, Info, Piano, Play, RadioTower, Settings2, SlidersVertical } from "lucide-react";

export const liveNavItem = {
    path: "/live",
    label: "Live",
    detail: "Performance racks, songs and setlists",
    section: "workspace",
    icon: RadioTower,
    tint: "live",
  } as const;

export const playNavItem = {
    path: "/play",
    label: "Play",
    detail: "Play and edit the active instrument",
    section: "workspace",
    icon: Play,
    tint: "play",
  } as const;

export const touchControllerNavItem = {
    path: "/controller",
    label: "Touch Controller",
    detail: "On-screen keyboard and pads",
    section: "workspace",
    icon: Piano,
    tint: "controller",
  } as const;

export const pluginManagerNavItem = {
    path: "/plugins",
    label: "Plugin Manager",
    detail: "Install, manage and configure instruments",
    section: "system",
    icon: Blocks,
    tint: "system",
  } as const;

/* MIDI controllers and what each control does in each plugin: violet, the
   colour code's input, as MIDI is everywhere else. */
export const controllersNavItem = {
    path: "/controllers",
    label: "Controllers",
    detail: "Map a MIDI controller's knobs, faders and buttons",
    section: "system",
    icon: SlidersVertical,
    tint: "controller",
  } as const;

export const settingsNavItem = {
    path: "/settings",
    label: "Settings",
    detail: "Audio, MIDI and host configuration",
    section: "system",
    icon: Settings2,
    tint: "system",
  } as const;

export const aboutItem = {
  path: "/about",
  label: "About RackForge",
  detail: "Version and runtime information",
  section: "system",
  icon: Info,
  tint: "system",
} as const;

/* No Home. RackForge is for playing, so you are either in LIVE or in PLAY —
   a dashboard that restated the topbar's readout and the rail's own list was
   a page about the machine rather than a place to work. The compact layout on
   a hardware controller still has a root to navigate from; that lives in the
   `little@1` contract, where four soft keys genuinely need somewhere to start
   from, not here where the rail is always on screen. */

export const workspaceNavItems = [
  liveNavItem,
  playNavItem,
  touchControllerNavItem,
];

export const systemNavItems = [
  pluginManagerNavItem,
  controllersNavItem,
  settingsNavItem,
  aboutItem,
];

export const navItems = [...workspaceNavItems, ...systemNavItems];

export const vstWorkspaceNavItems = [playNavItem];

export const vstSystemNavItems = [pluginManagerNavItem, aboutItem];

export const vstNavItems = [...vstWorkspaceNavItems, ...vstSystemNavItems];
