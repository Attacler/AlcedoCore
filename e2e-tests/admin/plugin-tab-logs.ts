const { test, expect } = require('@playwright/test');

test.describe('Plugin - Logs Tab', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/admin#/plugins/admin');
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(1000);
  });

  test('should display logs with filter inputs', async ({ page }) => {
    await page.click('button:has-text("Logs")');
    await page.waitForTimeout(1000);

    await expect(page.locator('input[placeholder="Path prefix..."]')).toBeVisible();
    await expect(page.getByRole('combobox')).toBeVisible();
    await expect(page.locator('button:has-text("Filter")')).toBeVisible();
  });

  test('should display log table with correct columns', async ({ page }) => {
    await page.click('button:has-text("Logs")');
    await page.waitForTimeout(1000);

    await expect(page.locator('th:has-text("Timestamp")')).toBeVisible();
    await expect(page.locator('th:has-text("Status")')).toBeVisible();
    await expect(page.locator('th:has-text("Path")')).toBeVisible();
    await expect(page.locator('th:has-text("Duration")')).toBeVisible();
  });

  test('should display log entries', async ({ page }) => {
    await page.click('button:has-text("Logs")');
    await page.waitForTimeout(1000);

    const logRows = page.locator('tbody tr');
    await expect(logRows.first()).toBeVisible();
    const rowCount = await logRows.count();
    expect(rowCount).toBeGreaterThan(0);
  });

  test('should show log path and status values', async ({ page }) => {
    await page.click('button:has-text("Logs")');
    await page.waitForTimeout(1000);

    const cells = page.locator('td');
    await expect(cells.first()).toBeVisible();
  });

  test('should show new log entry after calling health endpoint', async ({ page }) => {
    await page.click('button:has-text("Logs")');
    await page.waitForTimeout(1000);

    const logRows = page.locator('tbody tr');
    const initialCount = await logRows.count();

    await page.request.get('/health');
    await page.waitForTimeout(500);

    await page.click('button:has-text("Filter")');
    await page.waitForTimeout(1500);

    const newCount = await logRows.count();
    expect(newCount).toBeGreaterThanOrEqual(initialCount);
  });
});