const tabs = new Map();
    const panes = new Map();
    const pendingOutput = new Map();
    const pendingOutputMaxBytes = 2 * 1024 * 1024;
    const terminalWriteBatchDelayMs = 8;
    const terminalWriteBatchMaxBytes = 64 * 1024;
    const enableXtermDomColorNormalization = false;
    const fallbackCols = 120;
    const fallbackRows = 30;
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

    function estimatedDecodedByteLength(dataBase64) {
      const value = String(dataBase64 || '');
      if (!value) {
        return 0;
      }
      const padding = value.endsWith('==') ? 2 : value.endsWith('=') ? 1 : 0;
      return Math.max(0, Math.floor(value.length * 3 / 4) - padding);
    }

    function queuePendingOutput(paneId, dataBase64) {
      const bytes = estimatedDecodedByteLength(dataBase64);
      if (bytes <= 0) {
        return;
      }
      let pending = pendingOutput.get(paneId);
      if (!pending) {
        pending = { chunks: [], bytes: 0, droppedBytes: 0 };
        pendingOutput.set(paneId, pending);
      }
      pending.chunks.push(dataBase64);
      pending.bytes += bytes;
      while (pending.bytes > pendingOutputMaxBytes && pending.chunks.length > 0) {
        const dropped = pending.chunks.shift();
        const droppedBytes = estimatedDecodedByteLength(dropped);
        pending.bytes = Math.max(0, pending.bytes - droppedBytes);
        pending.droppedBytes += droppedBytes;
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
