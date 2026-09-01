import { describe, expect, it } from "vitest";
import {
  controllerIsAvailable,
  controllerIsDockable,
  controllerPresentationTransition,
  isImmersiveControllerViewport,
} from "./controllerPresentation";

describe("Touch Controller viewport policy", () => {
  it.each([
    { name: "modern phone landscape", width: 844, height: 390 },
    { name: "compact 16:9 phone landscape", width: 667, height: 375 },
    { name: "wide phone landscape", width: 915, height: 412 },
  ])("uses the immersive surface on a $name", ({ width, height }) => {
    expect(isImmersiveControllerViewport({ width, height, touchCapable: true })).toBe(true);
  });

  it.each([
    { name: "phone portrait", width: 390, height: 844, touchCapable: true },
    { name: "4:3 tablet landscape", width: 1024, height: 768, touchCapable: true },
    { name: "compact tablet landscape", width: 1024, height: 600, touchCapable: true },
    { name: "16:10 tablet landscape", width: 1280, height: 800, touchCapable: true },
    { name: "desktop landscape", width: 844, height: 390, touchCapable: false },
  ])("keeps the controller docked on a $name", ({ width, height, touchCapable }) => {
    expect(isImmersiveControllerViewport({ width, height, touchCapable })).toBe(false);
  });
});

describe("responsive Touch Controller presentation", () => {
  it("moves an open dock to the immersive surface when the viewport qualifies", () => {
    expect(controllerPresentationTransition({
      dockable: false,
      dockOpen: true,
      pathname: "/play",
      lastContentRoute: "/play",
    })).toEqual({ navigateTo: "/controller" });
  });

  it("moves the immersive surface back to the dock when the viewport no longer qualifies", () => {
    expect(controllerPresentationTransition({
      dockable: true,
      dockOpen: true,
      pathname: "/controller",
      lastContentRoute: "/play",
    })).toEqual({ openDock: true, navigateTo: "/play" });
  });

  it("does not open a controller the user already closed", () => {
    expect(controllerPresentationTransition({
      dockable: false,
      dockOpen: false,
      pathname: "/play",
      lastContentRoute: "/play",
    })).toBeNull();
  });

  it("keeps an already full-screen controller in place", () => {
    expect(controllerPresentationTransition({
      dockable: false,
      dockOpen: true,
      pathname: "/controller",
      lastContentRoute: "/live",
    })).toBeNull();
  });
});

describe("Touch Controller availability", () => {
  it("is gone from a VST3 plug-in, dock and all", () => {
    expect(controllerIsAvailable(true)).toBe(false);
    for (const immersive of [false, true]) {
      expect(controllerIsDockable({ vstHost: true, immersive })).toBe(false);
    }
  });

  it("is a dock on desktop and a surface of its own on a phone", () => {
    expect(controllerIsAvailable(false)).toBe(true);
    expect(controllerIsDockable({ vstHost: false, immersive: false })).toBe(true);
    expect(controllerIsDockable({ vstHost: false, immersive: true })).toBe(false);
  });
});
