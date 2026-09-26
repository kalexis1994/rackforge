import { lazy } from "react";

export const ResourceExplorerDialog = lazy(() =>
  import("../ResourceExplorerDialog").then((module) => ({
    default: module.ResourceExplorerDialog,
  })),
);
