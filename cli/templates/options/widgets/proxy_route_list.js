// proxyRouteList — attrsOf str mapping a public domain to a plain-http upstream
// (e.g. swag.proxyPass). Each entry renders as a route card:
//   https://<domain>  →  [TLS ends at the proxy]  →  http://<host>:<port>
//
// Editing keys of an attrs map in place is awkward (renaming on every keystroke
// re-keys the x-for and drops focus), so the widget edits an ordered row list in
// uiState[optionName].rows and commits it back to values[optionName] after each
// change. Blank rows are ignored; rows with errors keep the map dirty and make
// validate() return messages, which blocks save() in option_form.js.
//
// Load after registry.js. Mixins use the prl* prefix (proxy_route_list.html.hbs).
(function (root) {
  if (!root.NeoWidgets) {
    throw new Error('registry.js must load before proxy_route_list.js');
  }

  const HOST_LABEL = /^(?!-)[a-z0-9_-]{1,63}(?<!-)$/i;
  const DOMAIN_LABEL = /^(?!-)[a-z0-9-]{1,63}(?<!-)$/i;

  function trim(v) {
    return String(v == null ? '' : v).trim();
  }

  function isIPv4(host) {
    const parts = host.split('.');
    if (parts.length !== 4) return false;
    return parts.every((p) => /^\d{1,3}$/.test(p) && Number(p) <= 255);
  }

  function isHostname(host, requireDot) {
    if (!host || host.length > 253) return false;
    const labels = host.split('.');
    if (requireDot && labels.length < 2) return false;
    return labels.every((l) => HOST_LABEL.test(l));
  }

  function isLoopback(host) {
    const h = host.toLowerCase();
    return h === 'localhost' || h.endsWith('.localhost') || /^127\./.test(h)
      || h === '[::1]' || h === '0.0.0.0';
  }

  /**
   * Split an upstream URL into its parts without the URL API (so "http://a b"
   * and odd ports fail loudly instead of being normalized).
   * Returns null when there is no scheme://.
   */
  function parseUpstream(raw) {
    const s = trim(raw);
    const m = /^([a-z][a-z0-9+.-]*):\/\/(.*)$/i.exec(s);
    if (!m) return null;
    const scheme = m[1].toLowerCase();
    const rest = m[2];
    const cut = rest.search(/[/?#]/);
    const authority = cut >= 0 ? rest.slice(0, cut) : rest;
    const path = cut >= 0 ? rest.slice(cut) : '';
    let host = authority;
    let port = '';
    const v6 = /^(\[[0-9a-f:.]+\])(?::(.*))?$/i.exec(authority);
    if (v6) {
      host = v6[1];
      port = v6[2] !== undefined ? v6[2] : '';
    } else {
      const i = authority.lastIndexOf(':');
      if (i >= 0) {
        host = authority.slice(0, i);
        port = authority.slice(i + 1);
      }
    }
    const hasPort = v6 ? v6[2] !== undefined : authority.includes(':');
    return { scheme, host, port, hasPort, path, userinfo: host.includes('@') };
  }

  /** Strip scheme / path / port from something pasted into the domain field. */
  function cleanDomain(raw) {
    let s = trim(raw).replace(/^[a-z][a-z0-9+.-]*:\/\//i, '');
    const cut = s.search(/[/?#]/);
    if (cut >= 0) s = s.slice(0, cut);
    s = s.replace(/:\d*$/, '').replace(/\.$/, '');
    return s.toLowerCase();
  }

  function issue(level, text, fix) {
    const out = { level, text };
    if (fix) out.fix = fix;
    return out;
  }

  const mixins = {
    prlState(optionName) {
      let st = this.uiState[optionName];
      if (!st || !Array.isArray(st.rows)) {
        st = { rows: [], seq: 0 };
        this.uiState = { ...this.uiState, [optionName]: st };
      }
      return st;
    },

    /** Rebuild rows from the committed map (init / reset / revert). */
    prlLoad(optionName) {
      const map = this.values[optionName];
      const obj = (map && typeof map === 'object' && !Array.isArray(map)) ? map : {};
      let seq = 0;
      const rows = Object.keys(obj).map((domain) => ({
        id: ++seq,
        domain,
        upstream: obj[domain] == null ? '' : String(obj[domain]),
      }));
      this.uiState = { ...this.uiState, [optionName]: { rows, seq } };
      this.values[optionName] = { ...obj };
    },

    prlRows(optionName) {
      return this.prlState(optionName).rows;
    },

    prlRow(optionName, id) {
      return this.prlRows(optionName).find((r) => r.id === id) || null;
    },

    /** Rows → values map. Blank-domain rows are skipped; first duplicate wins. */
    prlCommit(optionName) {
      const out = {};
      const seen = new Set();
      this.prlRows(optionName).forEach((r) => {
        const d = trim(r.domain);
        if (!d) return;
        const k = d.toLowerCase();
        if (seen.has(k)) return;
        seen.add(k);
        out[d] = trim(r.upstream);
      });
      this.values[optionName] = out;
    },

    prlSetRows(optionName, rows, seq) {
      const st = this.prlState(optionName);
      this.uiState = {
        ...this.uiState,
        [optionName]: { rows, seq: seq !== undefined ? seq : st.seq },
      };
      this.prlCommit(optionName);
    },

    prlAdd(optionName) {
      const st = this.prlState(optionName);
      const id = st.seq + 1;
      this.prlSetRows(optionName, [...st.rows, { id, domain: '', upstream: '' }], id);
      return id;
    },

    prlRemove(optionName, id) {
      this.prlSetRows(optionName, this.prlRows(optionName).filter((r) => r.id !== id));
    },

    prlSet(optionName, id, field, value) {
      if (field !== 'domain' && field !== 'upstream') return;
      const rows = this.prlRows(optionName).map((r) => (
        r.id === id ? { ...r, [field]: value == null ? '' : String(value) } : r
      ));
      this.prlSetRows(optionName, rows);
    },

    prlApplyFix(optionName, id, fix) {
      if (!fix || !fix.field) return;
      this.prlSet(optionName, id, fix.field, fix.value);
    },

    prlIsBlank(row) {
      return !trim(row?.domain) && !trim(row?.upstream);
    },

    prlDomainIssues(optionName, row) {
      const out = [];
      const raw = trim(row.domain);
      if (!raw) {
        if (trim(row.upstream)) {
          out.push(issue('error', 'Enter the public domain this route answers on, e.g. octo.example.com.'));
        }
        return out;
      }
      if (/\s/.test(raw)) {
        out.push(issue('error', 'Domains cannot contain spaces.'));
        return out;
      }
      const cleaned = cleanDomain(raw);
      if (/^[a-z][a-z0-9+.-]*:\/\//i.test(raw)) {
        out.push(issue('error', 'Enter just the hostname — no http:// or https://. Visitors always reach it over https.',
          { field: 'domain', value: cleaned, label: `Use ${cleaned}` }));
        return out;
      }
      if (/[/?#]/.test(raw)) {
        out.push(issue('error', 'Enter just the hostname — a route always covers the whole domain, so paths are not allowed.',
          { field: 'domain', value: cleaned, label: `Use ${cleaned}` }));
        return out;
      }
      if (raw.includes(':')) {
        out.push(issue('error', 'No port here — the proxy serves this domain on the standard https port 443.',
          { field: 'domain', value: cleaned, label: `Use ${cleaned}` }));
        return out;
      }
      if (raw.includes('*')) {
        out.push(issue('error', 'Wildcards are not supported. Add one route per domain.'));
        return out;
      }
      const labels = raw.replace(/\.$/, '').split('.');
      const ok = raw.length <= 253 && labels.length >= 2
        && labels.every((l) => DOMAIN_LABEL.test(l)) && !/^\d+$/.test(labels[labels.length - 1]);
      if (!ok) {
        out.push(issue('error', labels.length < 2
          ? 'Use a fully qualified domain such as octo.example.com (a certificate is requested for it).'
          : 'Not a valid hostname. Use letters, digits and hyphens separated by dots.'));
        return out;
      }
      const key = raw.toLowerCase();
      const firstId = this.prlRows(optionName).find((r) => trim(r.domain).toLowerCase() === key)?.id;
      if (firstId !== undefined && firstId !== row.id) {
        out.push(issue('error', 'Duplicate domain — another route already uses it. Each domain can point to one upstream.'));
      }
      if (raw !== key || raw.endsWith('.')) {
        out.push(issue('warning', 'Domains are case-insensitive; prefer lowercase without a trailing dot.',
          { field: 'domain', value: cleaned, label: `Use ${cleaned}` }));
      }
      return out;
    },

    prlUpstreamIssues(optionName, row) {
      const out = [];
      const raw = trim(row.upstream);
      if (!raw) {
        if (trim(row.domain)) {
          out.push(issue('error', 'Enter the backend URL, e.g. http://192.168.1.20:8123.'));
        }
        return out;
      }
      if (/\s/.test(raw)) {
        out.push(issue('error', 'The upstream URL cannot contain spaces.'));
        return out;
      }
      const u = parseUpstream(raw);
      if (!u) {
        out.push(issue('error', 'The upstream must start with http:// followed by host[:port].',
          { field: 'upstream', value: `http://${raw.replace(/^\/+/, '')}`, label: 'Add http://' }));
        return out;
      }
      if (u.scheme !== 'http' && u.scheme !== 'https') {
        out.push(issue('error', `${u.scheme}:// is not supported — the upstream must be an http:// URL.`));
        return out;
      }
      if (u.userinfo) {
        out.push(issue('error', 'Credentials in the URL (user@host) are not supported.'));
        return out;
      }
      const host = u.host;
      const hostOk = /^\[[0-9a-f:.]+\]$/i.test(host) || isIPv4(host)
        || (!/^[\d.]+$/.test(host) && isHostname(host, false));
      if (!host || !hostOk) {
        out.push(issue('error', host ? `“${host}” is not a valid host name or IP address.` : 'The upstream is missing a host.'));
        return out;
      }
      if (u.hasPort) {
        const n = Number(u.port);
        if (!/^\d{1,5}$/.test(u.port) || n < 1 || n > 65535) {
          out.push(issue('error', 'The port must be a number between 1 and 65535.'));
          return out;
        }
      }
      const base = `http://${host}${u.hasPort ? `:${u.port}` : ''}`;
      if (u.scheme === 'https') {
        out.push(issue('warning',
          'No need for https:// here — the proxy already terminates TLS for visitors, and a plain http:// backend on your LAN is the normal setup. Keep https:// only if the backend refuses plain HTTP (its certificate is not verified).',
          { field: 'upstream', value: base, label: `Use ${base}` }));
      }
      if (u.path) {
        out.push(issue('warning',
          `Every request is sent to exactly “${u.path}” — the path replaces the visitor's URL instead of prefixing it. Usually you want only host:port.`,
          { field: 'upstream', value: u.scheme === 'https' ? `https://${host}${u.hasPort ? `:${u.port}` : ''}` : base, label: 'Remove path' }));
      }
      if (isLoopback(host)) {
        out.push(issue('warning',
          `${host} is the proxy container itself, not this server. Use the server's LAN IP or host.docker.internal.`));
      }
      if (!u.hasPort) {
        out.push(issue('info', `No port given — the default ${u.scheme === 'https' ? '443' : '80'} is used. Most apps listen elsewhere (e.g. :8123).`));
      }
      return out;
    },

    /** All issues for one row: [{ level: error|warning|info, field, text, fix? }]. */
    prlIssues(optionName, id) {
      const row = this.prlRow(optionName, id);
      if (!row || this.prlIsBlank(row)) return [];
      return [
        ...this.prlDomainIssues(optionName, row).map((i) => ({ ...i, field: 'domain' })),
        ...this.prlUpstreamIssues(optionName, row).map((i) => ({ ...i, field: 'upstream' })),
      ];
    },

    prlFieldLevel(optionName, id, field) {
      const levels = this.prlIssues(optionName, id).filter((i) => i.field === field).map((i) => i.level);
      if (levels.includes('error')) return 'error';
      if (levels.includes('warning')) return 'warning';
      return '';
    },

    /** '' (blank row) | 'ok' | 'info' | 'warning' | 'error'. */
    prlRowStatus(optionName, id) {
      const row = this.prlRow(optionName, id);
      if (!row || this.prlIsBlank(row)) return '';
      const levels = this.prlIssues(optionName, id).map((i) => i.level);
      if (levels.includes('error')) return 'error';
      if (levels.includes('warning')) return 'warning';
      return 'ok';
    },

    prlRowStatusLabel(optionName, id) {
      const status = this.prlRowStatus(optionName, id);
      if (status === 'error') return 'Needs fixing';
      if (status === 'warning') return 'Check';
      if (status === 'ok') return 'Ready';
      return 'Empty';
    },

    prlCount(optionName, status) {
      return this.prlRows(optionName).filter((r) => this.prlRowStatus(optionName, r.id) === status).length;
    },

    prlRouteCount(optionName) {
      return this.prlRows(optionName).filter((r) => !this.prlIsBlank(r)).length;
    },

    prlShownDomain(row) {
      return cleanDomain(row?.domain) || 'your.domain';
    },

    prlShownUpstream(row) {
      const u = trim(row?.upstream);
      return u ? u.replace(/\/+$/, '') : 'http://host:port';
    },

    prlEmptyHint(optionName) {
      return this.optUi(optionName)?.emptyHint
        || 'Publish a device or app that is not a Neo service — a router, printer, Home Assistant box — under its own domain. The proxy gets a certificate for the domain, serves it over https and forwards requests to the plain-http address on your network.';
    },

    prlEntryLabel(optionName) {
      return this.optUi(optionName)?.entryLabel || 'Route';
    },

    /** Human-readable save blockers (empty when every non-blank row is valid). */
    prlValidate(optionName) {
      const msgs = [];
      this.prlRows(optionName).forEach((r, idx) => {
        this.prlIssues(optionName, r.id)
          .filter((i) => i.level === 'error')
          .forEach((i) => {
            const d = trim(r.domain) || `${this.prlEntryLabel(optionName).toLowerCase()} ${idx + 1}`;
            msgs.push(`${d}: ${i.text}`);
          });
      });
      return msgs;
    },
  };

  const widget = {
    name: 'proxyRouteList',
    mixins,

    init(optionName) {
      this.prlLoad(optionName);
    },

    validate(optionName) {
      return this.prlValidate(optionName);
    },

    prepareSave(optionName) {
      this.prlCommit(optionName);
      const v = this.values[optionName] || {};
      if (this.deepEqual(v, this.defaults[optionName] || {})) return undefined;
      return this.cloneValue(v);
    },

    onReset(optionName) {
      this.prlLoad(optionName);
    },

    onRevert(optionName) {
      this.prlLoad(optionName);
    },
  };

  widget.parseUpstream = parseUpstream;
  widget.cleanDomain = cleanDomain;

  root.NeoWidgets.register(widget.name, widget);
  if (typeof module === 'object' && module.exports) module.exports = widget;
})(typeof globalThis !== 'undefined' ? globalThis : this);
