import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Provider } from "react-redux";
import { BrowserRouter, HashRouter } from "react-router";
import { App } from "./App";
import { IS_BROWSER_HOST } from "./host";
import { store } from "./store";
import "./styles.css";

// A RackForge serving its own interface answers every path, so it uses real
// URLs. The published demo is a static site with no server to answer them, so
// it keeps its routes in the fragment instead.
const Router = IS_BROWSER_HOST ? HashRouter : BrowserRouter;

if (IS_BROWSER_HOST) {
  // Deferred so a networked build never loads it at all.
  void import("./browser/pwa").then(({ registerServiceWorker }) => registerServiceWorker());
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Provider store={store}>
      <Router>
        <App />
      </Router>
    </Provider>
  </StrictMode>,
);
