import React, { Suspense, lazy } from "react";
import ReactDOM from "react-dom/client";
import "./index.css";
import "./styles/components/index.css";
import { applyBootAppearance } from "./shared/lib/themeRuntime";

const App = lazy(() => import("./App"));
const CompactPreviewWindow = lazy(() => import("./features/clipboard/components/CompactPreviewWindow"));
const QuickPasteWindow = lazy(() => import("./features/clipboard/components/QuickPasteWindow"));
const RegionSelectWindow = lazy(() => import("./features/clipboard/components/RegionSelectWindow"));

// Theme and colour mode go on <html>/<body> before React renders, so the first
// paint is already in the user's theme rather than the base stylesheet. The
// stylesheet itself is still fetched in parallel and not awaited.
applyBootAppearance();

const params = new URLSearchParams(window.location.search);
const isCompactPreview = params.get("window") === "compact-preview";
const isQuickPaste = params.get("window") === "quick-paste";
const isRegionSelect = params.get("window") === "region-select";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Suspense fallback={null}>
      {isRegionSelect ? (
        <RegionSelectWindow />
      ) : isQuickPaste ? (
        <QuickPasteWindow />
      ) : isCompactPreview ? (
        <CompactPreviewWindow />
      ) : (
        <App />
      )}
    </Suspense>
  </React.StrictMode>,
);
