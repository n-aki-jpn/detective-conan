(() => {
  'use strict';

  const installBtn = document.getElementById('pwa-install-btn');
  const installStatus = document.getElementById('pwa-install-status');
  const offlineBanner = document.getElementById('pwa-offline-banner');
  const updateBanner = document.getElementById('pwa-update-banner');
  const updateBtn = document.getElementById('pwa-update-btn');
  const updateDismissBtn = document.getElementById('pwa-update-dismiss');

  let deferredInstallPrompt = null;
  let waitingWorker = null;
  let reloadRequested = false;
  let refreshing = false;

  const isStandalone = () =>
    window.matchMedia('(display-mode: standalone)').matches || window.navigator.standalone === true;

  const supportsWebInstall =
    typeof Navigator !== 'undefined' && typeof Navigator.prototype.install === 'function';

  function setStatus(message) {
    if (installStatus) {
      installStatus.textContent = message;
    }
  }

  // ========== オフライン状態バナー ==========
  function syncOnlineStatus() {
    if (offlineBanner) {
      offlineBanner.hidden = navigator.onLine;
    }
  }
  window.addEventListener('online', syncOnlineStatus);
  window.addEventListener('offline', syncOnlineStatus);
  syncOnlineStatus();

  // ========== インストールボタン制御 & 手順ガイド ==========
  const guideModal = document.getElementById('pwa-install-guide-modal');
  const guideBody = document.getElementById('pwa-install-guide-body');
  const guideCloseBtn = document.getElementById('pwa-install-guide-close');

  function detectInstallEnv() {
    const ua = navigator.userAgent || '';
    const isIpadOs = navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1;
    const isIos = /iPhone|iPad|iPod/i.test(ua) || isIpadOs;
    const isAndroid = /Android/i.test(ua);
    const isCriOS = /CriOS/i.test(ua);
    const isEdgiOS = /EdgiOS/i.test(ua);
    const isSafari = isIos && !isCriOS && !isEdgiOS && /Safari/i.test(ua);
    const isEdge = /Edg/i.test(ua);
    const isFirefox = /Firefox|FxiOS/i.test(ua);
    return { isIos, isAndroid, isSafari, isCriOS, isEdgiOS, isEdge, isFirefox };
  }

  function buildInstallGuideHtml() {
    const env = detectInstallEnv();
    const isInsecure = typeof window.isSecureContext === 'boolean' && !window.isSecureContext;
    const steps = [];

    if (env.isIos) {
      if (env.isSafari) {
        steps.push({
          title: '📱 iPhone / iPad（Safari）での追加手順',
          items: [
            '画面下部（または上部）の <b>「共有」ボタン（□↑）</b> をタップ',
            'メニューを下にスクロールして <b>「ホーム画面に追加」</b> をタップ',
            '右上の <b>「追加」</b> をタップするとホーム画面にアプリとして追加されます',
          ],
        });
      } else {
        steps.push({
          title: '📱 iPhone / iPad（Chrome・その他ブラウザ）での追加手順',
          items: [
            'アドレスバー付近の <b>「共有」アイコン（□↑）</b> → <b>「ホーム画面に追加」</b> をタップ',
            '項目が見当たらない場合は、このページを <b>Safari</b> で開いて「共有（□↑）→ ホーム画面に追加」をお試しください',
          ],
        });
      }
    } else if (env.isAndroid) {
      steps.push({
        title: '🤖 Android（Chrome / Edge）での追加手順',
        items: [
          'ブラウザ右上の <b>「⋮」メニュー</b> をタップ',
          '<b>「アプリをインストール」</b> または <b>「ホーム画面に追加」</b> をタップして追加します',
        ],
      });
    } else {
      steps.push({
        title: env.isFirefox
          ? '💻 PC ブラウザでのご利用について'
          : '💻 PC（Chrome / Edge）での追加手順',
        items: env.isFirefox
          ? [
              '現在お使いのブラウザはワンクリックインストールに非対応です。',
              '<b>Google Chrome</b> または <b>Microsoft Edge</b> で開くと、アドレスバー右端のインストールアイコンやこのボタンからアプリとして追加できます。',
            ]
          : [
              'アドレスバー右端の <b>「インストール」アイコン（💻↓）</b> をクリック',
              '見つからない場合はブラウザ右上の <b>「⋮」メニュー</b> → <b>「キャスト、保存、共有」</b>（または「アプリ」）→ <b>「ページをアプリとしてインストール」</b> を選択してください',
            ],
      });
    }

    const blocks = steps.map((s) => `
      <div style="background:#0f172a; border:1px solid #334155; border-radius:8px; padding:10px 12px;">
        <div style="font-weight:700; color:#7dd3fc; margin-bottom:6px;">${s.title}</div>
        <ol style="margin:0; padding-left:18px; display:flex; flex-direction:column; gap:4px;">
          ${s.items.map((item) => `<li>${item}</li>`).join('')}
        </ol>
      </div>
    `);

    if (isInsecure) {
      blocks.push(`
        <div style="background:rgba(245,158,11,0.12); border:1px solid rgba(245,158,11,0.45); border-radius:8px; padding:8px 10px; font-size:0.76rem; color:#fde68a;">
          ⚠️ 現在 <code>http://</code> のローカル接続等で開いているため、ブラウザの自動インストール機能が有効になっていません。<code>https://</code> または <code>localhost</code> で開くとワンクリック追加が利用できます。
        </div>
      `);
    }

    return blocks.join('');
  }

  function openInstallGuideModal() {
    if (!guideModal || !guideBody) return;
    guideBody.innerHTML = buildInstallGuideHtml();
    guideModal.style.display = 'flex';
    setStatus('アプリの追加手順ガイドを表示しました。');
  }

  function closeInstallGuideModal() {
    if (!guideModal) return;
    guideModal.style.display = 'none';
  }

  if (guideModal) {
    guideModal.addEventListener('click', () => closeInstallGuideModal());
  }
  if (guideCloseBtn) {
    guideCloseBtn.addEventListener('click', () => closeInstallGuideModal());
  }
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && guideModal && guideModal.style.display === 'flex') {
      closeInstallGuideModal();
    }
  });

  function updateInstallButtonVisibility() {
    if (!installBtn) return;
    if (isStandalone()) {
      installBtn.hidden = true;
      delete installBtn.dataset.installReady;
      return;
    }
    installBtn.hidden = false;
    if (deferredInstallPrompt) {
      installBtn.dataset.installReady = 'true';
    } else {
      delete installBtn.dataset.installReady;
    }
  }

  if (installBtn) {
    updateInstallButtonVisibility();

    window.addEventListener('beforeinstallprompt', (event) => {
      event.preventDefault();
      deferredInstallPrompt = event;
      updateInstallButtonVisibility();
    });

    installBtn.addEventListener('click', async () => {
      if (installBtn.getAttribute('aria-disabled') === 'true') return;
      if (typeof window.closeNavMenu === 'function') {
        window.closeNavMenu();
      }
      installBtn.setAttribute('aria-disabled', 'true');
      try {
        if (deferredInstallPrompt) {
          const promptEvent = deferredInstallPrompt;
          deferredInstallPrompt = null;
          updateInstallButtonVisibility();
          await promptEvent.prompt();
          const choice = await promptEvent.userChoice;
          if (choice && choice.outcome === 'accepted') {
            installBtn.hidden = true;
            setStatus('アプリのインストールが承認されました。');
          } else {
            setStatus('アプリのインストールがキャンセルされました。');
          }
        } else {
          openInstallGuideModal();
        }
      } catch (err) {
        console.error('[pwa] インストール処理エラー:', err);
        if (err && err.name === 'AbortError') {
          updateInstallButtonVisibility();
          setStatus('インストールが中断されました。再度お試しいただけます。');
        } else {
          openInstallGuideModal();
        }
      } finally {
        installBtn.removeAttribute('aria-disabled');
      }
    });

    window.addEventListener('appinstalled', () => {
      deferredInstallPrompt = null;
      installBtn.hidden = true;
      delete installBtn.dataset.installReady;
      closeInstallGuideModal();
      setStatus('アプリとしてインストールされました。');
    });
  }

  // ========== Service Worker 登録 & 更新通知 ==========
  function showUpdateBanner(worker) {
    waitingWorker = worker;
    if (updateBanner) {
      updateBanner.hidden = false;
    }
  }

  if (updateBtn) {
    updateBtn.addEventListener('click', () => {
      if (!waitingWorker) return;
      reloadRequested = true;
      waitingWorker.postMessage({ type: 'SKIP_WAITING' });
    });
  }

  if (updateDismissBtn) {
    updateDismissBtn.addEventListener('click', () => {
      if (updateBanner) {
        updateBanner.hidden = true;
      }
    });
  }

  if ('serviceWorker' in navigator) {
    navigator.serviceWorker.addEventListener('controllerchange', () => {
      if (!reloadRequested || refreshing) return;
      refreshing = true;
      window.location.reload();
    });

    navigator.serviceWorker.register('/sw.js', { scope: '/' })
      .then((reg) => {
        if (reg.waiting && navigator.serviceWorker.controller) {
          showUpdateBanner(reg.waiting);
        }
        reg.addEventListener('updatefound', () => {
          const installing = reg.installing;
          if (!installing) return;
          installing.addEventListener('statechange', () => {
            if (installing.state === 'installed' && navigator.serviceWorker.controller) {
              showUpdateBanner(reg.waiting || installing);
            }
          });
        });
      })
      .catch((err) => {
        console.error('[pwa] Service Worker 登録失敗:', err);
      });
  }

  // ========== マニフェスト shortcuts (?tab=deck / ?tab=battle) ==========
  function applyTabShortcut() {
    const params = new URLSearchParams(window.location.search);
    const tab = params.get('tab');
    if ((tab === 'deck' || tab === 'battle') && typeof window.switchTab === 'function') {
      window.switchTab(tab);
    }
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', applyTabShortcut);
  } else {
    applyTabShortcut();
  }
})();
