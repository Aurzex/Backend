/*
 * dom.js — minimal XML DOM for the official converter harness.
 *
 * The official `BlockParser` (bundle module 41888) turns NEMO/Kitten `blocksXML`
 * into KN blockJson with `new DOMParser().parseFromString(xml, "text/xml")`,
 * `XMLSerializer`, `getAttribute/setAttribute`, `textContent`, `innerHTML`,
 * `appendChild/insertBefore`, and a handful of selectors:
 *
 *   :scope > value | :scope > empty | :scope > shadow | :scope > block
 *   :scope > field | :scope > next | :scope > mutation | :scope field[...]
 *   tag[attr]  tag[attr="v"]  tag[name="x"] > shadow[type="y"]  mutation
 *   procedures_2_parameter_shadow   block[type="..."] , block[type="..."]
 *
 * That is the whole CSS surface used by the conversion path, so this file
 * implements exactly: XML parsing/serialisation + those selectors. Anything the
 * converters do not reach throws loudly instead of silently returning junk.
 */
'use strict';

const ELEMENT = 1;
const TEXT = 3;
const COMMENT = 8;
const DOCUMENT = 9;
const FRAGMENT = 11;

const BASE_URL = 'https://creation.bcmcdn.com/neko/web/release/editor.html';

class AttrList extends Array {
  getNamedItem(name) {
    for (const a of this) if (a.name === name) return a;
    return null;
  }
  item(i) { return this[i] || null; }
  setNamedItem(attr) {
    const cur = this.getNamedItem(attr.name);
    if (cur) { cur.value = attr.value; return cur; }
    this.push(attr);
    return null;
  }
  removeNamedItem(name) {
    const i = this.findIndex((a) => a.name === name);
    if (i >= 0) return this.splice(i, 1)[0];
    return null;
  }
}

class Attr {
  constructor(name, value, ownerElement) {
    this.name = name;
    this.nodeName = name;
    this.localName = name;
    this.prefix = null;
    this.namespaceURI = null;
    this.specified = true;
    this.value = value === undefined ? '' : String(value);
    this.nodeValue = this.value;
    this.ownerElement = ownerElement || null;
    this.nodeType = 2;
  }
}

class Node {
  constructor(doc, nodeType, nodeName) {
    this.ownerDocument = doc;
    this.nodeType = nodeType;
    this.nodeName = nodeName;
    this.parentNode = null;
    this.childNodes = [];
  }
  get parentElement() { return this.parentNode && this.parentNode.nodeType === ELEMENT ? this.parentNode : null; }
  get firstChild() { return this.childNodes[0] || null; }
  get lastChild() { return this.childNodes[this.childNodes.length - 1] || null; }
  get nextSibling() {
    if (!this.parentNode) return null;
    const s = this.parentNode.childNodes;
    return s[s.indexOf(this) + 1] || null;
  }
  get previousSibling() {
    if (!this.parentNode) return null;
    const s = this.parentNode.childNodes;
    const i = s.indexOf(this);
    return i > 0 ? s[i - 1] : null;
  }
  get children() { return this.childNodes.filter((n) => n.nodeType === ELEMENT); }
  get nextElementSibling() {
    let n = this.nextSibling;
    while (n && n.nodeType !== ELEMENT) n = n.nextSibling;
    return n;
  }
  get previousElementSibling() {
    let n = this.previousSibling;
    while (n && n.nodeType !== ELEMENT) n = n.previousSibling;
    return n;
  }
  get childElementCount() { return this.children.length; }
  get firstElementChild() { return this.children[0] || null; }
  get lastElementChild() { const c = this.children; return c[c.length - 1] || null; }

  appendChild(node) {
    if (node.parentNode) node.parentNode.removeChild(node);
    node.parentNode = this;
    node.ownerDocument = this.ownerDocument || node.ownerDocument;
    this.childNodes.push(node);
    return node;
  }
  insertBefore(node, ref) {
    if (!ref) return this.appendChild(node);
    if (node.parentNode) node.parentNode.removeChild(node);
    const i = this.childNodes.indexOf(ref);
    if (i < 0) throw new Error('insertBefore: reference node is not a child');
    node.parentNode = this;
    node.ownerDocument = this.ownerDocument || node.ownerDocument;
    this.childNodes.splice(i, 0, node);
    return node;
  }
  removeChild(node) {
    const i = this.childNodes.indexOf(node);
    if (i < 0) throw new Error('removeChild: node is not a child');
    this.childNodes.splice(i, 1);
    node.parentNode = null;
    return node;
  }
  replaceChild(newNode, oldNode) {
    const i = this.childNodes.indexOf(oldNode);
    if (i < 0) throw new Error('replaceChild: node is not a child');
    if (newNode.parentNode) newNode.parentNode.removeChild(newNode);
    this.childNodes[i] = newNode;
    newNode.parentNode = this;
    oldNode.parentNode = null;
    return oldNode;
  }
  insertAdjacentElement(position, node) {
    const pos = String(position).toLowerCase();
    if (pos === 'beforebegin') this.parentNode.insertBefore(node, this);
    else if (pos === 'afterbegin') this.insertBefore(node, this.firstChild);
    else if (pos === 'beforeend') this.appendChild(node);
    else if (pos === 'afterend') {
      const next = this.nextSibling;
      if (next) this.parentNode.insertBefore(node, next);
      else this.parentNode.appendChild(node);
    } else throw new Error('insertAdjacentElement: bad position ' + position);
    return node;
  }
  contains(node) {
    if (node === this) return true;
    for (const c of this.childNodes) if (c.nodeType === ELEMENT && c.contains(node)) return true;
    return false;
  }
  cloneNode(deep) {
    const copy = this.nodeType === ELEMENT
      ? this.ownerDocument.createElement(this.nodeName)
      : new Node(this.ownerDocument, this.nodeType, this.nodeName);
    if (this.nodeType === ELEMENT) {
      for (const a of this.attributes) copy.setAttribute(a.name, a.value);
    } else {
      copy.nodeValue = this.nodeValue;
    }
    if (deep) for (const c of this.childNodes) copy.appendChild(c.cloneNode(true));
    return copy;
  }
  get textContent() {
    if (this.nodeType === TEXT || this.nodeType === COMMENT) return this.nodeValue;
    return this.childNodes.map((c) => c.textContent).join('');
  }
  set textContent(value) {
    if (this.nodeType === TEXT || this.nodeType === COMMENT) { this.nodeValue = String(value); return; }
    this.childNodes.length = 0;
    if (value !== '' && value !== null && value !== undefined) {
      this.appendChild(this.ownerDocument.createTextNode(String(value)));
    }
  }
  get innerHTML() { return this.childNodes.map(serialize).join(''); }
  set innerHTML(value) {
    this.childNodes.length = 0;
    const frag = parseFragment(String(value), this.ownerDocument);
    for (const n of frag.childNodes.slice()) this.appendChild(n);
  }
  get outerHTML() { return serialize(this); }
  querySelector(sel) { const r = querySelectorAll(this, sel); return r.length ? r[0] : null; }
  querySelectorAll(sel) { return querySelectorAll(this, sel); }
  getElementsByTagName(name) { return descendants(this).filter((n) => n.nodeType === ELEMENT && (name === '*' || n.nodeName === name)); }
  getElementsByTagNameNS(ns, name) { return this.getElementsByTagName(name); }
  getElementsByClassName() { throw new Error('dom.js: getElementsByClassName not implemented'); }
  toString() { return serialize(this); }
}

class Element extends Node {
  constructor(doc, name) {
    super(doc, ELEMENT, name);
    this.tagName = name;
    this.localName = name;
    this.prefix = null;
    this.namespaceURI = null;
    this.attributes = new AttrList();
  }
  getAttribute(name) {
    const a = this.attributes.getNamedItem(name);
    return a ? a.value : null;
  }
  hasAttribute(name) { return this.attributes.getNamedItem(name) !== null; }
  setAttribute(name, value) {
    const a = this.attributes.getNamedItem(name);
    if (a) a.value = a.nodeValue = String(value);
    else this.attributes.push(new Attr(name, value, this));
  }
  removeAttribute(name) { this.attributes.removeNamedItem(name); }
  getAttributeNames() { return this.attributes.map((a) => a.name); }
  // `document.createElement("a")` doubles as a URL parser in the bundles
  // (fabric.js / axios do it); mirror the browser reflection for `a`-like tags.
  resolveUrl() {
    const v = this.getAttribute('href');
    if (v === null) return null;
    try { return new URL(v, BASE_URL); } catch (e) { return null; }
  }
  get href() { const u = this.resolveUrl(); return u ? u.href : this.getAttribute('href') || ''; }
  set href(v) { this.setAttribute('href', String(v)); }
  get protocol() { const u = this.resolveUrl(); return u ? u.protocol : ''; }
  get host() { const u = this.resolveUrl(); return u ? u.host : ''; }
  get hostname() { const u = this.resolveUrl(); return u ? u.hostname : ''; }
  get port() { const u = this.resolveUrl(); return u ? u.port : ''; }
  get pathname() { const u = this.resolveUrl(); return u ? u.pathname : ''; }
  get search() { const u = this.resolveUrl(); return u ? u.search : ''; }
  get hash() { const u = this.resolveUrl(); return u ? u.hash : ''; }
  matches(sel) { return matchesCompound(this, sel); }
  closest(sel) {
    const groups = parseSelector(sel);
    for (let n = this; n && n.nodeType === ELEMENT; n = n.parentNode) {
      for (const steps of groups) if (matchFrom(n, steps, steps.length - 1, n)) return n;
    }
    return null;
  }
  get id() { return this.getAttribute('id') || ''; }
  get style() {
    if (!this.__style) this.__style = makeStyle();
    return this.__style;
  }
  getContext(type) {
    if (String(type).toLowerCase() === '2d') return makeCanvas2dContext(this);
    return null; // webgl etc. — callers in the converter path handle null
  }
  toDataURL() { return 'data:image/png;base64,'; }
  toBlob(cb) { if (cb) cb(null); }
  getBoundingClientRect() { return { x: 0, y: 0, top: 0, left: 0, right: 0, bottom: 0, width: this.width || 0, height: this.height || 0 }; }
  focus() {}
  blur() {}
  click() {}
  addEventListener() {}
  removeEventListener() {}
  dispatchEvent() { return true; }
  get width() { return this.__width === undefined ? 300 : this.__width; }
  set width(v) { this.__width = Number(v) || 0; }
  get height() { return this.__height === undefined ? 150 : this.__height; }
  set height(v) { this.__height = Number(v) || 0; }
  setAttributeNS(ns, name, value) { this.setAttribute(name, value); }
  getAttributeNS(ns, name) { return this.getAttribute(name); }
  removeAttributeNS(ns, name) { this.removeAttribute(name); }
}

function makeStyle() {
  return {
    setProperty(name, value) { this[name] = value; },
    getPropertyValue(name) { return this[name] || ''; },
    removeProperty(name) { delete this[name]; },
    cssText: '',
  };
}

function makeCanvas2dContext(canvas) {
  const noop = () => {};
  const ctx = {
    canvas,
    fillStyle: '#000000',
    strokeStyle: '#000000',
    lineWidth: 1,
    globalAlpha: 1,
    globalCompositeOperation: 'source-over',
    font: '10px sans-serif',
    textAlign: 'start',
    textBaseline: 'alphabetic',
    imageSmoothingEnabled: true,
    save: noop, restore: noop, scale: noop, rotate: noop, translate: noop, transform: noop,
    setTransform: noop, resetTransform: noop, clip: noop,
    beginPath: noop, closePath: noop, moveTo: noop, lineTo: noop, bezierCurveTo: noop,
    quadraticCurveTo: noop, arc: noop, arcTo: noop, rect: noop, ellipse: noop,
    fill: noop, stroke: noop, fillRect: noop, strokeRect: noop, clearRect: noop,
    fillText: noop, strokeText: noop,
    drawImage: noop,
    createLinearGradient: () => ({ addColorStop: noop }),
    createRadialGradient: () => ({ addColorStop: noop }),
    createPattern: () => null,
    measureText: (t) => ({ width: String(t).length, actualBoundingBoxAscent: 0, actualBoundingBoxDescent: 0 }),
    getImageData: (x, y, w, h) => ({ width: w, height: h, data: new Uint8ClampedArray(Math.max(0, w * h * 4)) }),
    putImageData: noop,
    createImageData: (w, h) => ({ width: w, height: h, data: new Uint8ClampedArray(Math.max(0, w * h * 4)) }),
    setLineDash: noop, getLineDash: () => [],
  };
  return ctx;
}

class Text extends Node {
  constructor(doc, value) {
    super(doc, TEXT, '#text');
    this.nodeValue = value;
  }
}

class Comment extends Node {
  constructor(doc, value) {
    super(doc, COMMENT, '#comment');
    this.nodeValue = value;
  }
}

class Document extends Node {
  constructor() {
    super(null, DOCUMENT, '#document');
    this.ownerDocument = this;
  }
  get documentElement() { return this.children[0] || null; }
  createElement(name) { return new Element(this, name); }
  createElementNS(ns, name) { return new Element(this, name); }
  createTextNode(value) { return new Text(this, String(value)); }
  createComment(value) { return new Comment(this, String(value)); }
  createAttribute(name) { return new Attr(name, '', null); }
  createDocumentFragment() { return new Node(this, FRAGMENT, '#document-fragment'); }
  get implementation() { throw new Error('dom.js: document.implementation not implemented'); }
}

// ------------------------------------------------------------- parsing ------

const NAMED_ENTITIES = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: '\u00a0' };

function decodeEntities(text) {
  return text.replace(/&(#x?[0-9a-fA-F]+|[a-zA-Z][a-zA-Z0-9]*);/g, (m, body) => {
    if (body[0] === '#') {
      const code = body[1] === 'x' || body[1] === 'X' ? parseInt(body.slice(2), 16) : parseInt(body.slice(1), 10);
      return Number.isFinite(code) ? String.fromCodePoint(code) : m;
    }
    return Object.prototype.hasOwnProperty.call(NAMED_ENTITIES, body) ? NAMED_ENTITIES[body] : m;
  });
}

function encodeText(text) {
  return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/\r/g, '&#13;');
}
function encodeAttr(value) {
  return String(value).replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function parseFragment(xml, doc) {
  const frag = doc.createDocumentFragment();
  const src = String(xml);
  const stack = [frag];
  const top = () => stack[stack.length - 1];
  let i = 0;
  let error = null;
  const fail = (msg) => { if (!error) error = msg + ' (at offset ' + i + ')'; };
  while (i < src.length) {
    const lt = src.indexOf('<', i);
    if (lt === -1) {
      const rest = src.slice(i);
      if (rest) top().appendChild(doc.createTextNode(decodeEntities(rest)));
      break;
    }
    if (lt > i) top().appendChild(doc.createTextNode(decodeEntities(src.slice(i, lt))));
    i = lt;
    if (src.startsWith('<!--', i)) {
      const end = src.indexOf('-->', i + 4);
      if (end === -1) { fail('unterminated comment'); break; }
      top().appendChild(new Comment(doc, src.slice(i + 4, end)));
      i = end + 3;
      continue;
    }
    if (src.startsWith('<![CDATA[', i)) {
      const end = src.indexOf(']]>', i + 9);
      if (end === -1) { fail('unterminated CDATA'); break; }
      top().appendChild(doc.createTextNode(src.slice(i + 9, end)));
      i = end + 3;
      continue;
    }
    if (src.startsWith('<?', i) || src.startsWith('<!', i)) {
      const end = src.indexOf('>', i);
      if (end === -1) { fail('unterminated processing instruction'); break; }
      i = end + 1;
      continue;
    }
    const gt = findTagEnd(src, i);
    if (gt === -1) { fail('unterminated tag'); break; }
    const raw = src.slice(i + 1, gt);
    i = gt + 1;
    if (raw[0] === '/') {
      const name = raw.slice(1).trim();
      const open = top();
      if (open === frag || open.nodeName !== name) {
        fail('mismatched closing tag </' + name + '>');
        // Recover like a browser does: close the nearest matching ancestor if any.
        const idx = stack.map((n) => n.nodeName).lastIndexOf(name);
        if (idx > 0) stack.length = idx;
        continue;
      }
      stack.pop();
      continue;
    }
    const selfClosing = raw.endsWith('/');
    const body = selfClosing ? raw.slice(0, -1) : raw;
    const m = /^([^\s/>]+)([\s\S]*)$/.exec(body);
    if (!m) { fail('bad tag'); continue; }
    const el = doc.createElement(m[1]);
    for (const a of parseAttrs(m[2])) el.setAttribute(a[0], a[1]);
    top().appendChild(el);
    if (!selfClosing) stack.push(el);
  }
  if (stack.length > 1 && !error) fail('unclosed tag ' + stack[stack.length - 1].nodeName);
  return { nodes: frag, error };
}

function findTagEnd(src, start) {
  let quote = null;
  for (let i = start + 1; i < src.length; i++) {
    const c = src[i];
    if (quote) { if (c === quote) quote = null; continue; }
    if (c === '"' || c === "'") { quote = c; continue; }
    if (c === '>') return i;
  }
  return -1;
}

function parseAttrs(text) {
  const out = [];
  const re = /([^\s=/>]+)(?:\s*=\s*("[^"]*"|'[^']*'|[^\s>]*))?/g;
  let m;
  while ((m = re.exec(text)) !== null) {
    if (!m[0].trim()) continue;
    let value = m[2] === undefined ? '' : m[2];
    if (value[0] === '"' || value[0] === "'") value = value.slice(1, -1);
    out.push([m[1], decodeEntities(value)]);
  }
  return out;
}

function makeErrorDocument(doc, message) {
  // Mirrors how a browser reports XML parse errors: <html><body><parsererror>.
  const html = doc.createElement('html');
  const body = doc.createElement('body');
  const err = doc.createElement('parsererror');
  err.textContent = message;
  body.appendChild(err);
  html.appendChild(body);
  doc.appendChild(html);
  return doc;
}

class DOMParser {
  parseFromString(text, type) {
    if (type !== undefined && type !== null && !/^\s*(text\/xml|application\/xml|application\/xhtml\+xml|text\/html)\s*$/i.test(type)) {
      throw new TypeError('DOMParser: unsupported type ' + type);
    }
    const doc = new Document();
    const { nodes, error } = parseFragment(text, doc);
    if (error) return makeErrorDocument(doc, error);
    const roots = nodes.childNodes.slice();
    const elements = roots.filter((n) => n.nodeType === ELEMENT);
    if (elements.length !== 1) {
      return makeErrorDocument(doc, 'document must have exactly one root element, got ' + elements.length);
    }
    const before = roots.slice(0, roots.indexOf(elements[0])).filter((n) => n.nodeType === ELEMENT);
    if (before.length) return makeErrorDocument(doc, 'junk before root element');
    doc.appendChild(elements[0]);
    return doc;
  }
}

function serialize(node) {
  if (node.nodeType === TEXT) return encodeText(node.nodeValue);
  if (node.nodeType === COMMENT) return '<!--' + node.nodeValue + '-->';
  if (node.nodeType === DOCUMENT || node.nodeType === FRAGMENT) {
    return node.childNodes.map(serialize).join('');
  }
  const attrs = node.attributes.map((a) => ' ' + a.name + '="' + encodeAttr(a.value) + '"').join('');
  if (!node.childNodes.length) return '<' + node.nodeName + attrs + '/>';
  return '<' + node.nodeName + attrs + '>' + node.childNodes.map(serialize).join('') + '</' + node.nodeName + '>';
}

class XMLSerializer {
  serializeToString(node) {
    if (node && node.nodeType === DOCUMENT) return node.documentElement ? serialize(node.documentElement) : '';
    return serialize(node);
  }
}

// ------------------------------------------------------------ selectors -----

function descendants(node) {
  const out = [];
  const walk = (n) => {
    for (const c of n.childNodes) {
      if (c.nodeType !== ELEMENT) continue;
      out.push(c);
      walk(c);
    }
  };
  walk(node);
  return out;
}

// Splits a comma-less selector into steps of {combinator, tag, attrs, scope}.
function parseCompound(text, combinator) {
  const step = { combinator, tag: null, attrs: [], scope: false };
  let s = text.trim();
  if (s.startsWith(':scope')) { step.scope = true; s = s.slice(6).trim(); }
  const m = /^([^\s\[>]*)/.exec(s);
  if (m && m[1] && m[1] !== '*') step.tag = m[1];
  s = s.slice(m ? m[1].length : 0);
  const re = /\[\s*([^\s\]=]+)\s*(?:([~^$*|]?=)\s*("[^"]*"|'[^']*'|[^\]]*))?\s*\]/g;
  let am;
  while ((am = re.exec(s)) !== null) {
    if (am[2] && am[2] !== '=') throw new Error('dom.js: unsupported attribute operator ' + am[2]);
    let v = am[3];
    if (v === undefined) v = null;
    else if ((v[0] === '"' && v[v.length - 1] === '"') || (v[0] === "'" && v[v.length - 1] === "'")) v = v.slice(1, -1);
    step.attrs.push({ name: am[1], value: v });
  }
  if (/\S/.test(s.replace(re, ''))) throw new Error('dom.js: unsupported selector syntax: ' + text);
  return step;
}

function parseSelector(selector) {
  const groups = [];
  for (const groupText of splitTopLevel(selector, ',')) {
    const steps = [];
    // Tokens are either a `>` combinator or a compound selector; whitespace
    // alone is the descendant combinator.
    const tokens = groupText.trim().split(/(\s*>\s*|\s+)/).filter((t) => t !== '');
    let pending = ' ';
    for (const t of tokens) {
      if (/^\s*>\s*$/.test(t)) { pending = '>'; continue; }
      if (/^\s+$/.test(t)) { if (steps.length) pending = ' '; continue; }
      steps.push(parseCompound(t, pending));
      pending = ' ';
    }
    if (steps.length) groups.push(steps);
  }
  return groups;
}

function splitTopLevel(text, sep, extra) {
  const out = [];
  let depth = 0;
  let quote = null;
  let cur = '';
  for (const c of text) {
    if (quote) { cur += c; if (c === quote) quote = null; continue; }
    if (c === '"' || c === "'") { quote = c; cur += c; continue; }
    if (c === '[') depth++;
    if (c === ']') depth--;
    if (depth === 0 && (c === sep || (extra && c === extra))) { out.push(cur); cur = ''; continue; }
    cur += c;
  }
  out.push(cur);
  return out;
}

function compoundMatches(node, step, scopeRoot) {
  if (node.nodeType !== ELEMENT) return false;
  if (step.scope && node !== scopeRoot) return false;
  if (step.tag && node.nodeName !== step.tag) return false;
  for (const a of step.attrs) {
    if (!node.hasAttribute(a.name)) return false;
    if (a.value !== null && node.getAttribute(a.name) !== a.value) return false;
  }
  return true;
}

function matchesCompound(node, selector) { // Element.matches
  const groups = parseSelector(selector);
  return groups.some((steps) => matchFrom(node, steps, steps.length - 1, node));
}

function matchFrom(node, steps, idx, scopeRoot) {
  if (!compoundMatches(node, steps[idx], scopeRoot)) return false;
  if (idx === 0) return true;
  const combinator = steps[idx].combinator;
  if (combinator === '>') {
    const p = node.parentNode;
    return !!p && matchFrom(p, steps, idx - 1, scopeRoot);
  }
  for (let p = node.parentNode; p; p = p.parentNode) {
    if (p.nodeType !== ELEMENT) continue;
    if (matchFrom(p, steps, idx - 1, scopeRoot)) return true;
  }
  return false;
}

function querySelectorAll(root, selector) {
  const scopeRoot = root.nodeType === DOCUMENT ? root.documentElement : root;
  const groups = parseSelector(selector);
  const candidates = root.nodeType === DOCUMENT
    ? (root.documentElement ? [root.documentElement, ...descendants(root)] : [])
    : descendants(root);
  const out = [];
  for (const node of candidates) {
    for (const steps of groups) {
      if (matchFrom(node, steps, steps.length - 1, scopeRoot)) { out.push(node); break; }
    }
  }
  return out;
}

function install(sandbox) {
  const doc = new Document();
  sandbox.DOMParser = DOMParser;
  sandbox.XMLSerializer = XMLSerializer;
  sandbox.document = doc;
  sandbox.Node = Node;
  sandbox.Element = Element;
  sandbox.Text = Text;
  return doc;
}

module.exports = { install, Document, DOMParser, XMLSerializer, parseFragment, serialize };
