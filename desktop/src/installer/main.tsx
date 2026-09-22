import React from "react";
import ReactDOM from "react-dom/client";
import { Installer } from "./Installer";
import { detectLocale, setLocale } from "../i18n";
import "../styles.css";
import "./installer.css";

setLocale(detectLocale());
ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode><Installer /></React.StrictMode>,
);

\n