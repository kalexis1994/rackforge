import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Provider } from "react-redux";
import { BrowserRouter, HashRouter } from "react-router";
import { App } from "./App";
import { IS_BROWSER_HOST, isVstHost } from "./host";
import { store } from "./store";
import {
  InteractionFeedbackRoot,
  RouteExperienceObserver,
} from "./ui/InteractionFeedbackRoot";
import { startExperienceMonitoring } from "./ux/metrics";
import "./design/tokens.css";
import "./styles.css";

// A RackForge serving its own interface answers every path, so it uses real
// URLs. The published demo is a static site with no server to answer them, so
// it keeps its routes in the fragment instead.
const Router = IS_BROWSER_HOST || isVstHost() ? HashRouter : BrowserRouter;

startExperienceMonitoring();

if (IS_BROWSER_HOST && import.meta.env.PROD) {
  // A networked build never loads it. The browser host starts registration
  // immediately so its first plugin-catalog response cannot outrun the worker.
  void import("./browser/pwa").then(({ registerServiceWorker }) => registerServiceWorker());
} else if (IS_BROWSER_HOST && "serviceWorker" in navigator) {
  // A production worker must not pin yesterday's worklet while developing.
  // It is reinstalled by the next production build; local Vite sessions stay
  // fully controlled by HMR and explicit reloads.
  void navigator.serviceWorker.getRegistrations().then((registrations) =>
    Promise.all(registrations.map((registration) => registration.unregister())),
  ).catch(() => undefined);
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Provider store={store}>
      <Router>
        <InteractionFeedbackRoot />
        <RouteExperienceObserver />
        <App />
      </Router>
    </Provider>
  </StrictMode>,
);
