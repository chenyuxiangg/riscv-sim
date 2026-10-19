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

/** A DOM element. */
export class StubElement {
    constructor(tagName = "div") {
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
        this._listeners = new Map();
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
        this._html = "";
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

    addEventListener(type, handler) {
        let set = this._listeners.get(type);
        if (!set) {
            set = new Set();
            this._listeners.set(type, set);
        }
        set.add(handler);
    }

    removeEventListener(type, handler) {
        this._listeners.get(type)?.delete(handler);
    }

    /** Dispatch with bubbling — `event.target` is the dispatching node. */
    dispatchEvent(event) {
        setEventProp(event, "target", this);
        let node = this;
        while (node) {
            setEventProp(event, "currentTarget", node);
            const handlers = node._listeners?.get(event.type);
            if (handlers) for (const handler of [...handlers]) handler.call(node, event);
            node = node.parentNode;
        }
        return true;
    }
}

/** The `window` global. */
export class StubWindow {
    constructor() {
        this._listeners = new Map();
    }
    addEventListener(type, handler) {
        let set = this._listeners.get(type);
        if (!set) {
            set = new Set();
            this._listeners.set(type, set);
        }
        set.add(handler);
    }
    removeEventListener(type, handler) {
        this._listeners.get(type)?.delete(handler);
    }
    dispatchEvent(event) {
        setEventProp(event, "target", this);
        setEventProp(event, "currentTarget", this);
        const handlers = this._listeners.get(event.type);
        if (handlers) for (const handler of [...handlers]) handler.call(this, event);
        return true;
    }
}

// ---------------------------------------------------------------------------
// Selector matching
// ---------------------------------------------------------------------------

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
            if (!am) return false;
            if (!el.hasAttribute(am[1])) return false;
            if (am[2]) {
                const value = am[3] ?? am[4] ?? am[5] ?? "";
                if (am[2] === "=" && el.getAttribute(am[1]) !== value) return false;
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

    const doc = {
        body,
        documentElement: new StubElement("html"),
        getElementById(id) { return findById(body, id); },
        querySelector(selector) { return body.querySelector(selector); },
        querySelectorAll(selector) { return body.querySelectorAll(selector); },
        createElement(tag) { return new StubElement(tag); },
    };

    const win = new StubWindow();
    const location = {
        hash: "",
        search: "",
        protocol: "http:",
        href: "http://localhost:8080/",
    };
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

    return { document: doc, window: win, location, history };
}
