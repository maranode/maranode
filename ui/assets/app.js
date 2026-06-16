const $ = id => document.getElementById(id);
let ragAvailable = false;

function toast(msg, type = '') {
  const el = $('toast');
  $('toast-msg').textContent = msg;
  el.className = 'show ' + type;
  clearTimeout(el._t);
  el._t = setTimeout(() => { el.className = ''; }, 3200);
}

function esc(s) {
  return String(s ?? '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function renderMd(text) {
  if (typeof marked === 'undefined') return esc(text);
  return marked.parse(String(text ?? ''), { gfm: true, breaks: false });
}

function toggleGenSettings() {
  const panel = $('gen-settings-panel');
  const btn   = $('gen-settings-toggle');
  const open  = panel.style.display !== 'none';
  panel.style.display = open ? 'none' : '';
  btn.classList.toggle('active', !open);
  btn.setAttribute('aria-expanded', open ? 'false' : 'true');
}

function toggleAdv(id) {
  const body    = $(id);
  const section = body.closest('.adv-section');
  const chevron = section.querySelector('.adv-chevron');
  const toggle  = section.querySelector('.adv-toggle');
  const open    = body.style.display !== 'none';
  body.style.display = open ? 'none' : '';
  chevron.style.transform = open ? '' : 'rotate(180deg)';
  toggle?.setAttribute('aria-expanded', open ? 'false' : 'true');
}

function hasCitationMarkers(text) {
  return /\[\d[\d\s,;p\.]*\]/.test(text);
}

function cleanDisplayText(text) {
  return text
    .replace(/\s*\[general knowledge\]/gi, '')
    .replace(/\n+(?:#{1,3}\s*)?References:?[\s\S]*/i, '')
    .trim();
}

const SOURCES_PREVIEW = 2;

function buildSourcesEl(sources) {
  const el = document.createElement('div');
  el.className = 'sources';

  const header = document.createElement('div');
  header.className = 'sources-header';
  header.innerHTML = `<span class="sources-label">References</span>`;
  el.appendChild(header);

  const list = document.createElement('div');
  list.className = 'sources-list';

  sources.forEach((s, i) => {
    const displayTitle = s.title || sourceDisplayName(s.source) || `Source ${s.index}`;
    const metaParts = [];
    if (s.author) metaParts.push(s.author);
    if (s.page_number > 0) metaParts.push(`p. ${s.page_number}`);
    const meta = metaParts.join(' · ');

    const item = document.createElement('div');
    item.className = 'source-item' + (i >= SOURCES_PREVIEW ? ' source-hidden' : '');
    item.innerHTML = `
      <span class="source-idx">${s.index}</span>
      <div class="source-body">
        <span class="source-title">${esc(displayTitle)}</span>
        ${meta ? `<span class="source-meta">${esc(meta)}</span>` : ''}
      </div>`;
    list.appendChild(item);
  });

  el.appendChild(list);

  if (sources.length > SOURCES_PREVIEW) {
    const toggle = document.createElement('div');
    toggle.className = 'sources-toggle';
    toggle.innerHTML = `<svg width="16" height="16" fill="none" stroke="currentColor" stroke-width="2.5" viewBox="0 0 24 24"><polyline points="6 9 12 15 18 9"/></svg>`;
    toggle.addEventListener('click', () => {
      const expanded = toggle.classList.toggle('expanded');
      list.querySelectorAll('.source-hidden').forEach(item => {
        item.style.display = expanded ? 'flex' : 'none';
      });
      if (!expanded) {
        list.querySelectorAll('.source-item').forEach((item, i) => {
          if (i >= SOURCES_PREVIEW) item.style.display = 'none';
        });
      }
    });
    el.appendChild(toggle);
  }

  return el;
}

function linkCitations(html, sources) {
  if (!sources?.length) return html;
  return html.replace(/\[(\d[\d\s,;p\.]*)\]/g, (match, inner) => {
    const parts = inner.split(';');
    const badges = [];
    for (const part of parts) {
      const m = part.trim().match(/^(\d+)(?:\s*,\s*p\.?\s*(\d+))?/);
      if (!m) continue;
      const num = parseInt(m[1], 10);
      const s = sources[num - 1];
      if (!s) continue;
      const label = m[2] ? `${num}, p.${m[2]}` : `${num}`;
      badges.push(`<span class="cite-badge">[${label}]</span>`);
    }
    return badges.length ? badges.join('') : match;
  });
}

function sourceDisplayName(source) {
  if (!source) return null;
  let s = source.split('?')[0].split('#')[0];
  const parts = s.replace(/\\/g, '/').split('/');
  const filename = parts[parts.length - 1];
  if (!filename) return null;
  const dot = filename.lastIndexOf('.');
  const stem = dot > 0 ? filename.slice(0, dot) : filename;
  const pretty = stem.replace(/[_-]+/g, ' ').replace(/\b\w/g, c => c.toUpperCase());
  return pretty || null;
}

function relTime(iso) {
  const s = Math.floor((Date.now() - new Date(iso)) / 1000);
  if (s < 60)    return 'just now';
  if (s < 3600)  return Math.floor(s / 60) + 'm ago';
  if (s < 86400) return Math.floor(s / 3600) + 'h ago';
  return new Date(iso).toLocaleDateString();
}

function fmtTime(iso) {
  return new Date(iso).toLocaleString(undefined, {
    month: 'short', day: '2-digit',
    hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false,
  });
}

const WS_SLUG_KEY = 'ax_ws_slug';
const WS_KEY_KEY  = 'ax_ws_key';
const WS_ADMIN_KEY = 'ax_admin_key';

function getWsSlug() { return localStorage.getItem(WS_SLUG_KEY) || 'default'; }
function getWsKey()  { return localStorage.getItem(WS_KEY_KEY)  || ''; }
function getAdminKey() { return localStorage.getItem(WS_ADMIN_KEY) || ''; }

function wsHeaders(useAdmin = false) {
  const h = { 'X-Maranode-Workspace': getWsSlug() };
  const key = useAdmin ? getAdminKey() : getWsKey();
  if (key) h['Authorization'] = 'Bearer ' + key;
  return h;
}

async function apiFetch(path, opts = {}) {
  const tok = localStorage.getItem('ax_session_token') || '';
  const authH = tok ? { 'Authorization': 'Bearer ' + tok } : {};
  opts.headers = Object.assign({}, authH, wsHeaders(), opts.headers || {});
  const res = await fetch(path, opts);
  if (res.status === 401) {
    document.getElementById('page-login') && (document.getElementById('page-login').style.display = 'flex');
    throw new Error('unauthenticated');
  }
  if (!res.ok) {
    const j = await res.json().catch(() => ({}));
    throw new Error(j?.error?.message || `HTTP ${res.status}`);
  }
  return res;
}

async function adminFetch(path, opts = {}) {
  opts.headers = Object.assign({}, wsHeaders(true), opts.headers || {});
  const res = await fetch(path, opts);
  if (!res.ok) {
    const j = await res.json().catch(() => ({}));
    throw new Error(j?.error?.message || `HTTP ${res.status}`);
  }
  return res;
}

function showPage(name, btn) {
  document.querySelectorAll('.page').forEach(p => p.classList.remove('active'));
  document.querySelectorAll('.sidebar-nav-btn').forEach(b => { b.classList.remove('active'); b.removeAttribute('aria-current'); });
  $('library-nav-btn')?.classList.remove('active');
  $('library-nav-btn')?.removeAttribute('aria-current');
  $('page-' + name).classList.add('active');
  const fallbackBtn = name === 'library' ? $('library-nav-btn') : $('nav-' + name);
  const activeBtn = btn || fallbackBtn;
  activeBtn?.classList.add('active');
  activeBtn?.setAttribute('aria-current', 'page');
  closeSidebar();
  if (name === 'models')     loadModels();
  if (name === 'audit')      loadAudit();
  if (name === 'rag')        loadCollections();
  if (name === 'library')    renderLibrary();
  if (name === 'workspaces') loadWorkspaces();
  if (name === 'users')      { loadUsers(); loadSessions(); }
}

async function checkHealth() {
  try {
    const d = await (await fetch('/health')).json();
    $('status-dot').className = 'ok';
    $('status-text').textContent = 'online';
    $('badge-ver').textContent = 'v' + (d.version || '?');
    if (d.air_gap) $('badge-airgap').style.display = '';
  } catch {
    $('status-dot').className = 'err';
    $('status-text').textContent = 'offline';
  }

  // Detect RAG availability
  try {
    const r = await fetch('/v1/rag/collections');
    ragAvailable = r.status !== 501;
  } catch { ragAvailable = false; }

  if (ragAvailable) {
    $('badge-rag').style.display = '';
    $('rag-hint').style.display = 'inline-flex';
    $('gen-rag-options').style.display = '';
    loadCollectionSelector();
  } else {
    $('badge-rag').style.display = 'none';
    $('rag-hint').style.display = 'none';
    $('collection-selector-wrap').style.display = 'none';
  }
}

async function loadCollectionSelector() {
  try {
    const data = await (await apiFetch('/v1/rag/collections')).json();
    const sel = $('topbar-collection');
    const cur = sel.value;
    sel.innerHTML = '<option value="">All collections</option>';
    data.forEach(c => {
      const o = document.createElement('option');
      o.value = c.name;
      o.textContent = c.name;
      if (o.value === cur) o.selected = true;
      sel.appendChild(o);
    });
    $('collection-selector-wrap').style.display = data.length > 0 ? '' : 'none';
  } catch {
    $('collection-selector-wrap').style.display = 'none';
  }
}

async function loadStats() {
  try {
    const d = await (await apiFetch('/stats')).json();
    // $('stat-requests').textContent   = d.requests ?? '—';
    // $('stat-tokens-out').textContent = d.tokens_out ?? '—';
    $('stat-latency').textContent    = d.avg_latency_ms != null ? d.avg_latency_ms + ' ms' : '—';
    const qd = d.queue_depth ?? 0;
    const qm = d.queue_max   ?? 0;
    $('stat-queue').textContent      = qm > 0 ? `${qd} / ${qm}` : '—';
  } catch { /* stats are optional */ }
}

async function loadModels() {
  const tbody = $('models-tbody');
  tbody.innerHTML = '<tr><td colspan="7"><div class="empty-state"><p>Loading…</p></div></td></tr>';

  try {
    const resp = await (await apiFetch('/v1/models/details')).json();
    const d = Array.isArray(resp) ? resp : (resp.data || []);

    const llms   = d.filter(m => !m.model_type || m.model_type === 'llm');
    const embeds = d.filter(m => m.model_type === 'embedding');

    // Populate chat model selector - LLM models only
    const sel = $('topbar-model');
    const cur = sel.value;
    sel.innerHTML = '';
    if (!llms.length) {
      sel.innerHTML = '<option value="">No LLM models installed</option>';
    } else {
      llms.forEach(m => {
        const o = document.createElement('option');
        o.value = m.id || `${m.name}:${m.tag}`;
        o.textContent = `${m.name}:${m.tag}`;
        if (o.value === cur) o.selected = true;
        sel.appendChild(o);
      });
    }

    if (!d.length) {
      tbody.innerHTML = `<tr><td colspan="7"><div class="empty-state">
        <svg width="40" height="40" fill="none" stroke="currentColor" stroke-width="1.5" viewBox="0 0 24 24">
          <path d="M21 16V8a2 2 0 00-1-1.73l-7-4a2 2 0 00-2 0l-7 4A2 2 0 003 8v8a2 2 0 001 1.73l7 4a2 2 0 002 0l7-4A2 2 0 0021 16z"/>
        </svg>
        <p>No models installed. Use the CLI to import a GGUF model.</p>
      </div></td></tr>`;
      return;
    }

    const modelRow = m => {
      const typeColor = m.model_type === 'embedding' ? 'var(--purple)' : 'var(--accent)';
      const typeLabel = m.model_type === 'embedding' ? 'embedding' : 'llm';
      return `<tr>
        <td><strong style="color:var(--text)">${esc(m.name)}</strong> <span class="tag blue">${esc(m.tag)}</span></td>
        <td><span style="font-size:11px;font-weight:600;color:${typeColor}">${typeLabel}</span></td>
        <td style="color:var(--text-mid)">${esc(m.size_human || '—')}</td>
        <td>${m.quantization ? `<span class="tag green">${esc(m.quantization)}</span>` : '<span class="tag">—</span>'}</td>
        <td class="mono" title="${esc(m.sha256)}" style="color:var(--text-dim)">${esc((m.sha256 || '').slice(0, 16))}…</td>
        <td style="color:var(--text-dim);font-size:12px">${fmtTime(m.imported_at)}</td>
        <td><button class="btn btn-danger" style="padding:4px 10px;font-size:12px"
            onclick="removeModel(${JSON.stringify(m.id || m.name + ':' + m.tag)})">Remove</button></td>
      </tr>`;
    };

    let html = '';
    if (llms.length) {
      html += `<tr><td colspan="7" style="padding:12px 16px 6px;color:var(--text-dim);font-size:11px;text-transform:uppercase;letter-spacing:.08em;border-bottom:1px solid var(--border)">Language Models</td></tr>`;
      html += llms.map(modelRow).join('');
    }
    if (embeds.length) {
      html += `<tr><td colspan="7" style="padding:16px 16px 6px;color:var(--text-dim);font-size:11px;text-transform:uppercase;letter-spacing:.08em;border-bottom:1px solid var(--border)">Embedding Models</td></tr>`;
      html += embeds.map(modelRow).join('');
    }
    tbody.innerHTML = html;
  } catch (e) {
    tbody.innerHTML = `<tr><td colspan="7"><div class="empty-state"><p>Error: ${esc(e.message)}</p></div></td></tr>`;
  }
}

async function removeModel(id) {
  if (!confirm(`Remove "${id}"? This cannot be undone.`)) return;
  try {
    await apiFetch('/v1/models/' + encodeURIComponent(id), { method: 'DELETE' });
    toast('Model removed', 'ok');
    loadModels();
  } catch (e) { toast('Error: ' + e.message, 'err'); }
}

const STORAGE_KEY = 'maranode_conversations';
const MAX_STORED  = 50;

let conversations = [];
let activeConvId  = null;

function saveConversations() {
  try {
    const toSave = conversations.slice(0, MAX_STORED).map(c => ({
      ...c,
      messages: c.messages.slice(-100),
    }));
    localStorage.setItem(STORAGE_KEY, JSON.stringify(toSave));
  } catch (e) {
    console.warn('Could not save chat history:', e);
  }
}

function loadConversations() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    return JSON.parse(raw).map(c => ({ ...c, created: new Date(c.created) }));
  } catch (e) {
    console.warn('Could not load chat history:', e);
    return [];
  }
}

function newConversation() {
  const id = Date.now();
  conversations.unshift({ id, title: 'New Conversation', messages: [], created: new Date() });
  saveConversations();
  setActiveConv(id);
  renderConvList();
  $('messages').innerHTML = '';
  showWelcome();
  $('topbar-title').textContent = 'New Conversation';
  showPage('chat');
}

function setActiveConv(id) {
  activeConvId = id;
  renderConvList();
}

function getConv() {
  return conversations.find(c => c.id === activeConvId);
}

function deleteConversation(id, e) {
  e.stopPropagation();
  if (!confirm('Delete this conversation? This cannot be undone.')) return;
  conversations = conversations.filter(c => c.id !== id);
  saveConversations();
  if (activeConvId === id) {
    if (conversations.length) {
      switchConv(conversations[0].id);
    } else {
      newConversation();
    }
  } else {
    renderConvList();
  }
}

const _pinSvg  = `<svg width="11" height="11" fill="none" stroke="currentColor" stroke-width="2.5" viewBox="0 0 24 24"><line x1="12" y1="17" x2="12" y2="22"/><path d="M5 17h14v-1.76a2 2 0 00-1.11-1.79l-1.78-.9A2 2 0 0115 10.76V6h1a2 2 0 000-4H8a2 2 0 000 4h1v4.76a2 2 0 01-1.11 1.79l-1.78.9A2 2 0 005 15.24z"/></svg>`;
const _bmkSvg  = `<svg width="11" height="11" fill="none" stroke="currentColor" stroke-width="2.5" viewBox="0 0 24 24"><path d="M19 21l-7-5-7 5V5a2 2 0 012-2h10a2 2 0 012 2z"/></svg>`;
const _trshSvg = `<svg width="11" height="11" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><polyline points="3 6 5 6 21 6"/><path d="M19 6l-1 14H6L5 6"/><path d="M10 11v6M14 11v6"/></svg>`;

function renderConvList() {
  const list = $('conv-list');
  if (!conversations.length) {
    list.innerHTML = `<div style="padding:12px 10px;font-size:12px;color:var(--text-dim);text-align:center">No conversations yet</div>`;
    return;
  }

  const byRecent = (a, b) => new Date(b.created) - new Date(a.created);
  const pinned   = conversations.filter(c =>  c.pinned).sort(byRecent);
  const unpinned = conversations.filter(c => !c.pinned).sort(byRecent);

  const item = c => `
    <div class="conv-item ${c.id === activeConvId ? 'active' : ''}${c.pinned ? ' pinned' : ''}" onclick="switchConv(${c.id})">
      <div class="conv-title">${esc(c.title)}</div>
      <div class="conv-meta">${relTime(c.created)}</div>
      <div class="conv-actions">
        <button class="conv-action pin-btn${c.pinned ? ' active' : ''}" onclick="togglePin(${c.id},event)" title="${c.pinned ? 'Unpin' : 'Pin to top'}">${_pinSvg}</button>
        <button class="conv-action save-btn" onclick="openSaveModal(${c.id},event)" title="Save to Library">${_bmkSvg}</button>
        <button class="conv-action del-btn"  onclick="deleteConversation(${c.id},event)" title="Delete">${_trshSvg}</button>
      </div>
    </div>`;

  let html = '';
  if (pinned.length) {
    html += `<div class="conv-section-label">Pinned</div>`;
    html += pinned.map(item).join('');
    if (unpinned.length) html += `<div class="conv-section-label">Recent</div>`;
  }
  html += unpinned.map(item).join('');
  list.innerHTML = html;
}

function switchConv(id) {
  const conv = conversations.find(c => c.id === id);
  if (!conv) return;
  activeConvId = id;
  renderConvList();
  $('topbar-title').textContent = conv.title;
  renderMessages(conv);
  showPage('chat');
}

function renderMessages(conv) {
  const wrap = $('messages');
  if (!conv.messages.length) { showWelcome(); return; }
  wrap.innerHTML = '';
  conv.messages.forEach(m => appendBubble(m.role, m.display || m.content, m.sources, m.attachment));
  wrap.scrollTop = wrap.scrollHeight;
}

function showWelcome() {
  $('messages').innerHTML = `
    <div id="welcome">
      <div id="welcome-mark">
        <div id="welcome-logo"><svg width="32" height="32" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg"><path d="M8 8.5L10.5 5.5L12 7.5L13.5 5.5L16 8.5H8Z" fill="white"/><circle cx="12" cy="11" r="2.5" fill="white"/><path d="M12 13.5C9 15 6.5 14.5 5.5 17C4.5 19.5 7.5 21 11 20" stroke="white" stroke-width="2.2" stroke-linecap="round"/><path d="M11 20L9.5 23" stroke="white" stroke-width="1.5" stroke-linecap="round"/></svg></div>
        <div id="welcome-wordmark">Maranode</div>
      </div>
      <div id="welcome-tagline">Private inference, on your machine.</div>
    </div>`;
}

function quickSend(text) {
  $('chat-input').value = text;
  sendMessage();
}

let attachedFile = null;

$('file-input').addEventListener('change', function () {
  const f = this.files[0];
  if (!f) return;
  attachedFile = f;
  $('attachment-filename').textContent = f.name;
  $('attachment-bar').classList.add('show');
  $('attach-btn').classList.add('active');
});

function clearAttachment() {
  attachedFile = null;
  $('file-input').value = '';
  $('attachment-bar').classList.remove('show');
  $('attach-btn').classList.remove('active');
}

function appendBubble(role, content, sources, attachment) {
  const wrap = $('messages');
  const welcome = document.getElementById('welcome');
  if (welcome) welcome.remove();

  const row = document.createElement('div');
  row.className = 'msg-row ' + (role === 'user' ? 'user' : 'assist');

  const cleanContent = role === 'user' ? content : cleanDisplayText(content);
  const contentHtml = role === 'user'
    ? linkCitations(esc(cleanContent), sources)
    : linkCitations(renderMd(cleanContent), sources);

  const bubble = document.createElement('div');
  bubble.className = 'bubble';
  const textDiv = document.createElement('div');
  textDiv.innerHTML = contentHtml;
  bubble.appendChild(textDiv);

  if (attachment) {
    const fileTag = document.createElement('div');
    fileTag.className = 'bubble-attachment';
    fileTag.innerHTML = `<svg width="11" height="11" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path d="M21.44 11.05l-9.19 9.19a6 6 0 01-8.49-8.49l9.19-9.19a4 4 0 015.66 5.66l-9.2 9.19a2 2 0 01-2.83-2.83l8.49-8.48"/></svg>${esc(attachment)}`;
    bubble.appendChild(fileTag);
  }

  if (sources?.length) {
    bubble.appendChild(buildSourcesEl(sources));
  }

  row.appendChild(bubble);
  wrap.appendChild(row);
  wrap.scrollTop = wrap.scrollHeight;
  return row.querySelector('.bubble');
}

function showStatusBubble() {
  const wrap = $('messages');
  const welcome = document.getElementById('welcome');
  if (welcome) welcome.remove();

  const row = document.createElement('div');
  row.className = 'msg-row assist';
  row.id = 'status-indicator';
  row.innerHTML = `<div class="status-bubble">
    <div class="spinner"></div>
    <span>Thinking…</span>
  </div>`;
  wrap.appendChild(row);
  wrap.scrollTop = wrap.scrollHeight;
  return row;
}

function removeStatus() {
  const el = $('status-indicator');
  if (el) el.remove();
}

async function sendMessage() {
  const input  = $('chat-input');
  const model  = $('topbar-model').value;
  const prompt = input.value.trim();

  if (!prompt && !attachedFile) return;
  if (!model) { toast('Select a model first', 'err'); return; }

  if (!activeConvId) newConversation();
  const conv = getConv();

  input.value = '';
  input.style.height = '';
  $('send-btn').disabled = true;

  const file = attachedFile;
  if (file) clearAttachment();

  showStatusBubble();

  let inlinePrefix = '';
  if (file) {
    try {
      const form = new FormData();
      form.append('file', file, file.name);
      const res = await fetch('/v1/rag/extract', { method: 'POST', body: form });
      if (!res.ok) {
        const j = await res.json().catch(() => ({}));
        throw new Error(j?.error?.message || `HTTP ${res.status}`);
      }
      const d = await res.json();
      const MAX_FILE_CHARS = 8000;
      const rawText = d.text || '';
      const truncated = rawText.length > MAX_FILE_CHARS;
      const fileText = truncated
        ? rawText.slice(0, MAX_FILE_CHARS) + `\n\n[… truncated - document is ${d.chars?.toLocaleString() ?? rawText.length} chars, showing first ${MAX_FILE_CHARS.toLocaleString()} …]`
        : rawText;
      inlinePrefix = `[Attached file: ${d.filename}]\n\n${fileText}\n\n---\n\n`;
    } catch (e) {
      removeStatus();
      toast('Could not read file: ' + e.message, 'err');
      $('send-btn').disabled = false;
      return;
    }
  }

  const displayText = prompt || `[Analysing ${file?.name}]`;
  appendBubble('user', displayText, null, file?.name);
  conv.messages.push({ role: 'user', content: inlinePrefix + (prompt || ''), display: displayText, attachment: file?.name });
  if (conv.messages.length === 1) {
    conv.title = displayText.slice(0, 50);
    $('topbar-title').textContent = conv.title;
    renderConvList();
  }
  saveConversations();

  const selectedCollection = $('topbar-collection')?.value || '';
  const ragEnabled = ragAvailable;

  const temperature   = parseFloat($('gen-temperature')?.value ?? '0.7');
  const maxTokens     = parseInt($('gen-max-tokens')?.value ?? '2048', 10);
  const seedRaw       = $('gen-seed')?.value.trim();
  const deterministic = $('gen-deterministic')?.checked ?? false;
  const stopRaw       = $('gen-stop')?.value.trim();
  const ragTopK       = parseInt($('gen-rag-topk')?.value ?? '5', 10);
  const ragRequire    = $('gen-rag-require')?.checked ?? false;

  const ragOpts = ragEnabled
    ? {
        ...(selectedCollection ? { collection: selectedCollection } : {}),
        top_k: ragTopK,
        ...(ragRequire ? { require_context: true } : {}),
      }
    : null;

  const body = {
    model,
    messages: conv.messages
      .filter(m => m.role === 'user' || m.role === 'assistant')
      .map(m => ({ role: m.role, content: m.content })),
    stream: true,
    temperature,
    max_tokens: maxTokens,
    ...(deterministic ? { deterministic: true } : {}),
    ...(seedRaw ? { seed: parseInt(seedRaw, 10) } : {}),
    ...(stopRaw ? { stop: stopRaw.split(',').map(s => s.trim()).filter(Boolean) } : {}),
    ...(ragOpts ? { rag: ragOpts } : {}),
  };

  try {
    const res = await fetch('/v1/chat/completions', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', ...wsHeaders() },
      body: JSON.stringify(body),
    });
    if (!res.ok) {
      const j = await res.json().catch(() => ({}));
      throw new Error(j?.error?.message || `HTTP ${res.status}`);
    }

    removeStatus();
    const bubble = appendBubble('assistant', '');
    const textNode = bubble.querySelector('div');

    const reader  = res.body.getReader();
    const decoder = new TextDecoder();
    let buf    = '';
    let reply  = '';
    let sources = [];

    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      buf += decoder.decode(value, { stream: true });

      const lines = buf.split('\n');
      buf = lines.pop(); // keep incomplete line

      for (const line of lines) {
        if (!line.startsWith('data:')) continue;
        const raw = line.slice(5).trim();
        if (raw === '[DONE]') break;
        if (raw.startsWith('[ERROR]')) {
          textNode.textContent = '⚠ ' + raw.slice(7).trim();
          break;
        }
        try {
          const chunk = JSON.parse(raw);
          const delta = chunk.choices?.[0]?.delta?.content;
          if (delta) {
            reply += delta;
            textNode.innerHTML = renderMd(cleanDisplayText(reply));
            bubble.closest('.msg-row').parentElement.scrollTop = 99999;
          }
          if (chunk.choices?.[0]?.finish_reason === 'stop' && chunk.sources) {
            sources = chunk.sources;
          }
        } catch { /* skip malformed chunks */ }
      }
    }

    const cleanReply = cleanDisplayText(reply);
    textNode.innerHTML = linkCitations(renderMd(cleanReply), sources);

    if (sources.length) {
      bubble.appendChild(buildSourcesEl(sources));
      bubble.closest('.msg-row').parentElement.scrollTop = 99999;
    }

    conv.messages.push({ role: 'assistant', content: cleanReply, sources });
    saveConversations();
    loadStats();
  } catch (e) {
    removeStatus();
    appendBubble('assistant', '⚠ ' + e.message);
    toast(e.message, 'err');
  } finally {
    $('send-btn').disabled = false;
    input.focus();
  }
}

$('chat-input').addEventListener('keydown', e => {
  if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendMessage(); }
});

$('chat-input').addEventListener('input', function () {
  this.style.height = '';
  this.style.height = Math.min(this.scrollHeight, 160) + 'px';
});

function updateRagFileLabel(files) {
  const zone = $('rag-drop-zone');
  if (!files || files.length === 0) {
    $('rag-file-label').textContent = 'Click to choose files, or drag & drop';
    zone.classList.remove('has-file');
  } else if (files.length === 1) {
    $('rag-file-label').textContent = files[0].name;
    zone.classList.add('has-file');
  } else {
    $('rag-file-label').textContent = `${files.length} files selected`;
    zone.classList.add('has-file');
  }
}

document.getElementById('rag-file-input').addEventListener('change', function () {
  updateRagFileLabel(this.files);
});

const ragZone = $('rag-drop-zone');
ragZone.addEventListener('dragover',  e => { e.preventDefault(); ragZone.classList.add('drag-over'); });
ragZone.addEventListener('dragleave', ()  => ragZone.classList.remove('drag-over'));
ragZone.addEventListener('drop', e => {
  e.preventDefault();
  ragZone.classList.remove('drag-over');
  const dropped = e.dataTransfer.files;
  if (!dropped.length) return;
  const dt = new DataTransfer();
  for (const f of dropped) dt.items.add(f);
  $('rag-file-input').files = dt.files;
  updateRagFileLabel(dt.files);
});

async function loadCollections() {
  const list = $('rag-collections-list');
  list.innerHTML = '<div style="padding:16px;color:var(--text-dim);font-size:13px">Loading…</div>';
  try {
    const data = await (await apiFetch('/v1/rag/collections')).json();
    $('rag-disabled-banner').style.display = 'none';
    $('rag-content').style.display = '';

    // sync collection dropdown in add form
    const sel = $('rag-collection-select');
    const cur = sel.value === '__new__' ? '' : sel.value;
    sel.innerHTML = '';
    const names = data.map(c => c.name);
    if (!names.includes('default')) names.unshift('default');
    names.forEach(n => {
      const o = document.createElement('option');
      o.value = n; o.textContent = n;
      if (n === cur) o.selected = true;
      sel.appendChild(o);
    });
    const newOpt = document.createElement('option');
    newOpt.value = '__new__'; newOpt.textContent = '+ New collection…';
    sel.appendChild(newOpt);

    if (!data.length) {
      list.innerHTML = `<div class="empty-state" style="padding:24px">
        <p>No collections yet. Add a document to get started.</p>
      </div>`;
      return;
    }

    list.innerHTML = data.map(c => `
      <div class="collection-row" id="col-row-${esc(c.name)}">
        <div class="collection-header" onclick="toggleCollectionDocs('${esc(c.name)}')">
          <div class="collection-info">
            <strong>${esc(c.name)}</strong>
            <span class="tag">${esc(c.documents)} doc${c.documents !== 1 ? 's' : ''}</span>
            <span class="tag">${esc(c.chunks)} chunks</span>
            <span class="tag" style="font-size:10px;color:var(--text-dim)">${esc(c.embedding_model)}</span>
          </div>
          <div class="collection-actions">
            <button class="btn btn-danger" style="padding:3px 10px;font-size:12px"
              onclick="deleteCollection(event,'${esc(c.name)}')">Delete</button>
            <svg class="chevron" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><polyline points="6 9 12 15 18 9"/></svg>
          </div>
        </div>
        <div class="collection-docs" id="col-docs-${esc(c.name)}" style="display:none"></div>
      </div>`).join('');
  } catch (e) {
    if (e.message.includes('501') || e.message.includes('not enabled')) {
      $('rag-disabled-banner').style.display = '';
      $('rag-content').style.display = 'none';
    } else {
      list.innerHTML = `<div class="empty-state" style="padding:16px"><p>Error: ${esc(e.message)}</p></div>`;
    }
  }
}

async function toggleCollectionDocs(name) {
  const panel = $('col-docs-' + name);
  const row = $('col-row-' + name);
  const open = panel.style.display !== 'none';
  if (open) {
    panel.style.display = 'none';
    row.querySelector('.chevron').style.transform = '';
    return;
  }
  panel.style.display = 'block';
  row.querySelector('.chevron').style.transform = 'rotate(180deg)';
  panel.innerHTML = '<div style="padding:12px 16px;color:var(--text-dim);font-size:13px">Loading…</div>';
  try {
    const docs = await (await apiFetch(`/v1/rag/collections/${encodeURIComponent(name)}/documents`)).json();
    if (!docs.length) {
      panel.innerHTML = '<div style="padding:12px 16px;color:var(--text-dim);font-size:13px">No documents.</div>';
      return;
    }
    panel.innerHTML = `<table class="docs-table">
      <thead><tr><th>Source</th><th>Pages</th><th>Chunks</th><th>Ingested</th><th></th></tr></thead>
      <tbody>${docs.map(d => `
        <tr>
          <td>
            <div style="color:var(--text);font-weight:500">${esc(d.title || d.source)}</div>
            ${d.title ? `<div style="font-size:11px;color:var(--text-dim)">${esc(d.source)}</div>` : ''}
            ${d.author ? `<div style="font-size:11px;color:var(--text-muted)">by ${esc(d.author)}</div>` : ''}
          </td>
          <td style="color:var(--text-mid)">${d.page_count || '—'}</td>
          <td style="color:var(--text-mid)">${d.chunks}</td>
          <td style="color:var(--text-dim);font-size:12px">${fmtTime(d.ingested_at)}</td>
          <td><button class="btn btn-danger" style="padding:2px 8px;font-size:11px"
            onclick="deleteDocument('${esc(d.id)}','${esc(name)}')">Remove</button></td>
        </tr>
        ${d.summary ? `<tr class="doc-summary-row"><td colspan="5"><div class="doc-summary">${esc(d.summary)}</div></td></tr>` : ''}
      `).join('')}</tbody>
    </table>`;
  } catch (e) {
    panel.innerHTML = `<div style="padding:12px 16px;color:var(--red);font-size:13px">Error: ${esc(e.message)}</div>`;
  }
}

async function deleteCollection(e, name) {
  e.stopPropagation();
  if (!confirm(`Delete collection "${name}" and all its documents? This cannot be undone.`)) return;
  try {
    await apiFetch(`/v1/rag/collections/${encodeURIComponent(name)}`, { method: 'DELETE' });
    toast(`Collection "${name}" deleted`, 'ok');
    loadCollections();
    loadCollectionSelector();
  } catch (err) { toast('Error: ' + err.message, 'err'); }
}

async function deleteDocument(id, collection) {
  if (!confirm('Remove this document from the knowledge base?')) return;
  try {
    await apiFetch(`/v1/rag/documents/${encodeURIComponent(id)}`, { method: 'DELETE' });
    toast('Document removed', 'ok');
    toggleCollectionDocs(collection); // close
    toggleCollectionDocs(collection); // reopen/refresh
  } catch (err) { toast('Error: ' + err.message, 'err'); }
}

function onCollectionSelectChange() {
  const sel = $('rag-collection-select');
  const newInput = $('rag-collection-new');
  if (sel.value === '__new__') {
    newInput.style.display = '';
    newInput.focus();
  } else {
    newInput.style.display = 'none';
  }
}

function onNewCollectionInput() {
  // just keeps the typed value ready. resolved in ingestDocument()
}

async function ingestDocument() {
  const sel = $('rag-collection-select');
  const collection = sel.value === '__new__'
    ? ($('rag-collection-new').value.trim() || 'default')
    : (sel.value || 'default');
  const source    = $('rag-source').value.trim();
  const files     = $('rag-file-input').files;
  const text      = $('rag-text-input').value.trim();

  if (!files.length && !text) { toast('Choose a file or paste text', 'err'); return; }

  const btn = $('rag-ingest-btn');
  btn.disabled = true;
  btn.innerHTML = '<div class="spinner" style="width:12px;height:12px;border-width:2px"></div> Adding…';

  function resetForm() {
    $('rag-source').value = '';
    $('rag-text-input').value = '';
    $('rag-file-input').value = '';
    updateRagFileLabel(null);
    $('rag-collection-new').value = '';
    $('rag-collection-new').style.display = 'none';
    loadCollections();
    loadCollectionSelector();
  }

  try {
    if (files.length > 1) {
      const form = new FormData();
      for (const f of files) form.append('file', f, f.name);
      form.append('collection', collection);
      const res = await fetch('/v1/rag/documents/upload/batch', { method: 'POST', body: form });
      if (!res.ok) {
        const j = await res.json().catch(() => ({}));
        throw new Error(j?.error?.message || `HTTP ${res.status}`);
      }
      const results = await res.json();
      const ok  = results.filter(r => r.ok);
      const bad = results.filter(r => !r.ok);
      const totalChunks = ok.reduce((s, r) => s + (r.chunks || 0), 0);
      if (ok.length) {
        toast(`${ok.length} file${ok.length > 1 ? 's' : ''} added to "${collection}" — ${totalChunks} chunks`, 'ok');
      }
      if (bad.length) {
        bad.forEach(r => toast(`${r.filename}: ${r.error}`, 'err'));
      }
      resetForm();
    } else if (files.length === 1) {
      const file = files[0];
      const form = new FormData();
      form.append('file', file, file.name);
      form.append('collection', collection);
      if (source) form.append('source', source);
      const res = await fetch('/v1/rag/documents/upload', { method: 'POST', body: form });
      if (!res.ok) {
        const j = await res.json().catch(() => ({}));
        throw new Error(j?.error?.message || `HTTP ${res.status}`);
      }
      const data = await res.json();
      toast(`Added to "${data.collection}" — ${data.chunks} chunks indexed`, 'ok');
      resetForm();
    } else {
      const data = await (await apiFetch('/v1/rag/documents', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ collection, source: source || 'pasted-text', text }),
      })).json();
      toast(`Added to "${data.collection}" — ${data.chunks} chunks indexed`, 'ok');
      resetForm();
    }
  } catch (e) {
    toast('Failed: ' + e.message, 'err');
  } finally {
    btn.disabled = false;
    btn.innerHTML = '<svg width="13" height="13" fill="none" stroke="currentColor" stroke-width="2.5" viewBox="0 0 24 24"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg> Add to Knowledge Base';
  }
}

function auditSummary(ev) {
  if (!ev) return '';
  switch (ev.event) {
    case 'daemon_start':           return `v${esc(ev.version)} · air_gap=${ev.air_gap}`;
    case 'daemon_stop':            return esc(ev.reason);
    case 'model_imported':         return `${esc(ev.model?.name)}:${esc(ev.model?.tag)} · ${Math.round((ev.size_bytes || 0) / 1048576)}MB`;
    case 'model_removed':          return `${esc(ev.model?.name)}:${esc(ev.model?.tag)}`;
    case 'inference_start':        return `${esc(ev.model?.name)}:${esc(ev.model?.tag)}`;
    case 'inference_complete':     return `${esc(ev.tokens_in)}→${esc(ev.tokens_out)} tokens · ${esc(ev.duration_ms)}ms`;
    case 'inference_failed':       return esc(ev.reason);
    case 'rag_document_ingested':  return `${esc(ev.source)} → ${esc(ev.collection)} (${esc(ev.chunks)} chunks)`;
    case 'rag_retrieval':          return `${esc(ev.collection)} · ${esc(ev.hits)} hits`;
    case 'audit_verified':         return `${esc(ev.entries)} entries · ok=${ev.ok}`;
    default: return esc(JSON.stringify(ev).slice(0, 80));
  }
}

function eventTagClass(type) {
  const map = {
    daemon_start: 'green',     daemon_stop: 'yellow',
    model_imported: 'blue',    model_removed: 'yellow',
    inference_complete: 'green', inference_failed: 'red',
    rag_document_ingested: 'purple', rag_retrieval: 'blue',
    audit_verified: 'green',
  };
  return map[type] || '';
}

const SESSION_KEY = 'ax_session_token';
const SESSION_USER_KEY = 'ax_session_user';

function getSessionToken() { return localStorage.getItem(SESSION_KEY) || ''; }
function getSessionUser()  {
  try { return JSON.parse(localStorage.getItem(SESSION_USER_KEY) || 'null'); } catch { return null; }
}

function authHeaders() {
  const tok = getSessionToken();
  return tok ? { 'Authorization': 'Bearer ' + tok } : {};
}


async function checkAuth() {
  const tok = getSessionToken();
  if (!tok) { showLoginPage(); return; }
  try {
    const res = await fetch('/v1/auth/me', { headers: authHeaders() });
    if (!res.ok) { showLoginPage(); return; }
    const user = await res.json();
    localStorage.setItem(SESSION_USER_KEY, JSON.stringify(user));
    hideLoginPage(user);
  } catch { showLoginPage(); }
}

function showLoginPage() {
  $('page-login').style.display = 'flex';
  // load available providers
  fetch('/v1/auth/providers').then(r => r.json()).then(p => {
    const wrap = $('login-sso-buttons');
    let hasSSO = false;
    if (p.oidc) { $('login-oidc-btn').style.display = ''; hasSSO = true; }
    if (p.ldap) { $('login-ldap-btn').style.display = ''; hasSSO = true; }
    if (p.saml) { $('login-saml-btn').style.display = ''; hasSSO = true; }
    if (hasSSO) wrap.style.display = 'flex';
  }).catch(() => {});
}

function hideLoginPage(user) {
  $('page-login').style.display = 'none';
  updateCurrentUserBadge(user);
}

function updateCurrentUserBadge(user) {
  if (!user) return;
  const badge = $('current-user-badge');
  const role  = $('current-role-badge');
  if (badge) badge.textContent = user.username;
  if (role)  role.textContent  = user.role;
}

async function doLogin() {
  const username = $('login-username').value.trim();
  const password = $('login-password').value;
  const errEl    = $('login-error');
  errEl.style.display = 'none';

  if (!username || !password) {
    errEl.textContent = 'Enter username and password.';
    errEl.style.display = '';
    return;
  }

  try {
    const res = await fetch('/v1/auth/login', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ username, password }),
    });
    const j = await res.json();
    if (!res.ok) {
      errEl.textContent = j.error?.message || j.error || 'Login failed';
      errEl.style.display = '';
      return;
    }
    localStorage.setItem(SESSION_KEY, j.token);
    localStorage.setItem(SESSION_USER_KEY, JSON.stringify(j.user));
    $('login-password').value = '';
    hideLoginPage(j.user);
    toast('Signed in as ' + j.user.username);
  } catch (e) {
    errEl.textContent = 'Login failed: ' + e.message;
    errEl.style.display = '';
  }
}

async function doLogout() {
  const tok = getSessionToken();
  if (tok) {
    await fetch('/v1/auth/logout', { method: 'POST', headers: { 'Authorization': 'Bearer ' + tok } }).catch(() => {});
  }
  localStorage.removeItem(SESSION_KEY);
  localStorage.removeItem(SESSION_USER_KEY);
  showLoginPage();
}

async function loadUsers() {
  const tbody = $('users-tbody');
  tbody.innerHTML = '<tr><td colspan="7"><div class="empty-state"><p>Loading…</p></div></td></tr>';
  try {
    const data = await (await apiFetch('/v1/users', { headers: authHeaders() })).json();
    const users = data.users || [];
    if (!users.length) {
      tbody.innerHTML = '<tr><td colspan="7"><div class="empty-state"><p>No users yet.</p></div></td></tr>';
      return;
    }
    tbody.innerHTML = users.map(u => `<tr>
      <td><strong>${esc(u.username)}</strong></td>
      <td style="color:var(--text-dim)">${esc(u.email || '—')}</td>
      <td><span class="tag" style="color:var(--accent)">${esc(u.role)}</span></td>
      <td><span class="tag">${esc(u.provider)}</span></td>
      <td style="font-size:12px;color:var(--text-dim)">${u.last_login ? relTime(u.last_login) : 'never'}</td>
      <td><span class="tag ${u.active ? '' : 'tag-danger'}">${u.active ? 'active' : 'disabled'}</span></td>
      <td style="white-space:nowrap">
        <button class="btn btn-secondary" style="font-size:11px;padding:4px 10px" onclick="openEditUserModal('${esc(u.id)}')">Edit</button>
        <button class="btn btn-danger"    style="font-size:11px;padding:4px 10px;margin-left:4px" onclick="deleteUser('${esc(u.id)}','${esc(u.username)}')">Delete</button>
      </td>
    </tr>`).join('');

    // provider status
    const provWrap = $('providers-status');
    if (provWrap) {
      fetch('/v1/auth/providers').then(r => r.json()).then(p => {
        provWrap.innerHTML = [
          ['Local', p.local], ['OIDC', p.oidc], ['LDAP', p.ldap], ['SAML', p.saml]
        ].map(([name, on]) =>
          `<span class="tag ${on ? '' : 'tag-muted'}">${name}: ${on ? '✓ enabled' : '✗ not configured'}</span>`
        ).join('');
      }).catch(() => {});
    }
  } catch (e) {
    tbody.innerHTML = `<tr><td colspan="7"><div class="empty-state"><p>${esc(e.message)}</p></div></td></tr>`;
  }
}

function openNewUserModal() {
  $('user-new-username').value = '';
  $('user-new-password').value = '';
  $('user-new-email').value    = '';
  $('user-new-role').value     = 'viewer';
  $('user-new-modal').style.display = 'flex';
  setTimeout(() => $('user-new-username').focus(), 50);
}

function closeNewUserModal() { $('user-new-modal').style.display = 'none'; }

async function createUser() {
  const username = $('user-new-username').value.trim();
  const password = $('user-new-password').value;
  const email    = $('user-new-email').value.trim() || undefined;
  const role     = $('user-new-role').value;
  if (!username) { toast('Username is required', 'error'); return; }

  try {
    await apiFetch('/v1/users', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', ...authHeaders() },
      body: JSON.stringify({ username, password: password || undefined, email, role }),
    });
    closeNewUserModal();
    loadUsers();
    toast('User created');
  } catch (e) { toast(e.message, 'error'); }
}

let _editUserId = null;

function openEditUserModal(id) {
  _editUserId = id;
  apiFetch(`/v1/users/${id}`, { headers: authHeaders() }).then(r => r.json()).then(u => {
    $('user-edit-id').value        = u.id;
    $('user-edit-username-label').textContent = u.username;
    $('user-edit-email').value     = u.email || '';
    $('user-edit-role').value      = u.role;
    $('user-edit-active').value    = String(u.active);
    $('user-edit-password').value  = '';
    $('user-edit-modal').style.display = 'flex';
  }).catch(e => toast(e.message, 'error'));
}

function closeEditUserModal() { $('user-edit-modal').style.display = 'none'; }

async function saveEditUser() {
  const id     = $('user-edit-id').value;
  const email  = $('user-edit-email').value.trim() || undefined;
  const role   = $('user-edit-role').value;
  const active = $('user-edit-active').value === 'true';
  const newPw  = $('user-edit-password').value;

  try {
    await apiFetch(`/v1/users/${id}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json', ...authHeaders() },
      body: JSON.stringify({ email, role, active }),
    });
    if (newPw) {
      await apiFetch(`/v1/users/${id}/password`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json', ...authHeaders() },
        body: JSON.stringify({ password: newPw }),
      });
    }
    closeEditUserModal();
    loadUsers();
    toast('User updated');
  } catch (e) { toast(e.message, 'error'); }
}

async function deleteUser(id, username) {
  if (!confirm(`Delete user "${username}"? This cannot be undone.`)) return;
  try {
    await apiFetch(`/v1/users/${id}`, { method: 'DELETE', headers: authHeaders() });
    loadUsers();
    toast('User deleted');
  } catch (e) { toast(e.message, 'error'); }
}

async function loadSessions() {
  const tbody = $('sessions-tbody');
  if (!tbody) return;
  tbody.innerHTML = '<tr><td colspan="5"><div class="empty-state"><p>Loading…</p></div></td></tr>';
  try {
    const data = await (await apiFetch('/v1/sessions', { headers: authHeaders() })).json();
    const sessions = data.sessions || [];
    if (!sessions.length) {
      tbody.innerHTML = '<tr><td colspan="5"><div class="empty-state"><p>No active sessions.</p></div></td></tr>';
      return;
    }
    tbody.innerHTML = sessions.map(s => `<tr>
      <td class="mono" style="font-size:12px;color:var(--text-mid)">${esc(s.token_prefix)}…${s.username ? ` <span style="color:var(--text-dim)">(${esc(s.username)})</span>` : ''}</td>
      <td style="font-size:12px;color:var(--text-dim)">${relTime(s.created_at)}</td>
      <td style="font-size:12px;color:var(--text-dim)">${relTime(s.expires_at)}</td>
      <td>${s.is_current ? '<span class="tag" style="color:var(--accent)">current</span>' : '<span class="tag">active</span>'}</td>
      <td>${s.is_current ? '' : `<button class="btn btn-danger" style="font-size:11px;padding:4px 10px" onclick="revokeSession('${esc(s.token_prefix)}')">Revoke</button>`}</td>
    </tr>`).join('');
  } catch (e) {
    tbody.innerHTML = `<tr><td colspan="5"><div class="empty-state"><p>${esc(e.message)}</p></div></td></tr>`;
  }
}

async function revokeSession(tokenPrefix) {
  if (!confirm('Revoke this session? The device using it will be signed out.')) return;
  try {
    await apiFetch(`/v1/sessions/${tokenPrefix}`, { method: 'DELETE', headers: authHeaders() });
    loadSessions();
    toast('Session revoked');
  } catch (e) { toast(e.message, 'error'); }
}

async function revokeAllOtherSessions() {
  if (!confirm('Revoke all other sessions? Every other device will be signed out.')) return;
  try {
    const data = await (await apiFetch('/v1/sessions', { method: 'DELETE', headers: authHeaders() })).json();
    loadSessions();
    toast(`Revoked ${data.revoked} session${data.revoked === 1 ? '' : 's'}`);
  } catch (e) { toast(e.message, 'error'); }
}

async function loadAudit() {
  const tbody = $('audit-tbody');
  const limit = $('audit-limit').value;
  tbody.innerHTML = '<tr><td colspan="5"><div class="empty-state"><p>Loading…</p></div></td></tr>';
  try {
    const data = await (await apiFetch(`/v1/audit/entries?limit=${limit}`)).json();
    if (!data.length) {
      tbody.innerHTML = '<tr><td colspan="5"><div class="empty-state"><p>No audit entries yet.</p></div></td></tr>';
      return;
    }
    tbody.innerHTML = [...data].reverse().map(e => `<tr>
      <td class="mono" style="color:var(--text-dim)">${e.seq}</td>
      <td style="white-space:nowrap;color:var(--text-dim);font-size:12px">${fmtTime(e.ts)}</td>
      <td><span class="tag">${esc(e.actor)}</span></td>
      <td><span class="tag ${eventTagClass(e.event?.event)} evt">${esc(e.event?.event || '?')}</span></td>
      <td style="color:var(--text-mid);font-size:12px">${auditSummary(e.event)}</td>
    </tr>`).join('');
  } catch (e) {
    tbody.innerHTML = `<tr><td colspan="5"><div class="empty-state"><p>Error: ${esc(e.message)}</p></div></td></tr>`;
  }
}

async function downloadComplianceExport() {
  const format = $('compliance-format').value;
  const ws     = $('compliance-workspace').value.trim();
  const from   = $('compliance-from').value;
  const to     = $('compliance-to').value;

  let url = `/v1/audit/export?format=${encodeURIComponent(format)}`;
  if (ws)   url += `&workspace=${encodeURIComponent(ws)}`;
  if (from) url += `&from=${encodeURIComponent(new Date(from).toISOString())}`;
  if (to)   url += `&to=${encodeURIComponent(new Date(to).toISOString())}`;

  try {
    const res = await fetch(url, { headers: { 'Authorization': 'Bearer ' + getAdminKey() } });
    if (!res.ok) {
      const j = await res.json().catch(() => ({}));
      toast(j.error || 'Export failed', 'error');
      return;
    }
    const blob = await res.blob();
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = `audit_${format}.csv`;
    a.click();
    URL.revokeObjectURL(a.href);
  } catch (e) {
    toast('Export failed: ' + e.message, 'error');
  }
}

async function downloadAuditBundle() {
  const ws = $('compliance-workspace')?.value.trim();
  const url = ws ? `/v1/audit/bundle/${encodeURIComponent(ws)}` : '/v1/audit/bundle';
  const filename = ws ? `audit_bundle_${ws}.zip` : 'audit_bundle.zip';
  try {
    const res = await fetch(url, { headers: { 'Authorization': 'Bearer ' + getAdminKey() } });
    if (!res.ok) {
      const j = await res.json().catch(() => ({}));
      toast(j.error?.message || j.error || 'Bundle failed', 'error');
      return;
    }
    const blob = await res.blob();
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = filename;
    a.click();
    URL.revokeObjectURL(a.href);
  } catch (e) {
    toast('Bundle download failed: ' + e.message, 'error');
  }
}

async function pruneAuditLog() {
  const days = parseInt($('retention-days').value, 10);
  if (!days || days < 1) { toast('Enter a valid number of days', 'error'); return; }
  if (!confirm(`Permanently delete audit entries older than ${days} days? This cannot be undone.`)) return;

  const el = $('prune-result');
  el.style.display = 'none';

  try {
    const res = await fetch('/v1/audit/prune', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'Authorization': 'Bearer ' + getAdminKey() },
      body: JSON.stringify({ retain_days: days }),
    });
    const j = await res.json();
    if (!res.ok) { toast(j.error || 'Prune failed', 'error'); return; }
    el.style.display = 'block';
    el.style.color = 'var(--success, #4caf50)';
    el.textContent = `Done - ${j.pruned} entr${j.pruned === 1 ? 'y' : 'ies'} removed.`;
    if (j.pruned > 0) loadAudit();
  } catch (e) {
    toast('Prune failed: ' + e.message, 'error');
  }
}

function togglePin(id, e) {
  e.stopPropagation();
  const conv = conversations.find(c => c.id === id);
  if (!conv) return;
  conv.pinned = !conv.pinned;
  saveConversations();
  renderConvList();
}

const LIBRARY_KEY = 'maranode_library';

function loadLibrary() {
  try {
    const raw = localStorage.getItem(LIBRARY_KEY);
    if (raw) return JSON.parse(raw);
  } catch {}
  return { folders: [{ id: 1, name: 'General', parentId: null }], saved: [] };
}

function saveLibrary() {
  localStorage.setItem(LIBRARY_KEY, JSON.stringify(library));
}

let library = loadLibrary();

let savingConvId = null;

function openSaveModal(id, e) {
  e.stopPropagation();
  savingConvId = id;
  const conv = conversations.find(c => c.id === id);
  $('save-name').value = conv?.title || '';
  populateFolderSelect($('save-folder-select'));
  $('save-new-folder-name').style.display = 'none';
  $('save-modal').style.display = 'flex';
  setTimeout(() => { $('save-name').focus(); $('save-name').select(); }, 50);
}

function closeSaveModal() {
  $('save-modal').style.display = 'none';
  savingConvId = null;
}

function modalOverlayClick(e) {
  if (e.target === $('save-modal')) closeSaveModal();
}

function onSaveFolderChange() {
  const isNew = $('save-folder-select').value === '__new__';
  $('save-new-folder-name').style.display = isNew ? '' : 'none';
  if (isNew) $('save-new-folder-name').focus();
}

function populateFolderSelect(sel) {
  sel.innerHTML = '';
  const roots = library.folders.filter(f => f.parentId === null);
  roots.forEach(f => {
    const o = document.createElement('option');
    o.value = f.id; o.textContent = f.name;
    sel.appendChild(o);
    library.folders.filter(sf => sf.parentId === f.id).forEach(sf => {
      const so = document.createElement('option');
      so.value = sf.id; so.textContent = '  └ ' + sf.name;
      sel.appendChild(so);
    });
  });
  const newOpt = document.createElement('option');
  newOpt.value = '__new__'; newOpt.textContent = '+ New folder…';
  sel.appendChild(newOpt);
}

function confirmSave() {
  const name = $('save-name').value.trim() || 'Untitled';
  const selVal = $('save-folder-select').value;
  let folderId;

  if (selVal === '__new__') {
    const folderName = $('save-new-folder-name').value.trim() || 'New Folder';
    const newFolder = { id: Date.now(), name: folderName, parentId: null };
    library.folders.push(newFolder);
    folderId = newFolder.id;
  } else {
    folderId = parseInt(selVal);
  }

  const conv = conversations.find(c => c.id === savingConvId);
  if (!conv) { closeSaveModal(); return; }

  library.saved.push({
    id: Date.now(),
    name,
    folderId,
    messages: conv.messages.slice(),
    savedAt: new Date().toISOString(),
  });
  saveLibrary();
  closeSaveModal();
  toast(`Saved to Library`, 'ok');
}

const expandedFolders = new Set([1]); // general open by default

function renderLibrary() {
  const tree = $('library-tree');
  const roots = library.folders.filter(f => f.parentId === null);
  if (!roots.length) {
    tree.innerHTML = '<div class="empty-state" style="padding:32px"><p>No folders yet. Create one to start saving conversations.</p></div>';
    return;
  }
  tree.innerHTML = roots.map(f => renderLibFolder(f, true)).join('');
}

function renderLibFolder(folder, isRoot) {
  const subs  = library.folders.filter(f => f.parentId === folder.id);
  const chats = library.saved.filter(s => s.folderId === folder.id);
  const open  = expandedFolders.has(folder.id);
  const count = subs.length + chats.length;

  const chevron = `<svg class="lib-chevron" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><polyline points="9 18 15 12 9 6"/></svg>`;
  const folderIcon = `<svg class="lib-folder-icon" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z"/></svg>`;

  const subfolderBtn = isRoot
    ? `<button class="btn btn-secondary" style="padding:3px 9px;font-size:11px" onclick="startSubfolder(${folder.id},event)">+ Subfolder</button>`
    : '';
  const renameBtn = `<button class="btn btn-secondary" style="padding:3px 9px;font-size:11px" onclick="renameFolder(${folder.id},event)">Rename</button>`;
  const deleteBtn = `<button class="btn btn-danger"    style="padding:3px 9px;font-size:11px" onclick="deleteFolder(${folder.id},event)">${count ? 'Delete' : 'Remove'}</button>`;

  const bodyItems = [
    subs.map(sf => `<div class="lib-subfolder">${renderLibFolder(sf, false)}</div>`).join(''),
    chats.map(s => renderSavedItem(s)).join(''),
    `<div id="sfi-wrap-${folder.id}"></div>`,
    (!count ? `<div class="lib-empty">Empty folder</div>` : ''),
  ].join('');

  return `
    <div class="lib-folder${open ? ' open' : ''}" id="lf-${folder.id}">
      <div class="lib-folder-header" onclick="toggleLibFolder(${folder.id})">
        ${chevron}${folderIcon}
        <span class="lib-folder-name">${esc(folder.name)}</span>
        ${count ? `<span class="tag">${count}</span>` : ''}
        <div class="lib-folder-actions" onclick="event.stopPropagation()">
          ${subfolderBtn}
          ${renameBtn}
          ${deleteBtn}
        </div>
      </div>
      <div class="lib-folder-body" id="lfb-${folder.id}" style="${open ? '' : 'display:none'}">
        ${bodyItems}
      </div>
    </div>`;
}

function renderSavedItem(s) {
  const openSvg = `<svg width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>`;
  return `
    <div class="lib-saved-item">
      <div class="lib-saved-info">
        <div class="lib-saved-name">${esc(s.name)}</div>
        <div class="lib-saved-meta">${s.messages.length} messages · saved ${relTime(s.savedAt)}</div>
      </div>
      <div class="lib-saved-actions">
        <button class="btn btn-secondary" style="padding:4px 10px;font-size:12px;gap:5px" onclick="openSavedChat(${s.id})">${openSvg} Open</button>
        <button class="btn btn-danger"    style="padding:4px 8px;font-size:12px" onclick="deleteSaved(${s.id},event)">✕</button>
      </div>
    </div>`;
}

function toggleLibFolder(id) {
  const body   = $('lfb-' + id);
  const folder = $('lf-'  + id);
  if (!body) return;
  if (expandedFolders.has(id)) {
    expandedFolders.delete(id);
    body.style.display = 'none';
    folder?.classList.remove('open');
  } else {
    expandedFolders.add(id);
    body.style.display = 'block';
    folder?.classList.add('open');
  }
}

function startNewFolder() {
  $('new-folder-btn').style.display     = 'none';
  $('new-folder-input').style.display   = '';
  $('new-folder-confirm').style.display = '';
  $('new-folder-cancel').style.display  = '';
  $('new-folder-input').value = '';
  $('new-folder-input').focus();
}

function cancelNewFolder() {
  $('new-folder-btn').style.display     = '';
  $('new-folder-input').style.display   = 'none';
  $('new-folder-confirm').style.display = 'none';
  $('new-folder-cancel').style.display  = 'none';
}

function confirmNewFolder() {
  const name = $('new-folder-input').value.trim();
  if (!name) { $('new-folder-input').focus(); return; }
  library.folders.push({ id: Date.now(), name, parentId: null });
  saveLibrary();
  cancelNewFolder();
  renderLibrary();
}

function startSubfolder(parentId, e) {
  e.stopPropagation();
  const wrap = $('sfi-wrap-' + parentId);
  if (!wrap) return;
  if (!expandedFolders.has(parentId)) toggleLibFolder(parentId);
  wrap.innerHTML = `
    <div class="lib-inline-input">
      <input type="text" placeholder="Subfolder name" id="sfi-${parentId}"
        onkeydown="if(event.key==='Enter')confirmSubfolder(${parentId});else if(event.key==='Escape')cancelSubfolder(${parentId})">
      <button class="btn btn-secondary" style="padding:6px 10px;font-size:12px" onclick="confirmSubfolder(${parentId})">Create</button>
      <button class="btn btn-secondary" style="padding:6px 9px;font-size:12px" onclick="cancelSubfolder(${parentId})">✕</button>
    </div>`;
  $('sfi-' + parentId)?.focus();
}

function cancelSubfolder(parentId) {
  const wrap = $('sfi-wrap-' + parentId);
  if (wrap) wrap.innerHTML = '';
}

function confirmSubfolder(parentId) {
  const input = $('sfi-' + parentId);
  const name  = input?.value.trim();
  if (!name) { input?.focus(); return; }
  library.folders.push({ id: Date.now(), name, parentId });
  saveLibrary();
  renderLibrary();
}

function renameFolder(id, e) {
  e.stopPropagation();
  const folder  = library.folders.find(f => f.id === id);
  if (!folder) return;
  const nameEl  = document.querySelector(`#lf-${id} > .lib-folder-header .lib-folder-name`);
  if (!nameEl) return;

  const input = document.createElement('input');
  input.type  = 'text';
  input.value = folder.name;
  input.style.cssText = 'flex:1;padding:2px 8px;font-size:13px;font-weight:600;border-radius:4px;border:1px solid var(--accent);background:var(--surface);color:var(--text);outline:none;min-width:0;';

  const commit = () => {
    const name = input.value.trim();
    if (name && name !== folder.name) { folder.name = name; saveLibrary(); }
    renderLibrary();
  };
  input.onkeydown = ev => {
    ev.stopPropagation();
    if (ev.key === 'Enter')  commit();
    if (ev.key === 'Escape') renderLibrary();
  };
  input.onblur = commit;

  nameEl.replaceWith(input);
  input.focus();
  input.select();
}

function deleteFolder(id, e) {
  e.stopPropagation();
  const folder = library.folders.find(f => f.id === id);
  if (!folder) return;
  const subIds  = library.folders.filter(f => f.parentId === id).map(f => f.id);
  const allIds  = [id, ...subIds];
  const hasChats = library.saved.some(s => allIds.includes(s.folderId));
  const msg = hasChats
    ? `Delete "${folder.name}" and all saved chats inside? This cannot be undone.`
    : `Remove folder "${folder.name}"?`;
  if (!confirm(msg)) return;
  library.folders = library.folders.filter(f => !allIds.includes(f.id));
  library.saved   = library.saved.filter(s => !allIds.includes(s.folderId));
  saveLibrary();
  renderLibrary();
}

function deleteSaved(id, e) {
  e.stopPropagation();
  if (!confirm('Remove this saved chat?')) return;
  library.saved = library.saved.filter(s => s.id !== id);
  saveLibrary();
  renderLibrary();
}

function openSavedChat(savedId) {
  const saved = library.saved.find(s => s.id === savedId);
  if (!saved) return;
  const id = Date.now();
  conversations.unshift({ id, title: saved.name, messages: saved.messages.slice(), created: new Date() });
  saveConversations();
  switchConv(id);
  toast('Opened in a new conversation', 'ok');
}

conversations = loadConversations();
if (conversations.length) {
  activeConvId = conversations[0].id;
  renderConvList();
  $('topbar-title').textContent = conversations[0].title;
  renderMessages(conversations[0]);
} else {
  newConversation();
}
checkAuth();
checkHealth();
loadModels();
loadStats();
initWorkspaceBadge();
setInterval(checkHealth, 30000);
setInterval(loadStats, 30000);
  
let _wsCache = [];   // last fetched workspace list, used to populate dropdowns

function initWorkspaceBadge() {
  const slug = getWsSlug();
  $('badge-workspace-name').textContent = slug;
  if ($('ws-active-name')) $('ws-active-name').textContent = slug;
  const adminKey = getAdminKey();
  if ($('ws-admin-key-input')) $('ws-admin-key-input').value = adminKey;
}

function saveAdminKey() {
  const k = $('ws-admin-key-input').value.trim();
  if (k) localStorage.setItem(WS_ADMIN_KEY, k);
  else   localStorage.removeItem(WS_ADMIN_KEY);
  loadWorkspaces();
}

function populateWsSwitchSelect(workspaces) {
  const sel = $('ws-switch-select');
  if (!sel) return;
  const cur = getWsSlug();
  sel.innerHTML = workspaces.map(ws =>
    `<option value="${esc(ws.slug)}" ${ws.slug === cur ? 'selected' : ''}>${esc(ws.slug)}${ws.name !== ws.slug ? ' - ' + esc(ws.name) : ''}${ws.has_key ? ' 🔒' : ''}</option>`
  ).join('');
  onWsSwitchSelectChange();
}

function onWsSwitchSelectChange() {
  const sel = $('ws-switch-select');
  const slug = sel?.value;
  const ws = _wsCache.find(w => w.slug === slug);
  const hint = $('ws-switch-key-hint');
  if (hint) hint.textContent = ws?.has_key ? '(required)' : '(not required)';
  const keyField = $('ws-switch-key');
  if (keyField && slug === getWsSlug()) keyField.value = getWsKey();
  else if (keyField) keyField.value = '';
}

async function loadWorkspaces() {
  const tbody = $('ws-tbody');
  tbody.innerHTML = '<tr><td colspan="7"><div class="empty-state"><p>Loading…</p></div></td></tr>';

  try {
    const d = await (await adminFetch('/v1/workspaces')).json();
    $('ws-admin-key-banner').style.display = 'none';
    _wsCache = d.workspaces || [];
    populateWsSwitchSelect(_wsCache);

    if (!_wsCache.length) {
      tbody.innerHTML = '<tr><td colspan="7"><div class="empty-state"><p>No workspaces found.</p></div></td></tr>';
      return;
    }

    tbody.innerHTML = _wsCache.map(ws => `<tr>
      <td>
        <strong style="color:var(--text)">${esc(ws.slug)}</strong>
        ${ws.slug === 'default' ? '<span class="tag blue" style="margin-left:4px">default</span>' : ''}
        ${ws.slug === getWsSlug() ? '<span class="tag green" style="margin-left:4px">active</span>' : ''}
        <div style="font-size:11px;color:var(--text-dim);margin-top:2px">${esc(ws.name)}</div>
      </td>
      <td>${ws.has_key
        ? '<span style="color:var(--green);font-size:12px">🔒 Protected</span>'
        : '<span style="color:var(--text-dim);font-size:12px">Open</span>'}</td>
      <td style="font-size:12px;color:var(--text-mid)">${ws.model_allowlist?.length
        ? ws.model_allowlist.map(m => `<span class="tag">${esc(m)}</span>`).join(' ')
        : '<span style="color:var(--text-dim)">All</span>'}</td>
      <td style="font-size:12px;color:var(--text-mid)">${ws.rate_limit_rpm != null ? ws.rate_limit_rpm + ' rpm' : '—'}</td>
      <td style="font-size:12px;color:var(--text-mid)">${ws.has_system_prompt ? '✓' : '—'}</td>
      <td style="color:var(--text-dim);font-size:12px">${fmtTime(ws.created_at)}</td>
      <td>
        <button class="btn btn-secondary" style="padding:4px 10px;font-size:12px;margin-right:4px"
          onclick="openEditWorkspaceModal('${ws.slug}')">Edit</button>
        <button class="btn btn-secondary" style="padding:4px 10px;font-size:12px;margin-right:4px"
          onclick="activateWorkspaceRow('${ws.slug}', ${ws.has_key})">Use</button>
        ${ws.slug !== 'default' ? `<button class="btn btn-danger" style="padding:4px 10px;font-size:12px"
          onclick="deleteWorkspace('${ws.slug}')">Delete</button>` : ''}
      </td>
    </tr>`).join('');
  } catch (e) {
    if (e.message.includes('403') || e.message.includes('admin')) {
      tbody.innerHTML = '<tr><td colspan="7"><div class="empty-state"><p>Enter your admin key above to manage workspaces.</p></div></td></tr>';
      $('ws-admin-key-banner').style.display = '';
    } else {
      tbody.innerHTML = `<tr><td colspan="7"><div class="empty-state"><p>Error: ${esc(e.message)}</p></div></td></tr>`;
    }
  }
}

function activateWorkspaceRow(slug, hasKey) {
  // scroll to the active workspace card and pre-select it
  const sel = $('ws-switch-select');
  if (sel) { sel.value = slug; onWsSwitchSelectChange(); }
  if (!hasKey) {
    // open workspace, switch directly
    doSwitchWorkspace(slug, '');
  } else {
    $('ws-switch-key').value = '';
    $('ws-switch-key').focus();
  }
}

function switchWorkspace() {
  const slug = $('ws-switch-select')?.value || 'default';
  const key  = $('ws-switch-key').value.trim();
  doSwitchWorkspace(slug, key);
}

function doSwitchWorkspace(slug, key) {
  localStorage.setItem(WS_SLUG_KEY, slug);
  if (key) localStorage.setItem(WS_KEY_KEY, key);
  else     localStorage.removeItem(WS_KEY_KEY);
  initWorkspaceBadge();
  // refresh active badge on table rows without full reload
  document.querySelectorAll('#ws-tbody tr').forEach(tr => {
    const strong = tr.querySelector('td:first-child strong');
    const existing = tr.querySelector('.tag.green');
    if (existing) existing.remove();
    if (strong?.textContent === slug) {
      const span = document.createElement('span');
      span.className = 'tag green'; span.style.marginLeft = '4px'; span.textContent = 'active';
      strong.insertAdjacentElement('afterend', span);
    }
  });
  toast('Switched to workspace: ' + slug, 'ok');
}

async function deleteWorkspace(slug) {
  if (!confirm(`Delete workspace "${slug}"? This cannot be undone.`)) return;
  try {
    await adminFetch(`/v1/workspaces/${slug}`, { method: 'DELETE' });
    if (getWsSlug() === slug) doSwitchWorkspace('default', '');
    toast('Workspace deleted', 'ok');
    loadWorkspaces();
  } catch (e) {
    toast('Error: ' + e.message, 'err');
  }
}

// new workspace modal

function onWsProtectionChange() {
  const val = document.querySelector('input[name="ws-protection"]:checked')?.value;
  $('ws-new-custom-key').style.display = val === 'custom' ? '' : 'none';
}

function openNewWorkspaceModal() {
  $('ws-new-slug').value = '';
  $('ws-new-name').value = '';
  $('ws-new-models').value = '';
  $('ws-new-rpm').value = '';
  $('ws-new-prompt').value = '';
  $('ws-new-custom-key').value = '';
  $('ws-new-custom-key').style.display = 'none';
  $('ws-new-max-req').value = '';
  $('ws-new-max-models').value = '';
  $('ws-new-max-mem').value = '';
  $('ws-new-netns').checked = false;
  $('ws-new-adv').style.display = 'none';
  const advChevron = document.querySelector('#ws-new-modal .adv-chevron');
  if (advChevron) advChevron.style.transform = '';
  document.querySelector('input[name="ws-protection"][value="open"]').checked = true;
  $('ws-new-modal').style.display = 'flex';
  setTimeout(() => $('ws-new-slug').focus(), 50);
}

function closeNewWorkspaceModal() {
  $('ws-new-modal').style.display = 'none';
}

async function createWorkspace() {
  const slug = $('ws-new-slug').value.trim();
  const name = $('ws-new-name').value.trim() || slug;
  if (!slug) { toast('Slug is required', 'err'); return; }

  const protection = document.querySelector('input[name="ws-protection"]:checked')?.value || 'open';
  const modelsRaw  = $('ws-new-models').value.trim();
  const rpmRaw     = $('ws-new-rpm').value.trim();
  const prompt     = $('ws-new-prompt').value.trim();

  const body = { slug, name };
  if (protection === 'custom') {
    const k = $('ws-new-custom-key').value.trim();
    if (!k) { toast('Enter an API key or choose auto-generate', 'err'); return; }
    body.api_key = k;
  } else if (protection === 'open') {
    body.no_key = true;
  }
  // protection === 'auto': backend auto-generates, nothing extra needed

  if (modelsRaw) body.model_allowlist = modelsRaw.split(',').map(s => s.trim()).filter(Boolean);
  if (rpmRaw)    body.rate_limit_rpm  = parseInt(rpmRaw, 10);
  if (prompt)    body.system_prompt   = prompt;

  const maxReqRaw    = $('ws-new-max-req').value.trim();
  const maxModRaw    = $('ws-new-max-models').value.trim();
  const maxMemRaw    = $('ws-new-max-mem').value.trim();
  const netns        = $('ws-new-netns').checked;
  if (maxReqRaw) body.max_concurrent_requests = parseInt(maxReqRaw, 10);
  if (maxModRaw) body.max_models              = parseInt(maxModRaw, 10);
  if (maxMemRaw) body.max_memory_bytes        = parseInt(maxMemRaw, 10) * 1024 * 1024;
  if (netns)     body.net_namespace           = true;

  try {
    const d = await (await adminFetch('/v1/workspaces', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })).json();

    closeNewWorkspaceModal();
    loadWorkspaces();
    showWsCreatedModal(d);
  } catch (e) {
    toast('Error: ' + e.message, 'err');
  }
}

function showWsCreatedModal(d) {
  const ws  = d.workspace;
  const key = d.api_key;

  const keyHtml = key
    ? `<div style="margin-top:14px">
        <label class="field-label">API key <span class="field-label-hint" style="color:var(--yellow)">— save this now, not shown again</span></label>
        <div style="display:flex;gap:8px;align-items:center;margin-top:4px">
          <input type="text" id="ws-created-key-val" value="${esc(key)}" readonly
            style="flex:1;font-family:monospace;font-size:12px;letter-spacing:0.5px">
          <button class="btn btn-secondary" onclick="copyCreatedKey()">Copy</button>
        </div>
      </div>`
    : `<div style="margin-top:14px;color:var(--text-dim);font-size:13px">Open workspace - no API key required.</div>`;

  $('ws-created-details').innerHTML = `
    <div class="ws-created-row"><span>Slug</span><strong>${esc(ws.slug)}</strong></div>
    <div class="ws-created-row"><span>Name</span><strong>${esc(ws.name)}</strong></div>
    <div class="ws-created-row"><span>Access</span><strong>${ws.has_key ? '🔒 Protected' : 'Open'}</strong></div>
    ${ws.model_allowlist?.length ? `<div class="ws-created-row"><span>Models</span><strong>${ws.model_allowlist.join(', ')}</strong></div>` : ''}
    ${ws.rate_limit_rpm != null  ? `<div class="ws-created-row"><span>Rate limit</span><strong>${ws.rate_limit_rpm} rpm</strong></div>` : ''}
    ${keyHtml}
  `;
  $('ws-created-modal').style.display = 'flex';
}

function copyCreatedKey() {
  const val = $('ws-created-key-val')?.value;
  if (!val) return;
  navigator.clipboard.writeText(val).then(() => toast('Key copied', 'ok')).catch(() => {
    $('ws-created-key-val').select();
    toast('Select all and copy manually', '');
  });
}

function closeWsCreatedModal() {
  $('ws-created-modal').style.display = 'none';
}

// edit workspace modal

let _editingSlug = null;

function onWsEditProtectionChange() {
  const val = document.querySelector('input[name="ws-edit-protection"]:checked')?.value;
  $('ws-edit-new-key').style.display = val === 'newkey' ? '' : 'none';
}

function openEditWorkspaceModal(slug) {
  const ws = _wsCache.find(w => w.slug === slug);
  if (!ws) { toast('Workspace not found in cache - refresh the page', 'err'); return; }
  _editingSlug = slug;
  $('ws-edit-slug-label').textContent = slug;
  $('ws-edit-name').value    = ws.name || '';
  $('ws-edit-models').value  = (ws.model_allowlist || []).join(', ');
  $('ws-edit-rpm').value     = ws.rate_limit_rpm != null ? ws.rate_limit_rpm : '';
  $('ws-edit-prompt').value  = ws.system_prompt || '';
  $('ws-edit-clear-prompt').checked = false;
  $('ws-edit-new-key').value = '';
  $('ws-edit-new-key').style.display = 'none';
  $('ws-edit-max-req').value    = ws.max_concurrent_requests != null ? ws.max_concurrent_requests : '';
  $('ws-edit-max-models').value = ws.max_models != null ? ws.max_models : '';
  $('ws-edit-max-mem').value    = ws.max_memory_bytes != null ? Math.round(ws.max_memory_bytes / 1024 / 1024) : '';
  $('ws-edit-adv').style.display = 'none';
  const advChevron = document.querySelector('#ws-edit-modal .adv-chevron');
  if (advChevron) advChevron.style.transform = '';
  document.querySelector('input[name="ws-edit-protection"][value="keep"]').checked = true;
  $('ws-edit-modal').style.display = 'flex';
  setTimeout(() => $('ws-edit-name').focus(), 50);
}

function closeEditWorkspaceModal() {
  $('ws-edit-modal').style.display = 'none';
  _editingSlug = null;
}

async function saveEditWorkspace() {
  if (!_editingSlug) return;

  const protection = document.querySelector('input[name="ws-edit-protection"]:checked')?.value || 'keep';
  const modelsRaw  = $('ws-edit-models').value.trim();
  const rpmRaw     = $('ws-edit-rpm').value.trim();
  const promptVal  = $('ws-edit-prompt').value.trim();
  const clearPrompt = $('ws-edit-clear-prompt').checked;

  const body = {};
  const name = $('ws-edit-name').value.trim();
  if (name) body.name = name;

  if (protection === 'open') {
    body.clear_key = true;
  } else if (protection === 'newkey') {
    const k = $('ws-edit-new-key').value.trim();
    if (k) body.api_key = k;
    else   body.rotate_key = true;   // auto-generate; server returns the new key
  }

  body.model_allowlist = modelsRaw
    ? modelsRaw.split(',').map(s => s.trim()).filter(Boolean)
    : [];

  if (rpmRaw) body.rate_limit_rpm = parseInt(rpmRaw, 10);
  else        body.clear_rate_limit = true;

  if (clearPrompt) {
    body.clear_system_prompt = true;
  } else if (promptVal) {
    body.system_prompt = promptVal;
  }

  const maxReqRaw = $('ws-edit-max-req').value.trim();
  const maxModRaw = $('ws-edit-max-models').value.trim();
  const maxMemRaw = $('ws-edit-max-mem').value.trim();
  if (maxReqRaw) body.max_concurrent_requests = parseInt(maxReqRaw, 10);
  else           body.clear_max_concurrent_requests = true;
  if (maxModRaw) body.max_models = parseInt(maxModRaw, 10);
  else           body.clear_max_models = true;
  if (maxMemRaw) body.max_memory_bytes = parseInt(maxMemRaw, 10) * 1024 * 1024;
  else           body.clear_max_memory_bytes = true;

  try {
    const d = await (await adminFetch(`/v1/workspaces/${_editingSlug}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })).json();
    closeEditWorkspaceModal();
    loadWorkspaces();
    if (d.api_key) {
      showWsCreatedModal({ workspace: d.workspace, api_key: d.api_key });
    } else {
      toast('Workspace updated', 'ok');
    }
  } catch (e) {
    toast('Error: ' + e.message, 'err');
  }
}

// workspace switcher badge modal

let _wsSwitcherPending = null;   // slug waiting for key entry

function openWorkspaceSwitcher() {
  $('ws-switcher-current').textContent = getWsSlug();
  $('ws-switcher-key-wrap').style.display = 'none';
  $('ws-switcher-key').value = '';
  $('ws-switcher-modal').style.display = 'flex';
  _wsSwitcherPending = null;

  const list = $('ws-switcher-list');
  list.innerHTML = '<div style="color:var(--text-dim);font-size:12px;padding:8px 0">Loading…</div>';

  const src = _wsCache.length ? Promise.resolve({ workspaces: _wsCache })
    : adminFetch('/v1/workspaces').then(r => r.json());

  src.then(d => {
    const workspaces = d.workspaces || [];
    if (!workspaces.length) { list.innerHTML = ''; return; }
    list.innerHTML = workspaces.map(ws => `
      <div class="ws-switcher-item ${ws.slug === getWsSlug() ? 'active' : ''}"
        onclick="wsSwitcherSelect('${ws.slug}', ${ws.has_key})"
        data-slug="${ws.slug}">
        <div class="ws-switcher-slug">${esc(ws.slug)}</div>
        <div class="ws-switcher-name">${esc(ws.name)}</div>
        ${ws.has_key ? '<div class="ws-switcher-lock">🔒</div>' : '<div style="font-size:10px;color:var(--text-dim)">open</div>'}
      </div>`).join('');
  }).catch(() => { list.innerHTML = ''; });
}

function wsSwitcherSelect(slug, hasKey) {
  // highlight selection
  document.querySelectorAll('.ws-switcher-item').forEach(el => {
    el.classList.toggle('active', el.dataset.slug === slug);
  });
  _wsSwitcherPending = slug;
  if (hasKey) {
    $('ws-switcher-key-slug').textContent = slug;
    $('ws-switcher-key').value = slug === getWsSlug() ? getWsKey() : '';
    $('ws-switcher-key-wrap').style.display = '';
    setTimeout(() => $('ws-switcher-key').focus(), 50);
  } else {
    $('ws-switcher-key-wrap').style.display = 'none';
    doSwitchWorkspace(slug, '');
    closeWsSwitcher();
  }
}

function confirmWsSwitch() {
  if (!_wsSwitcherPending) return;
  const key = $('ws-switcher-key').value.trim();
  doSwitchWorkspace(_wsSwitcherPending, key);
  closeWsSwitcher();
}

function closeWsSwitcher() {
  $('ws-switcher-modal').style.display = 'none';
  _wsSwitcherPending = null;
}

/* ---------- mobile navigation drawer ---------- */

function toggleSidebar() {
  const open = document.body.classList.toggle('nav-open');
  $('sidebar-toggle')?.setAttribute('aria-expanded', open ? 'true' : 'false');
}

function closeSidebar() {
  document.body.classList.remove('nav-open');
  $('sidebar-toggle')?.setAttribute('aria-expanded', 'false');
}

/* ---------- accessibility wiring ---------- */

(function initA11y() {
  // decorative icons are skipped by screen readers
  document.querySelectorAll('svg').forEach(s => {
    if (!s.getAttribute('aria-label')) {
      s.setAttribute('aria-hidden', 'true');
      s.setAttribute('focusable', 'false');
    }
  });

  // icon-only buttons borrow their name from the title
  document.querySelectorAll('button').forEach(b => {
    if (b.getAttribute('aria-label') || b.textContent.trim()) return;
    const t = b.getAttribute('title');
    if (t) b.setAttribute('aria-label', t);
  });

  // give every field an accessible name when it has no associated label
  document.querySelectorAll('input, select, textarea').forEach(c => {
    if (c.type === 'hidden') return;
    if (c.getAttribute('aria-label') || c.getAttribute('aria-labelledby')) return;
    if (c.id && document.querySelector('label[for="' + c.id + '"]')) return;
    if (c.closest('label')) return;
    let name = '';
    const lab = c.parentElement && c.parentElement.querySelector('label');
    if (lab && !lab.contains(c)) name = lab.textContent.trim();
    if (!name) name = c.getAttribute('placeholder') || '';
    if (name) c.setAttribute('aria-label', name);
  });

  // modals announced as labelled dialogs
  document.querySelectorAll('.modal-overlay').forEach(ov => {
    const modal = ov.querySelector('.modal');
    if (!modal) return;
    modal.setAttribute('role', 'dialog');
    modal.setAttribute('aria-modal', 'true');
    const h = modal.querySelector('h3');
    if (h) {
      if (!h.id) h.id = ov.id + '-title';
      modal.setAttribute('aria-labelledby', h.id);
    }
  });

  // clickable spans/divs operable from the keyboard
  document.querySelectorAll('[onclick]').forEach(el => {
    if (el.matches('button, a, input, select, textarea, label, .modal-overlay, #sidebar-backdrop')) return;
    if (!el.hasAttribute('tabindex')) el.setAttribute('tabindex', '0');
    if (!el.hasAttribute('role')) el.setAttribute('role', 'button');
  });
  document.addEventListener('keydown', e => {
    const t = e.target;
    if ((e.key === 'Enter' || e.key === ' ') && t.getAttribute &&
        t.getAttribute('role') === 'button' && t.tagName !== 'BUTTON') {
      e.preventDefault();
      t.click();
    }
  });

  document.querySelector('.sidebar-nav-btn.active')?.setAttribute('aria-current', 'page');
  $('gen-settings-toggle')?.setAttribute('aria-expanded', 'false');
  document.querySelectorAll('.adv-toggle').forEach(b => b.setAttribute('aria-expanded', 'false'));

  // dialog focus management
  const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';
  let lastFocused = null;
  const dialogs = [...document.querySelectorAll('.modal-overlay'), $('page-login')].filter(Boolean);

  const isOpen = ov => ov.style.display !== 'none';
  const scopeOf = ov => ov.querySelector('.modal') || ov;
  const focusables = scope => [...scope.querySelectorAll(FOCUSABLE)].filter(el => el.offsetParent !== null);
  const topOpen = () => dialogs.filter(isOpen).pop() || null;

  dialogs.forEach(ov => {
    let shown = isOpen(ov);
    new MutationObserver(() => {
      const now = isOpen(ov);
      if (now === shown) return;
      shown = now;
      if (now) {
        lastFocused = document.activeElement;
        const f = focusables(scopeOf(ov));
        if (f.length) f[0].focus();
      } else if (lastFocused && document.contains(lastFocused)) {
        try { lastFocused.focus(); } catch (_) {}
        lastFocused = null;
      }
    }).observe(ov, { attributes: true, attributeFilter: ['style'] });
  });

  document.addEventListener('keydown', e => {
    if (e.key === 'Escape' && document.body.classList.contains('nav-open')) {
      closeSidebar();
      return;
    }
    const ov = topOpen();
    if (!ov) return;
    if (e.key === 'Escape' && ov.id !== 'page-login') {
      e.preventDefault();
      const cancel = [...ov.querySelectorAll('button')].find(b => /cancel|close|done/i.test(b.textContent));
      if (cancel) cancel.click(); else ov.style.display = 'none';
    } else if (e.key === 'Tab') {
      const f = focusables(scopeOf(ov));
      if (!f.length) return;
      const first = f[0], last = f[f.length - 1];
      if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
      else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
    }
  });
})();
