import { describe, expect, it } from "vitest";
import { controllerPresentationTransition } from "./controllerPresentation";

describe("responsive Touch Controller presentation", () => {
  it("moves a portrait dock to the full-screen surface in landscape", () => {
    expect(controllerPresentationTransition({
      dockable: false,
      dockOpen: true,
      pathname: "/play",
      lastContentRoute: "/play",
    })).toEqual({ navigateTo: "/controller" });
  });

  it("moves the full-screen surface back to the portrait dock", () => {
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
