const tabs = new Map();
    const panes = new Map();
    const pendingOutput = new Map();
    const fallbackCols = 120;
    const fallbackRows = 30;
    let activeTabId = null;
    let activePaneId = null;
    let maxPanesPerTab = 6;
    let systemFontFamilies = [];
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

