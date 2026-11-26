import { test, expect } from '@playwright/test';

test.describe('マリガン選択肢 E2E', () => {
  test('0枚選択時は「このまま開始」・全戻しは「すべて戻す」になり、選択連動と全戻し送信が動作する', async ({
    page,
  }) => {
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());
    await page.reload();

    const nick = `Mulligan${Date.now().toString().slice(-6)}`;
    await page.fill('#login-nickname', nick);
    await page.click('button:has-text("新規登録")');
    await expect(page.locator('#login-overlay')).toBeHidden();

    const confirmBtn = page.locator('#mulligan-confirm-btn');
    const allBtn = page.locator('#mulligan-all-btn');
    await expect(confirmBtn).toBeVisible();
    await expect(allBtn).toBeVisible();

    // 1. 初期状態（0枚選択）: 「0枚戻す」ではなく「✨ このまま開始」と「🔄 すべて戻す」が表示される
    await expect(confirmBtn).toHaveText('✨ このまま開始');
    await expect(allBtn).toHaveText('🔄 すべて戻す');

    // 2. カードを1枚タップすると「🔄 1枚戻す」に変わり、再タップで「✨ このまま開始」に戻る
    const items = page.locator('.mulligan-item');
    const itemCount = await items.count();
    expect(itemCount).toBe(5);

    await items.first().click();
    await expect(confirmBtn).toHaveText('🔄 1枚戻す');

    await items.first().click();
    await expect(confirmBtn).toHaveText('✨ このまま開始');

    // 3. 「🔄 すべて戻す」をクリックすると全5枚の cardIds が /api/decision に送信されマリガンが終了する
    const decisionPromise = page.waitForRequest(
      (req) => req.url().includes('/api/decision') && req.method() === 'POST',
    );
    await allBtn.click();
    const req = await decisionPromise;
    const body = req.postDataJSON() as { value?: { cardIds?: string[] } };
    expect(body.value?.cardIds).toHaveLength(5);

    await expect(confirmBtn).toBeHidden();
    await expect(allBtn).toBeHidden();
  });
});
