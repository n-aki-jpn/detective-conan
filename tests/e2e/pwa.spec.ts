import { test, expect } from '@playwright/test';

const SCREENSHOT_DIR = 'test-results';

test.describe('PWA 統合検証 E2E', () => {
  test('マニフェスト・アイコン・Service Worker・オフラインページが正しく提供される', async ({
    page,
    request,
  }) => {
    const consoleErrors: string[] = [];
    const pageErrors: string[] = [];
    page.on('console', (msg) => {
      if (msg.type() === 'error') {
        consoleErrors.push(msg.text());
      }
    });
    page.on('pageerror', (err) => {
      pageErrors.push(err.message);
    });

    await page.goto('/');

    // 1. <link rel="manifest"> が /manifest.webmanifest を指すこと
    const manifestHref = await page
      .locator('link[rel="manifest"]')
      .getAttribute('href');
    expect(manifestHref).toBe('/manifest.webmanifest');

    // 2. /manifest.webmanifest が HTTP 200 & application/manifest+json & 有効な JSON であること
    const manifestRes = await request.get('/manifest.webmanifest');
    expect(manifestRes.status()).toBe(200);
    const manifestCt = manifestRes.headers()['content-type'] ?? '';
    expect(manifestCt.toLowerCase()).toContain('application/manifest+json');
    const manifest = await manifestRes.json();
    expect(manifest.id).toBe('/');
    expect(manifest.name).toBeTruthy();
    expect(manifest.short_name).toBeTruthy();
    expect(manifest.start_url).toBe('/');
    expect(manifest.display).toBe('standalone');
    expect(Array.isArray(manifest.icons)).toBe(true);
    expect(manifest.icons.length).toBeGreaterThan(0);

    // 3. マニフェスト内の全アイコン URL が HTTP 200 で取得でき、Content-Type が画像であること
    for (const icon of manifest.icons as Array<{ src: string }>) {
      const iconRes = await request.get(icon.src);
      expect(iconRes.status(), `icon ${icon.src} should be 200`).toBe(200);
      const iconCt = iconRes.headers()['content-type'] ?? '';
      expect(iconCt.toLowerCase(), `icon ${icon.src} content-type`).toMatch(
        /^image\//,
      );
    }

    // 4. /sw.js が HTTP 200 & Cache-Control: no-cache & Service-Worker-Allowed: /
    const swRes = await request.get('/sw.js');
    expect(swRes.status()).toBe(200);
    const swHeaders = swRes.headers();
    expect((swHeaders['cache-control'] ?? '').toLowerCase()).toContain('no-cache');
    expect(swHeaders['service-worker-allowed']).toBe('/');

    // Chromium 上で Service Worker が登録されて activated 状態になること
    const swState = await page.evaluate(async () => {
      if (!('serviceWorker' in navigator)) {
        return { supported: false, state: null };
      }
      const reg = await navigator.serviceWorker.ready;
      return {
        supported: true,
        state: reg.active?.state ?? null,
        scope: reg.scope,
      };
    });
    expect(swState.supported).toBe(true);
    expect(swState.state).toBe('activated');

    // 5. /offline.html が HTTP 200 で取得でき、オフライン時に再読み込みボタンが表示されること
    const offlineHttpRes = await request.get('/offline.html');
    expect(offlineHttpRes.status()).toBe(200);
    const offlineHtml = await offlineHttpRes.text();
    expect(offlineHtml).toContain('オフラインです');

    // オフライン状態にして /offline.html を表示し、#reload-btn が描画されることを確認
    await page.context().setOffline(true);
    try {
      await page.goto('/offline.html');
      await expect(page.locator('body')).toContainText('オフラインです');
      const reloadBtn = page.locator('#reload-btn');
      await expect(reloadBtn).toBeVisible();
      await expect(reloadBtn).toContainText('再読み込み');
    } finally {
      await page.context().setOffline(false);
    }

    expect(consoleErrors).toEqual([]);
    expect(pageErrors).toEqual([]);
  });

  test('PWA 導入後も認証・対戦・デッキ構築画面が正常に描画されエラーが出ない', async ({
    page,
  }) => {
    const consoleErrors: string[] = [];
    const pageErrors: string[] = [];
    page.on('console', (msg) => {
      if (msg.type() === 'error') {
        consoleErrors.push(msg.text());
      }
    });
    page.on('pageerror', (err) => {
      pageErrors.push(err.message);
    });

    await page.goto('/');
    await page.evaluate(() => localStorage.clear());
    await page.reload();

    // 認証オーバーレイで新規登録してログイン
    await expect(page.locator('#login-overlay')).toBeVisible();
    const nick = `PWA検証${Date.now().toString().slice(-6)}`;
    await page.fill('#login-nickname', nick);
    await page.click('button:has-text("新規登録")');
    await expect(page.locator('#login-overlay')).toBeHidden();
    await expect(page.locator('#auth-nickname-badge')).toContainText(nick);

    // 対戦画面とナビゲーション表示のスクリーンショットを撮影
    await page.screenshot({
      path: `${SCREENSHOT_DIR}/pwa-desktop.png`,
      fullPage: false,
    });

    // デッキ構築タブへ切り替え可能か確認
    const deckTabBtn = page.locator('button, a').filter({ hasText: 'デッキ構築' }).first();
    await expect(deckTabBtn).toBeVisible();
    await deckTabBtn.click();

    // 対戦タブに戻れるか確認
    const battleTabBtn = page.locator('button, a').filter({ hasText: '対戦' }).first();
    await expect(battleTabBtn).toBeVisible();
    await battleTabBtn.click();

    expect(consoleErrors).toEqual([]);
    expect(pageErrors).toEqual([]);
  });

  test('上部ナビゲーションバーがデスクトップ・モバイルともに1行に収まりメニューポップオーバーが動作する', async ({
    page,
  }) => {
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());
    await page.reload();

    const nick = `Nav確認${Date.now().toString().slice(-6)}`;
    await page.fill('#login-nickname', nick);
    await page.click('button:has-text("新規登録")');
    await expect(page.locator('#login-overlay')).toBeHidden();

    // デスクトップ (1280x720) 検証
    await page.setViewportSize({ width: 1280, height: 720 });
    const desktopMetrics = await page.evaluate(() => {
      const nav = document.querySelector('.nav-bar') as HTMLElement;
      return {
        height: nav.getBoundingClientRect().height,
        scrollWidth: nav.scrollWidth,
        clientWidth: nav.clientWidth,
      };
    });
    expect(desktopMetrics.height).toBeLessThanOrEqual(56);
    expect(desktopMetrics.scrollWidth).toBeLessThanOrEqual(desktopMetrics.clientWidth);

    // メニューポップオーバー開閉検証
    await page.click('#nav-menu-btn');
    const pop = page.locator('#nav-menu-popover');
    await expect(pop).toBeVisible();
    await expect(page.locator('#nav-menu-user-full')).toContainText(nick);
    await expect(page.locator('#bgm-volume-slider')).toBeVisible();
    await expect(page.locator('#btn-reset-match')).toBeVisible();
    await expect(page.locator('#btn-copy-board')).toBeVisible();
    await expect(page.locator('#btn-logout')).toBeVisible();

    await page.keyboard.press('Escape');
    await expect(pop).toBeHidden();

    // モバイル (390x844) 検証
    await page.setViewportSize({ width: 390, height: 844 });
    const mobileMetrics = await page.evaluate(() => {
      const nav = document.querySelector('.nav-bar') as HTMLElement;
      return {
        height: nav.getBoundingClientRect().height,
        scrollWidth: nav.scrollWidth,
        clientWidth: nav.clientWidth,
      };
    });
    expect(mobileMetrics.height).toBeLessThanOrEqual(56);
    expect(mobileMetrics.scrollWidth).toBeLessThanOrEqual(mobileMetrics.clientWidth);

    await page.click('#nav-menu-btn');
    await expect(pop).toBeVisible();
    await expect(page.locator('#bgm-volume-slider')).toBeVisible();
    await page.screenshot({
      path: `${SCREENSHOT_DIR}/pwa-mobile-menu.png`,
      fullPage: false,
    });
    await page.keyboard.press('Escape');
    await expect(pop).toBeHidden();
  });
});
