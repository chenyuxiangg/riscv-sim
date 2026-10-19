// Shared HTML text escaping (NFR-04 — cross-layer uniform escaping).
//
// Only needed where content is injected as an HTML *string*. When the
// content is assigned via `textContent` or `dataset`, do NOT escape it:
// `textContent` does not parse HTML, so escaping there makes the page
// show a literal "&lt;" (FRONTEND_Review.md #4).
//
// This is the single implementation — the front end used to carry two
// verbatim copies (FRONTEND_Review.md #9).

/**
 * Escape the five characters that carry meaning in HTML text and
 * attribute context.
 *
 * @param {unknown} s
 * @returns {string}
 */
export function escapeHtml(s) {
    return String(s)
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;")
        .replace(/'/g, "&#39;");
}
