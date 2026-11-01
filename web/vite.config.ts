import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// In development the API is proxied, so the browser sees one origin.
const api = process.env.PETER_API_URL ?? "http://127.0.0.1:8080";

export default defineConfig({
  plugins: [react()],
  server: { port: 5173, proxy: { "/v1": api, "/healthz": api } },
  preview: { port: 5173, proxy: { "/v1": api, "/healthz": api } },
});
