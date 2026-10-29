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
      return true;
    }
    get naturalWidth() {
      return 0;
    }
    get naturalHeight() {
      return 0;
    }
    get currentSrc() {
      return this.src;
    }
    get width() {
      return this.offsetWidth || parseInt(this.getAttribute('width'), 10) || 0;
    }
    set width(v) {
      this.setAttribute('width', v);
    }
    get height() {
      return this.offsetHeight || parseInt(this.getAttribute('height'), 10) || 0;
    }
    set height(v) {
      this.setAttribute('height', v);
    }
    decode() {
      return Promise.resolve();
    }
  }
  reflect(HTMLImageElement.prototype, [['src', 'src', 'url'], ['srcset', 'srcset'], ['alt', 'alt'], ['sizes', 'sizes'], ['loading', 'loading'], ['crossOrigin', 'crossorigin']]);
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
  class HTMLCanvasElement extends HTMLElement {
    getContext() {
      return null;
    }
    toDataURL() {
      return 'data:,';
    }
  }
  register(HTMLCanvasElement, 'canvas');
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
  class IntersectionObserver {
    constructor(callback, options) {
      this._callback = callback;
      this._targets = new Set();
      this.root = (options && options.root) || null;
      this.rootMargin = (options && options.rootMargin) || '0px';
      this.thresholds = [].concat((options && options.threshold) || 0);
    }
    observe(target) {
      if (this._targets.has(target)) return;
      this._targets.add(target);
      // Nothing scrolls here yet, so every observed element reports itself
      // visible, which is what lets "reveal on scroll" content appear.
      setTimeout(() => {
        if (!this._targets.has(target)) return;
        const r = target.getBoundingClientRect();
        try {
          this._callback.call(this, [{ target, isIntersecting: true, intersectionRatio: 1, time: nowMs(), boundingClientRect: r, intersectionRect: r, rootBounds: null }], this);
        } catch (e) {
          reportError(e);
        }
      }, 0);
    }
    unobserve(target) {
      this._targets.delete(target);
    }
    disconnect() {
      this._targets.clear();
    }
    takeRecords() {
      return [];
    }
  }
  class ResizeObserver {
    constructor(callback) {
      this._callback = callback;
      this._targets = new Set();
    }
    observe(target) {
      this._targets.add(target);
      setTimeout(() => {
        if (!this._targets.has(target)) return;
        const r = target.getBoundingClientRect();
        const box = [{ inlineSize: r.width, blockSize: r.height }];
        try {
          this._callback.call(this, [{ target, contentRect: r, borderBoxSize: box, contentBoxSize: box, devicePixelContentBoxSize: box }], this);
        } catch (e) {
          reportError(e);
        }
      }, 0);
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
    sendBeacon: () => true,
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
