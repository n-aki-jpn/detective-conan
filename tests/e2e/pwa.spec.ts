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

    // 収録弾フィルタが表示され、選択肢に CT-P01 などが含まれること
    const packSelect = page.locator('#pack-filter-select');
    await expect(packSelect).toBeVisible();
    await expect(packSelect).toContainText('CT-P01');
    await packSelect.selectOption('CT-P01');
    await expect(packSelect).toHaveValue('CT-P01');

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
    await expect(page.locator('#bgm-menu-toggle-btn')).toBeVisible();
    await expect(page.locator('#bgm-volume-slider')).toBeVisible();
    await expect(page.locator('#se-toggle-btn')).toBeVisible();
    await expect(page.locator('#se-volume-slider')).toBeVisible();
    await expect(page.locator('#btn-reset-match')).toBeVisible();
    await expect(page.locator('#btn-copy-board')).toBeVisible();
    await expect(page.locator('#pwa-install-btn')).toBeVisible();
    await expect(page.locator('#btn-logout')).toBeVisible();

    // CardGameSE インスタンス及びメソッドの存在確認
    const seMethodsOk = await page.evaluate(() => {
      const s = (window as unknown as { se?: Record<string, unknown> }).se;
      if (!s) return false;
      return ['draw', 'place', 'shuffle', 'tap', 'panel', 'turnStart', 'resolve'].every(
        (m) => typeof s[m] === 'function',
      );
    });
    expect(seMethodsOk).toBe(true);

    // BGM と SE の音量が独立して調整・永続化されること
    await page.locator('#bgm-volume-slider').evaluate((el: HTMLInputElement) => {
      el.value = '0.2';
      el.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await page.locator('#se-volume-slider').evaluate((el: HTMLInputElement) => {
      el.value = '0.85';
      el.dispatchEvent(new Event('input', { bubbles: true }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
    });
    const audioState = await page.evaluate(() => ({
      bgmStorage: localStorage.getItem('conan_bgm_volume'),
      seStorage: localStorage.getItem('conan_se_volume'),
      seVol: (window as unknown as { se?: { volume: number; muted: boolean } }).se?.volume,
      bgmLabel: document.getElementById('bgm-volume-val')?.textContent,
      seLabel: document.getElementById('se-volume-val')?.textContent,
    }));
    expect(audioState.bgmStorage).toBe('0.2');
    expect(audioState.seStorage).toBe('0.85');
    expect(audioState.seVol).toBe(0.85);
    expect(audioState.bgmLabel).toBe('20%');
    expect(audioState.seLabel).toBe('85%');

    // SE ミュート切り替えが BGM に影響しないこと
    await page.click('#se-toggle-btn');
    const seMutedState = await page.evaluate(() => ({
      seEnabled: localStorage.getItem('conan_se_enabled'),
      bgmEnabled: localStorage.getItem('conan_bgm_enabled'),
      seMuted: (window as unknown as { se?: { muted: boolean } }).se?.muted,
      seBtnText: document.getElementById('se-toggle-btn')?.textContent,
    }));
    expect(seMutedState.seEnabled).toBe('false');
    expect(seMutedState.bgmEnabled).toBeNull();
    expect(seMutedState.seMuted).toBe(true);
    expect(seMutedState.seBtnText).toContain('🔕');

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
    await expect(page.locator('#se-volume-slider')).toBeVisible();
    await page.screenshot({
      path: `${SCREENSHOT_DIR}/pwa-mobile-menu.png`,
      fullPage: false,
    });
    await page.keyboard.press('Escape');
    await expect(pop).toBeHidden();
  });

  test('beforeinstallprompt 非対応環境（iOS Safari 等）でもアプリ追加ボタンから手順ガイドモーダルが開く', async ({
    browser,
  }) => {
    const context = await browser.newContext({
      userAgent:
        'Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1',
      viewport: { width: 390, height: 844 },
    });
    const page = await context.newPage();
    await page.addInitScript(() => {
      window.addEventListener(
        'beforeinstallprompt',
        (e) => {
          e.stopImmediatePropagation();
        },
        true,
      );
    });

    await page.goto('/');
    await page.evaluate(() => localStorage.clear());
    await page.reload();

    const nick = `iOS案内${Date.now().toString().slice(-6)}`;
    await page.fill('#login-nickname', nick);
    await page.click('button:has-text("新規登録")');
    await expect(page.locator('#login-overlay')).toBeHidden();

    await page.click('#nav-menu-btn');
    await expect(page.locator('#nav-menu-popover')).toBeVisible();
    await expect(page.locator('#pwa-install-btn')).toBeVisible();

    await page.click('#pwa-install-btn');
    const guideModal = page.locator('#pwa-install-guide-modal');
    await expect(guideModal).toBeVisible();
    await expect(page.locator('#pwa-install-guide-body')).toContainText('ホーム画面に追加');

    await page.click('#pwa-install-guide-close');
    await expect(guideModal).toBeHidden();
    await context.close();
  });
});
