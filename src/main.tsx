import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { AppProvider } from "./state/AppContext";
import "./styles.css";

// `yarn dev` opened in a plain browser: stand in for the Tauri backend.
if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  const { installBrowserMock } = await import("./dev/browserMock");
  installBrowserMock();
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <AppProvider>
      <App />
    </AppProvider>
  </React.StrictMode>,
);
