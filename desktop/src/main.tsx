import React from "react";
import ReactDOM from "react-dom/client";
import { MainWindow } from "./MainWindow";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <MainWindow />
  </React.StrictMode>
);
