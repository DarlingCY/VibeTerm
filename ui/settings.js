function quoteFontFamily(fontFamily) {
      return `"${fontFamily.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
    }

    function appendFontOption(value, label) {
      const option = document.createElement('option');
      option.value = value;
      option.textContent = label;
      terminalFontSelect.appendChild(option);
    }

    function populateFontSelect(fontFamilies) {
      systemFontFamilies = Array.from(new Set((fontFamilies || [])
        .map(fontFamily => String(fontFamily).trim())
        .filter(Boolean)));
      systemFontFamilies.sort((left, right) => left.localeCompare(right, undefined, { sensitivity: 'base' }));

      terminalFontSelect.replaceChildren();
      appendFontOption(defaultTerminalFont, '默认');
      for (const fontFamily of systemFontFamilies) {
        appendFontOption(quoteFontFamily(fontFamily), fontFamily);
      }
      if (!systemFontFamilies.some(fontFamily => fontFamily.toLowerCase() === 'monospace')) {
        appendFontOption('monospace', 'monospace');
      }
      syncFontSelect();
    }

    function loadSystemFontsOnce() {
      if (systemFontsLoaded || systemFontsLoading) {
        return;
      }
      systemFontsLoading = true;
      post({ type: 'loadFontFamilies' });
    }

    function applyLoadedFontFamilies(fontFamilies) {
      systemFontsLoaded = true;
      systemFontsLoading = false;
      populateFontSelect(fontFamilies);
    }

    function syncFontSelect() {
      if (!Array.from(terminalFontSelect.options).some(option => option.value === terminalSettings.fontFamily)) {
        const option = document.createElement('option');
        option.value = terminalSettings.fontFamily;
        option.textContent = '已保存字体';
        terminalFontSelect.appendChild(option);
      }
      terminalFontSelect.value = terminalSettings.fontFamily;
    }

    function normalizeFontSize(value) {
      const parsed = Number.parseInt(value, 10);
      if (Number.isNaN(parsed)) {
        return defaultTerminalFontSize;
      }
      return Math.min(maxTerminalFontSize, Math.max(minTerminalFontSize, parsed));
    }

    function syncSettingsControls() {
      syncFontSelect();
      terminalFontSizeInput.value = String(terminalSettings.fontSize);
    }

    function syncTerminalFontCss() {
      document.documentElement.style.setProperty('--terminal-font-family', terminalSettings.fontFamily);
    }

    function applyFontToTerminalElement(element) {
      element.style.setProperty('font-family', terminalSettings.fontFamily, 'important');
      for (const node of element.querySelectorAll('.xterm, .xterm-screen, .xterm-rows, .xterm-char-measure-element, textarea')) {
        node.style.setProperty('font-family', terminalSettings.fontFamily, 'important');
      }
    }

    function setTerminalFontOption(term) {
      try {
        term.options.fontFamily = terminalSettings.fontFamily;
        term.options.fontSize = terminalSettings.fontSize;
        term.options.letterSpacing = 0;
        term.options.customGlyphs = true;
        term.options.rescaleOverlappingGlyphs = true;
      } catch (error) {}
      if (typeof term.setOption === 'function') {
        try {
          term.setOption('fontFamily', terminalSettings.fontFamily);
          term.setOption('fontSize', terminalSettings.fontSize);
          term.setOption('letterSpacing', 0);
          term.setOption('customGlyphs', true);
          term.setOption('rescaleOverlappingGlyphs', true);
        } catch (error) {}
      }
      if (typeof term.clearTextureAtlas === 'function') {
        try {
          term.clearTextureAtlas();
        } catch (error) {}
      }
      if (typeof term.refresh === 'function' && term.rows > 0) {
        term.refresh(0, term.rows - 1);
      }
    }

    function setSettingsOpen(open) {
      settingsPanel.hidden = !open;
      if (open) {
        loadSystemFontsOnce();
        syncSettingsControls();
        terminalFontSelect.focus();
      }
    }

    function persistSettings() {
      post({
        type: 'updateSettings',
        fontFamily: terminalSettings.fontFamily,
        fontSize: terminalSettings.fontSize,
      });
    }

    function applyTerminalAppearance({ fontFamily = terminalSettings.fontFamily, fontSize = terminalSettings.fontSize } = {}) {
      terminalSettings.fontFamily = String(fontFamily).trim() || defaultTerminalFont;
      terminalSettings.fontSize = normalizeFontSize(fontSize);
      syncTerminalFontCss();
      syncSettingsControls();
      for (const pane of panes.values()) {
        pane.updateFont();
      }
      persistSettings();
      requestAnimationFrame(fitVisiblePanes);
    }
