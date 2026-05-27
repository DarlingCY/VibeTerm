const tabs = new Map();
    const panes = new Map();
    const pendingOutput = new Map();
    const pendingOutputMaxBytes = 1 * 1024 * 1024;
    const pendingOutputGlobalMaxBytes = 3 * 1024 * 1024;
    const terminalWriteBatchDelayMs = 8;
    const terminalWriteBatchMaxBytes = 64 * 1024;
    const enableXtermDomColorNormalization = false;
    const fallbackCols = 120;
    const fallbackRows = 30;
    const pendingPaneFits = new Map();
    let pendingPaneFitFrame = null;
    let pendingPaneFitTimer = null;
    let activeTabId = null;
    let activePaneId = null;
    let maxPanesPerTab = 6;
    let systemFontFamilies = [];
    let systemFontsLoaded = false;
    let systemFontsLoading = false;
    let backendListenerBound = false;

    const titleBar = document.getElementById('titleBar');
    const tabBar = document.getElementById('tabBar');
    const newTabButton = document.getElementById('newTabButton');
    const settingsButton = document.getElementById('settingsButton');
    const settingsPanel = document.getElementById('settingsPanel');
    const terminalFontSelect = document.getElementById('terminalFontSelect');
    const terminalFontSizeInput = document.getElementById('terminalFontSizeInput');
    const appVersionText = document.getElementById('appVersionText');
    const updateStatus = document.getElementById('updateStatus');
    const checkUpdateButton = document.getElementById('checkUpdateButton');
    const workspace = document.getElementById('workspace');
    const statusBar = document.getElementById('statusBar');
    const status = document.getElementById('status');

    const defaultTerminalFont = 'Cascadia Mono, Cascadia Code, Consolas, monospace';
    const defaultTerminalFontSize = 14;
    const minTerminalFontSize = 10;
    const maxTerminalFontSize = 32;
    const terminalSettings = {
      fontFamily: defaultTerminalFont,
      fontSize: defaultTerminalFontSize,
    };
    let appVersion = '';
    let latestUpdate = null;
    let updateCheckInFlight = false;
    let updateInstallInFlight = false;
    let updateButtonMode = 'check';

    function decodedOutputItem(dataBase64) {
      const bytes = bytesFromBase64(dataBase64);
      if (bytes.length <= 0) {
        return null;
      }
      return {
        bytes,
        byteLength: bytes.length,
      };
    }

    function outputByteLengthFromBase64(dataBase64) {
      const value = String(dataBase64 || '');
      if (!value) {
        return 0;
      }
      const padding = value.endsWith('==') ? 2 : value.endsWith('=') ? 1 : 0;
      return Math.max(0, Math.floor(value.length * 3 / 4) - padding);
    }

    function rawOutputItem(dataBase64) {
      const byteLength = outputByteLengthFromBase64(dataBase64);
      return byteLength > 0 ? { dataBase64, byteLength } : null;
    }

    function ensureDecodedOutputItem(item) {
      if (!item || item.bytes) {
        return item || null;
      }
      return decodedOutputItem(item.dataBase64);
    }

    function outputItemByteLength(item) {
      return item && typeof item.byteLength === 'number' ? item.byteLength : 0;
    }

    function compactPendingChunks(pending) {
      if (pending.start > 0 && (pending.start > 64 || pending.start * 2 > pending.chunks.length)) {
        pending.chunks = pending.chunks.slice(pending.start);
        pending.start = 0;
      }
    }

    function dropOldestPendingChunk(pending) {
      if (!pending || pending.start >= pending.chunks.length) {
        return 0;
      }
      const dropped = pending.chunks[pending.start];
      pending.start += 1;
      const droppedBytes = outputItemByteLength(dropped);
      pending.bytes = Math.max(0, pending.bytes - droppedBytes);
      pending.droppedBytes += droppedBytes;
      compactPendingChunks(pending);
      return droppedBytes;
    }

    function pendingChunkCount(pending) {
      return pending && pending.chunks ? Math.max(0, pending.chunks.length - (pending.start || 0)) : 0;
    }

    function queuePendingOutput(paneId, item) {
      const bytes = outputItemByteLength(item);
      if (bytes <= 0) {
        return;
      }
      let pending = pendingOutput.get(paneId);
      if (!pending) {
        pending = { chunks: [], start: 0, bytes: 0, droppedBytes: 0 };
        pendingOutput.set(paneId, pending);
      }
      pending.chunks.push(item);
      pending.bytes += bytes;
      while (pending.bytes > pendingOutputMaxBytes && pendingChunkCount(pending) > 0) {
        dropOldestPendingChunk(pending);
      }
      capPendingOutputGlobal();
    }

    function capPendingOutputGlobal() {
      let totalBytes = 0;
      for (const pending of pendingOutput.values()) {
        totalBytes += pending.bytes || 0;
      }
      while (totalBytes > pendingOutputGlobalMaxBytes) {
        let droppedAny = false;
        for (const [paneId, pending] of pendingOutput.entries()) {
          if (pendingChunkCount(pending) === 0) {
            if ((pending.bytes || 0) <= 0) {
              pendingOutput.delete(paneId);
            }
            continue;
          }
          const droppedBytes = dropOldestPendingChunk(pending);
          totalBytes = Math.max(0, totalBytes - droppedBytes);
          droppedAny = true;
          break;
        }
        if (!droppedAny) {
          break;
        }
      }
    }

    function takePendingOutput(paneId) {
      const pending = pendingOutput.get(paneId) || null;
      pendingOutput.delete(paneId);
      return pending;
    }

    function pendingOutputDroppedMessage(pending) {
      if (!pending || !pending.droppedBytes) {
        return '';
      }
      const mib = pending.droppedBytes / (1024 * 1024);
      const amount = mib >= 1 ? `${mib.toFixed(1)} MiB` : `${Math.ceil(pending.droppedBytes / 1024)} KiB`;
      return `\r\n[VibeTerm dropped ${amount} of buffered output while this pane was inactive]\r\n`;
    }

    function pendingOutputSummary() {
      let totalBytes = 0;
      let totalDroppedBytes = 0;
      const entries = [];
      for (const [paneId, pending] of pendingOutput.entries()) {
        totalBytes += pending.bytes || 0;
        totalDroppedBytes += pending.droppedBytes || 0;
        entries.push(`pane#${paneId}:bytes=${pending.bytes || 0},dropped=${pending.droppedBytes || 0},chunks=${pendingChunkCount(pending)}`);
      }
      return `pendingOutput panes=${pendingOutput.size} bytes=${totalBytes} dropped=${totalDroppedBytes}${entries.length ? ` [${entries.join('; ')}]` : ''}`;
    }

    function runScheduledPaneFits() {
      pendingPaneFitFrame = null;
      if (pendingPaneFitTimer !== null) {
        clearTimeout(pendingPaneFitTimer);
        pendingPaneFitTimer = null;
      }
      const fits = Array.from(pendingPaneFits.values());
      pendingPaneFits.clear();
      for (const item of fits) {
        const pane = item.pane;
        if (!pane || pane.exited || !pane.visible) {
          continue;
        }
        const size = pane.fit({ forceBackendResize: item.forceBackendResize });
        if (item.ensureStarted && (size || !item.forceBackendResize)) {
          pane.ensureStarted();
        }
      }
    }

    function schedulePaneFit(pane, { forceBackendResize = false, ensureStarted = true } = {}) {
      if (!pane) {
        return;
      }
      const existing = pendingPaneFits.get(pane.id);
      pendingPaneFits.set(pane.id, {
        pane,
        forceBackendResize: forceBackendResize || Boolean(existing && existing.forceBackendResize),
        ensureStarted: ensureStarted || Boolean(existing && existing.ensureStarted),
      });
      if (pendingPaneFitFrame === null) {
        pendingPaneFitFrame = requestAnimationFrame(() => {
          pendingPaneFitFrame = requestAnimationFrame(runScheduledPaneFits);
        });
      }
      if (pendingPaneFitTimer === null) {
        pendingPaneFitTimer = setTimeout(runScheduledPaneFits, 160);
      }
    }

    function scheduleVisiblePaneFits({ forceBackendResize = false, ensureStarted = true } = {}) {
      const tab = tabs.get(activeTabId);
      if (!tab) {
        return;
      }
      for (const paneId of tab.panes) {
        schedulePaneFit(panes.get(paneId), { forceBackendResize, ensureStarted });
      }
    }

    function cancelScheduledPaneFit(paneId) {
      pendingPaneFits.delete(paneId);
    }
