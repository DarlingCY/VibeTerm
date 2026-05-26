function setUpdateStatus(message, kind = '') {
      updateStatus.textContent = message || '';
      updateStatus.className = `update-status${kind ? ` ${kind}` : ''}`;
    }

    function setUpdateButton(mode, { disabled = false } = {}) {
      updateButtonMode = mode;
      checkUpdateButton.disabled = disabled;
      checkUpdateButton.textContent = mode === 'install' ? '立即更新' : '检查更新';
      if (mode === 'checking') {
        checkUpdateButton.textContent = '检查中...';
      } else if (mode === 'installing') {
        checkUpdateButton.textContent = '更新中...';
      } else if (mode === 'launched') {
        checkUpdateButton.textContent = '已启动安装';
      }
    }

    function checkForUpdates(manual) {
      if (updateCheckInFlight || updateInstallInFlight) {
        return;
      }
      updateCheckInFlight = true;
      latestUpdate = null;
      setUpdateButton('checking', { disabled: true });
      setUpdateStatus(manual ? '正在检查更新...' : '正在自动检查更新...');
      post({ type: 'checkForUpdates', manual: Boolean(manual) });
    }

    function installLatestUpdate(silent) {
      if (updateInstallInFlight) {
        return;
      }
      if (!latestUpdate || !latestUpdate.assetUrl) {
        setUpdateStatus('没有可下载安装的更新包。', 'warning');
        setUpdateButton('check');
        return;
      }
      updateInstallInFlight = true;
      setUpdateButton('installing', { disabled: true });
      setUpdateStatus(silent ? `正在自动下载 ${latestUpdate.version}...` : `正在下载 ${latestUpdate.version}...`);
      post({
        type: 'installUpdate',
        version: latestUpdate.version,
        assetUrl: latestUpdate.assetUrl,
        silent: Boolean(silent),
      });
    }

    function handleUpdateButtonClick() {
      if (updateButtonMode === 'install' && latestUpdate && latestUpdate.assetUrl) {
        installLatestUpdate(false);
        return;
      }
      checkForUpdates(true);
    }

