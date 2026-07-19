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
      // NextRS owns the generated root barrel, so preserve it while replacing the API files.
      clean: ["!index.ts"],
      prettier: true,
    },
  },
});
