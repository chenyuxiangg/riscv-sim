// front/tests/dom-stub.mjs
//
// A minimal DOM good enough to boot the RISC-V Sim workbench under
// node:test. It is deliberately NOT a browser: it implements only the
// handful of APIs the front end actually touches.
//
// Implemented: element tree, attributes, class list, dataset proxy,
// textContent / innerHTML (with a small HTML parser), event dispatch
// with bubbling, `matches` / `closest` / `querySelector(All)` over
// tag / #id / .class / [attr] / [attr="value"] selectors, and the
// `document` / `window` / `location` / `history` globals.
//
// This file is a test double — it is not shipped with the front end.

const VOID_TAGS = new Set([
    "area", "base", "br", "col", "embed", "hr", "img", "input",
    "link", "meta", "param", "source", "track", "wbr",
]);

const ENTITIES = {
    amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: " ",
};

/** Decode the HTML entities the front end emits. */
function decodeEntities(text) {
    return text.replace(/&(#x?[0-9a-fA-F]+|[a-zA-Z]+);/g, (whole, body) => {
        if (body[0] === "#") {
            const code = body[1] === "x" || body[1] === "X"
                ? Number.parseInt(body.slice(2), 16)
                : Number.parseInt(body.slice(1), 10);
            return Number.isFinite(code) ? String.fromCodePoint(code) : whole;
        }
        return ENTITIES[body.toLowerCase()] ?? whole;
    });
}

/** `dataset` proxy → `data-*` attributes. */
function makeDataset(el) {
    const attrName = (prop) =>
        `data-${String(prop).replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)}`;
    return new Proxy({}, {
        get(_t, prop) {
            if (typeof prop !== "string") return undefined;
            return el.getAttribute(attrName(prop)) ?? undefined;
        },
        set(_t, prop, value) {
            el.setAttribute(attrName(prop), String(value));
            return true;
        },
        has(_t, prop) {
            return typeof prop === "string" && el.hasAttribute(attrName(prop));
        },
        deleteProperty(_t, prop) {
            el.removeAttribute(attrName(prop));
            return true;
        },
    });
}

/** `classList` backed by the `class` attribute. */
class ClassList {
    constructor(el) { this._el = el; }
    _tokens() {
        return (this._el.getAttribute("class") ?? "").split(/\s+/).filter(Boolean);
    }
    _write(tokens) { this._el.setAttribute("class", tokens.join(" ")); }
    add(...names) {
        const tokens = this._tokens();
        for (const n of names) if (!tokens.includes(n)) tokens.push(n);
        this._write(tokens);
    }
    remove(...names) {
        this._write(this._tokens().filter((n) => !names.includes(n)));
    }
    contains(name) { return this._tokens().includes(name); }
    toggle(name, force) {
        const want = force === undefined ? !this.contains(name) : Boolean(force);
        if (want) this.add(name);
        else this.remove(name);
        return want;
    }
    get length() { return this._tokens().length; }
    toString() { return this._tokens().join(" "); }
}

/**
 * Shadow Node's read-only `Event.target` / `Event.currentTarget` getters
 * with the dispatching element.
 */
function setEventProp(event, name, node) {
    try {
        Object.defineProperty(event, name, {
            value: node, configurable: true, writable: true, enumerable: true,
        });
    } catch (_e) {
        // Non-configurable — the listener just sees `null`.
    }
    return event;
}

/** Escape text the way the real DOM reports it in `innerHTML`. */
function escapeMarkup(text) {
    return String(text)
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;");
}

/**
 * Wrap `stopPropagation` so the stub can observe that propagation was
 * stopped, independently of how the host runtime reports it
 * (FRONTEND_Review.md R-05).
 */
function watchStopPropagation(event) {
    if (typeof event.stopPropagation !== "function" || event.__stubStopWatched) return;
    event.__stubStopWatched = true;
    const original = event.stopPropagation.bind(event);
    event.stopPropagation = () => {
        original();
        event.__stubStopped = true;
    };
}

/** True when the event asked the propagation to stop. */
function propagationStopped(event) {
    return Boolean(event.cancelBubble) || Boolean(event.__stubStopped);
}

/**
 * Shared event-target behaviour: listener registry with `once` / `capture`
 * support, plus the capture → bubble dispatch order. Without these the
 * stub happily accepts code that behaves differently in a real browser
 * (FRONTEND_Review.md R-05).
 */
class StubEventTarget {
    constructor() {
        /** @type {Map<string, Set<{handler: Function, once: boolean, capture: boolean}>>} */
        this._listeners = new Map();
    }

    addEventListener(type, handler, options) {
        const once = typeof options === "boolean" ? options : Boolean(options?.once);
        const capture = typeof options === "boolean" ? options : Boolean(options?.capture);
        let set = this._listeners.get(type);
        if (!set) {
            set = new Set();
            this._listeners.set(type, set);
        }
        set.add({ handler, once, capture });
    }

    removeEventListener(type, handler, options) {
        // Per DOM spec the match is (type, callback, capture) — removing
        // the *other* phase's registration is a spec deviation
        // (FRONTEND_Review.md R-19).
        const capture = typeof options === "boolean" ? options : Boolean(options?.capture);
        const set = this._listeners.get(type);
        if (!set) return;
        for (const entry of set) {
            if (entry.handler === handler && entry.capture === capture) {
                set.delete(entry);
                return;
            }
        }
    }

    /**
     * Fire the listeners registered on this node for one dispatch phase.
     * `event.eventPhase` is set the way the DOM does it — 1 = capture,
     * 2 = at target, 3 = bubble — so a listener can tell which phase it
     * was invoked in (FRONTEND_Review.md R-19).
     */
    _fire(event, phase) {
        const set = this._listeners.get(event.type);
        if (!set) return;
        const isTarget = event.target === this;
        for (const entry of [...set]) {
            if (phase === "capture" && !entry.capture) continue;
            if (phase === "bubble" && entry.capture) continue;
            setEventProp(event, "currentTarget", this);
            setEventProp(event, "eventPhase", isTarget ? 2 : (phase === "capture" ? 1 : 3));
            entry.handler.call(this, event);
            if (entry.once) set.delete(entry);
        }
    }
}

/** A DOM element. */
export class StubElement extends StubEventTarget {
    constructor(tagName = "div") {
        super();
        this.tagName = String(tagName).toUpperCase();
        this.childNodes = [];
        this.parentNode = null;
        this.attributes = new Map();
        this.dataset = makeDataset(this);
        this.classList = new ClassList(this);
        this.style = {};
        this.scrollTop = 0;
        this.scrollHeight = 0;
        this._text = "";
        this._html = "";
    }

    /** Element children (text nodes excluded). */
    get children() {
        return this.childNodes.filter((n) => n instanceof StubElement);
    }

    // --- attributes ---------------------------------------------------------

    getAttribute(name) {
        return this.attributes.has(name) ? this.attributes.get(name) : null;
    }
    setAttribute(name, value) { this.attributes.set(name, String(value)); }
    hasAttribute(name) { return this.attributes.has(name); }
    removeAttribute(name) { this.attributes.delete(name); }

    get id() { return this.getAttribute("id") ?? ""; }
    set id(value) { this.setAttribute("id", value); }

    /**
     * The `value` **property** of a form control.
     *
     * ⚠️ **为什么要有它**：真实的 `<input>` 有两套「值」——
     *   - `el.value`（**属性**）：用户当前输入的内容；
     *   - `el.getAttribute("value")`（**属性值**）：HTML 里写下的**默认值**。
     * 一旦用户敲了字，两者在真实浏览器里就**不再相等**（`value` 属性还
     * 停在默认值上）。
     *
     * 前端代码必须读 `el.value`（用户输入的事实），**不是**
     * `getAttribute("value")` ⇒ 替身也得给这个属性，否则替身会把
     * 「用户输入」与「默认值」混为一谈（那种混法会**静默**通过一条其实
     * 已经坏了的断言）。
     *
     * 替身里两者**共用**一个存储（`value` 属性）——这与真实浏览器的
     * 差别只出现在「用户手动改过之后」；替身里改 `el.value` 也就等于
     * 改了 `value` 属性，**读**的路径是同一个 ⇒ 断言照样抓得住。
     */
    get value() { return this.getAttribute("value") ?? ""; }
    set value(v) { this.setAttribute("value", String(v)); }

    // --- tree ---------------------------------------------------------------

    appendChild(child) {
        if (child.parentNode) child.parentNode.removeChild(child);
        child.parentNode = this;
        this.childNodes.push(child);
        return child;
    }

    removeChild(child) {
        const i = this.childNodes.indexOf(child);
        if (i >= 0) this.childNodes.splice(i, 1);
        child.parentNode = null;
        return child;
    }

    // --- text ---------------------------------------------------------------

    get textContent() {
        if (this.childNodes.length === 0) return this._text;
        return this._text + this.childNodes
            .map((n) => (n instanceof StubElement ? n.textContent : n.data))
            .join("");
    }

    set textContent(value) {
        this.childNodes = [];
        this._text = String(value);
        // The real DOM reports the text as escaped markup in innerHTML.
        this._html = escapeMarkup(this._text);
    }

    get innerHTML() { return this._html; }

    set innerHTML(value) {
        const html = String(value);
        this._html = html;
        this._text = "";
        this.childNodes = [];
        for (const child of parseHtml(html)) this.appendChild(child);
    }

    // --- selectors ----------------------------------------------------------

    querySelector(selector) {
        return queryAll(this, selector)[0] ?? null;
    }

    querySelectorAll(selector) {
        return queryAll(this, selector);
    }

    matches(selector) {
        return selector.split(",")
            .map((s) => s.trim())
            .filter(Boolean)
            .some((s) => matchesSimple(this, s));
    }

    closest(selector) {
        let node = this;
        while (node) {
            if (node instanceof StubElement && node.matches(selector)) return node;
            node = node.parentNode;
        }
        return null;
    }

    // --- events -------------------------------------------------------------

    /**
     * Programmatically click the element.
     *
     * The real DOM dispatches a `click` event (bubbling + cancelable) — and
     * that event is what actually **starts a download** for
     * `<a download="…">`. Without this method the stub would let a
     * "the download action never clicked anything" defect pass silently
     * (the same class of false green as FRONTEND_Review.md #20).
     */
    click() {
        this.dispatchEvent(new Event("click", { bubbles: true, cancelable: true }));
    }

    /** Dispatch with capture + bubble phases — `event.target` is this node. */
    dispatchEvent(event) {
        setEventProp(event, "target", this);
        watchStopPropagation(event);
        try {
            if (typeof event.cancelBubble !== "boolean") event.cancelBubble = false;
        } catch (_e) {
            // read-only cancelBubble — ignore
        }
        // 捕获阶段：根 → 目标
        const path = [];
        for (let node = this; node; node = node.parentNode) path.unshift(node);
        for (const node of path) {
            node._fire?.(event, "capture");
            if (propagationStopped(event)) return true;
        }
        // 冒泡阶段：目标 → 根（与真实 DOM 同序，stopPropagation 才有意义）
        for (const node of [...path].reverse()) {
            node._fire?.(event, "bubble");
            if (propagationStopped(event)) break;
        }
        return true;
    }
}

/** The `window` global. */
export class StubWindow extends StubEventTarget {
    constructor() {
        super();
    }
    dispatchEvent(event) {
        setEventProp(event, "target", this);
        watchStopPropagation(event);
        // Window listeners are not part of the capture/bubble split here.
        this._fire(event, "bubble");
        return true;
    }
}

// ---------------------------------------------------------------------------
// Selector matching
// ---------------------------------------------------------------------------

/**
 * Attribute operators. Every operator the CSS spec defines for attribute
 * selectors is implemented — an unimplemented operator used to degrade
 * into "attribute exists", which made the stub return the **wrong**
 * element (e.g. `[data-view-tab^="deb"]` matched `product`). A silent
 * false match is worse than a loud failure (FRONTEND_Review.md R-03).
 */
const ATTR_OPERATORS = {
    "=": (actual, expected) => actual === expected,
    "^=": (actual, expected) => actual.startsWith(expected),
    "$=": (actual, expected) => actual.endsWith(expected),
    "*=": (actual, expected) => actual.includes(expected),
    "~=": (actual, expected) => actual.split(/\s+/).includes(expected),
    "|=": (actual, expected) => actual === expected || actual.startsWith(`${expected}-`),
};

/** Match one compound selector: `tag`, `#id`, `.class`, `[attr]`, `[attr="v"]`. */
function matchesSimple(el, selector) {
    let rest = selector.trim();
    if (!rest) return false;

    const tagMatch = /^([a-zA-Z][\w-]*)/.exec(rest);
    if (tagMatch) {
        if (el.tagName.toLowerCase() !== tagMatch[1].toLowerCase()) return false;
        rest = rest.slice(tagMatch[0].length);
    }

    const partRe = /([.#])([\w-]+)|\[([^\]]+)\]/g;
    let m;
    let matched = Boolean(tagMatch);
    while ((m = partRe.exec(rest)) !== null) {
        matched = true;
        if (m[1] === ".") {
            if (!el.classList.contains(m[2])) return false;
        } else if (m[1] === "#") {
            if (el.getAttribute("id") !== m[2]) return false;
        } else {
            const expr = m[3].trim();
            const am = /^([\w:-]+)(?:\s*([~^$*|]?=)\s*(?:"([^"]*)"|'([^']*)'|([^\]]*)))?$/.exec(expr);
            if (!am) {
                throw new Error(
                    `dom-stub: 不支持的选择器表达式 "[${expr}]"——`
                    + "未实现的语法必须显式失败，而不是静默返回 false",
                );
            }
            if (!el.hasAttribute(am[1])) return false;
            if (am[2]) {
                const op = ATTR_OPERATORS[am[2]];
                if (!op) {
                    throw new Error(
                        `dom-stub: 不支持的选择器算符 "${am[2]}"（[${am[1]}]）——`
                        + "未实现的算符必须显式失败，而不是退化成「属性存在即匹配」",
                    );
                }
                const expected = am[3] ?? am[4] ?? am[5] ?? "";
                if (!op(el.getAttribute(am[1]) ?? "", expected)) return false;
            }
        }
    }
    return matched;
}

/** All descendants of `root` matching `selector`. */
function queryAll(root, selector) {
    const out = [];
    const visit = (node) => {
        for (const child of node.children) {
            if (child.matches(selector)) out.push(child);
            visit(child);
        }
    };
    visit(root);
    return out;
}

// ---------------------------------------------------------------------------
// HTML parsing (innerHTML only)
// ---------------------------------------------------------------------------

function parseAttrs(src) {
    const out = [];
    const re = /([^\s"'<>/=]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'<>`]+)))?/g;
    let m;
    while ((m = re.exec(src)) !== null) {
        out.push([m[1], decodeEntities(m[2] ?? m[3] ?? m[4] ?? "")]);
    }
    return out;
}

/** Parse an HTML fragment into a list of top-level elements. */
export function parseHtml(html) {
    const root = new StubElement("template");
    const stack = [root];
    const tagRe = /<!--[\s\S]*?-->|<(\/)?([a-zA-Z][\w:-]*)((?:\s+[^\s"'<>/=]+(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s"'<>`]+))?)*)\s*(\/)?>|([^<]+)/g;

    let m;
    while ((m = tagRe.exec(html)) !== null) {
        if (m[0].startsWith("<!--")) continue;

        if (m[5] !== undefined) {
            stack[stack.length - 1].childNodes.push({ data: decodeEntities(m[5]) });
            continue;
        }

        const tag = m[2].toLowerCase();
        if (m[1]) {
            for (let i = stack.length - 1; i > 0; i--) {
                if (stack[i].tagName.toLowerCase() === tag) {
                    stack.length = i;
                    break;
                }
            }
            continue;
        }

        const el = new StubElement(tag);
        for (const [name, value] of parseAttrs(m[3] ?? "")) el.setAttribute(name, value);
        stack[stack.length - 1].appendChild(el);
        if (!m[4] && !VOID_TAGS.has(tag)) stack.push(el);
    }
    return root.children;
}

/** Extract the `<body>` of an HTML document, minus `<script>` blocks. */
export function extractBody(html) {
    const m = /<body[^>]*>([\s\S]*)<\/body>/i.exec(html);
    const body = m ? m[1] : html;
    return body.replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, "");
}

function findById(root, id) {
    for (const child of root.children) {
        if (child.getAttribute("id") === id) return child;
        const hit = findById(child, id);
        if (hit) return hit;
    }
    return null;
}

// ---------------------------------------------------------------------------
// Installation
// ---------------------------------------------------------------------------

/**
 * Install the DOM globals. Call this **before** importing any front-end
 * module — `app.js` reads `document.getElementById("status")` at module
 * evaluation time.
 *
 * @param {string} html — full HTML document (e.g. `front/index.html`)
 */
export function installDom(html) {
    const body = new StubElement("body");
    for (const child of parseHtml(extractBody(html))) body.appendChild(child);

    const doc = new StubEventTarget();
    doc.body = body;
    doc.documentElement = new StubElement("html");
    doc.getElementById = (id) => findById(body, id);
    doc.querySelector = (selector) => body.querySelector(selector);
    doc.querySelectorAll = (selector) => body.querySelectorAll(selector);
    doc.createElement = (tag) => new StubElement(tag);
    doc.dispatchEvent = (event) => {
        setEventProp(event, "target", doc);
        watchStopPropagation(event);
        // The document is the root of the propagation path: capture-phase
        // listeners must fire too (FRONTEND_Review.md R-16).
        doc._fire(event, "capture");
        if (!propagationStopped(event)) doc._fire(event, "bubble");
        return true;
    };

    const win = new StubWindow();
    const location = {
        hash: "",
        search: "",
        protocol: "http:",
        href: "http://localhost:8080/",
    };
    // The real `window.location` is the same object as the global one.
    win.location = location;
    const history = {
        replaceState(_state, _title, url) {
            if (typeof url !== "string") return;
            const i = url.indexOf("#");
            location.hash = i >= 0 ? url.slice(i) : "";
            if (i < 0) location.href = url;
        },
    };

    globalThis.document = doc;
    globalThis.window = win;
    globalThis.location = location;
    globalThis.history = history;

    // The real DOM fires `DOMContentLoaded` **after** the deferred module
    // scripts have run — so a module that registers a listener in its
    // top level does receive the event. Firing it from a microtask here
    // would beat the (asynchronous) module evaluation and the listener
    // would never fire — a false green (FRONTEND_Review.md R-15).
    // The event is therefore dispatched explicitly, by the caller, once
    // the modules are loaded.
    // Counter of every `DOMContentLoaded` dispatch. The real browser
    // dispatches it exactly once, after the deferred module scripts ran —
    // an extra/early dispatch is a stub deviation the gate must catch
    // (FRONTEND_Review.md R-15).
    let domContentLoadedDispatches = 0;
    const dispatchDOMContentLoaded = () => {
        domContentLoadedDispatches += 1;
        doc.dispatchEvent(new Event("DOMContentLoaded"));
    };
    const fireDOMContentLoaded = dispatchDOMContentLoaded;

    return {
        document: doc,
        window: win,
        location,
        history,
        fireDOMContentLoaded,
        get domContentLoadedDispatches() { return domContentLoadedDispatches; },
    };
}
