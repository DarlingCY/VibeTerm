function syncTabCloseButtons() {
      const closable = tabs.size > 1;
      for (const tab of tabs.values()) {
        tab.close.hidden = !closable;
        tab.close.style.display = closable ? '' : 'none';
      }
    }

    function createTab(event) {
      if (tabs.has(event.tabId)) {
        return;
      }

      const tabId = event.tabId;
      const button = document.createElement('button');
      const title = document.createElement('span');
      title.className = 'tab-title';
      title.textContent = event.title;
      const close = document.createElement('span');
      close.className = 'tab-close';
      close.textContent = '×';
      close.title = '关闭标签页';
      close.addEventListener('pointerdown', event => {
        event.preventDefault();
        event.stopPropagation();
      }, true);
      close.addEventListener('click', event => {
        event.preventDefault();
        event.stopPropagation();
        post({ type: 'closeTab', tabId });
      });
      button.append(title, close);
      button.addEventListener('click', () => post({ type: 'selectTab', tabId }));
      tabBar.appendChild(button);

      const content = document.createElement('div');
      content.className = 'tab-content';
      workspace.appendChild(content);

      tabs.set(event.tabId, {
        id: event.tabId,
        title: event.title,
        button,
        close,
        content,
        panes: [],
      });
      syncTabCloseButtons();
    }

    function paneLayoutSlot(count, index) {
      if (count <= 3) {
        return { column: index + 1, row: 1, rowSpan: 1 };
      }
      const slots = [
        { column: 1, row: 1, rowSpan: 1 },
        { column: 2, row: 1, rowSpan: count === 4 ? 2 : 1 },
        { column: 3, row: 1, rowSpan: count <= 5 ? 2 : 1 },
        { column: 1, row: 2, rowSpan: 1 },
        { column: 2, row: 2, rowSpan: 1 },
        { column: 3, row: 2, rowSpan: 1 },
      ];
      return slots[index];
    }

    function layoutTab(tab) {
      const views = tab.panes.map(id => panes.get(id)).filter(Boolean);
      const count = views.length;
      const columns = count <= 3 ? Math.max(1, count) : 3;
      const rows = count <= 3 ? 1 : 2;

      tab.content.style.gap = '0';
      tab.content.style.gridTemplateColumns = `repeat(${columns}, minmax(0, 1fr))`;
      tab.content.style.gridTemplateRows = `repeat(${rows}, minmax(0, 1fr))`;

      views.forEach((pane, index) => {
        const slot = paneLayoutSlot(count, index);
        pane.element.style.gridColumn = `${slot.column} / span 1`;
        pane.element.style.gridRow = `${slot.row} / span ${slot.rowSpan}`;
        pane.setActive(count > 1 && pane.id === activePaneId);
        pane.syncControls(count);
      });

      if (tab.id === activeTabId) {
        requestAnimationFrame(() => {
          for (const pane of views) {
            pane.scheduleFitAndStart();
          }
        });
      }
    }

    function fitVisiblePanes() {
      const tab = tabs.get(activeTabId);
      if (!tab) {
        return;
      }
      for (const paneId of tab.panes) {
        const pane = panes.get(paneId);
        if (pane) {
          pane.fit();
          pane.ensureStarted();
        }
      }
    }

    function selectTab(tabId) {
      const tab = tabs.get(tabId);
      if (!tab) {
        return;
      }
      activeTabId = tabId;
      for (const existing of tabs.values()) {
        const active = existing.id === tabId;
        existing.button.classList.toggle('active', active);
        existing.content.classList.toggle('active', active);
      }
      layoutTab(tab);
      for (const paneId of tab.panes) {
        const pane = panes.get(paneId);
        if (pane) {
          pane.activate();
        }
      }
    }

    function createPane(event) {
      const tab = tabs.get(event.tabId);
      if (!tab || panes.has(event.paneId)) {
        return;
      }
      const pane = new PaneView(event, tab);
      panes.set(pane.id, pane);
      tab.panes.push(pane.id);
      pane.attach();
      if (event.tabId === activeTabId) {
        pane.activate();
      }
      layoutTab(tab);
      if (event.active) {
        selectPane(pane.id);
      }
    }

    function selectPane(paneId) {
      const pane = panes.get(paneId);
      if (!pane) {
        return;
      }
      selectTab(pane.tabId);
      pane.focus(false);
    }

    function resetPane(event) {
      const pane = panes.get(event.paneId);
      if (pane) {
        pane.reset(event);
      }
    }

    function closePane(paneId) {
      const pane = panes.get(paneId);
      if (!pane) {
        return;
      }
      const tab = tabs.get(pane.tabId);
      pane.dispose();
      panes.delete(paneId);
      if (tab) {
        tab.panes = tab.panes.filter(id => id !== paneId);
        layoutTab(tab);
      }
      if (activePaneId === paneId) {
        activePaneId = null;
      }
    }

    function closeTab(tabId) {
      const tab = tabs.get(tabId);
      if (!tab) {
        return;
      }
      for (const paneId of [...tab.panes]) {
        const pane = panes.get(paneId);
        if (pane) {
          pane.dispose();
          panes.delete(paneId);
        }
      }
      tab.button.remove();
      tab.content.remove();
      tabs.delete(tabId);
      if (activeTabId === tabId) {
        activeTabId = null;
      }
      syncTabCloseButtons();
    }

    function writePane(event) {
      const pane = panes.get(event.paneId);
      if (!pane) {
        queuePendingOutput(event.paneId, event.dataBase64);
        return;
      }
      pane.write(event.dataBase64);
    }

    function markExited(event) {
      const pane = panes.get(event.paneId);
      if (pane) {
        pane.markExited();
      }
    }

    function isTitlebarInteractive(target) {
      return Boolean(target.closest('button, #newTabButton, #tabBar, #windowControls'));
    }
