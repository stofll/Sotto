import React from "react";
import ReactDOM from "react-dom/client";
import { TrayApp } from "./TrayApp";
import "../styles.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <TrayApp />
  </React.StrictMode>
);
