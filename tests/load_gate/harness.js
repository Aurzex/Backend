#!/usr/bin/env node
/*
 * harness.js — run the official NEMO/Kitten -> KN converters straight out of the
 * webpack bundles in temp/web/ (no browser, no npm deps).
 *
 * Usage:
 *   node harness.js <inputFile> <bcmVersion> <nemo|kitten> <outputFile> [--no-validate]
 *   node harness.js --validate <knJsonFile>            # only run BcmHelpers.validateBcm
 *   node harness.js --dump <json|keys|decompile-file>  # inspect a NEMO work file
 *
 * What it does:
 *   1. evaluates every temp/web/*.js chunk file inside a Node `vm` sandbox whose
 *      `self.webpackChunkneko` is a plain array, so the chunk `push()` calls just
 *      hand us their module maps;
 *   2. installs a minimal webpack runtime (`__webpack_require__` + .d/.r/.n/.o/.t/.g);
 *   3. `require(41888)` -> { nemoBcmToNekoBcmUtils, kittenBcmToNekoBcmUtils, ... }
 *      and `require(87123)` -> { validateBcm, ... } (BcmHelpers);
 *   4. converts, then validates the KN document with the official validator.
 *
 * The bundles are the same files the official editor loads; only the webpack
 * runtime is re-implemented (the module bodies and the conversion code are the
 * original ones, byte for byte).
 */
'use strict';

const fs = require('fs');
const path = require('path');
const vm = require('vm');
const dom = require('./dom.js');

// 官方 bundle 不入库:由调用方用 `BCM_WEB_DIR` 指向下载目录(默认 `tests/web`)。
const WEB_DIR = process.env.BCM_WEB_DIR
  ? path.resolve(process.env.BCM_WEB_DIR)
  : path.resolve(__dirname, '..', 'web');

// ---------------------------------------------------------------- sandbox ----

function makeSandbox(opts) {
  opts = opts || {};
  const sandbox = {};
  sandbox.console = opts.quiet ? quietConsole() : console;
  sandbox.self = sandbox;
  sandbox.window = sandbox;
  sandbox.globalThis = sandbox;
  sandbox.global = sandbox;
  sandbox.navigator = { userAgent: 'node-harness', platform: 'Linux x86_64', language: 'zh-CN' };
  sandbox.location = {
    href: 'https://creation.bcmcdn.com/neko/web/release/static/js/main.js',
    protocol: 'https:',
    host: 'creation.bcmcdn.com',
    hostname: 'creation.bcmcdn.com',
    origin: 'https://creation.bcmcdn.com',
    pathname: '/neko/web/release/editor.html',
    search: '',
    hash: '',
    port: '',
    toString() { return this.href; },
  };
  // The converters need a real XML DOM (DOMParser / XMLSerializer / selectors).
  const doc = dom.install(sandbox);
  sandbox.localStorage = { getItem: () => null, setItem() {}, removeItem() {} };
  sandbox.process = { env: { NODE_ENV: 'production' }, platform: 'linux', version: process.version, nextTick: process.nextTick.bind(process) };
  sandbox.crypto = globalThis.crypto || require('crypto').webcrypto;
  sandbox.setTimeout = setTimeout;
  sandbox.clearTimeout = clearTimeout;
  sandbox.setInterval = setInterval;
  sandbox.clearInterval = clearInterval;
  sandbox.queueMicrotask = queueMicrotask;
  sandbox.TextEncoder = TextEncoder;
  sandbox.TextDecoder = TextDecoder;
  sandbox.URL = URL;
  sandbox.URLSearchParams = URLSearchParams;
  sandbox.Blob = class Blob { constructor(parts, o) { this.parts = parts; this.type = (o && o.type) || ''; this.size = (parts || []).reduce((a, p) => a + (p && p.length ? p.length : 0), 0); } };
  sandbox.File = sandbox.Blob;
  sandbox.performance = { now: () => Date.now() };
  sandbox.atob = (s) => Buffer.from(s, 'base64').toString('binary');
  sandbox.btoa = (s) => Buffer.from(s, 'binary').toString('base64');
  sandbox.fetch = () => Promise.reject(new Error('harness: fetch disabled'));
  sandbox.XMLHttpRequest = function () { throw new Error('harness: XHR disabled'); };
  sandbox.requestAnimationFrame = (cb) => setTimeout(() => cb(Date.now()), 0);
  sandbox.cancelAnimationFrame = clearTimeout;
  sandbox.addEventListener = function () {};
  sandbox.removeEventListener = function () {};
  sandbox.__IS_KIDS__ = false;
  sandbox.__IS_HARDWARE__ = false;
  sandbox.__IS_MINI__ = false;
  sandbox.__IS_BROWSER__ = true;
  sandbox.__NEKO_TIME_OBJ__ = {};
  return sandbox;
}

function quietConsole() {
  const noop = () => {};
  return { log: noop, info: noop, warn: noop, error: noop, debug: noop, trace: noop, group: noop, groupEnd: noop, table: noop, dir: noop };
}

// Sentinel thrown from the sandboxed `webpackChunkneko.push` so that evaluating a
// chunk file stops right after its module map is handed to us (the chunk files
// also contain the webpack runtime + app bootstrap after the push; we do not want
// to run the React app).
const STOP = Symbol('chunk-registered');

// Load every webpack chunk file and collect its module map.
function loadChunkModules(ctx, files) {
  const modules = {};
  const chunks = {};
  const arr = (ctx.webpackChunkneko = []);
  arr.push = function (...args) {
    Array.prototype.push.apply(this, args);
    throw STOP;
  };
  for (const f of files) {
    const full = path.join(WEB_DIR, f);
    if (!fs.existsSync(full)) continue;
    const code = fs.readFileSync(full, 'utf8');
    if (code.indexOf('webpackChunkneko') !== -1) {
      try {
        vm.runInContext(code, ctx, { filename: full, timeout: 600000 });
      } catch (e) {
        if (e !== STOP) throw e;
      }
      for (const entry of arr) {
        const ids = entry[0];
        const map = entry[1];
        if (!map) continue;
        chunks[Array.isArray(ids) ? ids.join(',') : String(ids)] = Object.keys(map).length;
        for (const k of Object.keys(map)) if (!(k in modules)) modules[k] = map[k];
      }
      arr.length = 0;
    }
    // Entry bundle: `(function(){var __webpack_modules__={...},__webpack_module_cache__={}; ...})()`.
    // Lift the object literal out and evaluate it as an expression so the runtime
    // (and the app bootstrap that follows it) never runs.
    const marker = 'var __webpack_modules__=';
    const start = code.indexOf(marker);
    if (start !== -1) {
      const litStart = start + marker.length;
      const litEnd = code.indexOf(',__webpack_module_cache__=', litStart);
      if (litEnd === -1) throw new Error('cannot find end of __webpack_modules__ in ' + f);
      const literal = code.slice(litStart, litEnd);
      const map = vm.runInContext('(' + literal + ')', ctx, { filename: full + '#modules', timeout: 600000 });
      chunks['entry:' + f] = Object.keys(map).length;
      for (const k of Object.keys(map)) if (!(k in modules)) modules[k] = map[k];
    }
  }
  arr.push = Array.prototype.push.bind(arr);
  return { modules, chunks };
}

function makeRequire(ctx, modules) {
  const cache = Object.create(null);
  ctx.__webpack_module_cache__ = cache;
  const req = function (id) {
    const key = String(id);
    if (cache[key]) return cache[key].exports;
    const factory = modules[key];
    if (typeof factory !== 'function') {
      throw new Error('webpack module ' + key + ' not found (chunk not loaded)');
    }
    const module = (cache[key] = { id: key, loaded: false, exports: {} });
    const thrown = true;
    try {
      factory.call(module.exports, module, module.exports, req);
      module.loaded = true;
    } catch (err) {
      delete cache[key];
      throw err;
    }
    return module.exports;
  };
  req.d = function (exports, definition) {
    for (const key in definition) {
      if (Object.prototype.hasOwnProperty.call(definition, key) && !Object.prototype.hasOwnProperty.call(exports, key)) {
        Object.defineProperty(exports, key, { enumerable: true, get: definition[key] });
      }
    }
  };
  req.r = function (exports) {
    if (typeof Symbol !== 'undefined' && Symbol.toStringTag) {
      Object.defineProperty(exports, Symbol.toStringTag, { value: 'Module' });
    }
    Object.defineProperty(exports, '__esModule', { value: true });
  };
  req.n = function (m) {
    const getter = m && m.__esModule ? () => m['default'] : () => m;
    req.d(getter, { a: getter });
    return getter;
  };
  req.o = function (o, p) { return Object.prototype.hasOwnProperty.call(o, p); };
  req.nmd = function (module) {
    if (!module.paths) module.paths = [];
    if (!module.children) module.children = [];
    return module;
  };
  req.t = function (value, mode) {
    if (mode & 1) value = req(value);
    if (mode & 8) return value;
    if (mode & 4 && typeof value === 'object' && value && value.__esModule) return value;
    const ns = Object.create(null);
    req.r(ns);
    Object.defineProperty(ns, 'default', { enumerable: true, value });
    if (mode & 2 && typeof value !== 'string') {
      for (const key in value) {
        if (req.o(value, key)) req.d(ns, key, ((k) => () => value[k])(key));
      }
    }
    return ns;
  };
  req.g = ctx;
  req.p = 'https://creation.bcmcdn.com/neko/web/release/';
  req.u = (chunkId) => 'static/js/' + chunkId + '.js';
  req.miniCssF = (chunkId) => 'static/css/' + chunkId + '.css';
  req.e = (chunkId) => Promise.reject(new Error('harness: dynamic chunk ' + chunkId + ' not available'));
  req.bind = (chunkId) => () => { throw new Error('harness: dynamic chunk ' + chunkId + ' not available'); };
  req.m = modules;
  req.c = cache;
  return req;
}

const CHUNK_FILES = ['base.f158ddbf.js', 'crc_libs.8196dbd1.js', 'main-vendors.9b801394.js', 'main.js'];

let booted = null;
function boot(opts) {
  opts = opts || {};
  if (booted && !opts.reload) return booted;
  const sandbox = makeSandbox(opts);
  const ctx = vm.createContext(sandbox);
  // The editor page loads `reflect-metadata` before the app bundle; the bundle
  // itself only calls `Reflect.hasOwnMetadata`, so provide the polyfill.
  vm.runInContext(fs.readFileSync(path.join(__dirname, 'reflect_metadata.js'), 'utf8'), ctx, { filename: 'reflect_metadata.js' });
  const { modules, chunks } = loadChunkModules(ctx, CHUNK_FILES);
  const req = makeRequire(ctx, modules);
  booted = { ctx, modules, chunks, req };
  return booted;
}

function conv() {
  const { req } = boot();
  return req(41888);
}
function bcmHelpers() {
  const { req } = boot();
  return req(87123);
}

// ------------------------------------------------------------------ main -----

function readWork(file) {
  const raw = fs.readFileSync(file, 'utf8');
  let data;
  try {
    data = JSON.parse(raw);
  } catch (e) {
    // NEMO files are sometimes wrapped: {"data": "...json..."} or a plain blob.
    throw new Error('input is not JSON: ' + file + ' (' + raw.slice(0, 80) + ')');
  }
  if (data && data.data && typeof data.data === 'object' && data.data.actors) data = data.data;
  return data;
}

function summarizeDoc(doc, label) {
  const out = { label, topKeys: Object.keys(doc) };
  if (doc.actors && doc.actors.actorsDict) {
    const ids = Object.keys(doc.actors.actorsDict);
    out.actors = ids.length;
    out.sampleActor = doc.actors.actorsDict[ids[0]];
  }
  if (doc.scenes && doc.scenes.scenesDict) {
    out.scenes = Object.keys(doc.scenes.scenesDict).length;
    out.sortList = doc.scenes.sortList && doc.scenes.sortList.length;
  }
  if (doc.styles && doc.styles.stylesDict) {
    out.styles = Object.keys(doc.styles.stylesDict).length;
    out.firstStyle = doc.styles.stylesDict[Object.keys(doc.styles.stylesDict)[0]];
  }
  if (doc.stageSize) out.stageSize = doc.stageSize;
  return out;
}

function countBlocks(doc) {
  // KN document: blocks live under actors/scenes .. "blocks" arrays (nested).
  const counts = {};
  let total = 0;
  const walk = (nodes) => {
    if (!Array.isArray(nodes)) return;
    for (const n of nodes) {
      if (!n || typeof n !== 'object') continue;
      // KN "blockJson" node: { id, type, fields:{}, inputs:{}, ... }
      const t = n.type || n.blockType;
      if (typeof t === 'string' || typeof t === 'number') {
        const key = String(t);
        counts[key] = (counts[key] || 0) + 1;
        total++;
      }
      if (n.inputs) {
        for (const k of Object.keys(n.inputs)) {
          const v = n.inputs[k];
          if (Array.isArray(v)) walk(v);
          else if (v && typeof v === 'object' && Array.isArray(v.blocks)) walk(v.blocks);
        }
      }
      if (n.statements) walk(n.statements);
      if (n.blocks) walk(n.blocks);
      if (n.next) walk([n.next]);
      if (n.children) walk(n.children);
    }
  };
  for (const section of ['actors', 'scenes', 'styles']) {
    const d = doc[section];
    if (!d) continue;
    for (const dictKey of Object.keys(d)) {
      const dict = d[dictKey];
      if (!dict || typeof dict !== 'object') continue;
      for (const id of Object.keys(dict)) {
        const ent = dict[id];
        if (ent && Array.isArray(ent.blocks)) walk(ent.blocks);
      }
    }
  }
  return { total, counts };
}

function runConvert(inputFile, bcmVersion, kind, outputFile, doValidate) {
  const nemo = conv();
  const data = readWork(inputFile);
  console.log('== input:', inputFile);
  console.log('   top-level keys:', Object.keys(data).join(', '));
  if (data.actors) console.log('   actors.actors_dict:', Object.keys(data.actors.actors_dict || {}).length, '| sample actor keys:', Object.keys(Object.values(data.actors.actors_dict || {})[0] || {}).join(','));
  if (data.styles) console.log('   styles keys:', Object.keys(data.styles).join(','), '| stylesDict:', Object.keys(data.styles.stylesDict || data.styles.styles_dict || {}).length);
  if (data.scenes) console.log('   scenes keys:', Object.keys(data.scenes).join(','), '| scene count:', Object.keys(data.scenes.scenesDict || data.scenes.scenes_dict || {}).length);
  console.log('   stage_size/stageSize:', JSON.stringify(data.stage_size || data.stageSize));
  console.log('   app_version/application_version:', data.app_version || data.application_version, '| passed bcmVersion:', bcmVersion);

  const fn = kind === 'kitten' ? nemo.kittenBcmToNekoBcmUtils : nemo.nemoBcmToNekoBcmUtils;
  if (typeof fn !== 'function') throw new Error(kind + ' converter missing; exports: ' + Object.keys(nemo).join(','));
  const t0 = Date.now();
  const out = fn(data, bcmVersion);
  console.log('== converted in', Date.now() - t0, 'ms ->', kind);
  console.log('   output type:', typeof out, out && out.then ? '(promise)' : '');
  const promise = out && typeof out.then === 'function' ? out : Promise.resolve(out);
  return promise.then((doc) => {
    const s = summarizeDoc(doc, 'output');
    console.log('   output top-level keys:', s.topKeys.join(', '));
    if (s.actors !== undefined) console.log('   actors.actorsDict:', s.actors, '| scenes:', s.scenes, '| styles:', s.styles);
    console.log('   stageSize:', JSON.stringify(s.stageSize));
    if (outputFile) {
      fs.writeFileSync(outputFile, JSON.stringify(doc, null, 2));
      console.log('== wrote', outputFile, '(' + fs.statSync(outputFile).size + ' bytes)');
    }
    if (doValidate !== false) {
      const { validateBcm } = bcmHelpers();
      let verdict;
      try {
        verdict = validateBcm(doc);
      } catch (e) {
        verdict = 'THREW: ' + e.message;
      }
      console.log('== validateBcm ->', verdict);
      if (verdict !== true) process.exitCode = 3;
    }
    return doc;
  });
}

function main() {
  const argv = process.argv.slice(2);
  if (argv[0] === '--dump') {
    const data = readWork(argv[1]);
    console.log(JSON.stringify(data, null, 2).slice(0, 4000));
    return Promise.resolve();
  }
  if (argv[0] === '--validate') {
    const doc = JSON.parse(fs.readFileSync(argv[1], 'utf8'));
    const { validateBcm } = bcmHelpers();
    const v = validateBcm(doc);
    console.log('validateBcm ->', v);
    if (v !== true) process.exitCode = 3;
    return Promise.resolve();
  }
  if (argv[0] === '--exports') {
    const m = conv();
    for (const k of Object.keys(m)) console.log(k, typeof m[k]);
    return Promise.resolve();
  }
  const [input, version, kind, output, ...rest] = argv;
  if (!input || !version || !kind) {
    console.error('usage: node harness.js <inputFile> <bcmVersion> <nemo|kitten> <outputFile> [--no-validate]');
    process.exit(2);
  }
  return runConvert(input, version, kind, output, !rest.includes('--no-validate'));
}

if (require.main === module) {
  main().catch((e) => {
    console.error('ERROR:', e && e.stack || e);
    process.exit(1);
  });
} else {
  // Contract used by `src/core/convert/translate/mod.rs`:
  //   const h = require('<repo>/temp/harness/harness.js');
  //   const { validateBcm } = h.require(87123);
  // `require(id)` boots the bundles on first use (cached afterwards) and returns
  // the webpack module's exports.
  module.exports = {
    require: (id) => boot().req(id),
    boot,
    conv,
    bcmHelpers,
    makeSandbox,
    loadChunkModules,
    makeRequire,
    readWork,
    countBlocks,
    summarizeDoc,
    CHUNK_FILES,
  };
}
