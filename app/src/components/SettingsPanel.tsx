import { useEffect, useRef, useState } from "react";
import { api, LlmProvider, Settings } from "../api";
import { useT } from "../i18n";
import { Banner, Field, Spinner } from "./common";
import { useProjectActivities, useProjectActivity } from "../projectActivity";

export default function SettingsPanel({ root, onSaved }: { root: string; onSaved: () => Promise<void> }) {
  const t = useT();
  const [settings, setSettings] = useState<Settings>({
    project_dir: "",
    llm_base_url: "http://127.0.0.1:18180/v1",
    llm_api_key: "local",
    llm_model: "",
    disable_thinking: true,
    llm_provider: "api",
  });
  const originalSettings = useRef<Settings | undefined>(undefined);
  const [sourceDirectory, setSourceDirectory] = useState("");
  const [busy, setBusy] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [loading, setLoading] = useState(true);
  const [loaded, setLoaded] = useState(false);
  const [savedSource, setSavedSource] = useState("");
  const activities = useProjectActivities();
  useProjectActivity("settings", dirty, busy || loading);
  const [banner, setBanner] = useState<{ kind: "ok" | "err" | "info"; msg: string } | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true); setLoaded(false);
    Promise.all([api.getSettings(), api.getSourceSettings()]).then(([next, source]) => {
      if (cancelled) return;
      originalSettings.current = next;
      setSettings(next); setSourceDirectory(source.directory); setSavedSource(source.directory); setLoaded(true);
    }).catch((e) => { if (!cancelled) setBanner({ kind: "err", msg: String(e) }); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [root]);

  function update<K extends keyof Settings>(key: K, value: Settings[K]) {
    setDirty(true);
    setSettings((s) => ({ ...s, [key]: value }));
  }

  async function onSave() {
    if (!loaded || busy) return;
    if ([...activities.entries()].some(([key, state]) => key !== "settings" && state.busy)) {
      setBanner({ kind: "err", msg: t("pj.busy") }); return;
    }
    if (!sourceDirectory.trim()) { setBanner({ kind: "err", msg: t("pj.sourceRequired") }); return; }
    if (sourceDirectory.trim() !== savedSource && (activities.get("workbench")?.dirty || activities.get("chapter-title")?.dirty)
      && !window.confirm(t("pj.sourceConfirm"))) return;
    setBusy(true);
    try {
      await api.saveSettings(settings, originalSettings.current);
      const merged = await api.getSettings();
      setSettings(merged); originalSettings.current = merged;
      if (sourceDirectory.trim() !== savedSource) await api.saveSourceSettings({ directory: sourceDirectory.trim() }, savedSource);
      await onSaved();
      setSavedSource(sourceDirectory.trim());
      setDirty(false);
      setBanner({ kind: "ok", msg: t("st.saved") });
    } catch (e) {
      setBanner({ kind: "err", msg: String(e) });
    } finally {
      setBusy(false);
    }
  }

  const provider: LlmProvider = settings.llm_provider ?? "api";
  const isApi = provider === "api";

  const modelHint =
    provider === "claude_code"
      ? t("st.subModelHintClaude")
      : provider === "codex"
      ? t("st.subModelHintCodex")
      : t("st.modelHint");

  return (
    <div className="settings">
      <fieldset className="panel settings-fields" disabled={busy || loading || !loaded}>
        <h2>{t("st.title")}</h2>
        <p className="muted">{t("st.desc")}</p>

        <Field label={t("st.projectDir")} hint={t("st.projectDirHint")}>
          <input
            value={settings.project_dir}
            readOnly
          />
        </Field>

        <Field label={t("cx.sourceDir")} hint={t("cx.sourceHint")}>
          <input value={sourceDirectory} onChange={(e) => { setSourceDirectory(e.target.value); setDirty(true); }} placeholder="正文 / chapters" />
        </Field>

        <Field label={t("st.provider")} hint={t("st.providerHint")}>
          <select
            value={provider}
            onChange={(e) => update("llm_provider", e.target.value as LlmProvider)}
          >
            <option value="api">{t("st.providerApi")}</option>
            <option value="claude_code">{t("st.providerClaude")}</option>
            <option value="codex">{t("st.providerCodex")}</option>
          </select>
        </Field>

        {provider === "claude_code" ? (
          <Banner kind="info">{t("st.subNoteClaude")}</Banner>
        ) : null}
        {provider === "codex" ? (
          <Banner kind="info">{t("st.subNoteCodex")}</Banner>
        ) : null}

        {isApi ? (
          <>
            <Field label={t("st.baseUrl")} hint={t("st.baseUrlHint")}>
              <input
                value={settings.llm_base_url}
                placeholder="http://127.0.0.1:18180/v1"
                onChange={(e) => update("llm_base_url", e.target.value)}
              />
            </Field>

            <Field label={t("st.apiKey")} hint={t("st.apiKeyHint")}>
              <input
                type="password"
                value={settings.llm_api_key}
                onChange={(e) => update("llm_api_key", e.target.value)}
              />
            </Field>
          </>
        ) : null}

        <Field label={t("st.model")} hint={modelHint}>
          <input value={settings.llm_model} onChange={(e) => update("llm_model", e.target.value)} />
        </Field>

        {isApi ? (
          <label className="check-row">
            <input
              type="checkbox"
              checked={settings.disable_thinking !== false}
              onChange={(e) => update("disable_thinking", e.target.checked)}
            />
            <span className="check-text">
              <span className="check-title">{t("st.disableThinking")}</span>
              <span className="field-hint">{t("st.disableThinkingHint")}</span>
            </span>
          </label>
        ) : null}

        <div className="actions">
          <button className="primary" onClick={onSave} disabled={busy}>
            {busy ? <Spinner /> : null} {t("st.save")}
          </button>
        </div>

        {banner ? <Banner kind={banner.kind}>{banner.msg}</Banner> : null}

        <div className="root-info">
          <span className="muted">{t("st.root")}</span>
          <code>{root || t("st.locating")}</code>
        </div>
      </fieldset>
    </div>
  );
}
