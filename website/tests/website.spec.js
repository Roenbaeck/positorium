import { test, expect } from "@playwright/test";

async function run(page) {
  await page.locator("#run-query").click();
  await expect(page.locator("#run-query")).toBeEnabled();
  await expect(page.locator("#query-error")).toBeHidden();
}

test("all four examples return real evidence and a typed execution plan", async ({
  page,
  baseURL,
}) => {
  const errors = [];
  const external = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => {
    if (new URL(request.url()).origin !== new URL(baseURL).origin)
      external.push(request.url());
  });
  await page.goto("/");
  await run(page);
  await expect(page.locator("#table-result tbody tr")).toHaveCount(2);
  await expect(page.locator("#table-result")).toContainText('"high risk"');
  await expect(page.locator("#table-result")).toContainText('"needs review"');
  await expect(page.locator("#table-result")).not.toContainText('"low risk"');
  await page.getByRole("button", { name: "JSON", exact: true }).click();
  const output = JSON.parse(await page.locator("#json-result").innerText());
  expect(output.interface_version).toBe("1");
  expect(output.traqula_version).toBe(1);
  expect(output.result_sets[0].rows).toHaveLength(2);
  await page.getByRole("button", { name: "Table", exact: true }).click();
  await page.locator("[data-example=rewind]").click();
  await run(page);
  await expect(page.locator("#table-result tbody tr")).toHaveCount(2);
  await expect(page.locator("#table-result")).toContainText('"low risk"');
  await expect(page.locator("#table-result")).toContainText("95%");
  await expect(page.locator("#table-result")).not.toContainText(
    '"needs review"',
  );
  await page.locator("[data-example=history]").click();
  await run(page);
  await expect(page.locator("#table-result tbody tr")).toHaveCount(4);
  await expect(page.locator("#table-result")).toContainText("0%");
  await expect(page.locator("#table-result")).toContainText(
    "2026-01-12 09:30:00",
  );
  await page.locator("[data-example=plan]").click();
  await run(page);
  const plan = JSON.parse(await page.locator(".plan-output").innerText());
  expect(plan).toHaveProperty("root");
  expect(plan).toHaveProperty("version", 2);
  expect(errors).toEqual([]);
  expect(external).toEqual([]);
});

test("edits, multiple result sets, errors, fresh databases, and reset work", async ({
  page,
}) => {
  await page.goto("/");
  const editor = page.locator("#query-editor");
  await editor.fill("not valid Traqula");
  await page.locator("#run-query").click();
  await expect(page.locator("#query-error")).toBeVisible();
  await expect(page.locator("#query-status")).toHaveText(
    "The query could not complete.",
  );
  await editor.fill(
    'add role scratch; add posit [{(+x, scratch)}, "<img src=x onerror=alert(1)>", @NOW]; search [{(?x, scratch)}, ?v, *] return ?v; search [{(?x, name)}, ?name, *] return ?name;',
  );
  await run(page);
  await expect(page.locator("#result-set-control")).toBeVisible();
  await expect(page.locator("#table-result")).toContainText(
    "<img src=x onerror=alert(1)>",
  );
  await expect(page.locator("#table-result img")).toHaveCount(0);
  await page.locator("#result-set").selectOption("1");
  await expect(page.locator("#table-result tbody tr")).toHaveCount(3);
  await editor.fill(
    "add role scratch; search [{(?x, scratch)}, ?v, *] return ?v;",
  );
  await run(page);
  await expect(page.locator("#table-result")).toContainText("No matching rows");
  await page.locator("#reset-query").click();
  await expect(editor).toHaveValue(/in effect/);
  await expect(page.locator("#result-placeholder")).toBeVisible();
  await run(page);
  await expect(page.locator("#table-result tbody tr")).toHaveCount(2);
});

test("a cancelled download can be retried without a page reload", async ({
  page,
}) => {
  await page.route("**/pkg/positorium_bg.wasm", async (route) => {
    await new Promise((resolve) => setTimeout(resolve, 500));
    await route.continue().catch(() => {});
  });
  await page.goto("/");
  await page.locator("#run-query").click();
  await page.locator("#stop-query").click();
  await expect(page.locator("#query-status")).toHaveText("Query stopped.");
  await expect(page.locator("#run-query")).toBeEnabled();
  await run(page);
  await expect(page.locator("#table-result tbody tr")).toHaveCount(2);
});

test("a failed WASM download can recover on the next run", async ({ page }) => {
  await page.route("**/pkg/positorium_bg.wasm", route => route.abort("failed"));
  await page.goto("/");
  await page.locator("#run-query").click();
  await expect(page.locator("#query-error")).toBeVisible();
  await page.unroute("**/pkg/positorium_bg.wasm");
  await run(page);
  await expect(page.locator("#table-result tbody tr")).toHaveCount(2);
});

test("the illustration, copy action, and reduced motion are accessible", async ({
  page,
  context,
}) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/");
  await page.locator("[data-moment=original]").click();
  await expect(page.locator("#diagram-value")).toHaveText("“low risk”");
  await expect(page.locator("[data-moment=original]")).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await page.locator("[data-moment=revised]").click();
  await expect(page.locator("#diagram-value")).toHaveText("“needs review”");
  expect(
    await page
      .locator(".path-flow")
      .first()
      .evaluate((el) => getComputedStyle(el).animationName),
  ).toBe("none");
  await page.locator("#copy-install").click();
  await expect(page.locator("#copy-status")).toHaveText("Copied to clipboard");
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(
    "pip install --pre positorium",
  );
});

test("the packaged full Studio runs locally and the Terrain workspace loads", async ({
  page,
  baseURL,
}) => {
  const external = [];
  page.on("request", (request) => {
    if (new URL(request.url()).origin !== new URL(baseURL).origin)
      external.push(request.url());
  });
  await page.goto("/studio.html");
  await expect(page.locator("#wasmMode")).toBeChecked();
  await expect(page.locator("#statusBar")).toContainText(
    "Quick example loaded",
  );
  await page.locator("#runBtn").click();
  await expect(page.locator("#statusBar")).toContainText("Query complete");
  await expect(page.locator("#resultsTable tbody")).toContainText("needs review");
  await page.locator("[data-workspace=terrain]").click();
  await expect(page.locator("#terrainView")).toBeVisible();
  await expect(page.locator("#terrainSourceText")).toContainText(
    "recorded posits",
  );
  await page
    .getByRole("link", { name: "Back to the Positorium website" })
    .click();
  await expect(page.locator("#hero-title")).toBeVisible();
  expect(external).toEqual([]);
});

for (const width of [320, 390, 768, 1024, 1440]) {
  test(`responsive layout at ${width}px has no document overflow`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/");
    await page.evaluate(() => document.fonts.ready);
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth),
    ).toBeLessThanOrEqual(width);
    await page.getByRole("link", { name: "Try it in your browser" }).click();
    await run(page);
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth),
    ).toBeLessThanOrEqual(width);
    await expect(page.locator("#table-result tbody tr")).toHaveCount(2);
  });
}
