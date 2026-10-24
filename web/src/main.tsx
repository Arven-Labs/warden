import ReactDOM from "react-dom/client";
import "@fontsource-variable/inter";
import "@fontsource-variable/jetbrains-mono";
import "./tokens.css";
import App from "./App";

// No <StrictMode>: its double-mounted effects would open (and immediately close) a second hub
// connection on every load in development.
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<App />);
