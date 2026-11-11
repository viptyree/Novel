# Sodarie Novel desktop UI

React 19, TypeScript, Vite and Tauri 2. The frontend calls the existing Tauri commands through `src/api.ts`; the redesign adds no backend or file-format changes.

## Develop

From the repository root:

```powershell
npm --prefix app install
npm --prefix app run tauri dev
```

The Tauri development command builds the CLI sidecar and starts Vite. For browser layout work alone, run `npm --prefix app run dev`; real project operations require the desktop Tauri bridge.

```powershell
npm --prefix app run build
cargo build --locked -p app
```

`cargo build` checks the native application; a debug executable uses the running Vite server. Building an installer remains a separate Tauri bundle operation.

## UI structure

- The application shell provides a 56px navigation rail and appearance menu.
- The workbench contains a searchable 208px chapter directory, a flexible plain text manuscript editor and a 304px assistant. Below 1100 CSS pixels the assistant becomes a modal drawer with Escape, focus containment and focus restoration.
- Panels stay mounted when hidden. Focus mode and page navigation retain drafts; project/source changes continue to use the existing unsaved-work guards.
- `src/App.css` owns shared theme tokens and layouts. Light, dark and warm themes retain the existing `ui.theme` preference; Chinese and English retain `ui.lang`.

## Regression checks

With Vite running on port 1420:

```powershell
python scripts/check-desktop-ui.py
```

This optional check uses Python Playwright and its installed Chromium browser. It injects an in-memory Tauri bridge into a fresh browser context. No manuscripts, project registry, provider credentials or model services are accessed or changed. It covers panel/draft retention, chapter and project guards, generation and prompt preview, budgets, saving errors, titles, chapter tools, reports, settings, language, drawer keyboard navigation and smaller CSS viewports. It does not replace testing real model responses or changing Windows system DPI.

Screenshots under `../docs/screenshots/` are generated from fictional test content. `workbench.png` is the default light view; light/dark/warm variants and the memory and settings pages are captured by the same script. Native smoke captures, when made against actual projects, belong under ignored `target/ui-check/` instead.
