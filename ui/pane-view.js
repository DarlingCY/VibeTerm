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
        this.visible = false;
        this.deferredFitTimer = null;
        this.pendingFitFrame = null;
        this.pendingForceBackendResize = false;
        this.postStartResizeFrame = null;
        this.postStartResizeTimer = null;
        this.colorObserver = null;
        this.outputQueue = [];
        this.outputQueueBytes = 0;
        this.outputFlushTimer = null;
        this.outputWriteInFlight = false;
        this.pendingReplayChunks = [];
        this.pendingReplayStart = 0;
        this.pendingReplayFrame = null;
        this.lastFitCols = 0;
        this.lastFitRows = 0;
        this.lastFitPixelWidth = 0;
        this.lastFitPixelHeight = 0;
        this.exited = Boolean(event.exited);
        this.selection = '';
        this.cwd = String(event.cwd || '').trim();
        this.paneIndex = null;
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
        this.cloneButton = makePaneButton('pane-action clone', '复制当前路径打开 Pane', '&#10697;', () => {
          const activeTab = tabs.get(activeTabId);
          if ((!activeTab || activeTab.panes.length < maxPanesPerTab) && this.cwd) {
            post({ type: 'addPane', cwd: this.cwd });
          }
        });
        this.closeButton = makePaneButton('pane-action close', '关闭', '&#10005;', () => {
          post({ type: 'closePane', paneId: this.id });
        });

        this.headerElement.append(this.cwdElement, this.cloneButton, this.addButton, this.closeButton);

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
            readText: () => Promise.resolve(''),
            writeText: (_selection, data) => {
              post({ type: 'copyToClipboard', text: data || this.currentSelection() });
              return Promise.resolve();
            },
          });
          this.term.loadAddon(this.clipboardAddon);
        }
        this.term.onData(data => this.sendInput(data));
        if (typeof this.term.onSelectionChange === 'function') {
          this.term.onSelectionChange(() => this.syncSelection());
        }
        this.term.attachCustomKeyEventHandler(event => handleTerminalClipboardShortcut(event, this));
        this.terminalElement.addEventListener('mousedown', () => this.focus(true));
        this.terminalElement.addEventListener('contextmenu', event => {
          event.preventDefault();
          event.stopPropagation();
        }, true);
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
        this.setVisible(true);
        this.openTerminal();
        this.flushPendingOutput();
        this.scheduleFitAndStart({ forceBackendResize: true });
      }

      setVisible(visible) {
        this.visible = Boolean(visible);
        if (this.visible && this.opened) {
          this.flushPendingOutput();
        }
      }

      sendInput(data) {
        if (data === '\x03') {
          return;
        }
        post({ type: 'input', paneId: this.id, data });
      }

      openTerminal() {
        if (this.opened) {
          return;
        }
        this.term.open(this.terminalElement);
        this.opened = true;
        applyFontToTerminalElement(this.terminalElement);
        if (enableXtermDomColorNormalization) {
          this.installColorObserver();
          normalizeXtermDomColors(this.terminalElement);
        }
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
        schedulePaneFit(this, { forceBackendResize, ensureStarted: true });
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
          const dimensions = visibleTerminalDimensions(this.term);
          const proposed = dimensions
            ? {
                cols: Math.ceil(rect.width / dimensions.css.cell.width),
                rows: Math.ceil(rect.height / dimensions.css.cell.height),
              }
            : typeof this.fitAddon.proposeDimensions === 'function'
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
        cancelScheduledPaneFit(this.id);
        this.pendingFitFrame = null;
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
        this.clearOutputFlushTimer();
        this.pendingForceBackendResize = false;
      }

      clearOutputFlushTimer() {
        if (this.outputFlushTimer !== null) {
          clearTimeout(this.outputFlushTimer);
          this.outputFlushTimer = null;
        }
      }

      clearTerminalWriteQueue() {
        this.clearOutputFlushTimer();
        if (this.pendingReplayFrame !== null) {
          cancelAnimationFrame(this.pendingReplayFrame);
          this.pendingReplayFrame = null;
        }
        this.pendingReplayChunks = [];
        this.pendingReplayStart = 0;
        this.outputQueue = [];
        this.outputQueueBytes = 0;
        this.outputWriteInFlight = false;
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
      }

      syncControls(paneCount) {
        const closeHidden = paneCount <= 1;
        const addHidden = paneCount >= maxPanesPerTab;
        const cloneHidden = addHidden || !this.cwd;
        this.closeButton.hidden = closeHidden;
        this.cloneButton.hidden = cloneHidden;
        this.addButton.hidden = addHidden;
        this.closeButton.style.display = closeHidden ? 'none' : '';
        this.cloneButton.style.display = cloneHidden ? 'none' : '';
        this.addButton.style.display = addHidden ? 'none' : '';
      }

      syncLabel(index) {
        this.paneIndex = index;
        this.cwdElement.textContent = this.label();
      }

      focus(notify) {
        activePaneId = this.id;
        const tab = tabs.get(this.tabId);
        const paneIds = tab ? tab.panes : [];
        const canShowActive = paneIds.length > 1;
        for (const paneId of paneIds) {
          const pane = panes.get(paneId);
          if (pane) {
            pane.setActive(canShowActive && pane.id === this.id);
          }
        }
        if (this.tabId === activeTabId) {
          this.activate();
        }
        requestAnimationFrame(() => this.term.focus());
        if (notify) {
          post({ type: 'selectPane', paneId: this.id });
        }
      }

      reset(event) {
        pendingOutput.delete(this.id);
        this.started = false;
        this.starting = false;
        this.clearPaneTimers();
        this.clearTerminalWriteQueue();
        this.exited = false;
        this.cwd = String((event && event.cwd) || '').trim();
        this.selection = '';
        this.cwdElement.textContent = this.label();
        this.term.reset();
        this.term.clear();
        this.scheduleFitAndStart();
      }

      write(item) {
        if (!this.opened || !this.visible) {
          queuePendingOutput(this.id, item);
          return;
        }
        this.queueTerminalWrite(item);
      }

      queueTerminalWrite(item) {
        item = ensureDecodedOutputItem(item);
        const bytes = outputItemByteLength(item);
        if (bytes <= 0) {
          return;
        }
        this.outputQueue.push(item);
        this.outputQueueBytes += bytes;
        if (this.outputQueueBytes >= terminalWriteBatchMaxBytes) {
          this.flushTerminalWriteQueue();
          return;
        }
        this.scheduleTerminalWriteFlush();
      }

      scheduleTerminalWriteFlush() {
        if (this.outputFlushTimer !== null || this.outputWriteInFlight) {
          return;
        }
        this.outputFlushTimer = setTimeout(() => {
          this.outputFlushTimer = null;
          this.flushTerminalWriteQueue();
        }, terminalWriteBatchDelayMs);
      }

      flushTerminalWriteQueue() {
        if (this.outputWriteInFlight || this.outputQueue.length === 0 || !this.opened) {
          return;
        }
        this.clearOutputFlushTimer();
        const queue = this.outputQueue;
        const totalBytes = this.outputQueueBytes;
        this.outputQueue = [];
        this.outputQueueBytes = 0;
        const chunks = [];
        let decodedBytes = 0;
        for (const item of queue) {
          chunks.push(item.bytes);
          decodedBytes += item.byteLength;
        }
        this.outputWriteInFlight = true;
        this.term.write(concatenateBytes(chunks, decodedBytes || totalBytes), () => {
          this.outputWriteInFlight = false;
          if (this.outputQueue.length > 0) {
            this.flushTerminalWriteQueue();
          }
        });
      }

      flushPendingOutput() {
        const pending = takePendingOutput(this.id);
        if (!pending) {
          return;
        }
        const droppedMessage = pendingOutputDroppedMessage(pending);
        if (droppedMessage) {
          this.term.write(droppedMessage);
        }
        if (pendingChunkCount(pending) > 0) {
          this.pendingReplayChunks.push(...pending.chunks.slice(pending.start || 0));
          this.schedulePendingReplay();
        }
      }

      schedulePendingReplay() {
        if (this.pendingReplayFrame !== null || this.pendingReplayStart >= this.pendingReplayChunks.length) {
          return;
        }
        this.pendingReplayFrame = requestAnimationFrame(() => {
          this.pendingReplayFrame = null;
          this.replayPendingOutputBatch();
        });
      }

      replayPendingOutputBatch() {
        if (!this.opened || !this.visible || this.pendingReplayStart >= this.pendingReplayChunks.length) {
          return;
        }
        let replayedBytes = 0;
        while (this.pendingReplayStart < this.pendingReplayChunks.length && replayedBytes < terminalWriteBatchMaxBytes) {
          const chunk = this.pendingReplayChunks[this.pendingReplayStart];
          this.pendingReplayStart += 1;
          replayedBytes += outputItemByteLength(chunk);
          this.write(chunk);
          if (this.pendingReplayStart > 64 && this.pendingReplayStart * 2 > this.pendingReplayChunks.length) {
            this.pendingReplayChunks = this.pendingReplayChunks.slice(this.pendingReplayStart);
            this.pendingReplayStart = 0;
          }
        }
        if (this.pendingReplayStart >= this.pendingReplayChunks.length) {
          this.pendingReplayChunks = [];
          this.pendingReplayStart = 0;
        }
        if (this.pendingReplayStart < this.pendingReplayChunks.length) {
          this.schedulePendingReplay();
        }
      }

      markExited() {
        this.started = false;
        this.starting = false;
        this.clearPaneTimers();
        this.clearTerminalWriteQueue();
        this.exited = true;
        this.visible = false;
        this.cwdElement.textContent = this.label();
      }

      diagnosticsLine() {
        const buffer = this.term && this.term.buffer && this.term.buffer.normal ? this.term.buffer.normal : null;
        const normalLength = buffer && typeof buffer.length === 'number' ? buffer.length : 'unknown';
        const canvasCount = this.terminalElement.querySelectorAll('canvas').length;
        const spanCount = this.terminalElement.querySelectorAll('.xterm-rows span').length;
        return `pane#${this.id} tab#${this.tabId} opened=${this.opened} visible=${this.visible} started=${this.started} exited=${this.exited} size=${this.term.cols}x${this.term.rows} scrollback=${normalLength} writeQueueBytes=${this.outputQueueBytes} writeQueueChunks=${this.outputQueue.length} canvas=${canvasCount} spans=${spanCount} cwd=${this.cwd || '~'}`;
      }

      label() {
        return `Pane#${this.paneIndex || this.id}`;
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
        this.clearTerminalWriteQueue();
        pendingOutput.delete(this.id);
        this.resizeObserver.disconnect();
        this.term.dispose();
        this.element.remove();
      }
    }
