function makePaneButton(className, title, text, onClick) {
      const button = document.createElement('button');
      button.className = className;
      button.title = title;
      const label = document.createElement('span');
      label.className = 'pane-action-label';
      label.innerHTML = text;
      button.appendChild(label);
      button.addEventListener('pointerdown', event => {
        event.preventDefault();
        event.stopPropagation();
      }, true);
      button.addEventListener('click', event => {
        event.preventDefault();
        event.stopPropagation();
        onClick();
      });
      return button;
    }

    class PaneView {
      constructor(event, tab) {
        this.id = event.paneId;
        this.tabId = event.tabId;
        this.tab = tab;
        this.started = false;
        this.starting = false;
        this.opened = false;
        this.deferredFitTimer = null;
        this.pendingFitFrame = null;
        this.pendingForceBackendResize = false;
        this.postStartResizeFrame = null;
        this.postStartResizeTimer = null;
        this.colorObserver = null;
        this.lastFitCols = 0;
        this.lastFitRows = 0;
        this.lastFitPixelWidth = 0;
        this.lastFitPixelHeight = 0;
        this.exited = Boolean(event.exited);
        this.selection = '';

        this.element = document.createElement('section');
        this.element.className = 'pane';
        this.element.dataset.paneId = String(this.id);

        this.headerElement = document.createElement('div');
        this.headerElement.className = 'pane-header';

        this.cwdElement = document.createElement('span');
        this.cwdElement.className = 'pane-cwd';
        this.cwdElement.textContent = this.label();

        this.addButton = makePaneButton('pane-action add', '新增 Pane', '+', () => {
          const activeTab = tabs.get(activeTabId);
          if (!activeTab || activeTab.panes.length < maxPanesPerTab) {
            post({ type: 'addPane' });
          }
        });
        this.closeButton = makePaneButton('pane-action close', '关闭', '&#10005;', () => {
          post({ type: 'closePane', paneId: this.id });
        });

        this.headerElement.append(this.cwdElement, this.addButton, this.closeButton);

        this.terminalElement = document.createElement('div');
        this.terminalElement.className = 'terminal';
        applyFontToTerminalElement(this.terminalElement);
        this.element.append(this.headerElement, this.terminalElement);

        this.term = new window.Terminal(makeTerminalOptions());
        this.fitAddon = new window.FitAddon.FitAddon();
        this.term.loadAddon(this.fitAddon);
        if (window.CanvasAddon && typeof window.CanvasAddon.CanvasAddon === 'function') {
          this.canvasAddon = new window.CanvasAddon.CanvasAddon();
          this.term.loadAddon(this.canvasAddon);
        }
        if (window.Unicode11Addon && typeof window.Unicode11Addon.Unicode11Addon === 'function') {
          this.unicode11Addon = new window.Unicode11Addon.Unicode11Addon();
          this.term.loadAddon(this.unicode11Addon);
          try {
            this.term.unicode.activeVersion = '11';
          } catch (error) {}
        }
        if (window.ClipboardAddon && typeof window.ClipboardAddon.ClipboardAddon === 'function') {
          this.clipboardAddon = new window.ClipboardAddon.ClipboardAddon(undefined, {
            readText: () => navigator.clipboard ? navigator.clipboard.readText() : Promise.resolve(''),
            writeText: (_selection, data) => {
              post({ type: 'copyToClipboard', text: data || this.currentSelection() });
              return navigator.clipboard ? navigator.clipboard.writeText(data || this.currentSelection()).catch(() => {}) : Promise.resolve();
            },
          });
          this.term.loadAddon(this.clipboardAddon);
        }
        this.term.onData(data => post({ type: 'input', paneId: this.id, data }));
        if (typeof this.term.onSelectionChange === 'function') {
          this.term.onSelectionChange(() => this.syncSelection());
        }
        this.term.attachCustomKeyEventHandler(event => handleTerminalClipboardShortcut(event, this));
        this.terminalElement.addEventListener('mousedown', () => this.focus(true));
        this.element.addEventListener('wheel', event => this.handleWheel(event), {
          passive: false,
          capture: true,
        });
        this.resizeObserver = new ResizeObserver(() => {
          this.scheduleFitAndStart();
        });
        this.resizeObserver.observe(this.terminalElement);
      }

      attach() {
        this.tab.content.appendChild(this.element);
      }

      activate() {
        if (!this.element.isConnected) {
          return;
        }
        this.openTerminal();
        this.flushPendingOutput();
        this.scheduleFitAndStart({ forceBackendResize: true });
      }

      openTerminal() {
        if (this.opened) {
          return;
        }
        this.term.open(this.terminalElement);
        this.opened = true;
        applyFontToTerminalElement(this.terminalElement);
        this.installColorObserver();
        normalizeXtermDomColors(this.terminalElement);
      }

      installColorObserver() {
        if (this.colorObserver || typeof MutationObserver !== 'function') {
          return;
        }
        this.colorObserver = new MutationObserver(mutations => {
          for (const mutation of mutations) {
            if (mutation.type === 'attributes' && mutation.target.nodeType === Node.ELEMENT_NODE) {
              normalizeXtermDomColors(mutation.target);
              continue;
            }
            for (const node of mutation.addedNodes) {
              if (node.nodeType === Node.ELEMENT_NODE) {
                normalizeXtermDomColors(node);
              }
            }
          }
        });
        this.colorObserver.observe(this.terminalElement, {
          attributes: true,
          attributeFilter: ['class', 'style'],
          childList: true,
          subtree: true,
        });
      }

      scheduleFitAndStart({ forceBackendResize = false } = {}) {
        this.pendingForceBackendResize = this.pendingForceBackendResize || forceBackendResize;
        this.clearPendingFitFrame();
        const runFitAndStart = () => {
          const shouldForceBackendResize = this.pendingForceBackendResize;
          const size = this.fit({ forceBackendResize: shouldForceBackendResize });
          if (size || !shouldForceBackendResize) {
            this.pendingForceBackendResize = false;
          }
          this.ensureStarted();
        };
        const scheduleFrame = framesRemaining => {
          this.pendingFitFrame = requestAnimationFrame(() => {
            if (framesRemaining > 1) {
              scheduleFrame(framesRemaining - 1);
              return;
            }
            this.pendingFitFrame = null;
            runFitAndStart();
          });
        };
        scheduleFrame(3);
        this.clearDeferredFitTimer();
        this.deferredFitTimer = setTimeout(() => {
          this.deferredFitTimer = null;
          runFitAndStart();
        }, 160);
      }

      fit({ forceBackendResize = false } = {}) {
        if (!this.opened || !this.element.isConnected || this.element.offsetParent === null) {
          return null;
        }
        const rect = this.terminalElement.getBoundingClientRect();
        if (rect.width < 20 || rect.height < 20) {
          return null;
        }
        try {
          if (!terminalFontsReady()) {
            if (document.fonts.ready && typeof document.fonts.ready.then === 'function') {
              document.fonts.ready.then(() => this.scheduleFitAndStart({ forceBackendResize })).catch(() => {});
            }
            return null;
          }
          const proposed = typeof this.fitAddon.proposeDimensions === 'function'
            ? this.fitAddon.proposeDimensions()
            : null;
          if (!proposed || !proposed.cols || !proposed.rows) {
            return null;
          }
          const cols = Math.max(2, Math.floor(proposed.cols));
          const rows = Math.max(1, Math.floor(proposed.rows));
          const pixels = panePixelSize(rect);
          if (cols !== this.term.cols || rows !== this.term.rows) {
            if (this.term._core && this.term._core._renderService) {
              this.term._core._renderService.clear();
            }
            this.term.resize(cols, rows);
          }
          const changed = cols !== this.lastFitCols || rows !== this.lastFitRows ||
            pixels.pixelWidth !== this.lastFitPixelWidth || pixels.pixelHeight !== this.lastFitPixelHeight;
          this.lastFitCols = cols;
          this.lastFitRows = rows;
          this.lastFitPixelWidth = pixels.pixelWidth;
          this.lastFitPixelHeight = pixels.pixelHeight;
          if (this.started && (changed || forceBackendResize)) {
            post({ type: 'resize', paneId: this.id, cols, rows, ...pixels });
          }
          return { cols, rows, ...pixels };
        } catch (error) {
          setStatus(String(error));
          return null;
        }
      }

      handleWheel(event) {
        if (!this.opened || !this.started) {
          return true;
        }
        const lines = wheelLineCount(event);
        if (lines <= 0) {
          return true;
        }
        event.preventDefault();
        event.stopPropagation();
        if (typeof event.stopImmediatePropagation === 'function') {
          event.stopImmediatePropagation();
        }
        const repeat = 2;
        const input = scrollInputForWheel(this.term, event).repeat(repeat);
        post({ type: 'input', paneId: this.id, data: input });
        return false;
      }

      ensureStarted() {
        if (this.exited || this.started || this.starting || !this.opened) {
          return;
        }
        const size = this.fit();
        if (!size) {
          return;
        }
        this.starting = true;
        try {
          post({
            type: 'startPane',
            paneId: this.id,
            cols: size.cols,
            rows: size.rows,
            pixelWidth: size.pixelWidth,
            pixelHeight: size.pixelHeight,
          });
          this.started = true;
          this.schedulePostStartResize();
        } catch (error) {
          this.started = false;
          throw error;
        } finally {
          this.starting = false;
        }
      }

      schedulePostStartResize() {
        this.clearPostStartResize();
        this.postStartResizeFrame = requestAnimationFrame(() => {
          this.postStartResizeFrame = null;
          this.fit({ forceBackendResize: true });
        });
        this.postStartResizeTimer = setTimeout(() => {
          this.postStartResizeTimer = null;
          this.fit({ forceBackendResize: true });
        }, 250);
      }

      clearDeferredFitTimer() {
        if (this.deferredFitTimer !== null) {
          clearTimeout(this.deferredFitTimer);
          this.deferredFitTimer = null;
        }
      }

      clearPendingFitFrame() {
        if (this.pendingFitFrame !== null) {
          cancelAnimationFrame(this.pendingFitFrame);
          this.pendingFitFrame = null;
        }
      }

      clearPostStartResize() {
        if (this.postStartResizeFrame !== null) {
          cancelAnimationFrame(this.postStartResizeFrame);
          this.postStartResizeFrame = null;
        }
        if (this.postStartResizeTimer !== null) {
          clearTimeout(this.postStartResizeTimer);
          this.postStartResizeTimer = null;
        }
      }

      clearPaneTimers() {
        this.clearDeferredFitTimer();
        this.clearPendingFitFrame();
        this.clearPostStartResize();
        this.pendingForceBackendResize = false;
      }

      setActive(active) {
        this.element.classList.toggle('active', active);
      }

      currentSelection() {
        const selection = this.term.getSelection();
        return selection || this.selection;
      }

      syncSelection() {
        this.selection = this.term.getSelection();
        post({ type: 'selectionChanged', paneId: this.id, text: this.selection });
      }

      syncControls(paneCount) {
        const closeHidden = paneCount <= 1;
        const addHidden = paneCount >= maxPanesPerTab;
        this.closeButton.hidden = closeHidden;
        this.addButton.hidden = addHidden;
        this.closeButton.style.display = closeHidden ? 'none' : '';
        this.addButton.style.display = addHidden ? 'none' : '';
      }

      focus(notify) {
        activePaneId = this.id;
        for (const pane of panes.values()) {
          const tab = tabs.get(pane.tabId);
          const canShowActive = tab && tab.panes.length > 1;
          pane.setActive(canShowActive && pane.id === this.id);
        }
        if (this.tabId === activeTabId) {
          this.activate();
        }
        requestAnimationFrame(() => this.term.focus());
        if (notify) {
          post({ type: 'selectPane', paneId: this.id });
        }
      }

      reset() {
        pendingOutput.delete(this.id);
        this.started = false;
        this.starting = false;
        this.clearPaneTimers();
        this.exited = false;
        this.selection = '';
        this.cwdElement.textContent = this.label();
        post({ type: 'selectionChanged', paneId: this.id, text: '' });
        this.term.reset();
        this.term.clear();
        this.scheduleFitAndStart();
      }

      write(dataBase64) {
        if (!this.opened) {
          const chunks = pendingOutput.get(this.id) || [];
          chunks.push(dataBase64);
          pendingOutput.set(this.id, chunks);
          return;
        }
        this.term.write(bytesFromBase64(dataBase64), () => {
          normalizeXtermDomColors(this.terminalElement);
        });
      }

      flushPendingOutput() {
        const chunks = pendingOutput.get(this.id);
        if (!chunks) {
          return;
        }
        pendingOutput.delete(this.id);
        for (const chunk of chunks) {
          this.write(chunk);
        }
      }

      markExited() {
        this.started = false;
        this.starting = false;
        this.clearPaneTimers();
        this.exited = true;
        this.cwdElement.textContent = this.label();
      }

      label() {
        return `Pane#${this.id}`;
      }

      updateFont() {
        applyFontToTerminalElement(this.terminalElement);
        setTerminalFontOption(this.term);
        requestAnimationFrame(() => {
          applyFontToTerminalElement(this.terminalElement);
          this.fit();
        });
      }

      dispose() {
        this.clearPaneTimers();
        if (this.colorObserver !== null) {
          this.colorObserver.disconnect();
          this.colorObserver = null;
        }
        pendingOutput.delete(this.id);
        this.resizeObserver.disconnect();
        this.term.dispose();
        this.element.remove();
      }
    }

