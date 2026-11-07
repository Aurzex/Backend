/*
 * reflect_metadata.js — the `reflect-metadata` shim the editor bundle expects as a
 * global polyfill (the bundle only ships `Reflect.hasOwnMetadata` call sites, not
 * the polyfill itself).
 *
 * Layout follows reflect-metadata: one WeakMap `target -> Map(propertyKey -> Map(metadataKey -> value))`;
 * the 3-argument forms (no propertyKey) live in a dedicated bucket, so e.g.
 * `design:paramtypes` and `inversify:paramtypes` are distinct entries on the same
 * class. The non-`Own` variants walk the prototype chain.
 *
 * Loaded into the harness `vm` context with `vm.runInContext`, so it installs onto
 * that context's own `Reflect` object.
 */
'use strict';
(function installReflectMetadata() {
  if (typeof Reflect === 'undefined' || typeof WeakMap === 'undefined') return;
  if (typeof Reflect.hasOwnMetadata === 'function') return;

  const TARGET_BUCKET = '__target__';
  const registry = new WeakMap(); // target -> Map(propertyKey -> Map(metadataKey -> value))

  function assertTarget(target) {
    if (target === null || (typeof target !== 'object' && typeof target !== 'function')) {
      throw new TypeError('Reflect metadata: target must be an object');
    }
    return target;
  }
  function bucketOf(target, propertyKey, create) {
    let buckets = registry.get(target);
    if (!buckets) {
      if (!create) return null;
      buckets = new Map();
      registry.set(target, buckets);
    }
    const key = propertyKey === undefined || propertyKey === null ? TARGET_BUCKET : propertyKey;
    let map = buckets.get(key);
    if (!map) {
      if (!create) return null;
      map = new Map();
      buckets.set(key, map);
    }
    return map;
  }
  function hasOwn(metadataKey, target, propertyKey) {
    const map = bucketOf(target, propertyKey, false);
    return !!map && map.has(metadataKey);
  }
  function getOwn(metadataKey, target, propertyKey) {
    const map = bucketOf(target, propertyKey, false);
    return map ? map.get(metadataKey) : undefined;
  }
  function get(metadataKey, target, propertyKey) {
    if (hasOwn(metadataKey, target, propertyKey)) return getOwn(metadataKey, target, propertyKey);
    const parent = Object.getPrototypeOf(target);
    return parent === null ? undefined : get(metadataKey, parent, propertyKey);
  }
  function keysOwn(target, propertyKey) {
    const map = bucketOf(target, propertyKey, false);
    return map ? Array.from(map.keys()) : [];
  }

  Reflect.defineMetadata = function defineMetadata(metadataKey, metadataValue, target, propertyKey) {
    bucketOf(assertTarget(target), propertyKey, true).set(metadataKey, metadataValue);
  };
  Reflect.hasOwnMetadata = function hasOwnMetadata(metadataKey, target, propertyKey) {
    return hasOwn(metadataKey, assertTarget(target), propertyKey);
  };
  Reflect.hasMetadata = function hasMetadata(metadataKey, target, propertyKey) {
    return get(metadataKey, assertTarget(target), propertyKey) !== undefined ||
      hasOwn(metadataKey, target, propertyKey);
  };
  Reflect.getOwnMetadata = function getOwnMetadata(metadataKey, target, propertyKey) {
    return getOwn(metadataKey, assertTarget(target), propertyKey);
  };
  Reflect.getMetadata = function getMetadata(metadataKey, target, propertyKey) {
    return get(metadataKey, assertTarget(target), propertyKey);
  };
  Reflect.getOwnMetadataKeys = function getOwnMetadataKeys(target, propertyKey) {
    return keysOwn(assertTarget(target), propertyKey);
  };
  Reflect.getMetadataKeys = function getMetadataKeys(target, propertyKey) {
    const keys = keysOwn(assertTarget(target), propertyKey).slice();
    const parent = Object.getPrototypeOf(target);
    if (parent !== null) {
      for (const k of Reflect.getMetadataKeys(parent, propertyKey)) if (keys.indexOf(k) === -1) keys.push(k);
    }
    return keys;
  };
  Reflect.deleteMetadata = function deleteMetadata(metadataKey, target, propertyKey) {
    const map = bucketOf(assertTarget(target), propertyKey, false);
    return map ? map.delete(metadataKey) : false;
  };
  Reflect.metadata = function metadata(metadataKey, metadataValue) {
    return function decorator(target, propertyKey) {
      Reflect.defineMetadata(metadataKey, metadataValue, target, propertyKey);
    };
  };
})();
