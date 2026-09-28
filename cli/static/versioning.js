// versioning.js — Alpine component for the Versioning tab (branches.html.hbs).
// History (activation commits grouped by day), system generations, and the
// server-rendered ZFS data-snapshots card. Diffs and job output open in the
// shared #changes-modal.

window.versioningPage = function versioningPage() {
  var TAB_KEY = 'neo.versioningTab';

  function pad2(n) {
    return n < 10 ? '0' + n : String(n);
  }

  function escapeHtml(s) {
    return String(s == null ? '' : s)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;');
  }

  function toast(msg, type) {
    if (typeof window.neoToast === 'function') window.neoToast(msg, type || 'info');
  }

  /** `activation_20260721-123033` → epoch seconds (local time), else null. */
  function activationNameTime(name) {
    var m = /activation_(\d{4})(\d{2})(\d{2})-(\d{2})(\d{2})(\d{2})/.exec(name || '');
    if (!m) return null;
    return new Date(+m[1], +m[2] - 1, +m[3], +m[4], +m[5], +m[6]).getTime() / 1000;
  }

  /** `2026-09-28 20:37:02` (server local) → epoch seconds, else null. */
  function genDateTime(s) {
    var m = /^(\d{4})-(\d{2})-(\d{2})[ T](\d{2}):(\d{2})(?::(\d{2}))?/.exec(s || '');
    if (!m) return null;
    return new Date(+m[1], +m[2] - 1, +m[3], +m[4], +m[5], +(m[6] || 0)).getTime() / 1000;
  }

  function kindOf(subject) {
    if (/^Activation:/i.test(subject)) return 'activation';
    if (/^Build:/i.test(subject)) return 'build';
    return 'other';
  }

  function titleOf(c, kind) {
    if (kind === 'activation') return 'Activation';
    if (kind === 'build') return 'Build';
    var s = (c.subject || '').replace(/activation_\d{8}-\d{6}/gi, '').replace(/\s+/g, ' ').trim();
    return s || 'Commit';
  }

  /** Output of a POST (activation monitor, alerts) in the shared dialog. */
  function openModal(title, html) {
    var m = document.getElementById('changes-modal');
    var body = document.getElementById('changes-body');
    if (!m || !body) return;
    var h = m.querySelector('h3');
    if (h) h.textContent = title;
    body.innerHTML = html;
    if (typeof htmx !== 'undefined' && htmx.process) htmx.process(body);
    if (!m.open) m.showModal();
  }

  return {
    tab: 'history',
    loading: true,
    error: '',
    all: [], // display entries, newest first (see buildEntries)
    head: '',
    dirty: false,
    showAll: false,
    expanded: null,
    compare: false,
    picks: [],
    services: {}, // commit id → { enabled: [] } | { error }
    gens: [],
    gensLoading: true,
    gensMessage: '',
    gensLimit: 20,
    zfsAvailable: false,
    // Re-render relative times once a minute.
    now: Date.now() / 1000,

    init() {
      try {
        var t = sessionStorage.getItem(TAB_KEY);
        if (t === 'history' || t === 'system' || t === 'data') this.tab = t;
      } catch (e) {}
      var self = this;
      this.loadGraph();
      this.loadGenerations();
      this.loadZfs();
      this._tick = setInterval(function () {
        if (!self.$root.isConnected) return clearInterval(self._tick);
        self.now = Date.now() / 1000;
      }, 60000);
    },

    destroy() {
      clearInterval(this._tick);
    },

    setTab(t) {
      this.tab = t;
      try {
        sessionStorage.setItem(TAB_KEY, t);
      } catch (e) {}
    },

    onEscape() {
      if (document.querySelector('dialog[open]')) return;
      if (this.compare) this.toggleCompare();
      else this.expanded = null;
    },

    // ─── Data ──────────────────────────────────────────────────────────────

    loadGraph() {
      var self = this;
      return fetch('/versioning/graph')
        .then(function (r) {
          return r.json();
        })
        .then(function (data) {
          self.head = data.head || '';
          self.all = self.buildEntries(data.commits || []);
          self.dirty = !!data.dirty;
          self.error = '';
        })
        .catch(function (e) {
          self.error = 'Failed to load history: ' + e;
        })
        .finally(function () {
          self.loading = false;
        });
    },

    loadGenerations() {
      var self = this;
      this.gensLoading = true;
      return fetch('/versioning/generations')
        .then(function (r) {
          return r.json();
        })
        .then(function (data) {
          if (data.unavailable) {
            self.gens = [];
            self.gensMessage = data.message || 'This machine has no NixOS system profile.';
            return;
          }
          self.gensMessage = '';
          self.gens = data.generations || [];
        })
        .catch(function (e) {
          self.gensMessage = String(e);
        })
        .finally(function () {
          self.gensLoading = false;
        });
    },

    /** The ZFS card renders `class="hidden"` when the machine has no restore hook. */
    loadZfs() {
      var self = this;
      fetch('/versioning/zfs', { headers: { 'HX-Request': 'true' } })
        .then(function (r) {
          return r.text();
        })
        .then(function (html) {
          var host = self.$refs.zfsHost;
          if (!host) return;
          host.innerHTML = html;
          var card = host.firstElementChild;
          self.zfsAvailable = !!card && !card.classList.contains('hidden');
          if (!self.zfsAvailable && self.tab === 'data') self.tab = 'history';
          if (typeof htmx !== 'undefined' && htmx.process) htmx.process(host);
        })
        .catch(function () {
          self.zfsAvailable = false;
        });
    },

    loadServices(id) {
      if (!id || id in this.services) return;
      var self = this;
      this.services[id] = null;
      fetch('/versioning/commit/' + encodeURIComponent(id) + '/services')
        .then(function (r) {
          return r.json();
        })
        .then(function (data) {
          self.services[id] = data.error ? { error: data.error } : { enabled: data.enabled || [] };
        })
        .catch(function (e) {
          self.services[id] = { error: String(e) };
        });
    },

    svc(id) {
      return this.services[id] || null;
    },

    // ─── Derived ───────────────────────────────────────────────────────────

    allEntries() {
      return this.all;
    },

    /** Commits as display entries, newest first; `prevId` = previous activation. */
    buildEntries(commits) {
      var head = this.head;
      var list = commits.map(function (c) {
        var kind = kindOf(c.subject || '');
        var branchTime = null;
        (c.branches || []).forEach(function (b) {
          var t = activationNameTime(b);
          if (t && (!branchTime || t > branchTime)) branchTime = t;
        });
        return {
          id: c.id,
          short: c.shortId || (c.id || '').slice(0, 7),
          kind: kind,
          title: titleOf(c, kind),
          time: branchTime || c.timestamp || 0,
          gen: c.generation || null,
          isHead: c.id === head,
          isTip: !!(c.branches && c.branches.length),
          parent: (c.parents || [])[0] || null,
        };
      });
      list.sort(function (a, b) {
        return b.time - a.time;
      });
      // Changes are measured against the next older activation (or git parent).
      for (var i = 0; i < list.length; i++) {
        var prev = null;
        for (var j = i + 1; j < list.length; j++) {
          if (list[j].kind === 'activation') {
            prev = list[j].id;
            break;
          }
        }
        list[i].prevId = prev || list[i].parent;
      }
      return list;
    },

    entries() {
      var all = this.allEntries();
      if (this.showAll) return all;
      return all.filter(function (e) {
        return e.kind === 'activation' || e.isHead || e.isTip;
      });
    },

    activationCount() {
      return this.allEntries().filter(function (e) {
        return e.kind === 'activation';
      }).length || '';
    },

    groups() {
      var out = [];
      var byKey = {};
      var self = this;
      this.entries().forEach(function (e) {
        var d = new Date(e.time * 1000);
        var key = d.getFullYear() + '-' + d.getMonth() + '-' + d.getDate();
        if (!byKey[key]) {
          byKey[key] = { key: key, label: self.dayLabel(d), items: [] };
          out.push(byKey[key]);
        }
        byKey[key].items.push(e);
      });
      return out;
    },

    headEntry() {
      var head = this.head;
      return (
        this.allEntries().find(function (e) {
          return e.id === head;
        }) || null
      );
    },

    runningGen() {
      return (
        this.gens.find(function (g) {
          return g.isRunning;
        }) || null
      );
    },

    bootGen() {
      return (
        this.gens.find(function (g) {
          return g.isCurrent;
        }) || null
      );
    },

    commitForGen(n) {
      return (
        this.allEntries().find(function (e) {
          return e.gen === n && e.kind === 'activation';
        }) || null
      );
    },

    canSwitchGen(e) {
      if (!e.gen) return false;
      var run = this.runningGen();
      if (run && run.number === e.gen) return false;
      return this.gens.some(function (g) {
        return g.number === e.gen;
      });
    },

    added(e) {
      var cur = this.svc(e.id);
      var prev = this.svc(e.prevId);
      if (!cur || !prev || cur.error || prev.error) return [];
      return cur.enabled.filter(function (s) {
        return prev.enabled.indexOf(s) === -1;
      });
    },

    removed(e) {
      var cur = this.svc(e.id);
      var prev = this.svc(e.prevId);
      if (!cur || !prev || cur.error || prev.error) return [];
      return prev.enabled.filter(function (s) {
        return cur.enabled.indexOf(s) === -1;
      });
    },

    // ─── Formatting ────────────────────────────────────────────────────────

    dayLabel(d) {
      var today = new Date();
      today.setHours(0, 0, 0, 0);
      var day = new Date(d.getTime());
      day.setHours(0, 0, 0, 0);
      var diff = Math.round((today - day) / 86400000);
      if (diff === 0) return 'Today';
      if (diff === 1) return 'Yesterday';
      var opts = { weekday: 'short', month: 'short', day: 'numeric' };
      if (d.getFullYear() !== today.getFullYear()) opts.year = 'numeric';
      return d.toLocaleDateString(undefined, opts);
    },

    clock(ts) {
      if (!ts) return '';
      var d = new Date(ts * 1000);
      return pad2(d.getHours()) + ':' + pad2(d.getMinutes());
    },

    fullTime(ts) {
      if (!ts) return '';
      var d = new Date(ts * 1000);
      return (
        d.getFullYear() + '-' + pad2(d.getMonth() + 1) + '-' + pad2(d.getDate()) +
        ' ' + pad2(d.getHours()) + ':' + pad2(d.getMinutes()) + ':' + pad2(d.getSeconds())
      );
    },

    relTime(ts) {
      if (!ts) return '';
      var s = Math.max(0, this.now - ts);
      if (s < 60) return 'just now';
      var m = Math.floor(s / 60);
      if (m < 60) return m + (m === 1 ? ' minute ago' : ' minutes ago');
      var h = Math.floor(m / 60);
      if (h < 24) return h + (h === 1 ? ' hour ago' : ' hours ago');
      var d = Math.floor(h / 24);
      if (d < 30) return d + (d === 1 ? ' day ago' : ' days ago');
      var mo = Math.floor(d / 30);
      if (mo < 12) return mo + (mo === 1 ? ' month ago' : ' months ago');
      var y = Math.floor(d / 365);
      return y + (y === 1 ? ' year ago' : ' years ago');
    },

    genRel(g) {
      var t = genDateTime(g && g.date);
      return t ? this.relTime(t) : (g && g.date) || '';
    },

    // ─── Interaction ───────────────────────────────────────────────────────

    onRow(e) {
      if (this.compare) {
        this.togglePick(e.id);
        return;
      }
      this.expanded = this.expanded === e.id ? null : e.id;
      if (this.expanded) {
        this.loadServices(e.id);
        if (e.prevId) this.loadServices(e.prevId);
      }
    },

    /** From a generation row: jump to its version in History. */
    openCommit(id) {
      this.setTab('history');
      this.compare = false;
      var e = this.allEntries().find(function (x) {
        return x.id === id;
      });
      if (!e) return;
      if (e.kind !== 'activation' && !e.isTip && !e.isHead) this.showAll = true;
      this.expanded = null;
      this.onRow(e);
      var self = this;
      this.$nextTick(function () {
        var btn = self.$root.querySelector('[aria-expanded="true"]');
        if (btn) btn.scrollIntoView({ block: 'center', behavior: 'smooth' });
      });
    },

    toggleCompare() {
      this.compare = !this.compare;
      this.picks = [];
    },

    togglePick(id) {
      var i = this.picks.indexOf(id);
      if (i >= 0) this.picks.splice(i, 1);
      else if (this.picks.length < 2) this.picks.push(id);
      else this.picks.splice(1, 1, id);
    },

    pickIndex(id) {
      return this.picks.indexOf(id);
    },

    pickLabel(i) {
      var id = this.picks[i];
      var e = this.allEntries().find(function (x) {
        return x.id === id;
      });
      if (!e) return '';
      var d = new Date(e.time * 1000);
      return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' }) + ', ' + this.clock(e.time);
    },

    entryHeading(id, letter) {
      var e = this.allEntries().find(function (x) {
        return x.id === id;
      });
      if (!e) return '<span class="font-mono">' + escapeHtml(id.slice(0, 7)) + '</span>';
      return (
        '<div class="rounded-field border border-base-300 bg-base-200/50 px-3 py-2 min-w-0 flex-1">' +
        '<div class="text-[11px] font-semibold uppercase tracking-wider text-base-content/50">' + letter + '</div>' +
        '<div class="text-sm font-semibold truncate">' + escapeHtml(e.title) +
        (e.gen ? ' <span class="font-normal text-base-content/60">· Generation ' + e.gen + '</span>' : '') +
        '</div>' +
        '<div class="text-xs text-base-content/60"><span>' + escapeHtml(this.fullTime(e.time)) + '</span> · ' +
        '<span class="font-mono">' + escapeHtml(e.short) + '</span></div></div>'
      );
    },

    /** settings.toml diff a → b in the shared dialog. */
    showDiff(a, b, title) {
      var head =
        '<div class="flex flex-col sm:flex-row sm:items-center gap-2 mb-3">' +
        this.entryHeading(a, 'From') +
        '<span class="text-base-content/40 self-center rotate-90 sm:rotate-0" aria-hidden="true">→</span>' +
        this.entryHeading(b, 'To') +
        '</div>';
      openModal(
        title,
        head + '<div class="flex justify-center py-8"><span class="loading loading-spinner loading-sm opacity-50"></span></div>'
      );
      fetch('/versioning/diff?a=' + encodeURIComponent(a) + '&b=' + encodeURIComponent(b))
        .then(function (r) {
          return r.text();
        })
        .then(function (html) {
          var body = document.getElementById('changes-body');
          if (!body) return;
          body.innerHTML = head + html;
          // Service links in the settings summary navigate via hx-get.
          if (typeof htmx !== 'undefined' && htmx.process) htmx.process(body);
        })
        .catch(function (e) {
          var body = document.getElementById('changes-body');
          if (body) body.innerHTML = head + '<div class="text-error text-sm">' + escapeHtml(String(e)) + '</div>';
        });
    },

    reviewPending() {
      openModal('Pending changes', '<div class="flex justify-center py-8"><span class="loading loading-spinner loading-sm opacity-50"></span></div>');
      fetch('/changes/summary')
        .then(function (r) {
          return r.text();
        })
        .then(function (html) {
          var body = document.getElementById('changes-body');
          if (!body) return;
          body.innerHTML = html;
          if (typeof htmx !== 'undefined' && htmx.process) htmx.process(body);
        });
    },

    post(url, title) {
      var self = this;
      return fetch(url, { method: 'POST', headers: { Accept: 'text/html' } })
        .then(function (r) {
          return r.text();
        })
        .then(function (html) {
          openModal(title, html);
          self.loadGraph();
        })
        .catch(function (e) {
          toast(String(e), 'error');
        });
    },

    restore(e) {
      var when = this.fullTime(e.time);
      if (this.dirty) {
        window.alert('You have unapplied changes. Activate or discard them before restoring an earlier version.');
        return;
      }
      var msg =
        'Restore the version from ' + when + '?\n\n' +
        'Your settings are switched back to this version and the server is rebuilt and activated. ' +
        'This takes a few minutes and saves a new version; nothing is deleted.';
      if (!window.confirm(msg)) return;
      this.post('/versioning/activate/' + encodeURIComponent(e.id), 'Restoring version');
    },

    switchGen(n) {
      var msg =
        'Switch the running system to generation ' + n + '?\n\n' +
        'Your settings are not changed. The switch runs in the background and may restart ' +
        'this web UI — wait a moment and reload if the page disconnects.';
      if (!window.confirm(msg)) return;
      var self = this;
      this.post('/versioning/generations/' + n + '/switch', 'Switching to generation ' + n).then(function () {
        toast('Generation switch started', 'info');
        setTimeout(function () {
          if (self.$root.isConnected) self.loadGenerations();
        }, 15000);
      });
    },
  };
};
