import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App.tsx";
import "./index.css";

import Worker from "../worker?worker";

const worker = new Worker();
worker.onmessage = (event) => {
  console.log("Received message from worker:", event.data);
};

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
