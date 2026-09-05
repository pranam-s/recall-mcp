import { expect, test } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

test.describe("Recall-MCP UI", () => {
  test("create memory, recall it, inspect the score breakdown", async ({ page }) => {
    await page.goto("/");

    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Recall-MCP memory console");

    // The store starts empty: create the namespace used by the rest of the flow.
    await page.getByLabel("New namespace name").fill("e2e");
    await page.getByRole("button", { name: "Create namespace" }).click();
    await expect(page.getByRole("status").first()).toHaveText(/created/i);

    // Create a memory through the UI form.
    await page.getByLabel("New memory text").fill("playwright e2e verifies the ranking explanation");
    await page.getByRole("button", { name: "Remember" }).click();
    await expect(page.getByRole("status").nth(1)).toHaveText(/stored/i);

    // Recall it and verify the breakdown table renders explainable numbers.
    await page.getByLabel("Query").fill("ranking explanation e2e");
    await page.getByRole("button", { name: "Recall" }).click();
    const table = page.getByRole("table", { name: "Recalled memories with score breakdowns" });
    await expect(table).toBeVisible();
    const firstRow = table.getByRole("row").nth(1);
    await expect(firstRow).toContainText("playwright e2e verifies the ranking explanation");
    for (const column of ["BM25", "Vector", "Recency", "Pinned", "Total"]) {
      await expect(table.getByRole("columnheader", { name: column })).toBeVisible();
    }
    // The total must be a real number in the row (not blank/NaN).
    const totalText = await firstRow.getByRole("cell").nth(5).innerText();
    expect(Number(totalText)).not.toBeNaN();

    // Keyboard: the recall button must be reachable and focusable.
    await page.getByLabel("Query").focus();
    await page.keyboard.press("Tab");
    await expect(page.getByRole("button", { name: "Recall" })).toBeFocused();
  });

  test("page is keyboard navigable and passes axe accessibility checks", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByRole("heading", { level: 1 })).toBeVisible();

    // Visible focus: tab to the first link and confirm it has an outline.
    await page.keyboard.press("Tab");
    const firstLink = page.getByRole("link", { name: "Namespaces" });
    await expect(firstLink).toBeFocused();

    const results = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
      .analyze();
    expect(
      results.violations.map((v) => `${v.id} (${v.nodes.length} nodes)`),
      JSON.stringify(results.violations, null, 2),
    ).toEqual([]);
  });
});
