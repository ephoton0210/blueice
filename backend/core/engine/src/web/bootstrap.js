// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The web platform a page script sees, written on top of the native DOM
// primitives (`__bi`, see dom_bridge.rs). Run once per document, before any
// page script. Everything here is ordinary JavaScript in the page's own realm:
// it can be inspected, shadowed and broken by the page without affecting the
// host, which validates every primitive it is handed.
(function (global) {
  'use strict';
  const bi = global.__bi;
  delete global.__bi;
  const startedAt = Date.now();
  const nowMs = () => Date.now() - startedAt;

  // ---------------------------------------------------------------- helpers
  const camelToKebab = (s) => s.replace(/[A-Z]/g, (c) => '-' + c.toLowerCase());
  const kebabToCamel = (s) => s.replace(/-([a-z])/g, (_, c) => c.toUpperCase());
  const isNode = (v) => v instanceof Node;
  const toStr = (v) => (v === null || v === undefined ? '' : String(v));
  const reportError = (e) => {
    try {
      console.error(e && e.stack ? e.stack : String(e));
    } catch (_) {}
  };
  const define = (target, props) => {
    for (const k of Object.keys(props)) {
      Object.defineProperty(target, k, { value: props[k], writable: true, configurable: true, enumerable: false });
    }
  };
  const getters = (proto, props) => {
    for (const k of Object.keys(props)) {
      const d = props[k];
      Object.defineProperty(proto, k, {
        get: typeof d === 'function' ? d : d.get,
        set: typeof d === 'function' ? undefined : d.set,
        configurable: true,
        enumerable: true,
      });
    }
  };

  // ----------------------------------------------------------------- events
  class Event {
    constructor(type, init) {
      init = init || {};
      this.type = String(type);
      this.bubbles = !!init.bubbles;
      this.cancelable = !!init.cancelable;
      this.composed = !!init.composed;
      this.defaultPrevented = false;
      this.isTrusted = false;
      this.timeStamp = nowMs();
      this.target = null;
      this.currentTarget = null;
      this.srcElement = null;
      this.eventPhase = 0;
      this.returnValue = true;
      this.cancelBubble = false;
      this._stop = false;
      this._stopNow = false;
      this._path = [];
    }
    stopPropagation() {
      this._stop = true;
      this.cancelBubble = true;
    }
    stopImmediatePropagation() {
      this._stop = true;
      this._stopNow = true;
      this.cancelBubble = true;
    }
    preventDefault() {
      if (this.cancelable) {
        this.defaultPrevented = true;
        this.returnValue = false;
      }
    }
    composedPath() {
      return this._path.slice();
    }
    initEvent(type, bubbles, cancelable) {
      this.type = String(type);
      this.bubbles = !!bubbles;
      this.cancelable = !!cancelable;
    }
  }
  Event.NONE = 0;
  Event.CAPTURING_PHASE = 1;
  Event.AT_TARGET = 2;
  Event.BUBBLING_PHASE = 3;
  class CustomEvent extends Event {
    constructor(type, init) {
      super(type, init);
      this.detail = init && 'detail' in init ? init.detail : null;
    }
  }
  class UIEvent extends Event {
    constructor(type, init) {
      super(type, init);
      this.view = (init && init.view) || null;
      this.detail = (init && init.detail) || 0;
    }
  }
  class MouseEvent extends UIEvent {
    constructor(type, init) {
      super(type, init);
      init = init || {};
      this.clientX = init.clientX || 0;
      this.clientY = init.clientY || 0;
      this.screenX = init.screenX || 0;
      this.screenY = init.screenY || 0;
      this.pageX = init.pageX || this.clientX;
      this.pageY = init.pageY || this.clientY;
      this.button = init.button || 0;
      this.buttons = init.buttons || 0;
      this.ctrlKey = !!init.ctrlKey;
      this.shiftKey = !!init.shiftKey;
      this.altKey = !!init.altKey;
      this.metaKey = !!init.metaKey;
      this.relatedTarget = init.relatedTarget || null;
    }
  }
  class PointerEvent extends MouseEvent {
    constructor(type, init) {
      super(type, init);
      this.pointerId = (init && init.pointerId) || 1;
      this.pointerType = (init && init.pointerType) || 'mouse';
    }
  }
  class KeyboardEvent extends UIEvent {
    constructor(type, init) {
      super(type, init);
      init = init || {};
      this.key = init.key || '';
      this.code = init.code || '';
      this.repeat = !!init.repeat;
      this.ctrlKey = !!init.ctrlKey;
      this.shiftKey = !!init.shiftKey;
      this.altKey = !!init.altKey;
      this.metaKey = !!init.metaKey;
    }
  }
  class FocusEvent extends UIEvent {
    constructor(type, init) {
      super(type, init);
      this.relatedTarget = (init && init.relatedTarget) || null;
    }
  }
  class InputEvent extends UIEvent {
    constructor(type, init) {
      super(type, init);
      this.data = (init && init.data) || null;
      this.inputType = (init && init.inputType) || '';
    }
  }
  class MessageEvent extends Event {
    constructor(type, init) {
      super(type, init);
      init = init || {};
      this.data = 'data' in init ? init.data : null;
      this.origin = init.origin || '';
      this.source = init.source || null;
      this.ports = init.ports || [];
    }
  }
  class PopStateEvent extends Event {
    constructor(type, init) {
      super(type, init);
      this.state = init && 'state' in init ? init.state : null;
    }
  }
  class HashChangeEvent extends Event {
    constructor(type, init) {
      super(type, init);
      this.oldURL = (init && init.oldURL) || '';
      this.newURL = (init && init.newURL) || '';
    }
  }
  class ErrorEvent extends Event {
    constructor(type, init) {
      super(type, init);
      init = init || {};
      this.message = init.message || '';
      this.error = init.error || null;
      this.filename = init.filename || '';
    }
  }
  class ProgressEvent extends Event {
    constructor(type, init) {
      super(type, init);
      init = init || {};
      this.loaded = init.loaded || 0;
      this.total = init.total || 0;
      this.lengthComputable = !!init.lengthComputable;
    }
  }
  class TransitionEvent extends Event {}
  class AnimationEvent extends Event {}
  class WheelEvent extends MouseEvent {}
  class TouchEvent extends UIEvent {}

  const listenersOf = new WeakMap();
  const registry = (target) => {
    let r = listenersOf.get(target);
    if (!r) {
      r = [];
      listenersOf.set(target, r);
    }
    return r;
  };

  class EventTarget {
    addEventListener(type, callback, options) {
      if (callback === null || callback === undefined) return;
      const capture = typeof options === 'boolean' ? options : !!(options && options.capture);
      const once = !!(options && typeof options === 'object' && options.once);
      const passive = !!(options && typeof options === 'object' && options.passive);
      const list = registry(this);
      type = String(type);
      for (const l of list) if (l.type === type && l.callback === callback && l.capture === capture) return;
      const entry = { type, callback, capture, once, passive, removed: false };
      list.push(entry);
      const signal = options && typeof options === 'object' ? options.signal : null;
      if (signal) {
        if (signal.aborted) {
          entry.removed = true;
          list.pop();
        } else {
          signal.addEventListener('abort', () => this.removeEventListener(type, callback, options));
        }
      }
    }
    removeEventListener(type, callback, options) {
      const capture = typeof options === 'boolean' ? options : !!(options && options.capture);
      const list = registry(this);
      for (let i = 0; i < list.length; i++) {
        const l = list[i];
        if (l.type === String(type) && l.callback === callback && l.capture === capture) {
          l.removed = true;
          list.splice(i, 1);
          return;
        }
      }
    }
    dispatchEvent(event) {
      if (!(event instanceof Event)) throw new TypeError('dispatchEvent needs an Event');
      event.target = this;
      event.srcElement = this;
      // The path: this target, its ancestors, then the window.
      const path = [];
      let node = this;
      while (node) {
        path.push(node);
        node = node._eventParent ? node._eventParent() : null;
      }
      event._path = path;
      event._stop = false;
      event._stopNow = false;
      const invoke = (current, phase) => {
        if (event._stop && phase !== 2) return;
        event.currentTarget = current;
        event.eventPhase = phase;
        const list = registry(current).slice();
        for (const l of list) {
          if (l.removed || l.type !== event.type) continue;
          if (phase === 1 && !l.capture) continue;
          if (phase === 3 && l.capture) continue;
          if (l.once) current.removeEventListener(l.type, l.callback, l.capture);
          try {
            if (typeof l.callback === 'function') l.callback.call(current, event);
            else if (l.callback && typeof l.callback.handleEvent === 'function') l.callback.handleEvent(event);
          } catch (e) {
            reportError(e);
          }
          if (event._stopNow) return;
        }
      };
      for (let i = path.length - 1; i > 0; i--) {
        invoke(path[i], 1);
        if (event._stop) break;
      }
      if (!event._stop) {
        invoke(path[0], 2);
      }
      if (event.bubbles) {
        for (let i = 1; i < path.length; i++) {
          if (event._stop) break;
          invoke(path[i], 3);
        }
      }
      event.currentTarget = null;
      event.eventPhase = 0;
      return !event.defaultPrevented;
    }
  }

  // `onclick`-style handler properties, each one listener held in place.
  const EVENT_HANDLERS = [
    'abort', 'animationend', 'animationiteration', 'animationstart', 'auxclick', 'beforeinput', 'blur', 'canplay',
    'change', 'click', 'close', 'contextmenu', 'copy', 'cut', 'dblclick', 'drag', 'dragend', 'dragenter', 'dragleave',
    'dragover', 'dragstart', 'drop', 'error', 'focus', 'focusin', 'focusout', 'input', 'invalid', 'keydown', 'keypress',
    'keyup', 'load', 'loadeddata', 'loadedmetadata', 'mousedown', 'mouseenter', 'mouseleave', 'mousemove', 'mouseout',
    'mouseover', 'mouseup', 'paste', 'pause', 'play', 'pointercancel', 'pointerdown', 'pointerenter', 'pointerleave',
    'pointermove', 'pointerout', 'pointerover', 'pointerup', 'reset', 'resize', 'scroll', 'select', 'submit',
    'touchcancel', 'touchend', 'touchmove', 'touchstart', 'transitionend', 'wheel',
  ];
  const WINDOW_HANDLERS = [
    'beforeunload', 'hashchange', 'languagechange', 'message', 'offline', 'online', 'pagehide', 'pageshow', 'popstate',
    'storage', 'unhandledrejection', 'unload', 'visibilitychange', 'DOMContentLoaded', 'readystatechange',
  ];
  const installHandlerProperties = (proto, names) => {
    for (const name of names) {
      const slot = '_on' + name;
      Object.defineProperty(proto, 'on' + name, {
        configurable: true,
        enumerable: true,
        get() {
          const entry = this[slot];
          return entry ? entry.callback : null;
        },
        set(fn) {
          const old = this[slot];
          if (old) {
            this.removeEventListener(name, old.callback);
            this[slot] = null;
          }
          if (typeof fn === 'function') {
            this.addEventListener(name, fn);
            this[slot] = { callback: fn };
          }
        },
      });
    }
  };

  // -------------------------------------------------------------- mutations
  const observers = new Set();
  let deliveryQueued = false;
  const queueRecord = (record) => {
    let any = false;
    for (const o of observers) {
      if (o._wants(record)) {
        o._records.push(record);
        any = true;
      }
    }
    if (any && !deliveryQueued) {
      deliveryQueued = true;
      queueMicrotask(() => {
        deliveryQueued = false;
        for (const o of Array.from(observers)) o._deliver();
      });
    }
  };
  class MutationRecord {
    constructor(init) {
      this.type = init.type;
      this.target = init.target;
      this.addedNodes = init.addedNodes || [];
      this.removedNodes = init.removedNodes || [];
      this.previousSibling = init.previousSibling || null;
      this.nextSibling = init.nextSibling || null;
      this.attributeName = init.attributeName || null;
      this.attributeNamespace = null;
      this.oldValue = init.oldValue === undefined ? null : init.oldValue;
    }
  }
  class MutationObserver {
    constructor(callback) {
      if (typeof callback !== 'function') throw new TypeError('MutationObserver needs a callback');
      this._callback = callback;
      this._records = [];
      this._targets = [];
    }
    observe(target, options) {
      options = options || {};
      const entry = {
        target,
        childList: !!options.childList,
        subtree: !!options.subtree,
        attributes: !!options.attributes || (options.attributeFilter !== undefined && options.attributes !== false),
        characterData: !!options.characterData,
        attributeOldValue: !!options.attributeOldValue,
        characterDataOldValue: !!options.characterDataOldValue,
        attributeFilter: options.attributeFilter || null,
      };
      this._targets = this._targets.filter((t) => t.target !== target);
      this._targets.push(entry);
      observers.add(this);
    }
    disconnect() {
      this._targets = [];
      this._records = [];
      observers.delete(this);
    }
    takeRecords() {
      const r = this._records;
      this._records = [];
      return r;
    }
    _wants(record) {
      for (const t of this._targets) {
        const inScope = t.target === record.target || (t.subtree && t.target.contains(record.target));
        if (!inScope) continue;
        if (record.type === 'childList' && t.childList) return true;
        if (record.type === 'attributes' && t.attributes) {
          if (!t.attributeFilter || t.attributeFilter.includes(record.attributeName)) return true;
        }
        if (record.type === 'characterData' && t.characterData) return true;
      }
      return false;
    }
    _deliver() {
      const records = this.takeRecords();
      if (records.length === 0) return;
      try {
        this._callback.call(this, records, this);
      } catch (e) {
        reportError(e);
      }
    }
  }

  // -------------------------------------------------------------- the nodes
  const wrappers = new Map();
  const customRegistry = new Map();
  const customByCtor = new Map();
  let upgradeTarget = null;

  const ELEMENT_CLASSES = new Map();
  const wrap = (id) => {
    if (id === null || id === undefined || id < 0) return null;
    let w = wrappers.get(id);
    if (w) return w;
    const kind = bi('kind', id);
    if (kind === 1) {
      const tag = bi('tag', id);
      const Ctor = customRegistry.has(tag) ? Element : ELEMENT_CLASSES.get(tag) || (tag.includes('-') ? HTMLElement : HTMLUnknownElement);
      w = Object.create(Ctor.prototype);
      Object.defineProperty(w, '_id', { value: id });
      wrappers.set(id, w);
      if (customRegistry.has(tag)) upgrade(w, customRegistry.get(tag));
    } else {
      const Ctor = kind === 3 ? Text : kind === 8 ? Comment : kind === 11 ? DocumentFragment : Document;
      w = Object.create(Ctor.prototype);
      Object.defineProperty(w, '_id', { value: id });
      wrappers.set(id, w);
    }
    return w;
  };
  const idOf = (node) => {
    if (!isNode(node)) throw new TypeError('The argument is not a Node');
    return node._id;
  };

  const invokeCustom = (el, name, ...args) => {
    const fn = el[name];
    if (typeof fn === 'function') {
      try {
        fn.apply(el, args);
      } catch (e) {
        reportError(e);
      }
    }
  };
  // Runs a custom element's constructor on an element that already exists.
  const upgrade = (el, definition) => {
    if (el._upgraded) return;
    Object.defineProperty(el, '_upgraded', { value: true, configurable: true });
    const previous = upgradeTarget;
    upgradeTarget = el;
    try {
      Object.setPrototypeOf(el, definition.ctor.prototype);
      Reflect.construct(definition.ctor, [], definition.ctor);
    } catch (e) {
      reportError(e);
    } finally {
      upgradeTarget = previous;
    }
    if (el.isConnected) {
      if (definition.observed.length) {
        for (const [k, v] of JSON.parse(bi('attrs', el._id))) {
          if (definition.observed.includes(k)) invokeCustom(el, 'attributeChangedCallback', k, null, v, null);
        }
      }
      invokeCustom(el, 'connectedCallback');
    }
  };
  const walkElements = (root, fn) => {
    if (root.nodeType === 1) fn(root);
    for (const id of JSON.parse(bi('children', root._id))) {
      const child = wrap(id);
      if (child && child.nodeType === 1) walkElements(child, fn);
    }
  };
  const connectedCallbacks = (node, connected) => {
    if (!customRegistry.size && !customByCtor.size) return;
    walkElements(node, (el) => {
      const def = customRegistry.get(el.localName);
      if (!def) return;
      if (connected) {
        if (!el._upgraded) upgrade(el, def);
        else invokeCustom(el, 'connectedCallback');
      } else if (el._upgraded) {
        invokeCustom(el, 'disconnectedCallback');
      }
    });
  };

  // Scripts inserted through the DOM run (inline now, external when the host
  // has fetched them); parser-inserted and innerHTML scripts never come here.
  const SCRIPT_TYPES = ['', 'text/javascript', 'application/javascript', 'text/ecmascript', 'application/ecmascript'];
  const runInsertedScripts = (root) => {
    walkElements(root, (el) => {
      if (el.localName !== 'script' || el._ran) return;
      const type = (el.getAttribute('type') || '').trim().toLowerCase();
      if (!SCRIPT_TYPES.includes(type)) return;
      Object.defineProperty(el, '_ran', { value: true });
      const src = el.getAttribute('src');
      if (src) {
        bi('loadScript', el._id, resolveUrl(src));
        return;
      }
      const previous = document._currentScript;
      document._currentScript = el;
      try {
        (0, eval)(el.textContent);
      } catch (e) {
        reportError(e);
      } finally {
        document._currentScript = previous || null;
      }
    });
  };

  class Node extends EventTarget {
    constructor() {
      super();
      throw new TypeError('Illegal constructor');
    }
    _eventParent() {
      const p = this.parentNode;
      if (p) return p;
      return this === document ? global : null;
    }
    get nodeType() {
      return bi('kind', this._id);
    }
    get nodeName() {
      const k = this.nodeType;
      if (k === 1) return bi('tag', this._id).toUpperCase();
      return k === 3 ? '#text' : k === 8 ? '#comment' : k === 11 ? '#document-fragment' : '#document';
    }
    get parentNode() {
      return wrap(bi('parent', this._id));
    }
    get parentElement() {
      const p = this.parentNode;
      return p && p.nodeType === 1 ? p : null;
    }
    get childNodes() {
      return JSON.parse(bi('children', this._id)).map(wrap);
    }
    get firstChild() {
      return wrap(bi('firstChild', this._id));
    }
    get lastChild() {
      return wrap(bi('lastChild', this._id));
    }
    get nextSibling() {
      return wrap(bi('nextSibling', this._id));
    }
    get previousSibling() {
      return wrap(bi('prevSibling', this._id));
    }
    get isConnected() {
      return bi('connected', this._id);
    }
    get ownerDocument() {
      return this.nodeType === 9 ? null : document;
    }
    get textContent() {
      return this.nodeType === 9 ? null : bi('textContent', this._id);
    }
    set textContent(v) {
      const k = this.nodeType;
      if (k === 3 || k === 8) {
        this.data = v;
        return;
      }
      if (k === 9) return;
      const removed = this.childNodes;
      bi('setTextContent', this._id, toStr(v));
      queueRecord(new MutationRecord({ type: 'childList', target: this, removedNodes: removed, addedNodes: this.childNodes }));
    }
    get nodeValue() {
      const k = this.nodeType;
      return k === 3 || k === 8 ? bi('data', this._id) : null;
    }
    set nodeValue(v) {
      if (this.nodeType === 3 || this.nodeType === 8) this.data = v;
    }
    hasChildNodes() {
      return bi('firstChild', this._id) !== null;
    }
    contains(other) {
      if (!other) return false;
      for (let n = other; n; n = n.parentNode) if (n === this) return true;
      return false;
    }
    getRootNode() {
      let n = this;
      while (n.parentNode) n = n.parentNode;
      return n;
    }
    appendChild(child) {
      return this.insertBefore(child, null);
    }
    insertBefore(child, reference) {
      const wasConnected = child.isConnected;
      const fragmentKids = child.nodeType === 11 ? child.childNodes : null;
      const previousParent = child.parentNode;
      const previousSibling = reference ? reference.previousSibling : this.lastChild;
      bi('insertBefore', this._id, idOf(child), reference ? idOf(reference) : null);
      const added = fragmentKids || [child];
      if (previousParent && !fragmentKids) {
        queueRecord(new MutationRecord({ type: 'childList', target: previousParent, removedNodes: [child] }));
      }
      queueRecord(new MutationRecord({ type: 'childList', target: this, addedNodes: added, previousSibling: previousSibling, nextSibling: reference || null }));
      if (this.isConnected) {
        for (const n of added) connectedCallbacks(n, true);
        for (const n of added) if (n.nodeType === 1) runInsertedScripts(n);
      } else if (wasConnected && !fragmentKids) connectedCallbacks(child, false);
      return child;
    }
    removeChild(child) {
      if (child.parentNode !== this) {
        throw new DOMException('The node to be removed is not a child of this node.', 'NotFoundError');
      }
      const wasConnected = child.isConnected;
      const previousSibling = child.previousSibling;
      const nextSibling = child.nextSibling;
      bi('remove', idOf(child));
      queueRecord(new MutationRecord({ type: 'childList', target: this, removedNodes: [child], previousSibling, nextSibling }));
      if (wasConnected) connectedCallbacks(child, false);
      return child;
    }
    replaceChild(next, old) {
      this.insertBefore(next, old);
      this.removeChild(old);
      return old;
    }
    cloneNode(deep) {
      return wrap(bi('clone', this._id, !!deep));
    }
    isEqualNode(other) {
      return !!other && this.nodeType === other.nodeType && this.nodeName === other.nodeName && (this.nodeType === 1 ? this.outerHTML === other.outerHTML : this.nodeValue === other.nodeValue);
    }
    isSameNode(other) {
      return this === other;
    }
    normalize() {}
    compareDocumentPosition(other) {
      if (this === other) return 0;
      if (this.contains(other)) return 20;
      if (other.contains(this)) return 10;
      const all = JSON.parse(bi('queryAll', document._id, '*'));
      const a = all.indexOf(this._id);
      const b = all.indexOf(other._id);
      return a < b ? 4 : 2;
    }
  }
  Object.assign(Node, {
    ELEMENT_NODE: 1, ATTRIBUTE_NODE: 2, TEXT_NODE: 3, CDATA_SECTION_NODE: 4, PROCESSING_INSTRUCTION_NODE: 7,
    COMMENT_NODE: 8, DOCUMENT_NODE: 9, DOCUMENT_TYPE_NODE: 10, DOCUMENT_FRAGMENT_NODE: 11,
    DOCUMENT_POSITION_DISCONNECTED: 1, DOCUMENT_POSITION_PRECEDING: 2, DOCUMENT_POSITION_FOLLOWING: 4,
    DOCUMENT_POSITION_CONTAINS: 8, DOCUMENT_POSITION_CONTAINED_BY: 16,
  });

  class CharacterData extends Node {
    get data() {
      return bi('data', this._id);
    }
    set data(v) {
      const old = this.data;
      bi('setData', this._id, toStr(v));
      queueRecord(new MutationRecord({ type: 'characterData', target: this, oldValue: old }));
    }
    get length() {
      return this.data.length;
    }
    get textContent() {
      return this.data;
    }
    set textContent(v) {
      this.data = v;
    }
    appendData(s) {
      this.data = this.data + s;
    }
    substringData(offset, count) {
      return this.data.substr(offset, count);
    }
    remove() {
      if (this.parentNode) this.parentNode.removeChild(this);
    }
    before(...nodes) {
      insertNodes(this.parentNode, this, nodes);
    }
    after(...nodes) {
      insertNodes(this.parentNode, this.nextSibling, nodes);
    }
    replaceWith(...nodes) {
      const parent = this.parentNode;
      if (!parent) return;
      insertNodes(parent, this, nodes);
      parent.removeChild(this);
    }
  }
  class Text extends CharacterData {
    constructor(data) {
      super();
    }
    get wholeText() {
      return this.data;
    }
    splitText(offset) {
      const rest = document.createTextNode(this.data.slice(offset));
      this.data = this.data.slice(0, offset);
      if (this.parentNode) this.parentNode.insertBefore(rest, this.nextSibling);
      return rest;
    }
  }
  class Comment extends CharacterData {}
  const toNode = (n) => (isNode(n) ? n : document.createTextNode(toStr(n)));
  const insertNodes = (parent, reference, nodes) => {
    if (!parent) return;
    const fragment = document.createDocumentFragment();
    for (const n of nodes) fragment.appendChild(toNode(n));
    parent.insertBefore(fragment, reference);
  };

  class ParentNodeMixin {}
  const parentNodeMethods = {
    get children() {
      return this.childNodes.filter((n) => n.nodeType === 1);
    },
    get childElementCount() {
      return this.children.length;
    },
    get firstElementChild() {
      return this.children[0] || null;
    },
    get lastElementChild() {
      const c = this.children;
      return c[c.length - 1] || null;
    },
    querySelector(selector) {
      return wrap(bi('query', this._id, toStr(selector)));
    },
    querySelectorAll(selector) {
      return JSON.parse(bi('queryAll', this._id, toStr(selector))).map(wrap);
    },
    append(...nodes) {
      for (const n of nodes) this.appendChild(toNode(n));
    },
    prepend(...nodes) {
      const first = this.firstChild;
      for (const n of nodes) this.insertBefore(toNode(n), first);
    },
    replaceChildren(...nodes) {
      for (const k of this.childNodes) this.removeChild(k);
      this.append(...nodes);
    },
    getElementsByTagName(name) {
      return this.querySelectorAll(name === '*' ? '*' : name);
    },
    getElementsByClassName(names) {
      const sel = toStr(names)
        .split(/\s+/)
        .filter(Boolean)
        .map((c) => '.' + CSS.escape(c))
        .join('');
      return sel ? this.querySelectorAll(sel) : [];
    },
  };
  const mixIn = (proto, source) => {
    for (const key of Object.getOwnPropertyNames(source)) {
      Object.defineProperty(proto, key, Object.getOwnPropertyDescriptor(source, key));
    }
  };

  class DocumentFragment extends Node {
    constructor() {
      super();
    }
    getElementById(id) {
      return this.querySelector('#' + CSS.escape(id));
    }
  }
  mixIn(DocumentFragment.prototype, parentNodeMethods);

  // ------------------------------------------------------ style, classList
  const parseStyle = (text) => {
    const map = new Map();
    for (const part of toStr(text).split(';')) {
      const i = part.indexOf(':');
      if (i < 0) continue;
      const k = part.slice(0, i).trim().toLowerCase();
      const v = part.slice(i + 1).trim();
      if (k) map.set(k, v);
    }
    return map;
  };
  const styleText = (map) => Array.from(map, ([k, v]) => k + ': ' + v + ';').join(' ');
  const makeStyle = (el) => {
    const read = () => parseStyle(bi('getAttr', el._id, 'style'));
    const write = (map) => {
      const text = styleText(map);
      if (text) bi('setAttr', el._id, 'style', text);
      else bi('removeAttr', el._id, 'style');
      queueRecord(new MutationRecord({ type: 'attributes', target: el, attributeName: 'style' }));
    };
    const api = {
      getPropertyValue: (name) => read().get(toStr(name).toLowerCase()) || '',
      setProperty: (name, value) => {
        const m = read();
        if (value === null || value === undefined || value === '') m.delete(toStr(name).toLowerCase());
        else m.set(toStr(name).toLowerCase(), toStr(value));
        write(m);
      },
      removeProperty: (name) => {
        const m = read();
        const old = m.get(toStr(name).toLowerCase()) || '';
        m.delete(toStr(name).toLowerCase());
        write(m);
        return old;
      },
      getPropertyPriority: () => '',
      item: (i) => Array.from(read().keys())[i] || '',
    };
    return new Proxy(api, {
      get(target, prop) {
        if (prop === 'cssText') return styleText(read());
        if (prop === 'length') return read().size;
        if (prop === 'parentRule') return null;
        if (typeof prop === 'symbol') return undefined;
        if (prop in target) return target[prop];
        return read().get(camelToKebab(prop === 'cssFloat' ? 'float' : prop)) || '';
      },
      set(target, prop, value) {
        if (prop === 'cssText') {
          const m = parseStyle(value);
          write(m);
          return true;
        }
        api.setProperty(camelToKebab(prop === 'cssFloat' ? 'float' : prop), value);
        return true;
      },
      has(target, prop) {
        return prop in target || read().has(camelToKebab(String(prop)));
      },
    });
  };

  class DOMTokenList {
    constructor(el, attribute) {
      Object.defineProperty(this, '_el', { value: el });
      Object.defineProperty(this, '_attr', { value: attribute || 'class' });
    }
    _tokens() {
      const v = bi('getAttr', this._el._id, this._attr);
      return v ? v.split(/\s+/).filter(Boolean) : [];
    }
    _write(tokens) {
      const old = bi('getAttr', this._el._id, this._attr);
      const value = Array.from(new Set(tokens)).join(' ');
      if (value === '' && old === null) return;
      bi('setAttr', this._el._id, this._attr, value);
      queueRecord(new MutationRecord({ type: 'attributes', target: this._el, attributeName: this._attr, oldValue: old }));
    }
    get length() {
      return this._tokens().length;
    }
    get value() {
      return bi('getAttr', this._el._id, this._attr) || '';
    }
    set value(v) {
      bi('setAttr', this._el._id, this._attr, toStr(v));
    }
    item(i) {
      return this._tokens()[i] || null;
    }
    contains(t) {
      return this._tokens().includes(String(t));
    }
    add(...ts) {
      const tokens = this._tokens();
      for (const t of ts) {
        if (!String(t) || /\s/.test(t)) throw new DOMException('The token provided must not be empty or contain whitespace.', 'InvalidCharacterError');
        if (!tokens.includes(String(t))) tokens.push(String(t));
      }
      this._write(tokens);
    }
    remove(...ts) {
      this._write(this._tokens().filter((t) => !ts.map(String).includes(t)));
    }
    toggle(t, force) {
      t = String(t);
      const has = this.contains(t);
      if (force === true || (force === undefined && !has)) {
        if (!has) this.add(t);
        return true;
      }
      if (has) this.remove(t);
      return false;
    }
    replace(a, b) {
      const tokens = this._tokens();
      const i = tokens.indexOf(String(a));
      if (i < 0) return false;
      tokens[i] = String(b);
      this._write(tokens);
      return true;
    }
    supports() {
      return true;
    }
    forEach(fn, thisArg) {
      this._tokens().forEach((t, i) => fn.call(thisArg, t, i, this));
    }
    keys() {
      return this._tokens().keys();
    }
    values() {
      return this._tokens().values();
    }
    entries() {
      return this._tokens().entries();
    }
    [Symbol.iterator]() {
      return this._tokens()[Symbol.iterator]();
    }
    toString() {
      return this.value;
    }
  }

  const makeDataset = (el) =>
    new Proxy(
      {},
      {
        get(_, prop) {
          if (typeof prop === 'symbol') return undefined;
          const v = bi('getAttr', el._id, 'data-' + camelToKebab(prop));
          return v === null ? undefined : v;
        },
        set(_, prop, value) {
          bi('setAttr', el._id, 'data-' + camelToKebab(prop), toStr(value));
          return true;
        },
        has(_, prop) {
          return bi('getAttr', el._id, 'data-' + camelToKebab(String(prop))) !== null;
        },
        deleteProperty(_, prop) {
          bi('removeAttr', el._id, 'data-' + camelToKebab(String(prop)));
          return true;
        },
        ownKeys() {
          return JSON.parse(bi('attrs', el._id))
            .map(([k]) => k)
            .filter((k) => k.startsWith('data-'))
            .map((k) => kebabToCamel(k.slice(5)));
        },
        getOwnPropertyDescriptor(_, prop) {
          const v = bi('getAttr', el._id, 'data-' + camelToKebab(String(prop)));
          return v === null ? undefined : { value: v, writable: true, enumerable: true, configurable: true };
        },
      },
    );

  // --------------------------------------------------------------- elements
  class Attr {
    constructor(name, value, owner) {
      this.name = name;
      this.localName = name;
      this.value = value;
      this.ownerElement = owner || null;
      this.namespaceURI = null;
      this.prefix = null;
      this.specified = true;
    }
    get nodeValue() {
      return this.value;
    }
    get textContent() {
      return this.value;
    }
  }

  class Element extends Node {
    constructor() {
      super();
    }
    get tagName() {
      return bi('tag', this._id).toUpperCase();
    }
    get localName() {
      return bi('tag', this._id);
    }
    get nodeName() {
      return this.tagName;
    }
    get namespaceURI() {
      return this._ns || 'http://www.w3.org/1999/xhtml';
    }
    get prefix() {
      return null;
    }
    get id() {
      return bi('getAttr', this._id, 'id') || '';
    }
    set id(v) {
      this.setAttribute('id', v);
    }
    get className() {
      return bi('getAttr', this._id, 'class') || '';
    }
    set className(v) {
      this.setAttribute('class', v);
    }
    get classList() {
      return new DOMTokenList(this, 'class');
    }
    get dataset() {
      return makeDataset(this);
    }
    get style() {
      return makeStyle(this);
    }
    set style(v) {
      this.setAttribute('style', v);
    }
    get attributes() {
      const list = JSON.parse(bi('attrs', this._id)).map(([k, v]) => new Attr(k, v, this));
      list.item = (i) => list[i] || null;
      list.getNamedItem = (n) => list.find((a) => a.name === n) || null;
      return list;
    }
    getAttribute(name) {
      return bi('getAttr', this._id, toStr(name).toLowerCase());
    }
    getAttributeNames() {
      return JSON.parse(bi('attrs', this._id)).map(([k]) => k);
    }
    setAttribute(name, value) {
      name = toStr(name).toLowerCase();
      const old = bi('getAttr', this._id, name);
      bi('setAttr', this._id, name, toStr(value));
      queueRecord(new MutationRecord({ type: 'attributes', target: this, attributeName: name, oldValue: old }));
      const def = this._upgraded && customRegistry.get(this.localName);
      if (def && def.observed.includes(name)) invokeCustom(this, 'attributeChangedCallback', name, old, toStr(value), null);
    }
    removeAttribute(name) {
      name = toStr(name).toLowerCase();
      const old = bi('getAttr', this._id, name);
      if (old === null) return;
      bi('removeAttr', this._id, name);
      queueRecord(new MutationRecord({ type: 'attributes', target: this, attributeName: name, oldValue: old }));
      const def = this._upgraded && customRegistry.get(this.localName);
      if (def && def.observed.includes(name)) invokeCustom(this, 'attributeChangedCallback', name, old, null, null);
    }
    hasAttribute(name) {
      return bi('getAttr', this._id, toStr(name).toLowerCase()) !== null;
    }
    hasAttributes() {
      return JSON.parse(bi('attrs', this._id)).length > 0;
    }
    toggleAttribute(name, force) {
      const has = this.hasAttribute(name);
      if (force === true || (force === undefined && !has)) {
        if (!has) this.setAttribute(name, '');
        return true;
      }
      if (has) this.removeAttribute(name);
      return false;
    }
    getAttributeNS(ns, name) {
      return this.getAttribute(name);
    }
    setAttributeNS(ns, name, value) {
      this.setAttribute(name, value);
    }
    removeAttributeNS(ns, name) {
      this.removeAttribute(name);
    }
    hasAttributeNS(ns, name) {
      return this.hasAttribute(name);
    }
    matches(selector) {
      return bi('matches', this._id, toStr(selector));
    }
    webkitMatchesSelector(selector) {
      return this.matches(selector);
    }
    closest(selector) {
      for (let n = this; n && n.nodeType === 1; n = n.parentNode) if (n.matches(selector)) return n;
      return null;
    }
    get innerHTML() {
      return bi('innerHTML', this._id);
    }
    set innerHTML(v) {
      const removed = this.childNodes;
      bi('setInnerHTML', this._id, toStr(v));
      const added = this.childNodes;
      queueRecord(new MutationRecord({ type: 'childList', target: this, removedNodes: removed, addedNodes: added }));
      if (this.isConnected) {
        for (const n of removed) connectedCallbacks(n, false);
        for (const n of added) connectedCallbacks(n, true);
      }
    }
    get outerHTML() {
      return bi('outerHTML', this._id);
    }
    set outerHTML(v) {
      const parent = this.parentNode;
      if (!parent) return;
      const holder = document.createElement('div');
      holder.innerHTML = v;
      const fragment = document.createDocumentFragment();
      for (const k of holder.childNodes) fragment.appendChild(k);
      parent.insertBefore(fragment, this);
      parent.removeChild(this);
    }
    get innerText() {
      return bi('textContent', this._id);
    }
    set innerText(v) {
      this.textContent = v;
    }
    get outerText() {
      return this.innerText;
    }
    insertAdjacentHTML(position, html) {
      const holder = document.createElement(position === 'beforebegin' || position === 'afterend' ? 'div' : this.localName);
      holder.innerHTML = html;
      const fragment = document.createDocumentFragment();
      for (const k of holder.childNodes) fragment.appendChild(k);
      this.insertAdjacentElement(position, fragment);
    }
    insertAdjacentElement(position, node) {
      switch (String(position).toLowerCase()) {
        case 'beforebegin':
          if (this.parentNode) this.parentNode.insertBefore(node, this);
          break;
        case 'afterbegin':
          this.insertBefore(node, this.firstChild);
          break;
        case 'beforeend':
          this.appendChild(node);
          break;
        case 'afterend':
          if (this.parentNode) this.parentNode.insertBefore(node, this.nextSibling);
          break;
        default:
          throw new DOMException('Invalid position', 'SyntaxError');
      }
      return node;
    }
    insertAdjacentText(position, text) {
      this.insertAdjacentElement(position, document.createTextNode(toStr(text)));
    }
    remove() {
      if (this.parentNode) this.parentNode.removeChild(this);
    }
    before(...nodes) {
      insertNodes(this.parentNode, this, nodes);
    }
    after(...nodes) {
      insertNodes(this.parentNode, this.nextSibling, nodes);
    }
    replaceWith(...nodes) {
      const parent = this.parentNode;
      if (!parent) return;
      insertNodes(parent, this, nodes);
      parent.removeChild(this);
    }
    get previousElementSibling() {
      let n = this.previousSibling;
      while (n && n.nodeType !== 1) n = n.previousSibling;
      return n;
    }
    get nextElementSibling() {
      let n = this.nextSibling;
      while (n && n.nodeType !== 1) n = n.nextSibling;
      return n;
    }
    getBoundingClientRect() {
      const [x, y, width, height] = JSON.parse(bi('rect', this._id));
      const sy = global.scrollY || 0;
      return { x, y: y - sy, width, height, top: y - sy, left: x, right: x + width, bottom: y - sy + height, toJSON() { return this; } };
    }
    getClientRects() {
      const r = this.getBoundingClientRect();
      return r.width || r.height ? [r] : [];
    }
    get offsetWidth() {
      return Math.round(JSON.parse(bi('rect', this._id))[2]);
    }
    get offsetHeight() {
      return Math.round(JSON.parse(bi('rect', this._id))[3]);
    }
    get offsetTop() {
      return Math.round(JSON.parse(bi('rect', this._id))[1]);
    }
    get offsetLeft() {
      return Math.round(JSON.parse(bi('rect', this._id))[0]);
    }
    get offsetParent() {
      return this.isConnected ? document.body : null;
    }
    get clientWidth() {
      return this.offsetWidth;
    }
    get clientHeight() {
      return this.offsetHeight;
    }
    get clientTop() {
      return 0;
    }
    get clientLeft() {
      return 0;
    }
    get scrollWidth() {
      return this.offsetWidth;
    }
    get scrollHeight() {
      return this.offsetHeight;
    }
    get scrollTop() {
      return this._scrollTop || 0;
    }
    set scrollTop(v) {
      this._scrollTop = +v || 0;
    }
    get scrollLeft() {
      return this._scrollLeft || 0;
    }
    set scrollLeft(v) {
      this._scrollLeft = +v || 0;
    }
    scrollIntoView() {}
    scrollTo() {}
    scrollBy() {}
    focus() {
      if (document._active === this) return;
      const previous = document._active;
      document._active = this;
      if (previous && previous !== this) previous.dispatchEvent(new FocusEvent('blur'));
      this.dispatchEvent(new FocusEvent('focus'));
      this.dispatchEvent(new FocusEvent('focusin', { bubbles: true }));
    }
    blur() {
      if (document._active === this) {
        document._active = null;
        this.dispatchEvent(new FocusEvent('blur'));
        this.dispatchEvent(new FocusEvent('focusout', { bubbles: true }));
      }
    }
    click() {
      this.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    }
    attachShadow(init) {
      const root = document.createDocumentFragment();
      Object.defineProperty(root, 'host', { value: this });
      Object.defineProperty(root, 'mode', { value: (init && init.mode) || 'open' });
      Object.defineProperty(this, '_shadow', { value: root, configurable: true });
      return root;
    }
    get shadowRoot() {
      return this._shadow && this._shadow.mode === 'open' ? this._shadow : null;
    }
    animate() {
      const done = Promise.resolve();
      return { finished: done, ready: done, cancel() {}, finish() {}, play() {}, pause() {}, reverse() {}, addEventListener() {}, removeEventListener() {}, onfinish: null, playState: 'finished', currentTime: 0 };
    }
    getAnimations() {
      return [];
    }
    checkVisibility() {
      return this.isConnected;
    }
    requestFullscreen() {
      return Promise.reject(new TypeError('Fullscreen is not available'));
    }
    setPointerCapture() {}
    releasePointerCapture() {}
    get slot() {
      return this.getAttribute('slot') || '';
    }
    set slot(v) {
      this.setAttribute('slot', v);
    }
    get tabIndex() {
      const v = this.getAttribute('tabindex');
      return v === null ? -1 : parseInt(v, 10) || 0;
    }
    set tabIndex(v) {
      this.setAttribute('tabindex', v);
    }
    get hidden() {
      return this.hasAttribute('hidden');
    }
    set hidden(v) {
      this.toggleAttribute('hidden', !!v);
    }
    get title() {
      return this.getAttribute('title') || '';
    }
    set title(v) {
      this.setAttribute('title', v);
    }
    get lang() {
      return this.getAttribute('lang') || '';
    }
    set lang(v) {
      this.setAttribute('lang', v);
    }
    get dir() {
      return this.getAttribute('dir') || '';
    }
    set dir(v) {
      this.setAttribute('dir', v);
    }
    get draggable() {
      return this.getAttribute('draggable') === 'true';
    }
    set draggable(v) {
      this.setAttribute('draggable', v ? 'true' : 'false');
    }
    get isContentEditable() {
      return this.getAttribute('contenteditable') === 'true';
    }
    get contentEditable() {
      return this.getAttribute('contenteditable') || 'inherit';
    }
    set contentEditable(v) {
      this.setAttribute('contenteditable', v);
    }
    get role() {
      return this.getAttribute('role');
    }
    set role(v) {
      this.setAttribute('role', v);
    }
    get ariaLabel() {
      return this.getAttribute('aria-label');
    }
    set ariaLabel(v) {
      this.setAttribute('aria-label', v);
    }
    get ariaHidden() {
      return this.getAttribute('aria-hidden');
    }
    set ariaHidden(v) {
      this.setAttribute('aria-hidden', v);
    }
  }
  mixIn(Element.prototype, parentNodeMethods);

  class HTMLElement extends Element {
    constructor() {
      if (upgradeTarget) {
        const target = upgradeTarget;
        upgradeTarget = null;
        return target;
      }
      // `new MyElement()` for a defined custom element.
      const def = customByCtor.get(new.target);
      if (!def) throw new TypeError('Illegal constructor');
      const el = wrap(bi('createElement', def.name));
      Object.setPrototypeOf(el, new.target.prototype);
      Object.defineProperty(el, '_upgraded', { value: true, configurable: true });
      return el;
    }
  }
  class HTMLUnknownElement extends HTMLElement {}
  class SVGElement extends Element {
    get ownerSVGElement() {
      return this.closest('svg');
    }
    get className() {
      return { baseVal: this.getAttribute('class') || '', animVal: this.getAttribute('class') || '' };
    }
    getBBox() {
      return { x: 0, y: 0, width: 0, height: 0 };
    }
    getComputedTextLength() {
      return 0;
    }
  }
  class SVGSVGElement extends SVGElement {
    createSVGPoint() {
      return { x: 0, y: 0 };
    }
  }

  const reflect = (proto, props) => {
    for (const [prop, attr, kind] of props) {
      Object.defineProperty(proto, prop, {
        configurable: true,
        enumerable: true,
        get() {
          const v = this.getAttribute(attr);
          if (kind === 'bool') return v !== null;
          if (kind === 'int') return parseInt(v, 10) || 0;
          if (kind === 'url') return v === null ? '' : resolveUrl(v);
          return v === null ? '' : v;
        },
        set(value) {
          if (kind === 'bool') this.toggleAttribute(attr, !!value);
          else this.setAttribute(attr, value);
        },
      });
    }
  };
  const register = (Ctor, ...tags) => {
    for (const t of tags) ELEMENT_CLASSES.set(t, Ctor);
  };
  const simple = (name, ...tags) => {
    const C = { [name]: class extends HTMLElement {} }[name];
    register(C, ...tags);
    return C;
  };

  class HTMLAnchorElement extends HTMLElement {
    get href() {
      const v = this.getAttribute('href');
      return v === null ? '' : resolveUrl(v);
    }
    set href(v) {
      this.setAttribute('href', v);
    }
    _url() {
      const v = this.getAttribute('href');
      return v === null ? null : parseUrl(v, global.location.href);
    }
    get origin() {
      const u = this._url();
      return u ? u.origin : '';
    }
    get protocol() {
      const u = this._url();
      return u ? u.protocol : ':';
    }
    get host() {
      const u = this._url();
      return u ? u.host : '';
    }
    get hostname() {
      const u = this._url();
      return u ? u.hostname : '';
    }
    get port() {
      const u = this._url();
      return u ? u.port : '';
    }
    get pathname() {
      const u = this._url();
      return u ? u.pathname : '';
    }
    get search() {
      const u = this._url();
      return u ? u.search : '';
    }
    get hash() {
      const u = this._url();
      return u ? u.hash : '';
    }
    toString() {
      return this.href;
    }
  }
  reflect(HTMLAnchorElement.prototype, [['target', 'target'], ['rel', 'rel'], ['download', 'download'], ['hreflang', 'hreflang'], ['type', 'type']]);
  register(HTMLAnchorElement, 'a');
  class HTMLImageElement extends HTMLElement {
    get complete() {
      return this._loading === undefined ? true : this._done === true;
    }
    get naturalWidth() {
      return this._natural()[0];
    }
    get naturalHeight() {
      return this._natural()[1];
    }
    _natural() {
      if (this._nat) return this._nat;
      // The size the engine decoded the picture at, once it has one.
      const v = bi('computed', this._id, '-blueice-intrinsic-size');
      const parts = v ? v.split(' ').map(Number) : [];
      return parts.length === 2 && parts[0] > 0 ? parts : [0, 0];
    }
    get currentSrc() {
      return this.src;
    }
    get width() {
      return this.offsetWidth || parseInt(this.getAttribute('width'), 10) || (this._nat ? this._nat[0] : 0);
    }
    set width(v) {
      this.setAttribute('width', v);
    }
    get height() {
      return this.offsetHeight || parseInt(this.getAttribute('height'), 10) || (this._nat ? this._nat[1] : 0);
    }
    set height(v) {
      this.setAttribute('height', v);
    }
    decode() {
      if (this._loading === undefined || this._done) return this._failed ? Promise.reject(new DOMException('The image could not be decoded.', 'EncodingError')) : Promise.resolve();
      return new Promise((resolve, reject) => (this._waiters = this._waiters || []).push([resolve, reject]));
    }
    // A script set `src`: fetch the picture (as an image request, which needs no
    // CORS headers), decode it, then report `load` or `error` like a browser.
    _startLoad() {
      const url = this.src;
      if (!url) return;
      this._loading = url;
      this._done = false;
      this._failed = false;
      this._nat = null;
      const finish = (ok) => {
        if (this._loading !== url) return;
        this._done = true;
        this._failed = !ok;
        const waiters = this._waiters || [];
        this._waiters = [];
        for (const [resolve, reject] of waiters) {
          if (ok) resolve(); else reject(new DOMException('The image could not be decoded.', 'EncodingError'));
        }
        this.dispatchEvent(new Event(ok ? 'load' : 'error'));
      };
      const decoded = (b64) => {
        const wh = bi('imageSet', this._id, url, b64);
        if (!wh) return finish(false);
        this._nat = wh.split('x').map(Number);
        finish(true);
      };
      // Delivered as a task, never before the script that set `src` has finished.
      if (url.startsWith('data:')) {
        setTimeout(() => decoded(''), 0);
      } else {
        netSend('GET', url, {}, null, (r) => {
          if (r.status >= 200 && r.status < 300) decoded(bytesToBase64(r.bytes));
          else finish(false);
        }, () => finish(false), 'image');
      }
    }
  }
  {
    reflect(HTMLImageElement.prototype, [['src', 'src', 'url']]);
    const reflected = Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, 'src');
    Object.defineProperty(HTMLImageElement.prototype, 'src', {
      configurable: true,
      get: reflected.get,
      set(v) {
        reflected.set.call(this, v);
        this._startLoad();
      },
    });
  }
  reflect(HTMLImageElement.prototype, [['srcset', 'srcset'], ['alt', 'alt'], ['sizes', 'sizes'], ['loading', 'loading'], ['crossOrigin', 'crossorigin']]);
  register(HTMLImageElement, 'img');
  class HTMLInputElement extends HTMLElement {
    get value() {
      return this._value !== undefined ? this._value : this.getAttribute('value') || '';
    }
    set value(v) {
      this._value = toStr(v);
      this.setAttribute('value', this._value);
    }
    get checked() {
      return this.hasAttribute('checked');
    }
    set checked(v) {
      this.toggleAttribute('checked', !!v);
    }
    get form() {
      return this.closest('form');
    }
    get files() {
      return [];
    }
    select() {}
    setSelectionRange() {}
    setCustomValidity() {}
    checkValidity() {
      return true;
    }
    reportValidity() {
      return true;
    }
    get validity() {
      return { valid: true };
    }
    get selectionStart() {
      return this.value.length;
    }
    get selectionEnd() {
      return this.value.length;
    }
    get type() {
      return (this.getAttribute('type') || 'text').toLowerCase();
    }
    set type(v) {
      this.setAttribute('type', v);
    }
  }
  reflect(HTMLInputElement.prototype, [['name', 'name'], ['placeholder', 'placeholder'], ['disabled', 'disabled', 'bool'], ['readOnly', 'readonly', 'bool'], ['required', 'required', 'bool'], ['autocomplete', 'autocomplete'], ['min', 'min'], ['max', 'max'], ['step', 'step'], ['pattern', 'pattern'], ['multiple', 'multiple', 'bool'], ['accept', 'accept']]);
  register(HTMLInputElement, 'input');
  class HTMLTextAreaElement extends HTMLInputElement {
    get value() {
      return this._value !== undefined ? this._value : this.textContent;
    }
    set value(v) {
      this._value = toStr(v);
    }
    get type() {
      return 'textarea';
    }
  }
  register(HTMLTextAreaElement, 'textarea');
  class HTMLSelectElement extends HTMLInputElement {
    get options() {
      return this.querySelectorAll('option');
    }
    get selectedIndex() {
      return this.options.findIndex((o) => o.hasAttribute('selected'));
    }
    get value() {
      const o = this.options.find((o) => o.hasAttribute('selected')) || this.options[0];
      return o ? o.value : '';
    }
    set value(v) {
      for (const o of this.options) o.toggleAttribute('selected', o.value === String(v));
    }
    get type() {
      return 'select-one';
    }
  }
  register(HTMLSelectElement, 'select');
  class HTMLOptionElement extends HTMLElement {
    get value() {
      return this.hasAttribute('value') ? this.getAttribute('value') : this.textContent;
    }
    set value(v) {
      this.setAttribute('value', v);
    }
    get selected() {
      return this.hasAttribute('selected');
    }
    set selected(v) {
      this.toggleAttribute('selected', !!v);
    }
    get text() {
      return this.textContent;
    }
  }
  register(HTMLOptionElement, 'option');
  class HTMLButtonElement extends HTMLElement {
    get type() {
      return (this.getAttribute('type') || 'submit').toLowerCase();
    }
    set type(v) {
      this.setAttribute('type', v);
    }
    get form() {
      return this.closest('form');
    }
  }
  reflect(HTMLButtonElement.prototype, [['name', 'name'], ['value', 'value'], ['disabled', 'disabled', 'bool']]);
  register(HTMLButtonElement, 'button');
  class HTMLFormElement extends HTMLElement {
    get elements() {
      return this.querySelectorAll('input,select,textarea,button');
    }
    submit() {}
    reset() {}
    requestSubmit() {
      const e = new Event('submit', { bubbles: true, cancelable: true });
      this.dispatchEvent(e);
    }
    checkValidity() {
      return true;
    }
    reportValidity() {
      return true;
    }
  }
  reflect(HTMLFormElement.prototype, [['action', 'action', 'url'], ['method', 'method'], ['name', 'name'], ['target', 'target']]);
  register(HTMLFormElement, 'form');
  class HTMLLabelElement extends HTMLElement {}
  reflect(HTMLLabelElement.prototype, [['htmlFor', 'for']]);
  register(HTMLLabelElement, 'label');
  class HTMLScriptElement extends HTMLElement {
    get text() {
      return this.textContent;
    }
    set text(v) {
      this.textContent = v;
    }
  }
  reflect(HTMLScriptElement.prototype, [['src', 'src', 'url'], ['type', 'type'], ['async', 'async', 'bool'], ['defer', 'defer', 'bool'], ['crossOrigin', 'crossorigin'], ['noModule', 'nomodule', 'bool']]);
  register(HTMLScriptElement, 'script');
  class HTMLLinkElement extends HTMLElement {
    get sheet() {
      return null;
    }
  }
  reflect(HTMLLinkElement.prototype, [['href', 'href', 'url'], ['rel', 'rel'], ['type', 'type'], ['media', 'media'], ['as', 'as'], ['crossOrigin', 'crossorigin']]);
  register(HTMLLinkElement, 'link');
  class HTMLStyleElement extends HTMLElement {
    get sheet() {
      return { cssRules: [], insertRule() {}, deleteRule() {} };
    }
  }
  register(HTMLStyleElement, 'style');
  class HTMLIFrameElement extends HTMLElement {
    get contentWindow() {
      return null;
    }
    get contentDocument() {
      return null;
    }
  }
  reflect(HTMLIFrameElement.prototype, [['src', 'src', 'url'], ['name', 'name']]);
  register(HTMLIFrameElement, 'iframe');

  // ----------------------------------------------------------- canvas 2D
  // Resolved at call time: `toBase64`/`fromBase64` are defined further down.
  const bytesToBase64 = (bytes) => toBase64(bytes);
  const base64ToBytes = (text) => fromBase64(text);
  class ImageData {
    constructor(a, b, c) {
      if (ArrayBuffer.isView(a)) {
        this.data = a instanceof Uint8ClampedArray ? a : new Uint8ClampedArray(a.buffer, a.byteOffset, a.byteLength);
        this.width = b;
        this.height = c !== undefined ? c : this.data.length / 4 / b;
      } else {
        this.width = a;
        this.height = b;
        this.data = new Uint8ClampedArray(a * b * 4);
      }
      this.colorSpace = 'srgb';
    }
  }
  const pixelsOf = (source) => {
    // [w, h, base64] for a picture a script can draw: an image, canvas or ImageData.
    if (source && source.data && source.width !== undefined && !source.tagName) {
      return [source.width, source.height, bytesToBase64(new Uint8Array(source.data.buffer, source.data.byteOffset, source.data.byteLength))];
    }
    if (source && source._id !== undefined && source.tagName) {
      const out = bi(source.tagName === 'CANVAS' ? 'canvasPixels' : 'imagePixels', source._id);
      if (out) {
        const sep = out.indexOf(':');
        const [w, h] = out.slice(0, sep).split('x').map(Number);
        return [w, h, out.slice(sep + 1)];
      }
    }
    return null;
  };
  class CanvasRenderingContext2D {
    constructor(canvas) {
      this.canvas = canvas;
      this._id = bi('c2dCreate', canvas._id, canvas.width, canvas.height);
      this._state = { fillStyle: '#000000', strokeStyle: '#000000', globalAlpha: 1, m: [1, 0, 0, 1, 0, 0], font: '10px sans-serif', lineWidth: 1 };
      this._stack = [];
    }
    _c(op, ...args) {
      return bi('c2d', this._id, op, ...args);
    }
    get fillStyle() { return this._state.fillStyle; }
    set fillStyle(v) { if (typeof v === 'string') this._state.fillStyle = v; }
    get strokeStyle() { return this._state.strokeStyle; }
    set strokeStyle(v) { if (typeof v === 'string') this._state.strokeStyle = v; }
    get globalAlpha() { return this._state.globalAlpha; }
    set globalAlpha(v) { v = Number(v); if (v >= 0 && v <= 1) this._state.globalAlpha = v; }
    get font() { return this._state.font; }
    set font(v) { this._state.font = String(v); }
    get lineWidth() { return this._state.lineWidth; }
    set lineWidth(v) { this._state.lineWidth = Number(v) || 1; }
    save() { this._stack.push(Object.assign({}, this._state, { m: this._state.m.slice() })); }
    restore() { if (this._stack.length) this._state = this._stack.pop(); }
    setTransform(a, b, c, d, e, f) {
      if (a && typeof a === 'object') { b = a.b; c = a.c; d = a.d; e = a.e; f = a.f; a = a.a; }
      this._state.m = [a, b, c, d, e, f];
    }
    resetTransform() { this._state.m = [1, 0, 0, 1, 0, 0]; }
    getTransform() { const m = this._state.m; return { a: m[0], b: m[1], c: m[2], d: m[3], e: m[4], f: m[5] }; }
    transform(a, b, c, d, e, f) {
      const m = this._state.m;
      this._state.m = [m[0] * a + m[2] * b, m[1] * a + m[3] * b, m[0] * c + m[2] * d, m[1] * c + m[3] * d, m[0] * e + m[2] * f + m[4], m[1] * e + m[3] * f + m[5]];
    }
    translate(x, y) { this.transform(1, 0, 0, 1, x, y); }
    scale(x, y) { this.transform(x, 0, 0, y, 0, 0); }
    rotate(a) { const c = Math.cos(a), s = Math.sin(a); this.transform(c, s, -s, c, 0, 0); }
    fillRect(x, y, w, h) { this._c('fillRect', x, y, w, h, this._state.fillStyle, this._state.globalAlpha, JSON.stringify(this._state.m)); }
    clearRect(x, y, w, h) { this._c('clearRect', x, y, w, h, '#000', 1, JSON.stringify(this._state.m)); }
    strokeRect() {}
    drawImage(source, a, b, c, d, e, f, g, h) {
      const px = pixelsOf(source);
      if (!px) return;
      const [w, hh, data] = px;
      let sx = 0, sy = 0, sw = w, sh = hh, dx, dy, dw, dh;
      const n = arguments.length;
      if (n >= 9) { sx = a; sy = b; sw = c; sh = d; dx = e; dy = f; dw = g; dh = h; }
      else if (n >= 5) { dx = a; dy = b; dw = c; dh = d; }
      else { dx = a; dy = b; dw = w; dh = hh; }
      this._c('drawImage', w, hh, data, sx, sy, sw, sh, dx, dy, dw, dh, this._state.globalAlpha, JSON.stringify(this._state.m));
    }
    getImageData(x, y, w, h) {
      const out = new ImageData(w, h);
      const b64 = this._c('getImageData', x, y, w, h);
      out.data.set(base64ToBytes(b64));
      return out;
    }
    putImageData(image, x, y) {
      const bytes = new Uint8Array(image.data.buffer, image.data.byteOffset, image.data.byteLength);
      this._c('putImageData', bytesToBase64(bytes), image.width, image.height, x, y);
    }
    createImageData(w, h) { return w && typeof w === 'object' ? new ImageData(w.width, w.height) : new ImageData(w, h); }
    // Paths, text and gradients are accepted and draw nothing yet.
    beginPath() {}
    closePath() {}
    moveTo() {}
    lineTo() {}
    bezierCurveTo() {}
    quadraticCurveTo() {}
    arc() {}
    arcTo() {}
    ellipse() {}
    rect() {}
    roundRect() {}
    fill() {}
    stroke() {}
    clip() {}
    fillText() {}
    strokeText() {}
    measureText(text) { return { width: String(text).length * 6, actualBoundingBoxLeft: 0, actualBoundingBoxRight: String(text).length * 6, actualBoundingBoxAscent: 8, actualBoundingBoxDescent: 2 }; }
    setLineDash() {}
    getLineDash() { return []; }
    createLinearGradient() { return { addColorStop() {} }; }
    createRadialGradient() { return { addColorStop() {} }; }
    createPattern() { return null; }
    isPointInPath() { return false; }
    getContextAttributes() { return { alpha: true, desynchronized: false, colorSpace: 'srgb', willReadFrequently: false }; }
    _resize(w, h) { this._c('resize', w, h); }
  }
  // -------------------------------------------------------------- WebGL
  const GL_CONSTANTS = {
    DEPTH_BUFFER_BIT: 256, STENCIL_BUFFER_BIT: 1024, COLOR_BUFFER_BIT: 16384,
    POINTS: 0, LINES: 1, LINE_LOOP: 2, LINE_STRIP: 3, TRIANGLES: 4, TRIANGLE_STRIP: 5, TRIANGLE_FAN: 6,
    ZERO: 0, ONE: 1, SRC_COLOR: 768, ONE_MINUS_SRC_COLOR: 769, SRC_ALPHA: 770, ONE_MINUS_SRC_ALPHA: 771,
    DST_ALPHA: 772, ONE_MINUS_DST_ALPHA: 773, DST_COLOR: 774, ONE_MINUS_DST_COLOR: 775, SRC_ALPHA_SATURATE: 776,
    FUNC_ADD: 32774, BLEND_EQUATION: 32777, BLEND_EQUATION_RGB: 32777, BLEND_EQUATION_ALPHA: 34877,
    FUNC_SUBTRACT: 32778, FUNC_REVERSE_SUBTRACT: 32779, MIN: 32775, MAX: 32776,
    BLEND_DST_RGB: 32968, BLEND_SRC_RGB: 32969, BLEND_DST_ALPHA: 32970, BLEND_SRC_ALPHA: 32971,
    CONSTANT_COLOR: 32769, ONE_MINUS_CONSTANT_COLOR: 32770, CONSTANT_ALPHA: 32771, ONE_MINUS_CONSTANT_ALPHA: 32772,
    BLEND_COLOR: 32773, ARRAY_BUFFER: 34962, ELEMENT_ARRAY_BUFFER: 34963, ARRAY_BUFFER_BINDING: 34964,
    ELEMENT_ARRAY_BUFFER_BINDING: 34965, STREAM_DRAW: 35040, STATIC_DRAW: 35044, DYNAMIC_DRAW: 35048,
    BUFFER_SIZE: 34660, BUFFER_USAGE: 34661, CURRENT_VERTEX_ATTRIB: 34342, FRONT: 1028, BACK: 1029, FRONT_AND_BACK: 1032,
    TEXTURE_2D: 3553, CULL_FACE: 2884, BLEND: 3042, DITHER: 3024, STENCIL_TEST: 2960, DEPTH_TEST: 2929,
    SCISSOR_TEST: 3089, POLYGON_OFFSET_FILL: 32823, SAMPLE_ALPHA_TO_COVERAGE: 32926, SAMPLE_COVERAGE: 32928,
    NO_ERROR: 0, INVALID_ENUM: 1280, INVALID_VALUE: 1281, INVALID_OPERATION: 1282, OUT_OF_MEMORY: 1285,
    CONTEXT_LOST_WEBGL: 37442, INVALID_FRAMEBUFFER_OPERATION: 1286,
    CW: 2304, CCW: 2305, LINE_WIDTH: 2849, ALIASED_POINT_SIZE_RANGE: 33901, ALIASED_LINE_WIDTH_RANGE: 33902,
    CULL_FACE_MODE: 2885, FRONT_FACE: 2886, DEPTH_RANGE: 2928, DEPTH_WRITEMASK: 2930, DEPTH_CLEAR_VALUE: 2931,
    DEPTH_FUNC: 2932, STENCIL_CLEAR_VALUE: 2961, STENCIL_FUNC: 2962, STENCIL_FAIL: 2964, STENCIL_PASS_DEPTH_FAIL: 2965,
    STENCIL_PASS_DEPTH_PASS: 2966, STENCIL_REF: 2967, STENCIL_VALUE_MASK: 2963, STENCIL_WRITEMASK: 2968,
    VIEWPORT: 2978, SCISSOR_BOX: 3088, COLOR_CLEAR_VALUE: 3106, COLOR_WRITEMASK: 3107,
    UNPACK_ALIGNMENT: 3317, PACK_ALIGNMENT: 3333, MAX_TEXTURE_SIZE: 3379, MAX_VIEWPORT_DIMS: 3386,
    SUBPIXEL_BITS: 3408, RED_BITS: 3410, GREEN_BITS: 3411, BLUE_BITS: 3412, ALPHA_BITS: 3413, DEPTH_BITS: 3414, STENCIL_BITS: 3415,
    POLYGON_OFFSET_UNITS: 10752, POLYGON_OFFSET_FACTOR: 32824, TEXTURE_BINDING_2D: 32873, SAMPLE_BUFFERS: 32936,
    SAMPLES: 32937, SAMPLE_COVERAGE_VALUE: 32938, SAMPLE_COVERAGE_INVERT: 32939,
    COMPRESSED_TEXTURE_FORMATS: 34467, DONT_CARE: 4352, FASTEST: 4353, NICEST: 4354, GENERATE_MIPMAP_HINT: 33170,
    BYTE: 5120, UNSIGNED_BYTE: 5121, SHORT: 5122, UNSIGNED_SHORT: 5123, INT: 5124, UNSIGNED_INT: 5125, FLOAT: 5126,
    HALF_FLOAT: 5131, DEPTH_COMPONENT: 6402, ALPHA: 6406, RGB: 6407, RGBA: 6408, LUMINANCE: 6409, LUMINANCE_ALPHA: 6410,
    UNSIGNED_SHORT_4_4_4_4: 32819, UNSIGNED_SHORT_5_5_5_1: 32820, UNSIGNED_SHORT_5_6_5: 33635,
    FRAGMENT_SHADER: 35632, VERTEX_SHADER: 35633, MAX_VERTEX_ATTRIBS: 34921, MAX_VERTEX_UNIFORM_VECTORS: 36347,
    MAX_VARYING_VECTORS: 36348, MAX_COMBINED_TEXTURE_IMAGE_UNITS: 35661, MAX_VERTEX_TEXTURE_IMAGE_UNITS: 35660,
    MAX_TEXTURE_IMAGE_UNITS: 34930, MAX_FRAGMENT_UNIFORM_VECTORS: 36349, SHADER_TYPE: 35663, DELETE_STATUS: 35712,
    LINK_STATUS: 35714, VALIDATE_STATUS: 35715, ATTACHED_SHADERS: 35717, ACTIVE_UNIFORMS: 35718, ACTIVE_ATTRIBUTES: 35721,
    SHADING_LANGUAGE_VERSION: 35724, CURRENT_PROGRAM: 35725, COMPILE_STATUS: 35713,
    NEVER: 512, LESS: 513, EQUAL: 514, LEQUAL: 515, GREATER: 516, NOTEQUAL: 517, GEQUAL: 518, ALWAYS: 519,
    KEEP: 7680, REPLACE: 7681, INCR: 7682, DECR: 7683, INVERT: 5386, INCR_WRAP: 34055, DECR_WRAP: 34056,
    VENDOR: 7936, RENDERER: 7937, VERSION: 7938, NEAREST: 9728, LINEAR: 9729,
    NEAREST_MIPMAP_NEAREST: 9984, LINEAR_MIPMAP_NEAREST: 9985, NEAREST_MIPMAP_LINEAR: 9986, LINEAR_MIPMAP_LINEAR: 9987,
    TEXTURE_MAG_FILTER: 10240, TEXTURE_MIN_FILTER: 10241, TEXTURE_WRAP_S: 10242, TEXTURE_WRAP_T: 10243,
    TEXTURE: 5890, TEXTURE_CUBE_MAP: 34067, TEXTURE_BINDING_CUBE_MAP: 34068,
    TEXTURE_CUBE_MAP_POSITIVE_X: 34069, TEXTURE_CUBE_MAP_NEGATIVE_X: 34070, TEXTURE_CUBE_MAP_POSITIVE_Y: 34071,
    TEXTURE_CUBE_MAP_NEGATIVE_Y: 34072, TEXTURE_CUBE_MAP_POSITIVE_Z: 34073, TEXTURE_CUBE_MAP_NEGATIVE_Z: 34074,
    MAX_CUBE_MAP_TEXTURE_SIZE: 34076, TEXTURE0: 33984, ACTIVE_TEXTURE: 34016,
    REPEAT: 10497, CLAMP_TO_EDGE: 33071, MIRRORED_REPEAT: 33648,
    FLOAT_VEC2: 35664, FLOAT_VEC3: 35665, FLOAT_VEC4: 35666, INT_VEC2: 35667, INT_VEC3: 35668, INT_VEC4: 35669,
    BOOL: 35670, BOOL_VEC2: 35671, BOOL_VEC3: 35672, BOOL_VEC4: 35673, FLOAT_MAT2: 35674, FLOAT_MAT3: 35675,
    FLOAT_MAT4: 35676, SAMPLER_2D: 35678, SAMPLER_CUBE: 35680,
    VERTEX_ATTRIB_ARRAY_ENABLED: 34338, VERTEX_ATTRIB_ARRAY_SIZE: 34339, VERTEX_ATTRIB_ARRAY_STRIDE: 34340,
    VERTEX_ATTRIB_ARRAY_TYPE: 34341, VERTEX_ATTRIB_ARRAY_NORMALIZED: 34922, VERTEX_ATTRIB_ARRAY_POINTER: 34373,
    VERTEX_ATTRIB_ARRAY_BUFFER_BINDING: 34975, IMPLEMENTATION_COLOR_READ_TYPE: 35738, IMPLEMENTATION_COLOR_READ_FORMAT: 35739,
    LOW_FLOAT: 36336, MEDIUM_FLOAT: 36337, HIGH_FLOAT: 36338, LOW_INT: 36339, MEDIUM_INT: 36340, HIGH_INT: 36341,
    FRAMEBUFFER: 36160, RENDERBUFFER: 36161, RGBA4: 32854, RGB5_A1: 32855, RGB565: 36194,
    DEPTH_COMPONENT16: 33189, STENCIL_INDEX8: 36168, DEPTH_STENCIL: 34041,
    RENDERBUFFER_WIDTH: 36162, RENDERBUFFER_HEIGHT: 36163, RENDERBUFFER_INTERNAL_FORMAT: 36164,
    FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE: 36048, FRAMEBUFFER_ATTACHMENT_OBJECT_NAME: 36049,
    COLOR_ATTACHMENT0: 36064, DEPTH_ATTACHMENT: 36096, STENCIL_ATTACHMENT: 36128, DEPTH_STENCIL_ATTACHMENT: 33306, NONE: 0,
    FRAMEBUFFER_COMPLETE: 36053, FRAMEBUFFER_INCOMPLETE_ATTACHMENT: 36054, FRAMEBUFFER_INCOMPLETE_MISSING_ATTACHMENT: 36055,
    FRAMEBUFFER_INCOMPLETE_DIMENSIONS: 36057, FRAMEBUFFER_UNSUPPORTED: 36061, FRAMEBUFFER_BINDING: 36006,
    RENDERBUFFER_BINDING: 36007, MAX_RENDERBUFFER_SIZE: 34024,
    UNPACK_FLIP_Y_WEBGL: 37440, UNPACK_PREMULTIPLY_ALPHA_WEBGL: 37441, UNPACK_COLORSPACE_CONVERSION_WEBGL: 37443,
    BROWSER_DEFAULT_WEBGL: 37444,
  };
  const GL2_CONSTANTS = {
    READ_BUFFER: 3074, UNPACK_ROW_LENGTH: 3314, UNPACK_SKIP_ROWS: 3315, UNPACK_SKIP_PIXELS: 3316, PACK_ROW_LENGTH: 3330,
    COLOR: 6144, DEPTH: 6145, STENCIL: 6146, RED: 6403, RGB8: 32849, RGBA8: 32856, RGB10_A2: 32857, TEXTURE_BINDING_3D: 32874,
    UNPACK_SKIP_IMAGES: 32877, UNPACK_IMAGE_HEIGHT: 32878, TEXTURE_3D: 32879, TEXTURE_WRAP_R: 32882, MAX_3D_TEXTURE_SIZE: 32883,
    UNSIGNED_INT_2_10_10_10_REV: 33640, MAX_ELEMENTS_VERTICES: 33000, MAX_ELEMENTS_INDICES: 33001, TEXTURE_MIN_LOD: 33082,
    TEXTURE_MAX_LOD: 33083, TEXTURE_BASE_LEVEL: 33084, TEXTURE_MAX_LEVEL: 33085, MIN: 32775, MAX: 32776,
    DEPTH_COMPONENT24: 33190, MAX_TEXTURE_LOD_BIAS: 34045, TEXTURE_COMPARE_MODE: 34892, TEXTURE_COMPARE_FUNC: 34893,
    CURRENT_QUERY: 34917, QUERY_RESULT: 34918, QUERY_RESULT_AVAILABLE: 34919, STREAM_READ: 35041, STREAM_COPY: 35042,
    STATIC_READ: 35045, STATIC_COPY: 35046, DYNAMIC_READ: 35049, DYNAMIC_COPY: 35050, MAX_DRAW_BUFFERS: 34852,
    DRAW_BUFFER0: 34853, MAX_FRAGMENT_UNIFORM_COMPONENTS: 35657, MAX_VERTEX_UNIFORM_COMPONENTS: 35658, SAMPLER_3D: 35679,
    SAMPLER_2D_SHADOW: 35682, FRAGMENT_SHADER_DERIVATIVE_HINT: 35723, PIXEL_PACK_BUFFER: 35051, PIXEL_UNPACK_BUFFER: 35052,
    PIXEL_PACK_BUFFER_BINDING: 35053, PIXEL_UNPACK_BUFFER_BINDING: 35055, FLOAT_MAT2x3: 35685, FLOAT_MAT2x4: 35686,
    FLOAT_MAT3x2: 35687, FLOAT_MAT3x4: 35688, FLOAT_MAT4x2: 35689, FLOAT_MAT4x3: 35690, SRGB: 35904, SRGB8: 35905,
    SRGB8_ALPHA8: 35907, COMPARE_REF_TO_TEXTURE: 34894, RGBA32F: 34836, RGB32F: 34837, RGBA16F: 34842, RGB16F: 34843,
    VERTEX_ATTRIB_ARRAY_INTEGER: 35069, MAX_ARRAY_TEXTURE_LAYERS: 35071, MIN_PROGRAM_TEXEL_OFFSET: 35076,
    MAX_PROGRAM_TEXEL_OFFSET: 35077, MAX_VARYING_COMPONENTS: 35659, TEXTURE_2D_ARRAY: 35866, TEXTURE_BINDING_2D_ARRAY: 35869,
    R11F_G11F_B10F: 35898, UNSIGNED_INT_10F_11F_11F_REV: 35899, RGB9_E5: 35901, UNSIGNED_INT_5_9_9_9_REV: 35902,
    TRANSFORM_FEEDBACK_BUFFER_MODE: 35967, MAX_TRANSFORM_FEEDBACK_SEPARATE_COMPONENTS: 35968, RGBA32UI: 36208,
    RGB32UI: 36209, RGBA16UI: 36214, RGB16UI: 36215, RGBA8UI: 36220, RGB8UI: 36221, RGBA32I: 36226, RGB32I: 36227,
    RGBA16I: 36232, RGB16I: 36233, RGBA8I: 36238, RGB8I: 36239, RED_INTEGER: 36244, RGB_INTEGER: 36248, RGBA_INTEGER: 36249,
    SAMPLER_2D_ARRAY: 36289, SAMPLER_2D_ARRAY_SHADOW: 36292, SAMPLER_CUBE_SHADOW: 36293, UNSIGNED_INT_VEC2: 36294,
    UNSIGNED_INT_VEC3: 36295, UNSIGNED_INT_VEC4: 36296, INT_SAMPLER_2D: 36298, UNSIGNED_INT_SAMPLER_2D: 36306,
    DEPTH_COMPONENT32F: 36012, DEPTH32F_STENCIL8: 36013, FLOAT_32_UNSIGNED_INT_24_8_REV: 36269,
    FRAMEBUFFER_ATTACHMENT_COLOR_ENCODING: 33296, FRAMEBUFFER_ATTACHMENT_COMPONENT_TYPE: 33297, FRAMEBUFFER_DEFAULT: 33304,
    UNSIGNED_INT_24_8: 34042, DEPTH24_STENCIL8: 35056, UNSIGNED_NORMALIZED: 35863, DRAW_FRAMEBUFFER_BINDING: 36006,
    READ_FRAMEBUFFER: 36008, DRAW_FRAMEBUFFER: 36009, READ_FRAMEBUFFER_BINDING: 36010, RENDERBUFFER_SAMPLES: 36011,
    COLOR_ATTACHMENT1: 36065, COLOR_ATTACHMENT2: 36066, COLOR_ATTACHMENT3: 36067, MAX_COLOR_ATTACHMENTS: 36063,
    MAX_SAMPLES: 36183, RG: 33319, RG_INTEGER: 33320, R8: 33321, RG8: 33323, R16F: 33325, R32F: 33326, RG16F: 33327,
    RG32F: 33328, R8I: 33329, R8UI: 33330, VERTEX_ARRAY_BINDING: 34229, SIGNED_NORMALIZED: 36764, COPY_READ_BUFFER: 36662,
    COPY_WRITE_BUFFER: 36663, UNIFORM_BUFFER: 35345, UNIFORM_BUFFER_BINDING: 35368, MAX_UNIFORM_BUFFER_BINDINGS: 35375,
    MAX_UNIFORM_BLOCK_SIZE: 35376, UNIFORM_BUFFER_OFFSET_ALIGNMENT: 35380, ACTIVE_UNIFORM_BLOCKS: 35382,
    INVALID_INDEX: 4294967295, MAX_VERTEX_OUTPUT_COMPONENTS: 37154, MAX_FRAGMENT_INPUT_COMPONENTS: 37157,
    MAX_SERVER_WAIT_TIMEOUT: 37137, TRANSFORM_FEEDBACK: 36386, VERTEX_ATTRIB_ARRAY_DIVISOR: 35070, ANY_SAMPLES_PASSED: 35887,
  };
  class WebGLObject {
    constructor(id) { this._id = id; }
  }
  class WebGLBuffer extends WebGLObject {}
  class WebGLShader extends WebGLObject {}
  class WebGLProgram extends WebGLObject {}
  class WebGLTexture extends WebGLObject {}
  class WebGLFramebuffer extends WebGLObject {}
  class WebGLRenderbuffer extends WebGLObject {}
  class WebGLVertexArrayObject extends WebGLObject {}
  class WebGLUniformLocation {
    constructor(loc, program) { this._loc = loc; this._program = program; }
  }
  class WebGLActiveInfo {
    constructor(name, type, size) { this.name = name; this.type = type; this.size = size; }
  }
  class WebGLShaderPrecisionFormat {
    constructor(a, b, c) { this.rangeMin = a; this.rangeMax = b; this.precision = c; }
  }
  const glBytes = (data) => {
    if (data === null || data === undefined) return null;
    if (data instanceof ArrayBuffer) return new Uint8Array(data);
    if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    return null;
  };
  const glNumbers = (list) => {
    const out = [];
    for (let i = 0; i < list.length; i++) out.push(Number.isFinite(list[i]) ? Number(list[i]) : 0);
    return JSON.stringify(out);
  };
  const GL_EXTENSIONS_1 = ['ANGLE_instanced_arrays', 'OES_vertex_array_object', 'OES_element_index_uint', 'OES_standard_derivatives',
    'OES_texture_float', 'OES_texture_half_float', 'OES_texture_float_linear', 'OES_texture_half_float_linear',
    'WEBGL_depth_texture', 'EXT_shader_texture_lod', 'WEBGL_lose_context', 'EXT_frag_depth'];
  const GL_EXTENSIONS_2 = ['EXT_color_buffer_float', 'OES_texture_float_linear', 'WEBGL_lose_context', 'EXT_texture_filter_anisotropic'];
  class WebGLRenderingContext {
    constructor(canvas, attrs, version) {
      attrs = attrs || {};
      this.canvas = canvas;
      this._version = version;
      this._attrs = {
        alpha: attrs.alpha !== false, antialias: false, depth: attrs.depth !== false, stencil: !!attrs.stencil,
        premultipliedAlpha: attrs.premultipliedAlpha !== false, preserveDrawingBuffer: !!attrs.preserveDrawingBuffer,
        powerPreference: attrs.powerPreference || 'default', failIfMajorPerformanceCaveat: false, desynchronized: false,
      };
      this._id = bi('glCreate', canvas._id, canvas.width, canvas.height, this._attrs.alpha, this._attrs.premultipliedAlpha);
      this._uniforms = new Map();
      this._attribs = new Map();
      this._bound = {};
      this._lost = false;
    }
    _c(op, ...args) {
      return bi('gl', this._id, op, ...args);
    }
    get drawingBufferWidth() { return this.canvas.width; }
    get drawingBufferHeight() { return this.canvas.height; }
    _resize(w, h) { this._c('resize', w, h); }
    getContextAttributes() { return Object.assign({}, this._attrs); }
    isContextLost() { return false; }
    getError() { return this._c('getError'); }
    getSupportedExtensions() { return (this._version === 2 ? GL_EXTENSIONS_2 : GL_EXTENSIONS_1).slice(); }
    getExtension(name) {
      const list = this._version === 2 ? GL_EXTENSIONS_2 : GL_EXTENSIONS_1;
      if (!list.includes(name)) return null;
      const gl = this;
      switch (name) {
        case 'ANGLE_instanced_arrays':
          return {
            VERTEX_ATTRIB_ARRAY_DIVISOR_ANGLE: 35070,
            drawArraysInstancedANGLE: (m, f, c, n) => gl.drawArraysInstanced(m, f, c, n),
            drawElementsInstancedANGLE: (m, c, t, o, n) => gl.drawElementsInstanced(m, c, t, o, n),
            vertexAttribDivisorANGLE: (i, d) => gl.vertexAttribDivisor(i, d),
          };
        case 'OES_vertex_array_object':
          return {
            VERTEX_ARRAY_BINDING_OES: 34229,
            createVertexArrayOES: () => gl.createVertexArray(),
            deleteVertexArrayOES: (v) => gl.deleteVertexArray(v),
            bindVertexArrayOES: (v) => gl.bindVertexArray(v),
            isVertexArrayOES: (v) => v instanceof WebGLVertexArrayObject,
          };
        case 'OES_texture_half_float': return { HALF_FLOAT_OES: 36193 };
        case 'WEBGL_depth_texture': return { UNSIGNED_INT_24_8_WEBGL: 34042 };
        case 'WEBGL_lose_context': return { loseContext() {}, restoreContext() {} };
        case 'EXT_texture_filter_anisotropic': return { MAX_TEXTURE_MAX_ANISOTROPY_EXT: 34047, TEXTURE_MAX_ANISOTROPY_EXT: 34046 };
        default: return {};
      }
    }
    getParameter(p) {
      const v2 = this._version === 2;
      switch (p) {
        case 3379: case 34076: case 34024: return 8192;
        case 34921: return 16;
        case 34930: case 35660: return 16;
        case 35661: return 32;
        case 36347: case 36349: return 1024;
        case 36348: return 30;
        case 3386: return new Int32Array([8192, 8192]);
        case 7936: return 'BlueIce';
        case 7937: return 'BlueIce WebGL';
        case 7938: return v2 ? 'WebGL 2.0 (BlueIce)' : 'WebGL 1.0 (BlueIce)';
        case 35724: return v2 ? 'WebGL GLSL ES 3.00 (BlueIce)' : 'WebGL GLSL ES 1.0 (BlueIce)';
        case 33902: return new Float32Array([1, 1]);
        case 33901: return new Float32Array([1, 256]);
        case 34852: case 36063: return 1;
        case 36183: return 0;
        case 32883: return 256;
        case 35071: return 256;
        case 35657: case 35658: return 4096;
        case 35659: return 120;
        case 35376: return 65536;
        case 35375: return 24;
        case 35380: return 256;
        case 37154: return 124;
        case 37157: return 120;
        case 33000: case 33001: return 65535;
        case 3410: case 3411: case 3412: case 3413: return 8;
        case 3414: return 24;
        case 3415: return 8;
        case 3408: return 4;
        case 34047: return 1;
        case 7939: return '';
        case 2978: { const v = JSON.parse(this._c('viewportState')); return new Int32Array(v); }
        case 3088: { const v = JSON.parse(this._c('viewportState')); return new Int32Array(v); }
        case 36006: return this._bound.framebuffer || null;
        case 36010: return this._bound.framebuffer || null;
        case 36007: return this._bound.renderbuffer || null;
        case 34964: return this._bound.arrayBuffer || null;
        case 34965: return this._bound.elementBuffer || null;
        case 35725: return this._bound.program || null;
        case 34229: return this._bound.vao || null;
        case 32873: return this._bound.texture2d || null;
        case 34068: return this._bound.textureCube || null;
        case 34016: return 33984 + (this._bound.unit || 0);
        case 3106: return new Float32Array(this._bound.clear || [0, 0, 0, 0]);
        case 3107: return [true, true, true, true];
        case 2929: case 3042: case 2884: case 3089: return this.isEnabled(p);
        case 37440: case 37441: return !!this._bound['pixel' + p];
        case 37443: return 37444;
        case 35738: return 5121;
        case 35739: return 6408;
        case 34467: return new Uint32Array(0);
        default: return null;
      }
    }
    getShaderPrecisionFormat(shader, precision) {
      return precision >= 36339 ? new WebGLShaderPrecisionFormat(31, 30, 0) : new WebGLShaderPrecisionFormat(127, 127, 23);
    }
    // buffers
    createBuffer() { return new WebGLBuffer(this._c('createBuffer')); }
    deleteBuffer(b) { if (b) this._c('deleteBuffer', b._id); }
    isBuffer(b) { return b instanceof WebGLBuffer; }
    bindBuffer(target, b) {
      this._c('bindBuffer', target, b ? b._id : 0);
      if (target === 34962) this._bound.arrayBuffer = b;
      else if (target === 34963) this._bound.elementBuffer = b;
    }
    bufferData(target, data, usage, srcOffset, length) {
      if (typeof data === 'number') { this._c('bufferDataSize', target, data); return; }
      let bytes = glBytes(data);
      if (bytes === null) { this._c('bufferDataSize', target, 0); return; }
      if (srcOffset) {
        const size = data.BYTES_PER_ELEMENT || 1;
        bytes = bytes.subarray(srcOffset * size, length ? (srcOffset + length) * size : undefined);
      }
      this._c('bufferData', target, toBase64(bytes), usage);
    }
    bufferSubData(target, offset, data, srcOffset, length) {
      let bytes = glBytes(data);
      if (bytes === null) return;
      if (srcOffset) {
        const size = data.BYTES_PER_ELEMENT || 1;
        bytes = bytes.subarray(srcOffset * size, length ? (srcOffset + length) * size : undefined);
      }
      this._c('bufferSubData', target, offset, toBase64(bytes));
    }
    getBufferParameter() { return 0; }
    copyBufferSubData() {}
    // vertex arrays
    createVertexArray() { return new WebGLVertexArrayObject(this._c('createVertexArray')); }
    deleteVertexArray(v) { if (v) this._c('deleteVertexArray', v._id); }
    isVertexArray(v) { return v instanceof WebGLVertexArrayObject; }
    bindVertexArray(v) { this._c('bindVertexArray', v ? v._id : 0); this._bound.vao = v; }
    enableVertexAttribArray(i) { this._c('enableVertexAttribArray', i); }
    disableVertexAttribArray(i) { this._c('disableVertexAttribArray', i); }
    vertexAttribPointer(i, size, type, normalized, stride, offset) {
      this._c('vertexAttribPointer', i, size, type, !!normalized, stride, offset, false);
    }
    vertexAttribIPointer(i, size, type, stride, offset) {
      this._c('vertexAttribPointer', i, size, type, false, stride, offset, true);
    }
    vertexAttribDivisor(i, d) { this._c('vertexAttribDivisor', i, d); }
    vertexAttrib1f(i, x) { this._c('vertexAttrib', i, glNumbers([x])); }
    vertexAttrib2f(i, x, y) { this._c('vertexAttrib', i, glNumbers([x, y])); }
    vertexAttrib3f(i, x, y, z) { this._c('vertexAttrib', i, glNumbers([x, y, z])); }
    vertexAttrib4f(i, x, y, z, w) { this._c('vertexAttrib', i, glNumbers([x, y, z, w])); }
    vertexAttrib1fv(i, v) { this._c('vertexAttrib', i, glNumbers(v)); }
    vertexAttrib2fv(i, v) { this._c('vertexAttrib', i, glNumbers(v)); }
    vertexAttrib3fv(i, v) { this._c('vertexAttrib', i, glNumbers(v)); }
    vertexAttrib4fv(i, v) { this._c('vertexAttrib', i, glNumbers(v)); }
    vertexAttribI4i(i, x, y, z, w) { this._c('vertexAttrib', i, glNumbers([x, y, z, w])); }
    vertexAttribI4ui(i, x, y, z, w) { this._c('vertexAttrib', i, glNumbers([x, y, z, w])); }
    getVertexAttrib() { return 0; }
    getVertexAttribOffset() { return 0; }
    // shaders and programs
    createShader(type) { return new WebGLShader(this._c('createShader', type)); }
    shaderSource(s, src) { this._c('shaderSource', s._id, String(src)); }
    compileShader(s) { this._c('compileShader', s._id); }
    getShaderParameter(s, p) {
      if (p === 35713) return this._c('shaderCompiled', s._id);
      if (p === 35663) return this._c('shaderSourceOf', s._id) !== null ? (s._type || 35633) : 35633;
      if (p === 35712) return false;
      return null;
    }
    getShaderInfoLog(s) { return this._c('shaderLog', s._id); }
    getShaderSource(s) { return this._c('shaderSourceOf', s._id); }
    deleteShader(s) { if (s) this._c('deleteShader', s._id); }
    isShader(s) { return s instanceof WebGLShader; }
    createProgram() { return new WebGLProgram(this._c('createProgram')); }
    attachShader(p, s) { this._c('attachShader', p._id, s._id); }
    detachShader(p, s) { this._c('detachShader', p._id, s._id); }
    bindAttribLocation(p, i, name) { this._c('bindAttribLocation', p._id, i, String(name)); }
    linkProgram(p) { this._c('linkProgram', p._id); this._uniforms.delete(p._id); this._attribs.delete(p._id); }
    validateProgram() {}
    useProgram(p) { this._c('useProgram', p ? p._id : 0); this._bound.program = p; }
    deleteProgram(p) { if (p) this._c('deleteProgram', p._id); }
    isProgram(p) { return p instanceof WebGLProgram; }
    getProgramParameter(p, name) {
      switch (name) {
        case 35714: return this._c('programLinked', p._id);
        case 35715: return true;
        case 35712: return false;
        case 35717: return 2;
        case 35718: return this._uniformList(p).length;
        case 35721: return this._attribList(p).length;
        case 35382: return 0;
        default: return null;
      }
    }
    getProgramInfoLog(p) { return this._c('programLog', p._id); }
    _uniformList(p) {
      if (!this._uniforms.has(p._id)) this._uniforms.set(p._id, JSON.parse(this._c('activeUniforms', p._id)));
      return this._uniforms.get(p._id);
    }
    _attribList(p) {
      if (!this._attribs.has(p._id)) this._attribs.set(p._id, JSON.parse(this._c('activeAttribs', p._id)));
      return this._attribs.get(p._id);
    }
    getActiveUniform(p, i) {
      const u = this._uniformList(p)[i];
      return u ? new WebGLActiveInfo(u.name, u.type, u.size) : null;
    }
    getActiveAttrib(p, i) {
      const a = this._attribList(p)[i];
      return a ? new WebGLActiveInfo(a.name, a.type, a.size) : null;
    }
    getAttribLocation(p, name) { return this._c('attribLocation', p._id, String(name)); }
    getUniformLocation(p, name) {
      const loc = this._c('uniformLocation', p._id, String(name));
      return loc < 0 ? null : new WebGLUniformLocation(loc, p);
    }
    getUniform() { return null; }
    getUniformBlockIndex() { return 4294967295; }
    uniformBlockBinding() {}
    // uniforms
    _u(loc, kind, values) { if (loc) this._c('uniform', loc._loc, kind, glNumbers(values)); }
    uniform1f(l, a) { this._u(l, 'f', [a]); }
    uniform2f(l, a, b) { this._u(l, 'f', [a, b]); }
    uniform3f(l, a, b, c) { this._u(l, 'f', [a, b, c]); }
    uniform4f(l, a, b, c, d) { this._u(l, 'f', [a, b, c, d]); }
    uniform1i(l, a) { this._u(l, 'i', [a]); }
    uniform2i(l, a, b) { this._u(l, 'i', [a, b]); }
    uniform3i(l, a, b, c) { this._u(l, 'i', [a, b, c]); }
    uniform4i(l, a, b, c, d) { this._u(l, 'i', [a, b, c, d]); }
    uniform1ui(l, a) { this._u(l, 'i', [a]); }
    uniform2ui(l, a, b) { this._u(l, 'i', [a, b]); }
    uniform3ui(l, a, b, c) { this._u(l, 'i', [a, b, c]); }
    uniform4ui(l, a, b, c, d) { this._u(l, 'i', [a, b, c, d]); }
    uniform1fv(l, v) { this._u(l, 'f', v); }
    uniform2fv(l, v) { this._u(l, 'f', v); }
    uniform3fv(l, v) { this._u(l, 'f', v); }
    uniform4fv(l, v) { this._u(l, 'f', v); }
    uniform1iv(l, v) { this._u(l, 'i', v); }
    uniform2iv(l, v) { this._u(l, 'i', v); }
    uniform3iv(l, v) { this._u(l, 'i', v); }
    uniform4iv(l, v) { this._u(l, 'i', v); }
    uniform1uiv(l, v) { this._u(l, 'i', v); }
    uniform2uiv(l, v) { this._u(l, 'i', v); }
    uniform3uiv(l, v) { this._u(l, 'i', v); }
    uniform4uiv(l, v) { this._u(l, 'i', v); }
    _m(l, c, r, transpose, v) {
      if (!l) return;
      let data = v;
      if (transpose) {
        data = [];
        const n = Math.floor(v.length / (c * r));
        for (let k = 0; k < n; k++) for (let col = 0; col < c; col++) for (let row = 0; row < r; row++) data.push(v[k * c * r + row * c + col]);
      }
      this._c('uniform', l._loc, 'm' + c + 'x' + r, glNumbers(data));
    }
    uniformMatrix2fv(l, t, v) { this._m(l, 2, 2, t, v); }
    uniformMatrix3fv(l, t, v) { this._m(l, 3, 3, t, v); }
    uniformMatrix4fv(l, t, v) { this._m(l, 4, 4, t, v); }
    // textures
    createTexture() { return new WebGLTexture(this._c('createTexture')); }
    deleteTexture(t) { if (t) this._c('deleteTexture', t._id); }
    isTexture(t) { return t instanceof WebGLTexture; }
    activeTexture(unit) { this._c('activeTexture', unit); this._bound.unit = unit - 33984; }
    bindTexture(target, t) {
      this._c('bindTexture', target, t ? t._id : 0);
      if (target === 3553) this._bound.texture2d = t;
      else if (target === 34067) this._bound.textureCube = t;
    }
    texParameteri(target, name, value) { this._c('texParameter', target, name, value); }
    texParameterf(target, name, value) { this._c('texParameter', target, name, value); }
    getTexParameter() { return null; }
    generateMipmap(target) { this._c('generateMipmap', target); }
    pixelStorei(name, value) {
      this._bound['pixel' + name] = value;
      this._c('pixelStore', name, !!value);
    }
    _pixels(source) {
      // The RGBA bytes of an image, canvas or ImageData, as [w, h, base64].
      return pixelsOf(source) || [1, 1, toBase64(new Uint8Array([0, 0, 0, 0]))];
    }
    texImage2D(target, level, internal, a, b, c, d, e, f) {
      if (arguments.length >= 8) {
        // (target, level, internalformat, width, height, border, format, type, pixels[, offset])
        const pixels = glBytes(f);
        this._c('texImage2D', target, level, internal, a, b, d, e, pixels ? toBase64(pixels) : '');
        return;
      }
      // (target, level, internalformat, format, type, source)
      const [w, h, data] = this._pixels(d === undefined ? c : f);
      this._c('texImage2D', target, level, internal, w, h, a, b, data);
    }
    texSubImage2D(target, level, x, y, a, b, c, d, e) {
      if (arguments.length >= 9) {
        const pixels = glBytes(e);
        if (pixels) this._c('texSubImage2D', target, level, x, y, a, b, c, d, toBase64(pixels));
        return;
      }
      const [w, h, data] = this._pixels(c);
      this._c('texSubImage2D', target, level, x, y, w, h, a, b, data);
    }
    texImage3D() {}
    texSubImage3D() {}
    texStorage2D(target, levels, internal, w, h) {
      const fmt = internal;
      for (let l = 0; l < levels; l++) {
        this._c('texImage2D', target, l, fmt, Math.max(1, w >> l), Math.max(1, h >> l), 6408, 5121, '');
      }
    }
    copyTexImage2D() {}
    compressedTexImage2D() {}
    // framebuffers
    createFramebuffer() { return new WebGLFramebuffer(this._c('createFramebuffer')); }
    deleteFramebuffer(f) { if (f) this._c('deleteFramebuffer', f._id); }
    isFramebuffer(f) { return f instanceof WebGLFramebuffer; }
    bindFramebuffer(target, f) { this._c('bindFramebuffer', target, f ? f._id : 0); this._bound.framebuffer = f; }
    createRenderbuffer() { return new WebGLRenderbuffer(this._c('createRenderbuffer')); }
    deleteRenderbuffer(r) { if (r) this._c('deleteRenderbuffer', r._id); }
    isRenderbuffer(r) { return r instanceof WebGLRenderbuffer; }
    bindRenderbuffer(target, r) { this._c('bindRenderbuffer', target, r ? r._id : 0); this._bound.renderbuffer = r; }
    renderbufferStorage(target, format, w, h) { this._c('renderbufferStorage', format, w, h); }
    renderbufferStorageMultisample(target, samples, format, w, h) { this._c('renderbufferStorage', format, w, h); }
    framebufferTexture2D(target, attachment, textarget, t, level) {
      this._c('framebufferTexture2D', attachment, textarget, 0, t ? t._id : 0, level || 0);
    }
    framebufferRenderbuffer(target, attachment, rbtarget, r) {
      this._c('framebufferRenderbuffer', attachment, r ? r._id : 0);
    }
    checkFramebufferStatus() { return this._c('checkFramebufferStatus'); }
    getFramebufferAttachmentParameter() { return null; }
    getRenderbufferParameter() { return 0; }
    drawBuffers() {}
    readBuffer() {}
    invalidateFramebuffer() {}
    blitFramebuffer() {}
    readPixels(x, y, w, h, format, type, pixels) {
      const b64 = this._c('readPixels', x, y, w, h);
      const bytes = fromBase64(b64);
      const view = glBytes(pixels);
      if (view) view.set(bytes.subarray(0, Math.min(view.length, bytes.length)));
    }
    // state
    enable(c) { this._c('enable', c); }
    disable(c) { this._c('disable', c); }
    isEnabled(c) { return this._c('isEnabled', c); }
    viewport(x, y, w, h) { this._c('viewport', x, y, w, h); }
    scissor(x, y, w, h) { this._c('scissor', x, y, w, h); }
    clearColor(r, g, b, a) { this._bound.clear = [r, g, b, a]; this._c('clearColor', r, g, b, a); }
    clearDepth(d) { this._c('clearDepth', d); }
    clearStencil() {}
    clear(mask) { this._c('clear', mask); }
    clearBufferfv(buffer, index, values) { this._c('clearBuffer', buffer, glNumbers(values)); }
    clearBufferiv() {}
    clearBufferuiv() {}
    clearBufferfi() {}
    colorMask(r, g, b, a) { this._c('colorMask', r, g, b, a); }
    depthMask(m) { this._c('depthMask', m); }
    depthFunc(f) { this._c('depthFunc', f); }
    depthRange(n, f) { this._c('depthRange', n, f); }
    stencilFunc() {}
    stencilFuncSeparate() {}
    stencilMask() {}
    stencilMaskSeparate() {}
    stencilOp() {}
    stencilOpSeparate() {}
    blendFunc(s, d) { this._c('blendFunc', s, d); }
    blendFuncSeparate(a, b, c, d) { this._c('blendFuncSeparate', a, b, c, d); }
    blendEquation(m) { this._c('blendEquation', m); }
    blendEquationSeparate(a, b) { this._c('blendEquationSeparate', a, b); }
    blendColor(r, g, b, a) { this._c('blendColor', r, g, b, a); }
    cullFace(m) { this._c('cullFace', m); }
    frontFace(m) { this._c('frontFace', m); }
    lineWidth(w) { this._c('lineWidth', w); }
    polygonOffset() {}
    sampleCoverage() {}
    hint() {}
    flush() {}
    finish() {}
    // drawing
    drawArrays(mode, first, count) { this._c('drawArrays', mode, first, count, 1); }
    drawElements(mode, count, type, offset) { this._c('drawElements', mode, count, type, offset, 1); }
    drawArraysInstanced(mode, first, count, n) { this._c('drawArrays', mode, first, count, n); }
    drawElementsInstanced(mode, count, type, offset, n) { this._c('drawElements', mode, count, type, offset, n); }
    drawRangeElements(mode, start, end, count, type, offset) { this._c('drawElements', mode, count, type, offset, 1); }
    // queries (never ready)
    createQuery() { return {}; }
    deleteQuery() {}
    beginQuery() {}
    endQuery() {}
    getQueryParameter() { return 0; }
    getSyncParameter() { return 0; }
    fenceSync() { return {}; }
    clientWaitSync() { return 36370; }
    deleteSync() {}
    createSampler() { return {}; }
    deleteSampler() {}
    bindSampler() {}
    samplerParameteri() {}
    samplerParameterf() {}
    createTransformFeedback() { return {}; }
    bindTransformFeedback() {}
    bindBufferBase() {}
    bindBufferRange() {}
  }
  for (const [k, v] of Object.entries(GL_CONSTANTS)) Object.defineProperty(WebGLRenderingContext, k, { value: v });
  for (const [k, v] of Object.entries(GL_CONSTANTS)) Object.defineProperty(WebGLRenderingContext.prototype, k, { value: v });
  class WebGL2RenderingContext extends WebGLRenderingContext {}
  for (const [k, v] of Object.entries(GL2_CONSTANTS)) Object.defineProperty(WebGL2RenderingContext.prototype, k, { value: v });
  for (const [k, v] of Object.entries(GL2_CONSTANTS)) Object.defineProperty(WebGL2RenderingContext, k, { value: v });
  class HTMLCanvasElement extends HTMLElement {
    getContext(type, attrs) {
      type = String(type);
      if (type === 'webgl' || type === 'experimental-webgl' || type === 'webgl2') {
        const version = type === 'webgl2' ? 2 : 1;
        if (this._gl) return this._gl._version === version ? this._gl : null;
        try {
          this._gl = new (version === 2 ? WebGL2RenderingContext : WebGLRenderingContext)(this, attrs, version);
        } catch (e) {
          return null;
        }
        return this._gl;
      }
      if (type === '2d') {
        if (this._gl) return null;
        if (!this._c2d) {
          try {
            this._c2d = new CanvasRenderingContext2D(this);
          } catch (e) {
            return null;
          }
        }
        return this._c2d;
      }
      return null;
    }
    get width() {
      const v = parseInt(this.getAttribute('width'), 10);
      return v >= 0 ? v : 300;
    }
    set width(v) {
      this.setAttribute('width', String(v >>> 0));
      if (this._gl) this._gl._resize(this.width, this.height);
      if (this._c2d) this._c2d._resize(this.width, this.height);
    }
    get height() {
      const v = parseInt(this.getAttribute('height'), 10);
      return v >= 0 ? v : 150;
    }
    set height(v) {
      this.setAttribute('height', String(v >>> 0));
      if (this._gl) this._gl._resize(this.width, this.height);
      if (this._c2d) this._c2d._resize(this.width, this.height);
    }
    toDataURL(type) {
      if (this._c2d) {
        const png = bi('c2dPng', this._c2d._id);
        if (png) return 'data:image/png;base64,' + png;
      }
      if (this._gl) {
        const png = bi('glPng', this._gl._id);
        if (png) return 'data:image/png;base64,' + png;
      }
      return 'data:,';
    }
    toBlob(callback) {
      const url = this.toDataURL();
      const bytes = url.startsWith('data:image/png;base64,') ? fromBase64(url.slice(22)) : new Uint8Array(0);
      setTimeout(() => callback(bytes.length ? new Blob([bytes], { type: 'image/png' }) : null), 0);
    }
    captureStream() { return null; }
    transferControlToOffscreen() { throw new DOMException('Not supported', 'InvalidStateError'); }
  }
  register(HTMLCanvasElement, 'canvas');
  Object.assign(global, { CanvasRenderingContext2D, ImageData, WebGLRenderingContext, WebGL2RenderingContext, WebGLBuffer, WebGLShader, WebGLProgram,
    WebGLTexture, WebGLFramebuffer, WebGLRenderbuffer, WebGLVertexArrayObject, WebGLUniformLocation, WebGLActiveInfo,
    WebGLShaderPrecisionFormat });
  class HTMLMediaElement extends HTMLElement {
    play() {
      return Promise.resolve();
    }
    pause() {}
    load() {}
    canPlayType() {
      return '';
    }
    get paused() {
      return true;
    }
    get muted() {
      return this.hasAttribute('muted');
    }
    set muted(v) {
      this.toggleAttribute('muted', !!v);
    }
    get currentTime() {
      return 0;
    }
    set currentTime(v) {}
    get duration() {
      return NaN;
    }
    get readyState() {
      return 0;
    }
  }
  reflect(HTMLMediaElement.prototype, [['src', 'src', 'url'], ['loop', 'loop', 'bool'], ['autoplay', 'autoplay', 'bool'], ['controls', 'controls', 'bool'], ['preload', 'preload']]);
  class HTMLVideoElement extends HTMLMediaElement {}
  class HTMLAudioElement extends HTMLMediaElement {}
  register(HTMLVideoElement, 'video');
  register(HTMLAudioElement, 'audio');
  class HTMLTemplateElement extends HTMLElement {
    get content() {
      if (!this._content) {
        const frag = document.createDocumentFragment();
        const holder = wrap(bi('createElement', 'div'));
        bi('setInnerHTML', holder._id, bi('innerHTML', this._id));
        for (const k of holder.childNodes) frag.appendChild(k);
        Object.defineProperty(this, '_content', { value: frag });
      }
      return this._content;
    }
  }
  register(HTMLTemplateElement, 'template');
  class HTMLTableElement extends HTMLElement {}
  class HTMLTableRowElement extends HTMLElement {}
  class HTMLTableCellElement extends HTMLElement {}
  register(HTMLTableElement, 'table');
  register(HTMLTableRowElement, 'tr');
  register(HTMLTableCellElement, 'td', 'th');
  class HTMLDialogElement extends HTMLElement {
    show() {
      this.setAttribute('open', '');
    }
    showModal() {
      this.setAttribute('open', '');
    }
    close() {
      this.removeAttribute('open');
      this.dispatchEvent(new Event('close'));
    }
    get open() {
      return this.hasAttribute('open');
    }
    set open(v) {
      this.toggleAttribute('open', !!v);
    }
  }
  register(HTMLDialogElement, 'dialog');
  const HTMLDivElement = simple('HTMLDivElement', 'div');
  const HTMLSpanElement = simple('HTMLSpanElement', 'span');
  const HTMLParagraphElement = simple('HTMLParagraphElement', 'p');
  const HTMLBodyElement = simple('HTMLBodyElement', 'body');
  const HTMLHeadElement = simple('HTMLHeadElement', 'head');
  const HTMLHtmlElement = simple('HTMLHtmlElement', 'html');
  const HTMLUListElement = simple('HTMLUListElement', 'ul');
  const HTMLOListElement = simple('HTMLOListElement', 'ol');
  const HTMLLIElement = simple('HTMLLIElement', 'li');
  const HTMLHeadingElement = simple('HTMLHeadingElement', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6');
  const HTMLPreElement = simple('HTMLPreElement', 'pre');
  const HTMLBRElement = simple('HTMLBRElement', 'br');
  const HTMLHRElement = simple('HTMLHRElement', 'hr');
  const HTMLMetaElement = simple('HTMLMetaElement', 'meta');
  const HTMLTitleElement = simple('HTMLTitleElement', 'title');
  const HTMLBaseElement = simple('HTMLBaseElement', 'base');
  const HTMLSourceElement = simple('HTMLSourceElement', 'source');
  const HTMLPictureElement = simple('HTMLPictureElement', 'picture');
  const HTMLSlotElement = simple('HTMLSlotElement', 'slot');
  const HTMLDetailsElement = simple('HTMLDetailsElement', 'details');
  const HTMLFieldSetElement = simple('HTMLFieldSetElement', 'fieldset');
  const HTMLLegendElement = simple('HTMLLegendElement', 'legend');
  const HTMLMainElement = simple('HTMLMainElement', 'main');
  const HTMLProgressElement = simple('HTMLProgressElement', 'progress');
  const HTMLObjectElement = simple('HTMLObjectElement', 'object');
  const HTMLEmbedElement = simple('HTMLEmbedElement', 'embed');
  const HTMLOutputElement = simple('HTMLOutputElement', 'output');
  const HTMLTableSectionElement = simple('HTMLTableSectionElement', 'thead', 'tbody', 'tfoot');
  const HTMLTableCaptionElement = simple('HTMLTableCaptionElement', 'caption');
  const HTMLQuoteElement = simple('HTMLQuoteElement', 'q', 'blockquote');
  const HTMLMenuElement = simple('HTMLMenuElement', 'menu');
  const HTMLDListElement = simple('HTMLDListElement', 'dl');
  const HTMLAreaElement = simple('HTMLAreaElement', 'area');
  const HTMLMapElement = simple('HTMLMapElement', 'map');
  const HTMLDataElement = simple('HTMLDataElement', 'data');
  const HTMLTimeElement = simple('HTMLTimeElement', 'time');
  const HTMLModElement = simple('HTMLModElement', 'ins', 'del');
  const HTMLParamElement = simple('HTMLParamElement', 'param');
  const HTMLTrackElement = simple('HTMLTrackElement', 'track');
  const HTMLOptGroupElement = simple('HTMLOptGroupElement', 'optgroup');
  const HTMLDataListElement = simple('HTMLDataListElement', 'datalist');
  const HTMLMeterElement = simple('HTMLMeterElement', 'meter');
  const HTMLTableColElement = simple('HTMLTableColElement', 'col', 'colgroup');

  // ----------------------------------------------------------------- urls
  const parseUrl = (input, base) => {
    const json = bi('parseUrl', toStr(input), base === undefined ? '' : toStr(base));
    return json === null ? null : JSON.parse(json);
  };
  const resolveUrl = (value) => {
    const u = parseUrl(value, global.location.href);
    return u ? u.href : value;
  };

  class URLSearchParams {
    constructor(init) {
      this._list = [];
      this._url = null;
      if (init === undefined || init === null) return;
      if (typeof init === 'string') {
        this._parse(init);
      } else if (init instanceof URLSearchParams) {
        this._list = init._list.map((p) => p.slice());
      } else if (typeof init[Symbol.iterator] === 'function') {
        for (const pair of init) this._list.push([String(pair[0]), String(pair[1])]);
      } else {
        for (const k of Object.keys(init)) this._list.push([k, String(init[k])]);
      }
    }
    _parse(text) {
      this._list = [];
      if (text[0] === '?') text = text.slice(1);
      for (const part of text.split('&')) {
        if (!part) continue;
        const i = part.indexOf('=');
        const dec = (s) => {
          try {
            return decodeURIComponent(s.replace(/\+/g, ' '));
          } catch (_) {
            return s;
          }
        };
        this._list.push(i < 0 ? [dec(part), ''] : [dec(part.slice(0, i)), dec(part.slice(i + 1))]);
      }
    }
    _update() {
      if (this._url) this._url._setSearch(this.toString());
    }
    get size() {
      return this._list.length;
    }
    append(k, v) {
      this._list.push([String(k), String(v)]);
      this._update();
    }
    delete(k) {
      this._list = this._list.filter((p) => p[0] !== String(k));
      this._update();
    }
    get(k) {
      const p = this._list.find((p) => p[0] === String(k));
      return p ? p[1] : null;
    }
    getAll(k) {
      return this._list.filter((p) => p[0] === String(k)).map((p) => p[1]);
    }
    has(k) {
      return this._list.some((p) => p[0] === String(k));
    }
    set(k, v) {
      k = String(k);
      const i = this._list.findIndex((p) => p[0] === k);
      if (i < 0) this._list.push([k, String(v)]);
      else {
        this._list[i][1] = String(v);
        this._list = this._list.filter((p, j) => p[0] !== k || j === i);
      }
      this._update();
    }
    sort() {
      this._list.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
      this._update();
    }
    forEach(fn, thisArg) {
      for (const [k, v] of this._list) fn.call(thisArg, v, k, this);
    }
    keys() {
      return this._list.map((p) => p[0])[Symbol.iterator]();
    }
    values() {
      return this._list.map((p) => p[1])[Symbol.iterator]();
    }
    entries() {
      return this._list.map((p) => [p[0], p[1]])[Symbol.iterator]();
    }
    [Symbol.iterator]() {
      return this.entries();
    }
    toString() {
      const enc = (s) => encodeURIComponent(s).replace(/%20/g, '+').replace(/[!'()~]/g, (c) => '%' + c.charCodeAt(0).toString(16).toUpperCase());
      return this._list.map(([k, v]) => enc(k) + '=' + enc(v)).join('&');
    }
  }
  class URL {
    constructor(input, base) {
      const u = parseUrl(input, base);
      if (!u) throw new TypeError("Failed to construct 'URL': Invalid URL");
      this._u = u;
      this._params = new URLSearchParams(u.search);
      this._params._url = this;
    }
    _setSearch(s) {
      this._u.search = s ? '?' + s : '';
      this._rebuild();
    }
    _rebuild() {
      const u = this._u;
      u.href = u.protocol + '//' + (u.username ? u.username + (u.password ? ':' + u.password : '') + '@' : '') + u.host + u.pathname + u.search + u.hash;
    }
    get href() {
      return this._u.href;
    }
    set href(v) {
      const u = parseUrl(v);
      if (!u) throw new TypeError('Invalid URL');
      this._u = u;
      this._params._parse(u.search);
    }
    get origin() {
      return this._u.origin;
    }
    get protocol() {
      return this._u.protocol;
    }
    set protocol(v) {
      this._u.protocol = String(v).replace(/:?$/, ':');
      this._rebuild();
    }
    get host() {
      return this._u.host;
    }
    set host(v) {
      this._u.host = String(v);
      this._u.hostname = String(v).split(':')[0];
      this._rebuild();
    }
    get hostname() {
      return this._u.hostname;
    }
    set hostname(v) {
      this._u.hostname = String(v);
      this._u.host = this._u.port ? v + ':' + this._u.port : String(v);
      this._rebuild();
    }
    get port() {
      return this._u.port;
    }
    set port(v) {
      this._u.port = String(v);
      this._u.host = this._u.hostname + (v ? ':' + v : '');
      this._rebuild();
    }
    get pathname() {
      return this._u.pathname;
    }
    set pathname(v) {
      this._u.pathname = String(v)[0] === '/' ? String(v) : '/' + v;
      this._rebuild();
    }
    get search() {
      return this._u.search;
    }
    set search(v) {
      v = String(v);
      this._u.search = v && v[0] !== '?' ? '?' + v : v;
      this._params._parse(this._u.search);
      this._rebuild();
    }
    get hash() {
      return this._u.hash;
    }
    set hash(v) {
      v = String(v);
      this._u.hash = v && v[0] !== '#' ? '#' + v : v;
      this._rebuild();
    }
    get username() {
      return this._u.username;
    }
    get password() {
      return this._u.password;
    }
    get searchParams() {
      return this._params;
    }
    toString() {
      return this._u.href;
    }
    toJSON() {
      return this._u.href;
    }
    static canParse(input, base) {
      return parseUrl(input, base) !== null;
    }
  }

  // --------------------------------------------------------------- document
  class Document extends Node {
    constructor() {
      super();
    }
    get nodeType() {
      return 9;
    }
    get nodeName() {
      return '#document';
    }
    get documentElement() {
      return wrap(bi('documentElement'));
    }
    get head() {
      return wrap(bi('head'));
    }
    get body() {
      return wrap(bi('body'));
    }
    set body(el) {
      const old = this.body;
      if (old) old.parentNode.replaceChild(el, old);
    }
    get doctype() {
      return null;
    }
    get title() {
      const t = this.querySelector('title');
      return t ? t.textContent.trim() : '';
    }
    set title(v) {
      let t = this.querySelector('title');
      if (!t) {
        t = this.createElement('title');
        if (this.head) this.head.appendChild(t);
      }
      t.textContent = v;
    }
    get URL() {
      return global.location.href;
    }
    get documentURI() {
      return global.location.href;
    }
    get baseURI() {
      return global.location.href;
    }
    get location() {
      return global.location;
    }
    get domain() {
      return global.location.hostname;
    }
    get referrer() {
      return '';
    }
    get cookie() {
      return bi('cookie');
    }
    set cookie(v) {
      bi('setCookie', toStr(v));
    }
    get readyState() {
      return this._readyState || 'loading';
    }
    get characterSet() {
      return 'UTF-8';
    }
    get charset() {
      return 'UTF-8';
    }
    get contentType() {
      return 'text/html';
    }
    get compatMode() {
      return 'CSS1Compat';
    }
    get hidden() {
      return false;
    }
    get visibilityState() {
      return 'visible';
    }
    get defaultView() {
      return global;
    }
    get activeElement() {
      return this._active && this._active.isConnected ? this._active : this.body;
    }
    get scrollingElement() {
      return this.documentElement;
    }
    get currentScript() {
      return this._currentScript || null;
    }
    get scripts() {
      return this.querySelectorAll('script');
    }
    get forms() {
      return this.querySelectorAll('form');
    }
    get images() {
      return this.querySelectorAll('img');
    }
    get links() {
      return this.querySelectorAll('a[href],area[href]');
    }
    get all() {
      return this.querySelectorAll('*');
    }
    get styleSheets() {
      return [];
    }
    get fonts() {
      return { ready: Promise.resolve(), load: () => Promise.resolve([]), check: () => true, addEventListener() {}, forEach() {} };
    }
    get adoptedStyleSheets() {
      return [];
    }
    set adoptedStyleSheets(v) {}
    get implementation() {
      return {
        createHTMLDocument: (title) => {
          const d = new DOMParser().parseFromString('<!doctype html><title>' + toStr(title) + '</title>', 'text/html');
          return d;
        },
        hasFeature: () => true,
      };
    }
    hasFocus() {
      return true;
    }
    createElement(name, options) {
      const el = wrap(bi('createElement', toStr(name)));
      if (options && options.is) el.setAttribute('is', options.is);
      return el;
    }
    createElementNS(ns, name) {
      const el = wrap(bi('createElement', toStr(name)));
      if (ns === 'http://www.w3.org/2000/svg') {
        Object.defineProperty(el, '_ns', { value: ns });
        Object.setPrototypeOf(el, name.toLowerCase() === 'svg' ? SVGSVGElement.prototype : SVGElement.prototype);
      }
      return el;
    }
    createTextNode(data) {
      return wrap(bi('createText', toStr(data)));
    }
    createComment(data) {
      return wrap(bi('createComment', toStr(data)));
    }
    createDocumentFragment() {
      return wrap(bi('createFragment'));
    }
    createEvent(type) {
      const t = String(type).toLowerCase();
      if (t.startsWith('mouse')) return new MouseEvent('');
      if (t.startsWith('key')) return new KeyboardEvent('');
      if (t.startsWith('custom')) return new CustomEvent('');
      return new Event('');
    }
    createRange() {
      return { setStart() {}, setEnd() {}, selectNodeContents() {}, collapse() {}, getBoundingClientRect: () => ({ x: 0, y: 0, width: 0, height: 0, top: 0, left: 0, right: 0, bottom: 0 }), createContextualFragment: (html) => { const f = document.createDocumentFragment(); const d = document.createElement('div'); d.innerHTML = html; for (const k of d.childNodes) f.appendChild(k); return f; } };
    }
    createTreeWalker() {
      return { nextNode: () => null, currentNode: null };
    }
    importNode(node, deep) {
      return node.cloneNode(deep);
    }
    adoptNode(node) {
      return node;
    }
    getElementById(id) {
      return wrap(bi('byId', toStr(id)));
    }
    getElementsByName(name) {
      return this.querySelectorAll('[name="' + toStr(name).replace(/"/g, '\\"') + '"]');
    }
    elementFromPoint() {
      return null;
    }
    elementsFromPoint() {
      return [];
    }
    getSelection() {
      return global.getSelection();
    }
    execCommand() {
      return false;
    }
    write() {}
    writeln() {}
    open() {
      return this;
    }
    close() {}
    exitFullscreen() {
      return Promise.resolve();
    }
    queryCommandSupported() {
      return false;
    }
  }
  mixIn(Document.prototype, parentNodeMethods);
  delete Document.prototype.append;
  delete Document.prototype.prepend;
  class HTMLDocument extends Document {}

  // A document built by DOMParser: queries run inside its own root fragment.
  const makeDetachedDocument = (rootFragmentId) => {
    const doc = Object.create(HTMLDocument.prototype);
    Object.defineProperty(doc, '_id', { value: rootFragmentId });
    const find = (tag) => {
      const el = doc.querySelector(tag);
      return el;
    };
    Object.defineProperties(doc, {
      documentElement: { get: () => find('html') },
      head: { get: () => find('head') },
      body: { get: () => find('body') },
      title: { get: () => { const t = find('title'); return t ? t.textContent.trim() : ''; }, set() {} },
      readyState: { get: () => 'complete' },
      defaultView: { get: () => null },
      isConnected: { get: () => false },
      ownerDocument: { get: () => null },
    });
    doc.createElement = (n) => document.createElement(n);
    doc.createTextNode = (d) => document.createTextNode(d);
    doc.getElementById = (id) => doc.querySelector('#' + CSS.escape(id));
    return doc;
  };
  class DOMParser {
    parseFromString(text, type) {
      const frag = wrap(bi('parseDocument', toStr(text)));
      return makeDetachedDocument(frag._id);
    }
  }
  class XMLSerializer {
    serializeToString(node) {
      return node.outerHTML !== undefined ? node.outerHTML : node.textContent;
    }
  }

  // ----------------------------------------------------------- custom elements
  const customElements = {
    define(name, ctor, options) {
      name = String(name);
      if (!/^[a-z][a-z0-9._]*-[a-z0-9._-]*$/.test(name)) throw new DOMException("'" + name + "' is not a valid custom element name", 'SyntaxError');
      if (customRegistry.has(name)) throw new DOMException("'" + name + "' has already been defined", 'NotSupportedError');
      const def = { name, ctor, observed: Array.from(ctor.observedAttributes || []).map(String) };
      customRegistry.set(name, def);
      customByCtor.set(ctor, def);
      const waiting = pending.get(name);
      if (waiting) {
        pending.delete(name);
        waiting.forEach((r) => r(ctor));
      }
      for (const el of JSON.parse(bi('queryAll', document._id, name)).map(wrap)) {
        if (el.isConnected) upgrade(el, def);
        else upgrade(el, def);
      }
    },
    get(name) {
      const d = customRegistry.get(String(name));
      return d ? d.ctor : undefined;
    },
    whenDefined(name) {
      name = String(name);
      if (customRegistry.has(name)) return Promise.resolve(customRegistry.get(name).ctor);
      return new Promise((resolve) => {
        if (!pending.has(name)) pending.set(name, []);
        pending.get(name).push(resolve);
      });
    },
    upgrade(root) {
      walkElements(root, (el) => {
        const d = customRegistry.get(el.localName);
        if (d) upgrade(el, d);
      });
    },
  };
  const pending = new Map();

  // --------------------------------------------------------- observers, misc
  const allIntersectionObservers = new Set();
  // "10px", "-20%", up to four values (top right bottom left) -> pixels.
  const parseMargins = (text, width, height) => {
    const parts = String(text || '0px').trim().split(/\s+/);
    const px = (v, whole) => (v.endsWith('%') ? (parseFloat(v) / 100) * whole : parseFloat(v) || 0);
    const [t, r = t, b = t, l = r] = parts;
    return { top: px(t, height), right: px(r, width), bottom: px(b, height), left: px(l, width) };
  };
  class IntersectionObserver {
    constructor(callback, options) {
      this._callback = callback;
      this._targets = new Map();
      this.root = (options && options.root) || null;
      this.rootMargin = (options && options.rootMargin) || '0px';
      const thresholds = [].concat(options && options.threshold !== undefined ? options.threshold : 0).map(Number);
      this.thresholds = thresholds.sort((a, b) => a - b);
    }
    observe(target) {
      if (this._targets.has(target)) return;
      this._targets.set(target, null);
      allIntersectionObservers.add(this);
      // The first report comes after the current task, with fresh geometry.
      setTimeout(() => this._check(), 0);
    }
    unobserve(target) {
      this._targets.delete(target);
    }
    disconnect() {
      this._targets.clear();
      allIntersectionObservers.delete(this);
    }
    takeRecords() {
      return [];
    }
    _check() {
      const [vw, vh] = viewportSize();
      const m = parseMargins(this.rootMargin, vw, vh);
      const entries = [];
      for (const [target, previous] of this._targets) {
        const r = target.getBoundingClientRect();
        const left = Math.max(r.left, -m.left);
        const top = Math.max(r.top, -m.top);
        const right = Math.min(r.right, vw + m.right);
        const bottom = Math.min(r.bottom, vh + m.bottom);
        const overlapW = right - left;
        const overlapH = bottom - top;
        const area = r.width * r.height;
        const isIntersecting = area > 0 ? overlapW > 0 && overlapH > 0 : overlapW >= 0 && overlapH >= 0 && r.bottom >= -m.top && r.top <= vh + m.bottom;
        const ratio = area > 0 && isIntersecting ? Math.min(1, (overlapW * overlapH) / area) : isIntersecting ? 1 : 0;
        let step = -1;
        for (let k = 0; k < this.thresholds.length; k++) if (ratio >= this.thresholds[k] && (isIntersecting || this.thresholds[k] === 0 && false)) step = k;
        if (previous && previous.isIntersecting === isIntersecting && previous.step === step) continue;
        this._targets.set(target, { isIntersecting, step });
        const intersection = { x: left, y: top, width: Math.max(0, overlapW), height: Math.max(0, overlapH), left, top, right, bottom };
        entries.push({ target, isIntersecting, intersectionRatio: ratio, time: nowMs(), boundingClientRect: r, intersectionRect: intersection, rootBounds: { x: 0, y: 0, width: vw, height: vh, left: 0, top: 0, right: vw, bottom: vh } });
      }
      if (entries.length) {
        try {
          this._callback.call(this, entries, this);
        } catch (e) {
          reportError(e);
        }
      }
    }
  }
  const allResizeObservers = new Set();
  class ResizeObserver {
    constructor(callback) {
      this._callback = callback;
      this._targets = new Map();
      allResizeObservers.add(this);
    }
    // Reports the targets whose box changed since the last report (the first
    // report is for any non-empty box, as in a browser, which compares with 0x0).
    _check() {
      const entries = [];
      for (const [target, last] of this._targets) {
        const r = target.getBoundingClientRect();
        if (r.width === last[0] && r.height === last[1]) continue;
        this._targets.set(target, [r.width, r.height]);
        const box = [{ inlineSize: r.width, blockSize: r.height }];
        entries.push({ target, contentRect: r, borderBoxSize: box, contentBoxSize: box, devicePixelContentBoxSize: box });
      }
      if (!entries.length) return;
      try {
        this._callback.call(this, entries, this);
      } catch (e) {
        reportError(e);
      }
    }
    observe(target) {
      if (this._targets.has(target)) return;
      this._targets.set(target, [0, 0]);
      // A browser delivers in the next rendering step, after the framework's
      // own effects have run (react-use-measure ignores a size reported before it
      // has mounted), so wait a frame.
      setTimeout(() => {
        if (this._targets.has(target)) this._check();
      }, 16);
    }
    unobserve(target) {
      this._targets.delete(target);
    }
    disconnect() {
      this._targets.clear();
    }
  }
  class PerformanceObserver {
    constructor(cb) {}
    observe() {}
    disconnect() {}
    takeRecords() {
      return [];
    }
    static get supportedEntryTypes() {
      return [];
    }
  }

  const makeStorage = () => {
    const data = new Map();
    const api = {
      getItem: (k) => (data.has(String(k)) ? data.get(String(k)) : null),
      setItem: (k, v) => {
        data.set(String(k), String(v));
      },
      removeItem: (k) => {
        data.delete(String(k));
      },
      clear: () => data.clear(),
      key: (i) => Array.from(data.keys())[i] ?? null,
    };
    return new Proxy(api, {
      get(t, p) {
        if (p === 'length') return data.size;
        if (typeof p === 'symbol') return t[p];
        if (p in t) return t[p];
        return data.has(p) ? data.get(p) : undefined;
      },
      set(t, p, v) {
        data.set(String(p), String(v));
        return true;
      },
      has(t, p) {
        return p in t || data.has(p);
      },
      deleteProperty(t, p) {
        data.delete(p);
        return true;
      },
      ownKeys() {
        return Array.from(data.keys());
      },
      getOwnPropertyDescriptor(t, p) {
        return data.has(p) ? { value: data.get(p), writable: true, enumerable: true, configurable: true } : undefined;
      },
    });
  };

  const encoder = {
    encode(str) {
      const out = [];
      str = toStr(str);
      for (let i = 0; i < str.length; i++) {
        let c = str.codePointAt(i);
        if (c > 0xffff) i++;
        if (c >= 0xd800 && c <= 0xdfff) c = 0xfffd;
        if (c < 0x80) out.push(c);
        else if (c < 0x800) out.push(0xc0 | (c >> 6), 0x80 | (c & 63));
        else if (c < 0x10000) out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
        else out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
      }
      return new Uint8Array(out);
    },
  };
  class TextEncoder {
    get encoding() {
      return 'utf-8';
    }
    encode(s) {
      return encoder.encode(s);
    }
    encodeInto(s, dest) {
      const bytes = encoder.encode(s);
      const n = Math.min(bytes.length, dest.length);
      dest.set(bytes.subarray(0, n));
      return { read: s.length, written: n };
    }
  }
  class TextDecoder {
    constructor(label) {
      this.encoding = (label || 'utf-8').toLowerCase();
      this.fatal = false;
      this.ignoreBOM = false;
    }
    decode(input) {
      if (input === undefined) return '';
      const b = input instanceof ArrayBuffer ? new Uint8Array(input) : new Uint8Array(input.buffer || input, input.byteOffset || 0, input.byteLength);
      let out = '';
      for (let i = 0; i < b.length; ) {
        const c = b[i++];
        let cp;
        if (c < 0x80) cp = c;
        else if (c >= 0xc0 && c < 0xe0) cp = ((c & 31) << 6) | (b[i++] & 63);
        else if (c >= 0xe0 && c < 0xf0) cp = ((c & 15) << 12) | ((b[i++] & 63) << 6) | (b[i++] & 63);
        else if (c >= 0xf0) cp = ((c & 7) << 18) | ((b[i++] & 63) << 12) | ((b[i++] & 63) << 6) | (b[i++] & 63);
        else cp = 0xfffd;
        out += String.fromCodePoint(cp > 0x10ffff ? 0xfffd : cp);
      }
      return out;
    }
  }
  const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  const btoa = (s) => {
    s = toStr(s);
    let out = '';
    for (let i = 0; i < s.length; i += 3) {
      const a = s.charCodeAt(i);
      const b = i + 1 < s.length ? s.charCodeAt(i + 1) : NaN;
      const c = i + 2 < s.length ? s.charCodeAt(i + 2) : NaN;
      if (a > 255 || b > 255 || c > 255) throw new DOMException('The string to be encoded contains characters outside of the Latin1 range.', 'InvalidCharacterError');
      out += B64[a >> 2] + B64[((a & 3) << 4) | ((b || 0) >> 4)] + (isNaN(b) ? '=' : B64[((b & 15) << 2) | ((c || 0) >> 6)]) + (isNaN(c) ? '=' : B64[c & 63]);
    }
    return out;
  };
  const atob = (s) => {
    s = toStr(s).replace(/[\t\n\f\r ]+/g, '').replace(/=+$/, '');
    let out = '';
    let bits = 0;
    let acc = 0;
    for (const ch of s) {
      const v = B64.indexOf(ch);
      if (v < 0) throw new DOMException('The string to be decoded is not correctly encoded.', 'InvalidCharacterError');
      acc = (acc << 6) | v;
      bits += 6;
      if (bits >= 8) {
        bits -= 8;
        out += String.fromCharCode((acc >> bits) & 255);
      }
    }
    return out;
  };
  const structuredCloneImpl = (v, seen) => {
    if (v === null || typeof v !== 'object') {
      if (typeof v === 'function' || typeof v === 'symbol') throw new DOMException('The object can not be cloned.', 'DataCloneError');
      return v;
    }
    seen = seen || new Map();
    if (seen.has(v)) return seen.get(v);
    let out;
    if (v instanceof Date) out = new Date(v.getTime());
    else if (v instanceof RegExp) out = new RegExp(v.source, v.flags);
    else if (v instanceof Map) {
      out = new Map();
      seen.set(v, out);
      for (const [k, x] of v) out.set(structuredCloneImpl(k, seen), structuredCloneImpl(x, seen));
    } else if (v instanceof Set) {
      out = new Set();
      seen.set(v, out);
      for (const x of v) out.add(structuredCloneImpl(x, seen));
    } else if (ArrayBuffer.isView(v)) out = new v.constructor(v);
    else if (Array.isArray(v)) {
      out = [];
      seen.set(v, out);
      for (let i = 0; i < v.length; i++) out[i] = structuredCloneImpl(v[i], seen);
    } else {
      out = {};
      seen.set(v, out);
      for (const k of Object.keys(v)) out[k] = structuredCloneImpl(v[k], seen);
    }
    seen.set(v, out);
    return out;
  };

  const cssEscape = (value) => {
    const s = String(value);
    let out = '';
    for (let i = 0; i < s.length; i++) {
      const c = s.charCodeAt(i);
      if (c === 0) out += '�';
      else if ((c >= 1 && c <= 31) || c === 127 || (i === 0 && c >= 48 && c <= 57) || (i === 1 && c >= 48 && c <= 57 && s.charCodeAt(0) === 45)) out += '\\' + c.toString(16) + ' ';
      else if (i === 0 && c === 45 && s.length === 1) out += '\\' + s[i];
      else if (c >= 128 || c === 45 || c === 95 || (c >= 48 && c <= 57) || (c >= 65 && c <= 90) || (c >= 97 && c <= 122)) out += s[i];
      else out += '\\' + s[i];
    }
    return out;
  };

  // ------------------------------------------------------- fetch and XHR
  // The engine's own Uint8Array base64 is native; the JavaScript versions below it walk
  // the text one character at a time and read a long string variable once per character,
  // which is quadratic, so they are only a fallback.
  const toBase64Slow = (bytes) => {
    let s = '';
    for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
    return btoa(s);
  };
  const fromBase64Slow = (text) => {
    const s = atob(text);
    const out = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
    return out;
  };
  const toBase64 = (bytes) => {
    if (!(bytes instanceof Uint8Array)) bytes = new Uint8Array(bytes.buffer || bytes, bytes.byteOffset || 0, bytes.byteLength);
    return typeof bytes.toBase64 === 'function' ? bytes.toBase64() : toBase64Slow(bytes);
  };
  const fromBase64 = (text) => (typeof Uint8Array.fromBase64 === 'function' ? Uint8Array.fromBase64(text) : fromBase64Slow(text));
  const netPending = new Map();
  const netSend = (method, url, headers, body, onDone, onFail, kind) => {
    const id = bi('netStart', method, url, JSON.stringify(headers), body && body.length ? toBase64(body) : '', kind || '');
    netPending.set(id, { onDone, onFail });
    return id;
  };

  class Headers {
    constructor(init) {
      this._h = new Map();
      if (!init) return;
      if (init instanceof Headers) init.forEach((v, k) => this.append(k, v));
      else if (typeof init[Symbol.iterator] === 'function') for (const [k, v] of init) this.append(k, v);
      else for (const k of Object.keys(init)) this.append(k, init[k]);
    }
    append(k, v) {
      k = String(k).toLowerCase();
      this._h.set(k, this._h.has(k) ? this._h.get(k) + ', ' + String(v) : String(v));
    }
    set(k, v) {
      this._h.set(String(k).toLowerCase(), String(v));
    }
    get(k) {
      const v = this._h.get(String(k).toLowerCase());
      return v === undefined ? null : v;
    }
    has(k) {
      return this._h.has(String(k).toLowerCase());
    }
    delete(k) {
      this._h.delete(String(k).toLowerCase());
    }
    forEach(fn, thisArg) {
      for (const [k, v] of this._h) fn.call(thisArg, v, k, this);
    }
    keys() {
      return this._h.keys();
    }
    values() {
      return this._h.values();
    }
    entries() {
      return this._h.entries();
    }
    [Symbol.iterator]() {
      return this._h.entries();
    }
  }

  class FormData {
    constructor() {
      this._e = [];
    }
    append(k, v) {
      this._e.push([String(k), typeof v === 'object' && v !== null ? v : String(v)]);
    }
    set(k, v) {
      this.delete(k);
      this.append(k, v);
    }
    get(k) {
      const e = this._e.find((p) => p[0] === String(k));
      return e ? e[1] : null;
    }
    getAll(k) {
      return this._e.filter((p) => p[0] === String(k)).map((p) => p[1]);
    }
    has(k) {
      return this._e.some((p) => p[0] === String(k));
    }
    delete(k) {
      this._e = this._e.filter((p) => p[0] !== String(k));
    }
    forEach(fn, thisArg) {
      for (const [k, v] of this._e) fn.call(thisArg, v, k, this);
    }
    keys() {
      return this._e.map((p) => p[0])[Symbol.iterator]();
    }
    values() {
      return this._e.map((p) => p[1])[Symbol.iterator]();
    }
    entries() {
      return this._e.map((p) => [p[0], p[1]])[Symbol.iterator]();
    }
    [Symbol.iterator]() {
      return this.entries();
    }
  }

  // A request body as bytes and the content type it implies.
  const encodeBody = (body) => {
    if (body === null || body === undefined) return { bytes: null, type: null };
    if (typeof body === 'string') return { bytes: encoder.encode(body), type: 'text/plain;charset=UTF-8' };
    if (body instanceof URLSearchParams) return { bytes: encoder.encode(body.toString()), type: 'application/x-www-form-urlencoded;charset=UTF-8' };
    if (body instanceof FormData) {
      const boundary = '----BlueIceFormBoundary' + Math.floor(Math.random() * 1e12);
      let text = '';
      for (const [k, v] of body._e) text += '--' + boundary + '\r\nContent-Disposition: form-data; name="' + k.replace(/"/g, '%22') + '"\r\n\r\n' + (typeof v === 'string' ? v : String(v)) + '\r\n';
      text += '--' + boundary + '--\r\n';
      return { bytes: encoder.encode(text), type: 'multipart/form-data; boundary=' + boundary };
    }
    if (body instanceof ArrayBuffer) return { bytes: new Uint8Array(body), type: null };
    if (ArrayBuffer.isView(body)) return { bytes: new Uint8Array(body.buffer, body.byteOffset, body.byteLength), type: null };
    return { bytes: encoder.encode(String(body)), type: 'text/plain;charset=UTF-8' };
  };

  class Response {
    constructor(body, init) {
      init = init || {};
      this._bytes = body === null || body === undefined ? new Uint8Array(0) : encodeBody(body).bytes || new Uint8Array(0);
      this.status = init.status === undefined ? 200 : init.status;
      this.statusText = init.statusText === undefined ? '' : String(init.statusText);
      this.headers = new Headers(init.headers);
      if (typeof body === 'string' && !this.headers.has('content-type')) this.headers.set('content-type', 'text/plain;charset=UTF-8');
      this.url = '';
      this.redirected = false;
      this.type = 'default';
      this.bodyUsed = false;
    }
    get ok() {
      return this.status >= 200 && this.status < 300;
    }
    get body() {
      const bytes = this._bytes;
      return new ReadableStream({ start(c) { c.enqueue(bytes); c.close(); } });
    }
    _take() {
      if (this.bodyUsed) return Promise.reject(new TypeError('Body has already been consumed.'));
      this.bodyUsed = true;
      return Promise.resolve(this._bytes);
    }
    text() {
      return this._take().then((b) => new TextDecoder().decode(b));
    }
    json() {
      return this.text().then((t) => JSON.parse(t));
    }
    arrayBuffer() {
      return this._take().then((b) => b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength));
    }
    blob() {
      return this._take().then((b) => new Blob([b], { type: this.headers.get('content-type') || '' }));
    }
    formData() {
      return Promise.reject(new TypeError('formData() is not supported'));
    }
    clone() {
      const r = new Response(null, { status: this.status, statusText: this.statusText, headers: this.headers });
      r._bytes = this._bytes;
      r.url = this.url;
      r.type = this.type;
      return r;
    }
    static error() {
      const r = new Response(null, { status: 0 });
      r.type = 'error';
      return r;
    }
    static json(data, init) {
      const r = new Response(JSON.stringify(data), init);
      r.headers.set('content-type', 'application/json');
      return r;
    }
  }
  class Request {
    constructor(input, init) {
      init = init || {};
      const from = input instanceof Request ? input : null;
      this.url = resolveUrl(from ? from.url : String(input));
      this.method = String(init.method || (from && from.method) || 'GET').toUpperCase();
      this.headers = new Headers(init.headers || (from && from.headers));
      this.body = init.body !== undefined ? init.body : from ? from.body : null;
      this.signal = init.signal || (from && from.signal) || null;
      this.credentials = init.credentials || 'same-origin';
      this.mode = init.mode || 'cors';
      this.cache = init.cache || 'default';
      this.redirect = init.redirect || 'follow';
      this.referrer = init.referrer || '';
      this.bodyUsed = false;
    }
    clone() {
      return new Request(this);
    }
    text() {
      const e = encodeBody(this.body).bytes;
      return Promise.resolve(e ? new TextDecoder().decode(e) : '');
    }
    json() {
      return this.text().then((t) => JSON.parse(t));
    }
  }

  const fetchImpl = (input, init) =>
    new Promise((resolve, reject) => {
      let request;
      try {
        request = new Request(input, init);
      } catch (e) {
        reject(e);
        return;
      }
      const signal = request.signal;
      const abortError = () => new DOMException('The operation was aborted.', 'AbortError');
      if (signal && signal.aborted) {
        reject(abortError());
        return;
      }
      const { bytes, type } = encodeBody(request.body);
      if (type && !request.headers.has('content-type')) request.headers.set('content-type', type);
      if (request.method === 'GET' || request.method === 'HEAD') {
        if (bytes && bytes.length) {
          reject(new TypeError('Request with GET/HEAD method cannot have body.'));
          return;
        }
      }
      let settled = false;
      const headers = Array.from(request.headers);
      let id;
      try {
        id = netSend(request.method, request.url, headers, bytes, (r) => {
          if (settled) return;
          settled = true;
          const response = new Response(null, { status: r.status, statusText: r.statusText, headers: r.headers });
          response._bytes = r.bytes;
          response.url = r.url;
          response.redirected = r.url !== request.url;
          response.type = r.url.split('/').slice(0, 3).join('/') === loc().origin ? 'basic' : 'cors';
          resolve(response);
        }, (message) => {
          if (settled) return;
          settled = true;
          console.error('Failed to fetch ' + request.url + ': ' + message);
          reject(new TypeError('Failed to fetch'));
        });
      } catch (e) {
        reject(new TypeError('Failed to fetch'));
        return;
      }
      if (signal) {
        signal.addEventListener('abort', () => {
          if (settled) return;
          settled = true;
          netPending.delete(id);
          reject(abortError());
        });
      }
    });

  class XMLHttpRequest extends EventTarget {
    constructor() {
      super();
      this.readyState = 0;
      this.status = 0;
      this.statusText = '';
      this.responseText = '';
      this.responseURL = '';
      this.responseType = '';
      this.timeout = 0;
      this.withCredentials = false;
      this.upload = new EventTarget();
      this._headers = new Headers();
      this._responseHeaders = null;
      this._bytes = null;
      this.onreadystatechange = null;
    }
    get response() {
      if (this.readyState !== 4) return this.responseType === '' || this.responseType === 'text' ? '' : null;
      switch (this.responseType) {
        case 'json':
          try {
            return JSON.parse(this.responseText);
          } catch (_) {
            return null;
          }
        case 'arraybuffer':
          return this._bytes.buffer.slice(this._bytes.byteOffset, this._bytes.byteOffset + this._bytes.byteLength);
        case 'blob':
          return new Blob([this._bytes], { type: this.getResponseHeader('content-type') || '' });
        default:
          return this.responseText;
      }
    }
    _fire(type, init) {
      const e = new ProgressEvent(type, init);
      e.target = this;
      const handler = this['on' + type];
      if (typeof handler === 'function') {
        try {
          handler.call(this, e);
        } catch (err) {
          reportError(err);
        }
      }
      this.dispatchEvent(e);
    }
    _state(n) {
      this.readyState = n;
      this._fire('readystatechange');
    }
    open(method, url, async) {
      if (async === false) throw new DOMException('Synchronous XMLHttpRequest is not supported', 'InvalidAccessError');
      this._method = String(method).toUpperCase();
      this._url = resolveUrl(String(url));
      this._headers = new Headers();
      this._aborted = false;
      this._state(1);
    }
    setRequestHeader(k, v) {
      this._headers.append(k, v);
    }
    overrideMimeType() {}
    getResponseHeader(name) {
      return this._responseHeaders ? this._responseHeaders.get(name) : null;
    }
    getAllResponseHeaders() {
      if (!this._responseHeaders) return '';
      let out = '';
      this._responseHeaders.forEach((v, k) => (out += k + ': ' + v + '\r\n'));
      return out;
    }
    send(body) {
      if (this.readyState !== 1) throw new DOMException('The object is in an invalid state.', 'InvalidStateError');
      const { bytes, type } = this._method === 'GET' || this._method === 'HEAD' ? { bytes: null, type: null } : encodeBody(body);
      if (type && !this._headers.has('content-type')) this._headers.set('content-type', type);
      this._fire('loadstart');
      let id;
      try {
        id = netSend(this._method, this._url, Array.from(this._headers), bytes, (r) => {
          if (this._aborted) return;
          this.status = r.status;
          this.statusText = r.statusText;
          this.responseURL = r.url;
          this._responseHeaders = r.headers;
          this._bytes = r.bytes;
          this.responseText = new TextDecoder().decode(r.bytes);
          this._state(2);
          this._state(3);
          this._state(4);
          this._fire('load', { loaded: r.bytes.length, total: r.bytes.length, lengthComputable: true });
          this._fire('loadend');
        }, (message) => {
          if (this._aborted) return;
          console.error('XMLHttpRequest failed for ' + this._url + ': ' + message);
          this._state(4);
          this._fire('error');
          this._fire('loadend');
        });
      } catch (e) {
        this._state(4);
        this._fire('error');
        this._fire('loadend');
        return;
      }
      this._id = id;
      if (this.timeout > 0) {
        setTimeout(() => {
          if (this.readyState === 4 || this._aborted) return;
          this._aborted = true;
          netPending.delete(id);
          this._state(4);
          this._fire('timeout');
          this._fire('loadend');
        }, this.timeout);
      }
    }
    abort() {
      if (this.readyState === 0 || this.readyState === 4) return;
      this._aborted = true;
      netPending.delete(this._id);
      this.readyState = 4;
      this._fire('readystatechange');
      this._fire('abort');
      this._fire('loadend');
    }
  }
  Object.assign(XMLHttpRequest, { UNSENT: 0, OPENED: 1, HEADERS_RECEIVED: 2, LOADING: 3, DONE: 4 });

  // --------------------------------------------------- location and history
  const locationApi = {};
  const loc = () => parseUrl(bi('url'));
  const navigateTo = (url, replace) => {
    const target = parseUrl(url, loc().href);
    if (!target) throw new DOMException("Failed to parse URL '" + url + "'", 'SyntaxError');
    const here = loc();
    // A change of only the fragment stays in the document.
    if (target.href.split('#')[0] === here.href.split('#')[0] && target.hash !== here.hash) {
      historyState.push(replace, null, target.href);
      bi('pushState', target.href, !!replace);
      queueMicrotask(() => global.dispatchEvent(new HashChangeEvent('hashchange', { oldURL: here.href, newURL: target.href })));
      return;
    }
    bi('navigate', target.href, !!replace);
  };
  for (const key of ['href', 'origin', 'protocol', 'host', 'hostname', 'port', 'pathname', 'search', 'hash']) {
    Object.defineProperty(locationApi, key, {
      enumerable: true,
      configurable: true,
      get() {
        return loc()[key];
      },
      set(v) {
        if (key === 'href') return navigateTo(v, false);
        if (key === 'origin') return;
        const u = new URL(loc().href);
        u[key] = v;
        navigateTo(u.href, false);
      },
    });
  }
  define(locationApi, {
    assign: (u) => navigateTo(String(u), false),
    replace: (u) => navigateTo(String(u), true),
    reload: () => bi('reload'),
    toString: () => loc().href,
    ancestorOrigins: [],
  });
  Object.defineProperty(locationApi, Symbol.toPrimitive, { value: () => loc().href });

  // Same-document history entries; going past them asks the host.
  const historyState = {
    entries: [{ state: null, url: bi('url') }],
    index: 0,
    push(replace, state, url) {
      if (replace) this.entries[this.index] = { state, url };
      else {
        this.entries.length = this.index + 1;
        this.entries.push({ state, url });
        this.index++;
      }
    },
  };
  const historyApi = {
    get length() {
      return historyState.entries.length;
    },
    get state() {
      return historyState.entries[historyState.index].state;
    },
    scrollRestoration: 'auto',
    pushState(state, title, url) {
      this._change(state, url, false);
    },
    replaceState(state, title, url) {
      this._change(state, url, true);
    },
    _change(state, url, replace) {
      let target = loc().href;
      if (url !== undefined && url !== null) {
        const u = parseUrl(String(url), target);
        if (!u) throw new DOMException("Failed to parse URL '" + url + "'", 'SecurityError');
        if (u.origin !== loc().origin) throw new DOMException("A history state object with URL '" + u.href + "' cannot be created in a document with origin '" + loc().origin + "'.", 'SecurityError');
        target = u.href;
      }
      historyState.push(replace, structuredCloneImpl(state), target);
      bi('pushState', target, replace);
    },
    go(delta) {
      delta = delta | 0;
      const next = historyState.index + delta;
      if (next >= 0 && next < historyState.entries.length && delta !== 0) {
        const from = loc().href;
        historyState.index = next;
        const entry = historyState.entries[next];
        const url = entry.url || from;
        bi('pushState', url, true);
        queueMicrotask(() => {
          global.dispatchEvent(new PopStateEvent('popstate', { state: entry.state }));
          if (url.split('#')[0] === from.split('#')[0] && url !== from) global.dispatchEvent(new HashChangeEvent('hashchange', { oldURL: from, newURL: url }));
        });
      } else if (delta !== 0) {
        bi('go', delta);
      }
    },
    back() {
      this.go(-1);
    },
    forward() {
      this.go(1);
    },
  };

  // ----------------------------------------------------------------- window
  const navigatorApi = {
    userAgent: 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) BlueIce/1.0 Safari/537.36',
    appName: 'Netscape',
    appVersion: '5.0 (X11)',
    appCodeName: 'Mozilla',
    product: 'Gecko',
    productSub: '20030107',
    vendor: 'BlueIce',
    platform: 'Linux x86_64',
    language: 'en-US',
    languages: ['en-US', 'en'],
    onLine: true,
    cookieEnabled: true,
    doNotTrack: null,
    webdriver: false,
    hardwareConcurrency: 4,
    maxTouchPoints: 0,
    deviceMemory: 4,
    pdfViewerEnabled: false,
    sendBeacon: (url, data) => {
      try {
        const { bytes, type } = encodeBody(data);
        netSend('POST', resolveUrl(String(url)), type ? [['content-type', type]] : [], bytes, () => {}, () => {});
        return true;
      } catch (_) {
        return false;
      }
    },
    javaEnabled: () => false,
    vibrate: () => false,
    clipboard: { writeText: () => Promise.resolve(), readText: () => Promise.resolve('') },
    permissions: { query: () => Promise.resolve({ state: 'denied', addEventListener() {} }) },
    mediaDevices: undefined,
    serviceWorker: undefined,
    geolocation: { getCurrentPosition(_, err) { if (err) err({ code: 1, message: 'denied' }); }, watchPosition: () => 0, clearWatch() {} },
    connection: undefined,
  };
  const screenApi = { width: 1280, height: 720, availWidth: 1280, availHeight: 720, colorDepth: 24, pixelDepth: 24, orientation: { type: 'landscape-primary', angle: 0, addEventListener() {} } };

  const viewportSize = () => JSON.parse(bi('viewport'));
  const scheduleCallbacks = {
    requestAnimationFrame(cb) {
      return setTimeout(() => cb(nowMs()), 16);
    },
    cancelAnimationFrame(id) {
      clearTimeout(id);
    },
    requestIdleCallback(cb, options) {
      const start = nowMs();
      return setTimeout(() => cb({ didTimeout: false, timeRemaining: () => Math.max(0, 50 - (nowMs() - start)) }), 1);
    },
    cancelIdleCallback(id) {
      clearTimeout(id);
    },
  };

  const selectionApi = { rangeCount: 0, isCollapsed: true, type: 'None', anchorNode: null, focusNode: null, toString: () => '', removeAllRanges() {}, addRange() {}, getRangeAt() { return document.createRange(); }, collapse() {}, empty() {}, containsNode: () => false };

  const matchMedia = (query) => {
    const q = toStr(query);
    const list = new EventTarget();
    Object.defineProperties(list, { matches: { get: () => bi('media', q) }, media: { value: q } });
    define(list, { addListener() {}, removeListener() {}, onchange: null });
    return list;
  };

  const computed = (el) => {
    const inline = el.style;
    const api = {
      getPropertyValue: (name) => bi('computed', el._id, toStr(name)) || inline.getPropertyValue(name),
      getPropertyPriority: () => '',
      item: () => '',
      length: 0,
    };
    return new Proxy(api, {
      get(t, p) {
        if (typeof p === 'symbol') return undefined;
        if (p in t) return t[p];
        return bi('computed', el._id, camelToKebab(p)) || inline[p] || '';
      },
    });
  };

  const allMessages = [];
  class MessageChannel {
    constructor() {
      const make = () => {
        const port = new EventTarget();
        port.onmessage = null;
        port.start = () => {};
        port.close = () => {};
        return port;
      };
      this.port1 = make();
      this.port2 = make();
      const link = (a, b) => {
        a.postMessage = (data) => {
          setTimeout(() => {
            const e = new MessageEvent('message', { data: structuredCloneImpl(data) });
            if (typeof b.onmessage === 'function') b.onmessage(e);
            b.dispatchEvent(e);
          }, 0);
        };
      };
      link(this.port1, this.port2);
      link(this.port2, this.port1);
    }
  }
  class BroadcastChannel extends EventTarget {
    constructor(name) {
      super();
      this.name = name;
    }
    postMessage() {}
    close() {}
  }

  const crypto = {
    getRandomValues(array) {
      for (let i = 0; i < array.length; i++) array[i] = Math.floor(Math.random() * 4294967296);
      return array;
    },
    randomUUID() {
      const h = (n) => Array.from({ length: n }, () => Math.floor(Math.random() * 16).toString(16)).join('');
      return h(8) + '-' + h(4) + '-4' + h(3) + '-' + '89ab'[Math.floor(Math.random() * 4)] + h(3) + '-' + h(12);
    },
    subtle: {},
  };

  const performance = {
    now: nowMs,
    timeOrigin: startedAt,
    mark: () => ({}),
    measure: () => ({}),
    clearMarks() {},
    clearMeasures() {},
    getEntries: () => [],
    getEntriesByType: () => [],
    getEntriesByName: () => [],
    toJSON: () => ({}),
    timing: { navigationStart: startedAt, domContentLoadedEventEnd: startedAt, loadEventEnd: startedAt },
    navigation: { type: 0 },
    memory: { usedJSHeapSize: 0, totalJSHeapSize: 0, jsHeapSizeLimit: 0 },
  };

  // `window` is the global object; EventTarget and handler properties go on it.
  const windowTarget = new EventTarget();
  for (const name of ['addEventListener', 'removeEventListener', 'dispatchEvent']) {
    define(global, { [name]: (...a) => windowTarget[name].apply(global, a) });
  }
  // The global's own listener list lives in `windowTarget`'s registry.
  listenersOf.set(global, registry(windowTarget));
  installHandlerProperties(global, EVENT_HANDLERS.concat(WINDOW_HANDLERS));
  installHandlerProperties(Node.prototype, EVENT_HANDLERS);
  installHandlerProperties(Document.prototype, WINDOW_HANDLERS);

  const document = Object.create(HTMLDocument.prototype);
  Object.defineProperty(document, '_id', { value: bi('root') });
  wrappers.set(document._id, document);

  const exported = {
    // events
    Event, CustomEvent, UIEvent, MouseEvent, PointerEvent, KeyboardEvent, FocusEvent, InputEvent, MessageEvent,
    PopStateEvent, HashChangeEvent, ErrorEvent, ProgressEvent, TransitionEvent, AnimationEvent, WheelEvent, TouchEvent,
    EventTarget,
    // nodes
    Node, CharacterData, Text, Comment, DocumentFragment, Document, HTMLDocument, Element, HTMLElement, HTMLUnknownElement,
    SVGElement, SVGSVGElement, Attr, DOMTokenList, MutationObserver, MutationRecord,
    HTMLAnchorElement, HTMLImageElement, HTMLInputElement, HTMLTextAreaElement, HTMLSelectElement, HTMLOptionElement,
    HTMLButtonElement, HTMLFormElement, HTMLLabelElement, HTMLScriptElement, HTMLLinkElement, HTMLStyleElement,
    HTMLIFrameElement, HTMLCanvasElement, HTMLMediaElement, HTMLVideoElement, HTMLAudioElement, HTMLTemplateElement,
    HTMLTableElement, HTMLTableRowElement, HTMLTableCellElement, HTMLDialogElement, HTMLDivElement, HTMLSpanElement,
    HTMLParagraphElement, HTMLBodyElement, HTMLHeadElement, HTMLHtmlElement, HTMLUListElement, HTMLOListElement,
    HTMLLIElement, HTMLHeadingElement, HTMLPreElement, HTMLBRElement, HTMLHRElement, HTMLMetaElement, HTMLTitleElement,
    HTMLBaseElement, HTMLSourceElement, HTMLPictureElement, HTMLSlotElement, HTMLDetailsElement, HTMLFieldSetElement,
    HTMLLegendElement, HTMLMainElement, HTMLProgressElement, HTMLObjectElement, HTMLEmbedElement, HTMLOutputElement,
    HTMLTableSectionElement, HTMLTableCaptionElement, HTMLQuoteElement, HTMLMenuElement, HTMLDListElement,
    HTMLAreaElement, HTMLMapElement, HTMLDataElement, HTMLTimeElement, HTMLModElement, HTMLParamElement,
    HTMLTrackElement, HTMLOptGroupElement, HTMLDataListElement, HTMLMeterElement, HTMLTableColElement,
    DOMParser, XMLSerializer, IntersectionObserver, ResizeObserver, PerformanceObserver, MessageChannel, BroadcastChannel,
    URL, URLSearchParams, TextEncoder, TextDecoder,
    // values
    document, customElements, navigator: navigatorApi, screen: screenApi, location: locationApi, history: historyApi,
    performance, crypto, localStorage: makeStorage(), sessionStorage: makeStorage(),
    CSS: { supports: () => true, escape: cssEscape },
    atob, btoa, structuredClone: (v) => structuredCloneImpl(v), reportError,
    Headers, FormData, Response, Request, XMLHttpRequest, fetch: fetchImpl,
  };
  for (const key of Object.keys(exported)) {
    Object.defineProperty(global, key, { value: exported[key], writable: true, configurable: true, enumerable: false });
  }
  define(global, {
    window: global,
    self: global,
    top: global,
    parent: global,
    frames: global,
    name: '',
    closed: false,
    length: 0,
    origin: loc().origin,
    isSecureContext: loc().protocol === 'https:',
    innerWidth: viewportSize()[0],
    innerHeight: viewportSize()[1],
    outerWidth: viewportSize()[0],
    outerHeight: viewportSize()[1],
    devicePixelRatio: 1,
    scrollX: 0,
    scrollY: 0,
    pageXOffset: 0,
    pageYOffset: 0,
    screenX: 0,
    screenY: 0,
    visualViewport: { width: viewportSize()[0], height: viewportSize()[1], scale: 1, offsetLeft: 0, offsetTop: 0, pageLeft: 0, pageTop: 0, addEventListener() {}, removeEventListener() {} },
    getComputedStyle: (el) => computed(el),
    matchMedia,
    getSelection: () => selectionApi,
    scrollTo() {},
    scrollBy() {},
    scroll() {},
    alert() {},
    confirm: () => false,
    prompt: () => null,
    print() {},
    stop() {},
    focus() {},
    blur() {},
    open: () => null,
    close() {},
    postMessage(data, targetOrigin) {
      setTimeout(() => global.dispatchEvent(new MessageEvent('message', { data: structuredCloneImpl(data), origin: loc().origin, source: global })), 0);
    },
    requestAnimationFrame: scheduleCallbacks.requestAnimationFrame,
    cancelAnimationFrame: scheduleCallbacks.cancelAnimationFrame,
    requestIdleCallback: scheduleCallbacks.requestIdleCallback,
    cancelIdleCallback: scheduleCallbacks.cancelIdleCallback,
    createImageBitmap: () => Promise.reject(new TypeError('createImageBitmap is not supported')),
    addEventListener: global.addEventListener,
  });
  // Constructors pages call directly.
  global.Image = function Image(width, height) {
    const img = document.createElement('img');
    if (width !== undefined) img.setAttribute('width', width);
    if (height !== undefined) img.setAttribute('height', height);
    return img;
  };
  global.Image.prototype = HTMLImageElement.prototype;
  global.Option = function Option(text, value) {
    const o = document.createElement('option');
    if (text !== undefined) o.textContent = text;
    if (value !== undefined) o.setAttribute('value', value);
    return o;
  };
  global.Audio = function Audio(src) {
    const a = document.createElement('audio');
    if (src !== undefined) a.setAttribute('src', src);
    return a;
  };
  const wrapNative = (name) => {
    const native = global[name];
    if (typeof native !== 'function') return;
    global[name] = function (callback, delay, ...args) {
      if (typeof callback === 'string') {
        const source = callback;
        callback = () => (0, eval)(source);
      }
      return native(callback, delay === undefined ? 0 : +delay || 0, ...args);
    };
  };
  wrapNative('setTimeout');
  wrapNative('setInterval');

  // The host drives these.
  define(global, {
    __blueice: {
      wrap,
      fire(type, init) {
        const e = new Event(type, init);
        e.isTrusted = true;
        return global.dispatchEvent(e);
      },
      fireDocument(type, init) {
        const e = new Event(type, init);
        e.isTrusted = true;
        return document.dispatchEvent(e);
      },
      netDone(id, status, statusText, headersJson, bodyBase64, url) {
        const entry = netPending.get(id);
        if (!entry) return;
        netPending.delete(id);
        entry.onDone({ status, statusText, headers: new Headers(JSON.parse(headersJson)), bytes: fromBase64(bodyBase64), url });
      },
      netFail(id, message) {
        const entry = netPending.get(id);
        if (!entry) return;
        netPending.delete(id);
        entry.onFail(message);
      },
      scrolled(y) {
        global.scrollY = y;
        global.pageYOffset = y;
        document.dispatchEvent(Object.assign(new Event('scroll', { bubbles: true }), { isTrusted: true }));
        for (const io of Array.from(allIntersectionObservers)) io._check();
      },
      afterLayout() {
        for (const io of Array.from(allIntersectionObservers)) io._check();
        for (const ro of Array.from(allResizeObservers)) ro._check();
      },
      setReadyState(state) {
        document._readyState = state;
        const e = new Event('readystatechange');
        document.dispatchEvent(e);
      },
      scriptLoaded(id, source, ok) {
        const el = wrap(id);
        if (!el) return;
        if (ok) {
          const previous = document._currentScript;
          document._currentScript = el;
          try {
            (0, eval)(source);
          } catch (e) {
            reportError(e);
          } finally {
            document._currentScript = previous || null;
          }
        }
        el.dispatchEvent(new Event(ok ? 'load' : 'error'));
      },
      setCurrentScript(id) {
        document._currentScript = id === null || id === undefined ? null : wrap(id);
      },
      dispatch(id, type, init) {
        const target = wrap(id);
        if (!target) return true;
        const make = { click: MouseEvent, mousedown: MouseEvent, mouseup: MouseEvent, input: InputEvent, keydown: KeyboardEvent, keyup: KeyboardEvent, focus: FocusEvent, blur: FocusEvent }[type] || Event;
        const e = new make(type, Object.assign({ bubbles: true, cancelable: true }, init || {}));
        e.isTrusted = true;
        return target.dispatchEvent(e);
      },
      popstate(url, stateJson) {
        let state = null;
        try {
          state = stateJson ? JSON.parse(stateJson) : null;
        } catch (_) {}
        global.dispatchEvent(new PopStateEvent('popstate', { state }));
      },
      customElements,
    },
  });
  Object.defineProperty(global, '__blueice', { enumerable: false });
})(globalThis);
