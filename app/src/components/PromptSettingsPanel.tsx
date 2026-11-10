import { useEffect, useRef, useState } from "react";
import { api, PromptSettings, TaskPrompt, GenerationPreset } from "../api";
import { useT } from "../i18n";
import { Banner, Field, Spinner } from "./common";
import { useProjectActivity } from "../projectActivity";

type Category = "generation" | "consistency" | "memory";
const CATEGORIES: Category[] = ["generation", "consistency", "memory"];
function readTask(settings: PromptSettings, category: Category, preset: GenerationPreset): TaskPrompt {
  return category === "generation" ? preset === "compact" ? settings.compact_generation : { system: settings.system, user_template: settings.user_template } : settings[category];
}
function replaceTask(settings: PromptSettings, category: Category, preset: GenerationPreset, task: TaskPrompt): PromptSettings {
  return category === "generation" ? preset === "compact" ? { ...settings, compact_generation: task } : { ...settings, ...task } : { ...settings, [category]: task };
}
function readCategory(settings: PromptSettings, category: Category) {
  if (category !== "generation") return settings[category];
  const { system, user_template, generation_preset, compact_generation, general_material_guidance, compact_material_guidance } = settings;
  return { system, user_template, generation_preset, compact_generation, general_material_guidance, compact_material_guidance };
}
function replaceCategory(settings: PromptSettings, draft: PromptSettings, category: Category): PromptSettings {
  return category === "generation" ? { ...settings, ...readCategory(draft, category) } : { ...settings, [category]: draft[category] };
}

export default function PromptSettingsPanel({ onSaved }: { onSaved: () => void }) {
  const t = useT();
  const [settings, setSettings] = useState<PromptSettings | null>(null);
  const originalSettings = useRef<PromptSettings | null>(null);
  const [preset, setPreset] = useState<GenerationPreset>("general");
  const [category, setCategory] = useState<Category>("generation");
  const [dirty, setDirty] = useState<Set<Category>>(new Set());
  const [busy, setBusy] = useState(false);
  useProjectActivity("prompts", dirty.size > 0, busy);
  const [banner, setBanner] = useState<{ kind: "ok" | "err" | "info"; msg: string } | null>(null);
  useEffect(() => {
    api.getPromptSettings().then((next) => { setSettings(next); setPreset(next.generation_preset); originalSettings.current = next; }).catch((e) => setBanner({ kind: "err", msg: String(e) }));
  }, []);
  const task = settings ? readTask(settings, category, preset) : null;
  function update(task: TaskPrompt) {
    setSettings((s) => s && replaceTask(s, category, preset, task));
    setDirty((s) => new Set(s).add(category));
    setBanner(null);
  }
  async function save() {
    if (!task || !settings) return;
    setBusy(true);
    try {
      // 只保存当前分类，不把其他分类尚未保存的草稿写入项目。
      const latest = await api.getPromptSettings();
      const before = originalSettings.current;
      if (before && JSON.stringify(readCategory(before, category)) !== JSON.stringify(readCategory(latest, category))) {
        throw new Error(t("pr.concurrentChange"));
      }
      const next = replaceCategory(latest, settings, category);
      await api.savePromptSettings(next, latest);
      originalSettings.current = before ? replaceCategory(before, settings, category) : next;
      setDirty((s) => { const next = new Set(s); next.delete(category); return next; });
      if (category === "generation") onSaved();
      setBanner({ kind: "ok", msg: t("pr.saved") });
    } catch (e) { setBanner({ kind: "err", msg: String(e) }); }
    finally { setBusy(false); }
  }
  async function reset() {
    if (!window.confirm(t("pr.restoreConfirm"))) return;
    setBusy(true);
    try {
      const defaults = await api.defaultPromptSettings();
      update(readTask(defaults, category, preset));
      if (category === "generation") setSettings((s) => s && { ...s,
        [preset === "compact" ? "compact_material_guidance" : "general_material_guidance"]:
          preset === "compact" ? defaults.compact_material_guidance : defaults.general_material_guidance });
    }
    catch (e) { setBanner({ kind: "err", msg: String(e) }); }
    finally { setBusy(false); }
  }
  function updateGeneration(patch: Partial<PromptSettings>) {
    setSettings((s) => s && { ...s, ...patch });
    setDirty((s) => new Set(s).add("generation"));
    setBanner(null);
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
      {settings && category === "generation" ? <>
        <div className="row">
          <Field label={t("pr.editPreset")} hint={t("pr.presetHint")}>
            <select disabled={busy} value={preset} onChange={(e) => setPreset(e.target.value as GenerationPreset)}>
              <option value="general">{t("pr.general")}</option>
              <option value="compact">{t("pr.compact")}</option>
            </select>
          </Field>
          <Field label={t("pr.defaultPreset")}>
            <select disabled={busy} value={settings.generation_preset}
              onChange={(e) => updateGeneration({ generation_preset: e.target.value as GenerationPreset })}>
              <option value="general">{t("pr.general")}</option>
              <option value="compact">{t("pr.compact")}</option>
            </select>
          </Field>
        </div>
        <label><input type="checkbox" disabled={busy}
          checked={preset === "compact" ? settings.compact_material_guidance : settings.general_material_guidance}
          onChange={(e) => updateGeneration({ [preset === "compact" ? "compact_material_guidance" : "general_material_guidance"]: e.target.checked })} />
          {t("pr.materialGuidance")}</label>
        <p className="muted">{t(preset === "compact" ? "pr.compactHint" : "pr.generalHint")}</p>
      </> : null}
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
