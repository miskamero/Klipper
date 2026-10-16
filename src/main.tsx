import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import FullscreenControls from "./components/native/FullscreenControls";
import "./styles/global.scss";

const controlsOverlay = new URLSearchParams(location.search).get("native-controls") === "1";
if (controlsOverlay) document.documentElement.dataset.controlsOverlay = "true";

ReactDOM.createRoot(document.getElementById("root")!).render(
	<React.StrictMode>{controlsOverlay ? <FullscreenControls /> : <App />}</React.StrictMode>,
);
