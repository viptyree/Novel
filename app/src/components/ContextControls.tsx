import { Dispatch, SetStateAction, useEffect, useState } from "react";
import { api, ContextCatalog, ContextOptions, PromptPreview } from "../api";
import { useT } from "../i18n";
import { Banner, Field } from "./common";

export function estimateTokens(text: string): number {
  let cjk = 0, ascii = 0, other = 0;
  for (const c of text) {
    const n = c.codePointAt(0)!;
    if (n >= 0x2e80 && n <= 0x9fff) cjk++;
    else if (n <= 127) ascii++;
    else other++;
  }
  return cjk + Math.ceil(ascii / 4) + Math.ceil(other / 2);
}
const sourceVariables: Record<string, string> = { character: "角色档案", summary: "章节摘要", event: "历史事件", timeline: "时间线", rules: "故事圣经", outline: "全书大纲", foreshadowing: "伏笔档案", style: "文风参考" };

export function ContextReport({ preview }: { preview: PromptPreview }) {
  const t = useT();
  const total = estimateTokens(preview.prompts.system) + estimateTokens(preview.prompts.user);
  const count = [...preview.prompts.system, ...preview.prompts.user].length;
  return <div className="context-report">
    <p>{t("cx.length")}: {count} {t("wb.words")} · ≈ {total} tokens / {preview.stats.budget}</p>
    <p className="field-hint">{t("cx.estimateHint")}</p>
    {total > preview.stats.budget ? <Banner kind="err">{t("cx.overBudget")}</Banner> : null}
    {preview.stats.warnings.filter((warning) => !warning.startsWith("输入超过预算") || total > preview.stats.budget).map((warning, i) => <div className="memory-note" key={i}>{warning}</div>)}
    <details><summary>{t("cx.sources")}</summary>
      <div className="context-source-list">{preview.stats.sources.map((source, i) => <div className="context-source" key={i}>
        <strong>{t(source.included ? "cx.included" : "cx.excluded")} · {source.kind === "chapter" ? `{{参考正文:${source.id}}}` : `{{${sourceVariables[source.kind] ?? source.kind}}}`} · {source.id}</strong>
        <div>{source.reason}</div><small className="muted">{source.file} · {source.characters} {t("wb.words")}</small>
      </div>)}</div>
    </details>
  </div>;
}

export default function ContextControls({ chapterId, options, onChange, revision, catalog, setCatalog, disabled }: {
  chapterId: string; options: ContextOptions; onChange: (o: ContextOptions) => void; revision: number;
  catalog: ContextCatalog | null; setCatalog: Dispatch<SetStateAction<ContextCatalog | null>>; disabled: boolean;
}) {
  const t = useT();
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [reload, setReload] = useState(0);
  const target = /^ch\d+$/.test(chapterId) ? Number(chapterId.slice(2)) : 0;
  useEffect(() => {
    let alive = true;
    if (!target) { setCatalog(null); return; }
    setLoading(true);
    api.contextCatalog(chapterId).then((c) => { if (alive) { setCatalog(c); setError(""); } })
      .catch((e) => { if (alive) { setError(String(e)); setCatalog(null); } })
      .finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, [chapterId, revision, reload, target, setCatalog]);
  function toggle(key: "characters" | "chapters" | "events", id: string) {
    onChange({ ...options, [key]: options[key].includes(id) ? options[key].filter((v) => v !== id) : [...options[key], id] });
  }
  return <details className="context-controls">
    <summary>{t("cx.title")}</summary>
    <p className="muted">{t("cx.hint")}</p>
    {catalog ? <p className="source-path">{t("cx.source")}: {catalog.chapter_file}</p> : null}
    <button disabled={disabled || loading || !target} onClick={() => setReload((n) => n + 1)}>{t("cx.refresh")}</button>
    {error ? <Banner kind="err">{error}</Banner> : null}
    <label className="check-row"><input type="checkbox" disabled={disabled} checked={options.automatic}
      onChange={(e) => onChange({ ...options, automatic: e.target.checked })} />{t("cx.automatic")}</label>
    <div className="row">
      <Field label={t("cx.recent")}><input type="number" min={0} max={10} disabled={disabled} value={options.recent_chapters}
        onChange={(e) => onChange({ ...options, recent_chapters: Number(e.target.value) })} /></Field>
      <Field label={t("cx.eventsLimit")}><input type="number" min={0} max={30} disabled={disabled} value={options.max_events}
        onChange={(e) => onChange({ ...options, max_events: Number(e.target.value) })} /></Field>
      <Field label={t("cx.budget")}><input type="number" min={500} max={100000} step={500} disabled={disabled} value={options.input_budget}
        onChange={(e) => onChange({ ...options, input_budget: Number(e.target.value) })} /></Field>
    </div>
    {catalog ? <div className="context-columns">
      {(["characters", "chapters", "events"] as const).map((key) => <details key={key}>
        <summary>{t(`cx.${key}`)} ({options[key].length})</summary>
        <div className="context-choices">{catalog[key].map((item) => {
          const future = item.chapter !== null && item.chapter >= target;
          return <label className="context-choice" key={item.id} title={item.label}>
            <input type="checkbox" disabled={disabled || future} checked={options[key].includes(item.id)} onChange={() => toggle(key, item.id)} />
            <span><strong>{item.id}</strong> {item.label}{future ? ` · ${t("cx.future")}` : ""}</span>
          </label>;
        })}</div>
      </details>)}
    </div> : null}
    <label className="check-row"><input type="checkbox" disabled={disabled} checked={options.include_outline}
      onChange={(e) => onChange({ ...options, include_outline: e.target.checked })} />{t("cx.outline")}</label>
    {options.include_outline ? <label className="check-row"><input type="checkbox" disabled={disabled} checked={options.outline_full}
      onChange={(e) => onChange({ ...options, outline_full: e.target.checked })} />{t("cx.outlineFull")}</label> : null}
    {options.include_outline && !options.outline_full ? <Field label={t("cx.outlineExcerpt")} hint={t("cx.outlineHint")}>
      <textarea disabled={disabled} value={options.outline_excerpt} onChange={(e) => onChange({ ...options, outline_excerpt: e.target.value })} />
    </Field> : null}
    {catalog?.notes ? <details><summary>{t("cx.notes")}</summary><pre className="author-notes">{catalog.notes}</pre></details> : null}
  </details>;
}
