import { useEffect, useRef, useState } from "react";
import { api, OpenProjectOptions, ProjectEntry } from "../api";
import { useT } from "../i18n";
import { Field, Spinner } from "./common";

interface Props {
  root: string;
  onOpen: (options: OpenProjectOptions) => Promise<boolean>;
}

export default function ProjectSwitcher({ root, onOpen }: Props) {
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  const [projects, setProjects] = useState<ProjectEntry[]>([]);
  const [mode, setMode] = useState<"recent" | "create" | "open" | "relocate">("recent");
  const [path, setPath] = useState("");
  const [name, setName] = useState("");
  const [previousPath, setPreviousPath] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  async function refresh() { setProjects(await api.listProjects()); }
  useEffect(() => { refresh().catch((e) => setError(String(e))); }, [root]);

  async function show() {
    setMode("recent"); setError("");
    dialog.current?.showModal();
    try { await refresh(); } catch (e) { setError(String(e)); }
  }

  function form(next: "create" | "open" | "relocate", project?: ProjectEntry) {
    setMode(next); setPath(""); setName(project?.name ?? "");
    setPreviousPath(project?.path); setError("");
  }

  async function open(options: OpenProjectOptions) {
    if (busy) return;
    setBusy(true); setError("");
    try {
      if (await onOpen(options)) dialog.current?.close();
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  }

  async function update(project: ProjectEntry, remove = false) {
    if (remove && !window.confirm(t("pj.removeConfirm"))) return;
    setBusy(true); setError("");
    try { await api.updateProjectEntry(project.path, remove ? null : !project.pinned, remove); await refresh(); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  }

  const current = projects.find((p) => p.current);
  return <>
    <button className="project-trigger" title={root || current?.path} onClick={show}>
      <span className="muted">{t("pj.current")}</span>
      <strong>{current?.name || t("pj.title")}</strong><span>▾</span>
    </button>
    <dialog ref={dialog} className="prompt-dialog project-dialog" aria-labelledby="project-dialog-title"
      onCancel={(e) => { if (busy) e.preventDefault(); }}>
      <div className="project-dialog-header">
        <h2 id="project-dialog-title">{t("pj.title")}</h2>
        <button disabled={busy} onClick={() => dialog.current?.close()}>{t("pj.close")}</button>
      </div>
      <p className="muted">{t("pj.hint")}</p>
      <div className="actions">
        <button disabled={busy} onClick={() => { setMode("recent"); setError(""); refresh().catch((e) => setError(String(e))); }}>{t("pj.recent")}</button>
        <button disabled={busy} onClick={() => form("create")}>{t("pj.create")}</button>
        <button disabled={busy} onClick={() => form("open")}>{t("pj.open")}</button>
      </div>
      {error && <div role="alert" className="banner banner-err">{error}</div>}
      {mode === "recent" ? <div className="project-list">
        {projects.length === 0 && <p className="muted">{t("pj.empty")}</p>}
        {projects.map((project) => <article key={project.path} className={`project-entry${project.current ? " current" : ""}`}>
          <div><strong>{project.pinned ? "★ " : ""}{project.name}</strong>
            {project.current && <span className="project-badge">{t("pj.current")}</span>}
            {!project.exists && <span className="project-missing">{t("pj.missing")}</span>}
          </div>
          <div className="source-path">{project.path}</div>
          <div className="muted">{t("pj.lastOpened")} {new Date(project.last_opened_at).toLocaleString()}</div>
          <div className="actions">
            <button className="primary" disabled={busy || !project.exists || project.current}
              onClick={() => open({ path: project.path, create: false })}>{t("pj.switch")}</button>
            <button disabled={busy} onClick={() => update(project)}>{t(project.pinned ? "pj.unpin" : "pj.pin")}</button>
            {!project.exists && <button disabled={busy} onClick={() => form("relocate", project)}>{t("pj.relocate")}</button>}
            <button disabled={busy || project.current} onClick={() => update(project, true)}>{t("pj.remove")}</button>
          </div>
        </article>)}
      </div> : <form onSubmit={(e) => {
        e.preventDefault();
        open({ path: path.trim(), create: mode === "create", name: mode === "create" ? name.trim() : undefined,
          previousPath: mode === "relocate" ? previousPath : undefined });
      }}>
        <h3>{t(mode === "create" ? "pj.create" : mode === "relocate" ? "pj.relocate" : "pj.open")}</h3>
        {mode === "relocate" && <p className="source-path">{previousPath}</p>}
        {mode === "create" && <Field label={t("pj.name")}><input disabled={busy} value={name} onChange={(e) => setName(e.target.value)} /></Field>}
        <Field label={t("pj.path")} hint={t(mode === "create" ? "pj.createHint" : "pj.openHint")}>
          <input required disabled={busy} value={path} onChange={(e) => setPath(e.target.value)} placeholder="D:\Novels\MyBook" />
        </Field>
        <div className="actions"><button className="primary" disabled={busy || !path.trim()} type="submit">
          {busy && <Spinner />}{t(mode === "create" ? "pj.create" : "pj.open")}
        </button></div>
      </form>}
    </dialog>
  </>;
}
