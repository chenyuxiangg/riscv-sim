// M-01 工作台装配与多视图路由 (Workbench assembly & multi-view routing).
// MVP: hash-based router lives in app.js; this module will own
// role-driven workbench assembly (product / embedded / admin) once
// the session role is wired up (M-04).

export async function mountWorkbench() {
    // No-op for MVP. Returns when the workbench DOM is ready.
    return Promise.resolve();
}
