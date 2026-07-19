import { defineConfig } from "orval";

export default defineConfig({
  api: {
    input: "./openapi.json",
    output: {
      mode: "tags-split",
      target: "./src/generated",
      schemas: "./src/generated/model",
      client: "react-query",
      httpClient: "fetch",
      baseUrl: "/",
      clean: true,
      prettier: true,
    },
  },
});
