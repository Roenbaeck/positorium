// Capture the real website artwork as a 1200 × 630 social preview.
// Start the built site at localhost:4173, then run `npm run social-image`.
import { chromium } from "@playwright/test";
import { fileURLToPath } from "node:url";

const browser = await chromium.launch(
  process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH
    ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH }
    : {},
);
try {
  const page = await browser.newPage({
    viewport: { width: 1200, height: 630 },
    deviceScaleFactor: 1,
    reducedMotion: "reduce",
  });
  await page.goto("http://127.0.0.1:4173/");
  await page.evaluate(() => document.fonts.ready);
  await page.addStyleTag({
    content: `
    .site-header { display: none }
    .hero { width: calc(100% - 100px); min-height: 630px; height: 630px; padding: 35px 0; gap: 15px }
    .hero h1 { font-size: 77px; letter-spacing: -4px }
    .hero-description { max-width: 340px; font-size: 15px }
    .hero-actions { display: none }
    .release-link { font-size: 19px; color: #d3f593; font-family: 'Space Grotesk'; font-weight: 500; letter-spacing: -0.5px; margin-bottom: 25px }
    .evidence-visual { transform: scale(.95); transform-origin: center }
    .hero-footnote { margin-top: 30px; font-size: 8px }
  `,
  });
  await page.locator(".release-link").evaluate((el) => {
    el.textContent = "positorium.";
  });
  await page.screenshot({
    path: fileURLToPath(new URL("../assets/social.png", import.meta.url)),
  });
} finally {
  await browser.close();
}
