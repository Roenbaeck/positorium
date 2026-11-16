import { defineConfig } from "@playwright/test";

const remoteURL = process.env.WEBSITE_BASE_URL;
const baseURL = remoteURL || "http://127.0.0.1:4173";

export default defineConfig({
  testDir: "./tests",
  testMatch: "**/*.spec.js",
  fullyParallel: false,
  workers: 1,
  timeout: 30_000,
  use: {
    baseURL,
    viewport: { width: 1440, height: 1000 },
    launchOptions: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH
      ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH }
      : {},
    screenshot: "only-on-failure",
  },
  webServer: remoteURL ? undefined : {
    command:
      "python3 -m http.server 4173 --bind 127.0.0.1 --directory ../dist/positorium-site",
    url: "http://127.0.0.1:4173",
    reuseExistingServer: !process.env.CI,
  },
});
