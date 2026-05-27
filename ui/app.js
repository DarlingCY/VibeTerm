    function makeTerminalOptions() {
      return {
        allowProposedApi: true,
        rendererType: 'canvas',
        cursorBlink: true,
        fontFamily: terminalSettings.fontFamily,
        fontSize: terminalSettings.fontSize,
        fontWeight: 'normal',
        fontWeightBold: 'bold',
        letterSpacing: 0,
        lineHeight: 1.2,
        customGlyphs: true,
        rescaleOverlappingGlyphs: true,
        windowsPty: {
          backend: 'conpty',
          buildNumber: 19044,
        },
        scrollback: 2000,
        scrollbarWidth: 0,
        theme: {
          background: '#0b0e14',
          foreground: '#dcdfe4',
          cursor: '#c678dd',
          selectionBackground: '#3e4451',
          black: '#282c34',
          red: '#e06c75',
          green: '#98c379',
          yellow: '#e5c07b',
          blue: '#61afef',
          magenta: '#c678dd',
          cyan: '#56b6c2',
          white: '#abb2bf',
          brightBlack: '#5c6370',
          brightRed: '#e06c75',
          brightGreen: '#98c379',
          brightYellow: '#e5c07b',
          brightBlue: '#61afef',
          brightMagenta: '#c678dd',
          brightCyan: '#56b6c2',
          brightWhite: '#ffffff',
        },
      };
    }

    function post(message) {
      if (typeof window.vibeTermInvoke !== 'function') {
        return Promise.resolve(null);
      }
      return window.vibeTermInvoke(message).catch(error => {
        console.error('frontend_message invoke failed', error);
        setStatus(String(error));
        return null;
      });
    }

    function setStatus(message) {
      const text = message || '';
      status.textContent = text;
      statusBar.hidden = text.length === 0;
    }

    function syncWindowMaximizeButton(maximized) {
      const button = document.getElementById('windowMaximize');
      if (!button) {
        return;
      }
      button.innerHTML = maximized ? '&#xE923;' : '&#xE922;';
      button.title = maximized ? '还原' : '最大化';
      button.setAttribute('aria-label', button.title);
    }

    function activePane() {
      return panes.get(activePaneId) || null;
    }

    function bytesFromBase64(dataBase64) {
      const binary = atob(dataBase64);
      const bytes = new Uint8Array(binary.length);
      for (let i = 0; i < binary.length; i += 1) {
        bytes[i] = binary.charCodeAt(i);
      }
      return bytes;
    }

    function concatenateBytes(chunks, totalBytes) {
      if (chunks.length === 1) {
        return chunks[0];
      }
      const combined = new Uint8Array(totalBytes);
      let offset = 0;
      for (const chunk of chunks) {
        combined.set(chunk, offset);
        offset += chunk.length;
      }
      return combined;
    }

    function terminalFontsReady() {
      if (!document.fonts) {
        return true;
      }
      return document.fonts.status !== 'loading';
    }

    function visibleTerminalDimensions(term) {
      const dimensions = term && term._core && term._core._renderService
        ? term._core._renderService.dimensions
        : null;
      if (!dimensions || !dimensions.css || !dimensions.css.cell ||
          !dimensions.css.cell.width || !dimensions.css.cell.height) {
        return null;
      }
      return dimensions;
    }

    function panePixelSize(rect) {
      return {
        pixelWidth: Math.max(1, Math.ceil(rect.width)),
        pixelHeight: Math.max(1, Math.ceil(rect.height)),
      };
    }

    function wheelLineCount(event) {
      const rawDelta = Math.abs(event.deltaY || 0);
      if (rawDelta === 0) {
        return 0;
      }
      if (event.deltaMode === WheelEvent.DOM_DELTA_PAGE) {
        return 12;
      }
      if (event.deltaMode === WheelEvent.DOM_DELTA_LINE) {
        return Math.max(1, Math.min(12, Math.round(rawDelta)));
      }
      return Math.max(1, Math.min(12, Math.round(rawDelta / 40)));
    }

    function scrollInputForWheel(term, event) {
      const up = event.deltaY < 0;
      // opencode binds line-by-line message scrolling to Ctrl+Alt+Y/E.
      // In terminals, Alt is encoded as an ESC prefix and Ctrl+Y/E are 0x19/0x05.
      return up ? '\x1b\x19' : '\x1b\x05';
    }

    function copyPaneSelection(pane) {
      if (!pane) {
        return false;
      }
      post({ type: 'copyToClipboard', text: pane.currentSelection() });
      return true;
    }

    function pasteIntoPane(pane) {
      if (!pane) {
        return false;
      }
      post({ type: 'pasteFromClipboard', paneId: pane.id });
      return true;
    }

    async function bindBackendEvents() {
      if (backendListenerBound) {
        return true;
      }
      const listen = window.__TAURI__ && window.__TAURI__.event && typeof window.__TAURI__.event.listen === 'function'
        ? window.__TAURI__.event.listen.bind(window.__TAURI__.event)
        : null;
      if (!listen) {
        throw new Error('Tauri event API unavailable');
      }
      await listen('frontend-event', event => {
        if (window.vibeTerm && event) {
          window.vibeTerm.receive(event.payload);
        }
      });
      backendListenerBound = true;
      return true;
    }

    window.vibeTerm = {
      receive(event) {
        switch (event.type) {
          case 'init':
            maxPanesPerTab = event.maxPanesPerTab;
            appVersion = event.appVersion || '';
            appVersionText.textContent = appVersion || '-';
            terminalSettings.fontFamily = event.fontFamily || defaultTerminalFont;
            terminalSettings.fontSize = normalizeFontSize(event.fontSize || defaultTerminalFontSize);
            syncTerminalFontCss();
            populateFontSelect(event.fontFamilies);
            syncSettingsControls();
            for (const pane of panes.values()) {
              pane.updateFont();
            }
            setStatus('');
            break;
          case 'tabCreated':
            createTab(event);
            break;
          case 'tabSelected':
            selectTab(event.tabId);
            break;
          case 'tabClosed':
            closeTab(event.tabId);
            break;
          case 'paneCreated':
            createPane(event);
            break;
          case 'paneSelected':
            selectPane(event.paneId);
            break;
          case 'paneReset':
            resetPane(event);
            break;
          case 'paneClosed':
            closePane(event.paneId);
            break;
          case 'output':
            writePane(event);
            break;
          case 'outputBatch':
            for (const chunk of event.chunks || []) {
              writePane(chunk);
            }
            break;
          case 'fontFamiliesLoaded':
            applyLoadedFontFamilies(event.fontFamilies);
            break;
          case 'diagnostics':
            setStatus('诊断信息已复制');
            copyDiagnosticText(event.text || '');
            break;
          case 'updateCheckStarted':
            updateCheckInFlight = true;
            setUpdateButton('checking', { disabled: true });
            setUpdateStatus(event.manual ? '正在检查更新...' : '正在自动检查更新...');
            break;
          case 'updateAvailable':
            updateCheckInFlight = false;
            latestUpdate = event;
            if (event.assetUrl) {
              setUpdateButton('install');
              setUpdateStatus(`发现新版本 ${event.version}，点击“立即更新”开始安装。`, 'success');
            } else {
              setUpdateButton('check');
              setUpdateStatus(`发现新版本 ${event.version}，但没有可自动安装的安装包。`, 'warning');
            }
            break;
          case 'updateNotAvailable':
            updateCheckInFlight = false;
            latestUpdate = null;
            setUpdateButton('check');
            setUpdateStatus(`已是最新版本 ${event.currentVersion}。`, 'success');
            break;
          case 'updateInstallStarted':
            updateInstallInFlight = true;
            setUpdateButton('installing', { disabled: true });
            setUpdateStatus(`正在下载 ${event.version}...`);
            break;
          case 'updateInstallLaunched':
            updateInstallInFlight = false;
            setUpdateButton('launched', { disabled: true });
            setUpdateStatus(`安装程序已启动：${event.version}`, 'success');
            break;
          case 'updateError':
            updateCheckInFlight = false;
            updateInstallInFlight = false;
            setUpdateButton(latestUpdate && latestUpdate.assetUrl ? 'install' : 'check');
            setUpdateStatus(event.message || '更新失败。', 'error');
            break;
          case 'windowState':
            syncWindowMaximizeButton(Boolean(event.maximized));
            break;
          case 'exit':
            markExited(event);
            break;
          case 'status':
            setStatus(event.message);
            break;
          case 'error':
            setStatus(event.message);
            break;
        }
      },
      fitActive() {
        fitVisiblePanes();
      },
    };

    window.addEventListener('resize', () => scheduleVisiblePaneFits());

    function boot() {
      const missingXtermGlobals = [];
      if (typeof window.Terminal !== 'function') missingXtermGlobals.push('Terminal');
      if (!window.FitAddon || typeof window.FitAddon.FitAddon !== 'function') missingXtermGlobals.push('FitAddon');
      if (!window.CanvasAddon || typeof window.CanvasAddon.CanvasAddon !== 'function') missingXtermGlobals.push('CanvasAddon');
      if (!window.ClipboardAddon || typeof window.ClipboardAddon.ClipboardAddon !== 'function') missingXtermGlobals.push('ClipboardAddon');
      if (!window.Unicode11Addon || typeof window.Unicode11Addon.Unicode11Addon !== 'function') missingXtermGlobals.push('Unicode11Addon');
      if (missingXtermGlobals.length) {
        setStatus(`xterm.js 加载失败：缺少 ${missingXtermGlobals.join(', ')}`);
        return;
      }
      bindUi();
      bindBackendEvents()
        .then(() => post({ type: 'ready' }))
        .catch(error => {
          const message = String(error);
          setStatus(`连接后端失败：${message}`);
          if (typeof window.vibeTermInvoke === 'function') {
            window.vibeTermInvoke({
              type: 'frontendError',
              message,
              stack: error && error.stack ? String(error.stack) : '',
            }).catch(() => {});
          }
        });
    }

    if (document.readyState === 'loading') {
      window.addEventListener('DOMContentLoaded', boot);
    } else {
      boot();
    }
