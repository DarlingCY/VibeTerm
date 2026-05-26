    (function () {
      if (typeof window.queueMicrotask !== 'function') {
        window.queueMicrotask = callback => Promise.resolve()
          .then(callback)
          .catch(error => setTimeout(() => { throw error; }, 0));
      }

      try {
        if (!navigator.platform) {
          Object.defineProperty(navigator, 'platform', {
            value: (navigator.userAgentData && navigator.userAgentData.platform) || 'Win32',
            configurable: true,
          });
        }
      } catch (error) {}

      if (typeof window.ResizeObserver !== 'function') {
        window.ResizeObserver = class {
          constructor(callback) {
            this.callback = callback;
            this.entries = new Map();
            this.timer = null;
          }

          observe(element) {
            this.entries.set(element, { width: 0, height: 0 });
            if (this.timer === null) {
              this.timer = setInterval(() => this.check(), 160);
            }
            this.check();
          }

          unobserve(element) {
            this.entries.delete(element);
            if (this.entries.size === 0) {
              this.disconnect();
            }
          }

          disconnect() {
            if (this.timer !== null) {
              clearInterval(this.timer);
              this.timer = null;
            }
            this.entries.clear();
          }

          check() {
            const changed = [];
            for (const [element, previous] of this.entries) {
              const rect = element.getBoundingClientRect();
              if (rect.width !== previous.width || rect.height !== previous.height) {
                this.entries.set(element, { width: rect.width, height: rect.height });
                changed.push({ target: element, contentRect: rect });
              }
            }
            if (changed.length > 0) {
              this.callback(changed, this);
            }
          }
        };
      }

      window.vibeTermInvoke = function (payload) {
        const invoke = window.__TAURI__ && window.__TAURI__.core && typeof window.__TAURI__.core.invoke === 'function'
          ? window.__TAURI__.core.invoke.bind(window.__TAURI__.core)
          : null;
        if (!invoke) {
          return Promise.resolve(null);
        }
        return invoke('frontend_message', { message: JSON.stringify(payload) });
      };

      function showStartupError(message) {
        const status = document.getElementById('status');
        const statusBar = document.getElementById('statusBar');
        if (status) status.textContent = message;
        if (statusBar) statusBar.hidden = false;
      }

      function reportStartupError(payload) {
        if (typeof window.vibeTermInvoke === 'function') {
          window.vibeTermInvoke({
            type: 'frontendError',
            message: payload.message || 'unknown error',
            source: payload.source || '',
            line: payload.line || 0,
            column: payload.column || 0,
            stack: payload.stack || '',
          }).catch(() => {});
        }
      }

      window.addEventListener('error', (event) => {
        const message = event.message || 'unknown error';
        showStartupError(`前端脚本错误：${message}`);
        reportStartupError({
          message,
          source: event.filename,
          line: event.lineno,
          column: event.colno,
          stack: event.error && event.error.stack ? String(event.error.stack) : '',
        });
      });

      window.addEventListener('unhandledrejection', (event) => {
        const reason = event.reason && (event.reason.message || event.reason);
        const message = reason || 'unknown error';
        showStartupError(`前端异步错误：${message}`);
        reportStartupError({
          message,
          stack: event.reason && event.reason.stack ? String(event.reason.stack) : '',
        });
      });
    })();
