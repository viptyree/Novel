import { useEffect, useState } from "react";
import { api, PromptSettings, TaskPrompt } from "../api";
import { useT } from "../i18n";
import { Banner, Field, Spinner } from "./common";
import { useProjectActivity } from "../projectActivity";

type Category = "generation" | "consistency" | "memory";
const CATEGORIES: Category[] = ["generation", "consistency", "memory"];
function readTask(settings: PromptSettings, category: Category): TaskPrompt {
  return category === "generation" ? { system: settings.system, user_template: settings.user_template } : settings[category];
}
function replaceTask(settings: PromptSettings, category: Category, task: TaskPrompt): PromptSettings {
  return category === "generation" ? { ...settings, ...task } : { ...settings, [category]: task };
}

export default function PromptSettingsPanel({ onSaved }: { onSaved: () => void }) {
  const t = useT();
  const [settings, setSettings] = useState<PromptSettings | null>(null);
  const [category, setCategory] = useState<Category>("generation");
  const [dirty, setDirty] = useState<Set<Category>>(new Set());
  const [busy, setBusy] = useState(false);
  useProjectActivity("prompts", dirty.size > 0, busy);
  const [banner, setBanner] = useState<{ kind: "ok" | "err" | "info"; msg: string } | null>(null);
  useEffect(() => {
    api.getPromptSettings().then(setSettings).catch((e) => setBanner({ kind: "err", msg: String(e) }));
  }, []);
  const task = settings ? readTask(settings, category) : null;
  function update(task: TaskPrompt) {
    setSettings((s) => s && replaceTask(s, category, task));
    setDirty((s) => new Set(s).add(category));
    setBanner(null);
  }
  async function save() {
    if (!task) return;
    setBusy(true);
    try {
      // 只保存当前分类，不把其他分类尚未保存的草稿写入项目。
      const latest = await api.getPromptSettings();
      await api.savePromptSettings(replaceTask(latest, category, task));
      setDirty((s) => { const next = new Set(s); next.delete(category); return next; });
      if (category === "generation") onSaved();
      setBanner({ kind: "ok", msg: t("pr.saved") });
    } catch (e) { setBanner({ kind: "err", msg: String(e) }); }
    finally { setBusy(false); }
  }
  async function reset() {
    if (!window.confirm(t("pr.restoreConfirm"))) return;
    setBusy(true);
    try { update(readTask(await api.defaultPromptSettings(), category)); }
    catch (e) { setBanner({ kind: "err", msg: String(e) }); }
    finally { setBusy(false); }
  }
  const hint = category === "generation" ? "pr.variables" : category === "consistency" ? "pr.consistencyVariables" : "pr.memoryVariables";
  return <div className="memory">
    <aside className="memory-list">
      <div className="memory-group-title">{t("pr.categories")}</div>
      {CATEGORIES.map((key) => <button key={key} disabled={busy}
        className={category === key ? "memory-item active" : "memory-item"}
        onClick={() => { setCategory(key); setBanner(null); }}>
        {t(`pr.${key}`)}{dirty.has(key) ? " *" : ""}
      </button>)}
    </aside>
    <div className="memory-edit">
      <div className="panel-head">
        <h2>{t(`pr.${category}`)}{dirty.has(category) ? " *" : ""}</h2>
        <div className="panel-actions">
          <button disabled={busy} onClick={reset}>{t("pr.restore")}</button>
          <button className="primary" disabled={busy || !dirty.has(category)} onClick={save}>
            {busy ? <Spinner /> : null}{t("st.save")}
          </button>
        </div>
      </div>
      <p className="muted">{t("pr.scope")}</p>
      {task ? <>
        <Field label={t("pr.system")}><textarea className="prompt-system" disabled={busy} value={task.system}
          onChange={(e) => update({ ...task, system: e.target.value })} /></Field>
        <Field label={t("pr.template")} hint={t(hint)}><textarea className="prompt-user" spellCheck={false} disabled={busy} value={task.user_template}
          onChange={(e) => update({ ...task, user_template: e.target.value })} /></Field>
      </> : null}
      {banner ? <Banner kind={banner.kind}>{banner.msg}</Banner> : null}
    </div>
  </div>;
}
