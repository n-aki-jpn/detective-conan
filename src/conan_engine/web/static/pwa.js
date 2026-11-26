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

  // ========== インストールボタン制御 ==========
  function updateInstallButtonVisibility() {
    if (!installBtn) return;
    if (isStandalone()) {
      installBtn.hidden = true;
      return;
    }
    if (supportsWebInstall || deferredInstallPrompt) {
      installBtn.hidden = false;
    }
  }

  if (installBtn) {
    if (supportsWebInstall && !isStandalone()) {
      installBtn.hidden = false;
    }

    window.addEventListener('beforeinstallprompt', (event) => {
      event.preventDefault();
      deferredInstallPrompt = event;
      updateInstallButtonVisibility();
    });

    installBtn.addEventListener('click', async () => {
      if (installBtn.getAttribute('aria-disabled') === 'true') return;
      installBtn.setAttribute('aria-disabled', 'true');
      try {
        if (supportsWebInstall) {
          await navigator.install();
          setStatus('アプリのインストールが完了しました。');
        } else if (deferredInstallPrompt) {
          const promptEvent = deferredInstallPrompt;
          deferredInstallPrompt = null;
          await promptEvent.prompt();
          const choice = await promptEvent.userChoice;
          if (choice && choice.outcome === 'accepted') {
            installBtn.hidden = true;
            setStatus('アプリのインストールが承認されました。');
          } else {
            setStatus('アプリのインストールがキャンセルされました。');
          }
        }
      } catch (err) {
        console.error('[pwa] インストール処理エラー:', err);
        if (err && err.name === 'AbortError') {
          updateInstallButtonVisibility();
          setStatus('インストールが中断されました。再度お試しいただけます。');
        }
      } finally {
        installBtn.removeAttribute('aria-disabled');
      }
    });

    window.addEventListener('appinstalled', () => {
      deferredInstallPrompt = null;
      installBtn.hidden = true;
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
