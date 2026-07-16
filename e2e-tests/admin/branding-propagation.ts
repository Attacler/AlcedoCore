import { test, expect } from '@playwright/test';

test.describe('Branding Propagation', () => {
  test.beforeEach(async ({ page }) => {
    // Seed clean branding settings via API before each test
    await page.request.post('/api/settings/batch', {
      data: {
        settings: {
          site_name: 'Test Site',
          logo_url: '',
          favicon_url: '',
          default_view_mode: 'table',
          default_page_size: 25,
        }
      }
    });
  });

  test('should update the browser title when site_name is set', async ({ page }) => {
    await page.goto('/admin#/settings');
    await page.waitForLoadState('networkidle');

    // Open the Branding category
    const brandingHeader = page.locator('div:has(span:text-is("palette")) div.font-semibold:has-text("Branding")');
    await brandingHeader.click();
    await page.waitForTimeout(300);

    // Find the site_name input
    const siteNameRow = page.locator('div:has(div:has-text("Site Name"))');
    await expect(siteNameRow).toBeVisible();

    const nameInput = siteNameRow.locator('input[type="text"]');
    await nameInput.fill('My App');
    await page.waitForTimeout(200);

    // Save
    await siteNameRow.locator('button:has-text("Save")').click();
    await page.waitForTimeout(1000);

    // Verify the browser title updated via watchEffect in AppLayout
    const title = await page.title();
    expect(title).toBe('My App');
  });

  test('should show logo image in sidebar header when logo_url is set', async ({ page }) => {
    await page.goto('/admin#/settings');
    await page.waitForLoadState('networkidle');

    // Open Branding category
    const brandingHeader = page.locator('div:has(span:text-is("palette")) div.font-semibold:has-text("Branding")');
    await brandingHeader.click();
    await page.waitForTimeout(300);

    // Set a valid logo URL
    const logoUrlRow = page.locator('div:has(div:has-text("Logo URL"))');
    const logoInput = logoUrlRow.locator('input[type="text"]');
    await logoInput.fill('https://via.placeholder.com/160x32');
    await logoUrlRow.locator('button:has-text("Save")').click();
    await page.waitForTimeout(1000);

    // Navigate to dashboard to see sidebar
    await page.goto('/admin#/dashboard');
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(1000);

    // The sidebar should have an <img> element with the logo URL
    const sidebarLogo = page.locator('aside img');
    await expect(sidebarLogo).toBeVisible();
    await expect(sidebarLogo).toHaveAttribute('src', 'https://via.placeholder.com/160x32');
  });

  test('should show fallback dashboard icon when no logo URL is set', async ({ page }) => {
    // Ensure logo_url is empty (seeded in beforeEach)
    await page.goto('/admin#/dashboard');
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(1000);

    // When no logo_url, the sidebar should show the fallback Material Icon
    const fallbackIcon = page.locator('aside span.material-symbols-outlined:text-is("dashboard")');

    // The sidebar header should have the fallback icon visible
    // AppLayout has: <span v-else class="material-symbols-outlined text-2xl">dashboard</span>
    await expect(fallbackIcon).toBeVisible();
  });

  test('should update favicon link when favicon_url is set', async ({ page }) => {
    await page.goto('/admin#/settings');
    await page.waitForLoadState('networkidle');

    // Open Branding category
    const brandingHeader = page.locator('div:has(span:text-is("palette")) div.font-semibold:has-text("Branding")');
    await brandingHeader.click();
    await page.waitForTimeout(300);

    // Set favicon URL
    const faviconRow = page.locator('div:has(div:has-text("Favicon URL"))');
    const faviconInput = faviconRow.locator('input[type="text"]');
    await faviconInput.fill('https://example.com/favicon.ico');
    await faviconRow.locator('button:has-text("Save")').click();
    await page.waitForTimeout(1000);

    // Navigate to dashboard to trigger the AppLayout watcher
    await page.goto('/admin#/dashboard');
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(500);

    // Check the favicon link element in the page head
    const faviconLink = page.locator('#favicon');
    await expect(faviconLink).toHaveAttribute('href', 'https://example.com/favicon.ico');
  });

  test('should show site name in sidebar header and desktop top bar', async ({ page }) => {
    await page.goto('/admin#/settings');
    await page.waitForLoadState('networkidle');

    // Open Branding
    const brandingHeader = page.locator('div:has(span:text-is("palette")) div.font-semibold:has-text("Branding")');
    await brandingHeader.click();
    await page.waitForTimeout(300);

    // Set site name
    const siteNameRow = page.locator('div:has(div:has-text("Site Name"))');
    const nameInput = siteNameRow.locator('input[type="text"]');
    await nameInput.fill('My Custom Dashboard');
    await siteNameRow.locator('button:has-text("Save")').click();
    await page.waitForTimeout(1000);

    // Navigate to dashboard
    await page.goto('/admin#/dashboard');
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(1000);

    // Verify site name in sidebar (span with class text-sm font-semibold truncate inside aside header)
    await expect(page.locator('aside span:has-text("My Custom Dashboard")').first()).toBeVisible();

    // Verify site name in desktop top bar (h1 inside the desktop header)
    const topBar = page.locator('header.hidden h1');
    if (await topBar.count() > 0) {
      await expect(topBar).toHaveText('My Custom Dashboard');
    }
  });

  test('should persist all branding settings across page reload', async ({ page }) => {
    // Set all branding values
    await page.goto('/admin#/settings');
    await page.waitForLoadState('networkidle');

    const brandingHeader = page.locator('div:has(span:text-is("palette")) div.font-semibold:has-text("Branding")');
    await brandingHeader.click();
    await page.waitForTimeout(300);

    // Set site_name
    const siteNameRow = page.locator('div:has(div:has-text("Site Name"))');
    await siteNameRow.locator('input[type="text"]').fill('Persistent App');
    await siteNameRow.locator('button:has-text("Save")').click();
    await page.waitForTimeout(500);

    // Set logo_url
    const logoUrlRow = page.locator('div:has(div:has-text("Logo URL"))');
    await logoUrlRow.locator('input[type="text"]').fill('https://via.placeholder.com/160x32');
    await logoUrlRow.locator('button:has-text("Save")').click();
    await page.waitForTimeout(500);

    // Set favicon_url
    const faviconRow = page.locator('div:has(div:has-text("Favicon URL"))');
    await faviconRow.locator('input[type="text"]').fill('https://example.com/favicon.ico');
    await faviconRow.locator('button:has-text("Save")').click();
    await page.waitForTimeout(500);

    // Reload the page entirely (full page reload)
    await page.reload();
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(1500);

    // Verify site name persisted — document.title should be the set value
    const title = await page.title();
    expect(title).toBe('Persistent App');

    // Navigate to dashboard for full sidebar rendering
    await page.goto('/admin#/dashboard');
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(1500);

    // Verify logo image still renders with correct src
    const sidebarLogo = page.locator('aside img');
    await expect(sidebarLogo).toBeVisible();
    await expect(sidebarLogo).toHaveAttribute('src', 'https://via.placeholder.com/160x32');

    // Verify site name in sidebar
    await expect(page.locator('aside span:has-text("Persistent App")').first()).toBeVisible();

    // Verify favicon link persisted
    const faviconLink = page.locator('#favicon');
    await expect(faviconLink).toHaveAttribute('href', 'https://example.com/favicon.ico');
  });
});
