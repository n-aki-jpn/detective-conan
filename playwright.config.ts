import { defineConfig, devices } from '@playwright/test';

// T4: フロントエンドE2Eテスト（Playwright）
// テスト前に Rust サーバーを起動し、http://localhost:8000 に対して認証フローを検証する。
export default defineConfig({
  testDir: './tests/e2e',
  timeout: 60_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  workers: 1,
  reporter: [['list']],
  use: {
    baseURL: 'http://localhost:8000',
    trace: 'off',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
  // サーバーを起動してヘルスチェックで待ち合わせる
  webServer: {
    command: 'cargo run --quiet --manifest-path conan_server/Cargo.toml 8000',
    url: 'http://localhost:8000/health',
    reuseExistingServer: true,
    timeout: 120_000,
  },
});
