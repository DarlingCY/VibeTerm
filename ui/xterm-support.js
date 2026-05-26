function xtermCssLoaded() {
      const link = document.getElementById('xtermCss');
      if (!link) {
        return false;
      }
      try {
        return Boolean(link.sheet && link.sheet.cssRules && link.sheet.cssRules.length > 0);
      } catch (error) {
        return Boolean(link.sheet);
      }
    }

    function xtermAnsiColor(index) {
      const basic = [
        '#282c34', '#e06c75', '#98c379', '#e5c07b',
        '#61afef', '#c678dd', '#56b6c2', '#abb2bf',
        '#5c6370', '#e06c75', '#98c379', '#e5c07b',
        '#61afef', '#c678dd', '#56b6c2', '#ffffff',
      ];
      if (index < basic.length) {
        return basic[index];
      }
      if (index >= 16 && index <= 231) {
        const value = index - 16;
        const r = Math.floor(value / 36);
        const g = Math.floor((value % 36) / 6);
        const b = value % 6;
        const channel = n => (n === 0 ? 0 : 55 + n * 40);
        return `rgb(${channel(r)}, ${channel(g)}, ${channel(b)})`;
      }
      if (index >= 232 && index <= 255) {
        const level = 8 + (index - 232) * 10;
        return `rgb(${level}, ${level}, ${level})`;
      }
      return '#dcdfe4';
    }

    function installXtermAnsiPaletteFallback() {
      if (document.getElementById('xtermAnsiPaletteFallback')) {
        return;
      }
      const style = document.createElement('style');
      style.id = 'xtermAnsiPaletteFallback';
      let css = '';
      for (let index = 0; index < 256; index += 1) {
        const color = xtermAnsiColor(index);
        css += `.xterm .xterm-fg-${index}{color:${color} !important;}`;
        css += `.xterm .xterm-bg-${index}{background-color:${color} !important;}`;
      }
      style.textContent = css;
      document.head.appendChild(style);
    }

    function cssColorToHex(color) {
      const value = String(color || '').trim();
      const hex = value.match(/^#([0-9a-f]{3}|[0-9a-f]{6})$/i);
      if (hex) {
        const raw = hex[1].toLowerCase();
        return raw.length === 3
          ? raw.split('').map(ch => ch + ch).join('')
          : raw;
      }
      const rgb = value.match(/^rgba?\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})/i);
      if (rgb) {
        return [rgb[1], rgb[2], rgb[3]]
          .map(part => Math.max(0, Math.min(255, Number(part))).toString(16).padStart(2, '0'))
          .join('');
      }
      return '';
    }

    function inlineStyleColor(styleText, property) {
      const pattern = new RegExp(`${property}\\s*:\\s*([^;]+)`, 'i');
      const match = String(styleText || '').match(pattern);
      return match ? match[1].trim() : '';
    }

    const truecolorStyleOrder = [];
    const maxTruecolorStyles = 256;

    function ensureTruecolorClass(kind, color) {
      const hex = cssColorToHex(color);
      if (!hex) {
        return '';
      }
      const className = `vibeterm-${kind}-${hex}`;
      const styleId = `vibeterm-${kind}-style-${hex}`;
      if (!document.getElementById(styleId)) {
        if (truecolorStyleOrder.length >= maxTruecolorStyles) {
          const oldStyleId = truecolorStyleOrder.shift();
          const oldStyle = oldStyleId ? document.getElementById(oldStyleId) : null;
          if (oldStyle) {
            oldStyle.remove();
          }
        }
        const style = document.createElement('style');
        style.id = styleId;
        const property = kind === 'bg' ? 'background-color' : 'color';
        style.textContent = `.xterm .${className}{${property}:#${hex} !important;}`;
        document.head.appendChild(style);
        truecolorStyleOrder.push(styleId);
      }
      return className;
    }

    function normalizeXtermDomColors(root) {
      if (!root) {
        return;
      }
      const spans = root.matches && root.matches('.xterm-rows span')
        ? [root]
        : Array.from(root.querySelectorAll ? root.querySelectorAll('.xterm-rows span') : []);

      for (const span of spans) {
        const className = typeof span.className === 'string' ? span.className : '';
        const fgMatch = className.match(/(?:^|\s)xterm-fg-(\d+)(?:\s|$)/);
        const bgMatch = className.match(/(?:^|\s)xterm-bg-(\d+)(?:\s|$)/);

        if (fgMatch) {
          span.style.setProperty('color', xtermAnsiColor(Number(fgMatch[1])), 'important');
        } else {
          const inlineColor = inlineStyleColor(span.getAttribute('style'), 'color') || span.style.color;
          if (inlineColor) {
            const truecolorClass = ensureTruecolorClass('fg', inlineColor);
            if (truecolorClass && !span.classList.contains(truecolorClass)) {
              span.classList.add(truecolorClass);
            }
            span.style.setProperty('color', inlineColor, 'important');
          }
        }

        if (bgMatch) {
          span.style.setProperty('background-color', xtermAnsiColor(Number(bgMatch[1])), 'important');
        } else {
          const inlineBackground = inlineStyleColor(span.getAttribute('style'), 'background-color') || span.style.backgroundColor;
          if (inlineBackground) {
            const truecolorClass = ensureTruecolorClass('bg', inlineBackground);
            if (truecolorClass && !span.classList.contains(truecolorClass)) {
              span.classList.add(truecolorClass);
            }
            span.style.setProperty('background-color', inlineBackground, 'important');
          }
        }

        if (className.includes('xterm-bold')) {
          span.style.setProperty('font-weight', '700', 'important');
        }
      }
    }

    function spanInfoFor(spans, label) {
      const span = spans.find(item => item.textContent.includes(label));
      if (!span) {
        return 'no-span';
      }
      const style = getComputedStyle(span);
      return `${span.className || 'no-class'}|color=${style.color}|weight=${style.fontWeight}|dec=${style.textDecorationLine || 'none'}|style=${span.getAttribute('style') || ''}`;
    }

    function copyDiagnosticText(text) {
      if (navigator.clipboard && typeof navigator.clipboard.writeText === 'function') {
        navigator.clipboard.writeText(text).catch(() => {});
      }
      post({ type: 'copyToClipboard', text });
    }

    function collectAnsiSelfTestDiagnostics(pane) {
      const xterm = pane.terminalElement.querySelector('.xterm');
      const accessibility = pane.terminalElement.querySelector('.xterm-accessibility');
      const canvasCount = pane.terminalElement.querySelectorAll('.xterm-screen canvas').length;
      const rowsCount = pane.terminalElement.querySelectorAll('.xterm-rows > div').length;
      const accessibilityColor = accessibility ? getComputedStyle(accessibility).color : 'missing';
      const spans = Array.from(pane.terminalElement.querySelectorAll('.xterm-rows span'));
      const redInfo = spanInfoFor(spans, 'RED');
      const ansi256Info = spanInfoFor(spans, 'ANSI256');
      const truecolorInfo = spanInfoFor(spans, 'TRUECOLOR');
      const lastSpanInfo = spans.slice(-8).map((span, index) => {
        const style = getComputedStyle(span);
        return `${index}:${JSON.stringify(span.textContent)}|class=${span.className || 'no-class'}|color=${style.color}|weight=${style.fontWeight}|style=${span.getAttribute('style') || ''}`;
      }).join(' || ');
      const canvas2d = (() => {
        try {
          return Boolean(document.createElement('canvas').getContext('2d'));
        } catch (error) {
          return false;
        }
      })();
      const renderer = pane.term.options && pane.term.options.rendererType ? pane.term.options.rendererType : 'unknown';
      return `renderer=${renderer}; canvas2d=${canvas2d}; xterm.css=${xtermCssLoaded() ? 'loaded' : 'NOT loaded'}; canvas=${canvasCount}; rows=${rowsCount}; spans=${spans.length}; red=${redInfo}; ansi256=${ansi256Info}; truecolor=${truecolorInfo}; accessibilityColor=${accessibilityColor}; xterm=${Boolean(xterm)}; lastSpans=${lastSpanInfo}`;
    }

    function writeAnsiSelfTest() {
      const pane = activePane();
      if (!pane || !pane.opened) {
        setStatus('ANSI 自检失败：没有可用终端 Pane');
        return false;
      }
      installXtermAnsiPaletteFallback();
      const testText = '\r\n\x1b[31mRED\x1b[0m \x1b[1mBOLD\x1b[0m \x1b[4mUNDERLINE\x1b[0m \x1b[38;5;202mANSI256\x1b[0m \x1b[38;2;224;108;117mTRUECOLOR\x1b[0m\r\n┌─ frontend ansi self test ─┐\r\n';
      pane.term.write(testText, () => {
        requestAnimationFrame(() => {
          setTimeout(() => {
            normalizeXtermDomColors(pane.terminalElement);
            const diagnostics = collectAnsiSelfTestDiagnostics(pane);
            setStatus(`ANSI 自检诊断已复制；${diagnostics}`);
            copyDiagnosticText(diagnostics);
            pane.term.write(`\r\n[VibeTerm ANSI diagnostics copied]\r\n${diagnostics}\r\n`);
          }, 40);
        });
      });
      return true;
    }

    function collectPerformanceDiagnostics() {
      const memory = performance && performance.memory
        ? `jsHeap=${performance.memory.usedJSHeapSize}/${performance.memory.totalJSHeapSize}/${performance.memory.jsHeapSizeLimit}`
        : 'jsHeap=unavailable';
      const paneLines = Array.from(panes.values()).map(pane => pane.diagnosticsLine());
      const text = [
        `frontend tabs=${tabs.size} panes=${panes.size} activeTab=${activeTabId} activePane=${activePaneId}`,
        `domNodes=${document.getElementsByTagName('*').length} styles=${document.querySelectorAll('style').length} canvases=${document.querySelectorAll('canvas').length}`,
        pendingOutputSummary(),
        memory,
        ...paneLines,
      ].join('\n');
      post({ type: 'diagnostics', frontend: text });
      copyDiagnosticText(text);
      setStatus('正在收集诊断信息...');
      return true;
    }

    function stopKeyboardShortcut(event) {
      event.preventDefault();
      event.stopPropagation();
      if (typeof event.stopImmediatePropagation === 'function') {
        event.stopImmediatePropagation();
      }
    }

    function shortcutKeyMatches(event, key) {
      return event.key.toLowerCase() === key || event.code === `Key${key.toUpperCase()}`;
    }

    function paneFromEventTarget(target) {
      const paneElement = target && target.closest ? target.closest('.pane') : null;
      if (!paneElement) {
        return activePane();
      }
      return panes.get(Number(paneElement.dataset.paneId)) || activePane();
    }

    function handleTerminalClipboardShortcut(event, pane) {
      if (event.type !== 'keydown' || !event.ctrlKey || event.altKey) {
        return true;
      }
      if (settingsPanel.contains(event.target)) {
        return true;
      }
      const targetPane = pane || paneFromEventTarget(event.target);
      if (shortcutKeyMatches(event, 'c')) {
        stopKeyboardShortcut(event);
        copyPaneSelection(targetPane);
        return false;
      }
      if (shortcutKeyMatches(event, 'v')) {
        stopKeyboardShortcut(event);
        pasteIntoPane(targetPane);
        return false;
      }
      return true;
    }

    function bindUi() {
      syncSettingsControls();
      settingsButton.addEventListener('click', event => {
        event.stopPropagation();
        setSettingsOpen(settingsPanel.hidden);
      });
      terminalFontSelect.addEventListener('change', () => {
        applyTerminalAppearance({ fontFamily: terminalFontSelect.value });
      });
      terminalFontSizeInput.addEventListener('change', () => {
        applyTerminalAppearance({ fontSize: terminalFontSizeInput.value });
      });
      checkUpdateButton.addEventListener('click', handleUpdateButtonClick);
      for (const input of [terminalFontSelect, terminalFontSizeInput]) {
        input.addEventListener('keydown', event => {
          if (event.key === 'Escape') {
            setSettingsOpen(false);
          }
        });
      }
      document.addEventListener('pointerdown', event => {
        if (!settingsPanel.hidden && !settingsPanel.contains(event.target) && event.target !== settingsButton) {
          setSettingsOpen(false);
        }
      });
      document.getElementById('windowMinimize').addEventListener('click', () => post({ type: 'minimizeWindow' }));
      document.getElementById('windowMaximize').addEventListener('click', () => post({ type: 'toggleMaximizeWindow' }));
      document.getElementById('windowClose').addEventListener('click', () => post({ type: 'closeWindow' }));
      titleBar.addEventListener('pointerdown', event => {
        if (event.button !== 0 || event.detail > 1 || isTitlebarInteractive(event.target)) {
          return;
        }
        post({ type: 'dragWindow' });
      });
      titleBar.addEventListener('dblclick', event => {
        if (!isTitlebarInteractive(event.target)) {
          post({ type: 'toggleMaximizeWindow' });
        }
      });
      let lastNewTabRequest = 0;
      const requestNewTab = event => {
        event.preventDefault();
        event.stopPropagation();
        const now = Date.now();
        if (now - lastNewTabRequest < 250) {
          return;
        }
        lastNewTabRequest = now;
        post({ type: 'newTab' });
      };
      newTabButton.addEventListener('pointerdown', requestNewTab, true);
      newTabButton.addEventListener('mousedown', requestNewTab, true);
      newTabButton.addEventListener('click', requestNewTab, true);
      newTabButton.addEventListener('keydown', event => {
        if (event.key === 'Enter' || event.key === ' ') {
          requestNewTab(event);
        }
      }, true);
      document.addEventListener('keydown', event => {
        if (event.ctrlKey && event.shiftKey && event.altKey && shortcutKeyMatches(event, 't')) {
          stopKeyboardShortcut(event);
          writeAnsiSelfTest();
          return;
        }
        if (event.ctrlKey && event.shiftKey && event.altKey && shortcutKeyMatches(event, 'm')) {
          stopKeyboardShortcut(event);
          collectPerformanceDiagnostics();
          return;
        }
        handleTerminalClipboardShortcut(event, paneFromEventTarget(event.target));
      }, true);
    }
