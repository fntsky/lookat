import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  plugins: [vue()],
  base: "/lookat/",
  server: {
    port: 5174,
    strictPort: true,
    proxy: {
      "/lookat/api": { target: "http://127.0.0.1:53212", changeOrigin: false },
    },
  },
});
