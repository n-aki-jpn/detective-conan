import { test, expect } from '@playwright/test';

// T4: フロントエンドE2E（認証フロー）
// 新規登録 → ログイン状態表示 → ログアウト → 再ログイン、の一連を確認する。
//ニックネームは実行ごとに一意化（サーバー側で重複登録が弾かれるため）
const uniqueNick = `E2Eユーザ${Date.now().toString().slice(-6)}`;

test.describe('認証フロー E2E', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    // 前のテストのトークン残留を防ぐ
    await page.evaluate(() => localStorage.clear());
    await page.reload();
  });

  test('未ログイン時はログインオーバーレイが表示される', async ({ page }) => {
    const overlay = page.locator('#login-overlay');
    await expect(overlay).toBeVisible();
  });

  test('新規登録 → ログイン状態 → ログアウト → 再ログイン', async ({ page }) => {
    await expect(page.locator('#login-overlay')).toBeVisible();

    // 新規登録
    await page.fill('#login-nickname', uniqueNick);
    await page.click('button:has-text("新規登録")');

    // オーバーレイが消え、ニックネームバッジとログアウトボタンが出る
    await expect(page.locator('#login-overlay')).toBeHidden();
    const badge = page.locator('#auth-nickname-badge');
    await expect(badge).toBeVisible();
    await expect(badge).toContainText(uniqueNick);
    await expect(page.locator('#btn-logout')).toBeVisible();

    // トークンが localStorage に保存されている
    const token = await page.evaluate(() => localStorage.getItem('conan_auth_token'));
    expect(token).toBeTruthy();

    // ログアウト
    await page.click('#btn-logout');
    await expect(page.locator('#login-overlay')).toBeVisible();
    const tokenAfter = await page.evaluate(() => localStorage.getItem('conan_auth_token'));
    expect(tokenAfter).toBeNull();

    // 同じニックネームで再ログイン
    await page.fill('#login-nickname', uniqueNick);
    await page.click('button:has-text("ログイン")');
    await expect(page.locator('#login-overlay')).toBeHidden();
    await expect(page.locator('#auth-nickname-badge')).toContainText(uniqueNick);
  });

  test('存在しないニックネームでのログインはエラーになる', async ({ page }) => {
    await expect(page.locator('#login-overlay')).toBeVisible();
    await page.fill('#login-nickname', `存在しない${Date.now()}`);
    await page.click('button:has-text("ログイン")');
    // エラーメッセージが表示される
    await expect(page.locator('#login-error')).toBeVisible();
  });
});
