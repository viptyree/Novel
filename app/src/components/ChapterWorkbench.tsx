import { ReactNode, useEffect, useRef, useState } from "react";
import { api, ChapterDetails, CommandResult, PromptPreview, ContextOptions, ContextCatalog, defaultContextOptions, GenerationPreset } from "../api";
import Icon from "./Icon";
import ChapterDirectory from "./ChapterDirectory";
import ChapterTitlePanel from "./ChapterTitlePanel";
import { useT } from "../i18n";
import ContextControls, { ContextReport, estimateTokens } from "./ContextControls";
import { Banner, Field, LogBox, Spinner } from "./common";
import { useProjectActivity } from "../projectActivity";

interface Props {
  focusMode: boolean;
  onFocusChange: (focus: boolean) => void;
  projectSwitcher: ReactNode;
  promptRevision: number;
  chapters: string[];
  chapterDetails: ChapterDetails[];
  projectRoot: string;
  selectChapter: (id: string) => void;
  currentChapter: string;
  setCurrentChapter: (id: string) => void;
  refreshChapters: () => Promise<void>;
}

interface ManualDraft { system: string; user: string; budget: number; }
const emptyManualDraft: ManualDraft = { system: "", user: "", budget: 100000 };

type Busy = "" | "generate" | "consistency" | "memory" | "reset" | "save" | "preview";

export default function ChapterWorkbench({
  focusMode, onFocusChange, projectSwitcher,
  chapters,
  chapterDetails,
  projectRoot,
  selectChapter,
  promptRevision,
  currentChapter,
  setCurrentChapter,
  refreshChapters,
}: Props) {
  const t = useT();
  const [directoryOpen, setDirectoryOpen] = useState(true);
  const [aiOpen, setAiOpen] = useState(() => !window.matchMedia("(max-width: 1099px)").matches);
  const [narrow, setNarrow] = useState(() => window.matchMedia("(max-width: 1099px)").matches);
  const aiToggleRef = useRef<HTMLButtonElement>(null);
  const assistantRef = useRef<HTMLElement>(null);
  const aiCloseRef = useRef<HTMLButtonElement>(null);
  const narrowViewport = useRef(window.matchMedia("(max-width: 1099px)"));
  useEffect(() => {
    const media = narrowViewport.current;
    const update = () => { setNarrow(media.matches); setAiOpen(!media.matches); };
    update(); media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);
  function closeAi() { setAiOpen(false); requestAnimationFrame(() => aiToggleRef.current?.focus()); }
  useEffect(() => {
    if (aiOpen && narrowViewport.current.matches) aiCloseRef.current?.focus();
  }, [aiOpen]);
  useEffect(() => {
    if (!aiOpen || !narrow || focusMode) return;
    const keepFocus = (event: KeyboardEvent) => {
      if (event.key !== "Tab" || document.querySelector("dialog[open]")) return;
      const nodes = Array.from(assistantRef.current?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), summary, [tabindex="0"]') ?? []).filter((node) => node.getClientRects().length > 0);
      const first = nodes[0], last = nodes[nodes.length - 1];
      if (!first) return;
      if (event.shiftKey && (document.activeElement === first || !assistantRef.current?.contains(document.activeElement))) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && (document.activeElement === last || !assistantRef.current?.contains(document.activeElement))) { event.preventDefault(); first.focus(); }
    };
    document.addEventListener("keydown", keepFocus);
    return () => document.removeEventListener("keydown", keepFocus);
  }, [aiOpen, narrow, focusMode]);
  const [contextOptions, setContextOptions] = useState<ContextOptions>({ ...defaultContextOptions });
  const [projectPreset, setProjectPreset] = useState<GenerationPreset>("general");
  const [catalog, setCatalog] = useState<ContextCatalog | null>(null);
  const [newId, setNewId] = useState("");
  const [idea, setIdea] = useState("");
  const [targetWords, setTargetWords] = useState(4000);
  const [useContext, setUseContext] = useState(true);
  const [overwrite, setOverwrite] = useState(true);
  const [includeMemory, setIncludeMemory] = useState(false);

  // 叙事控制（可选）
  const [povMode, setPovMode] = useState("");
  const [povChar, setPovChar] = useState("");
  const [narrative, setNarrative] = useState("");

  const [confirmedPrompt, setConfirmedPrompt] = useState<PromptPreview | undefined>();
  const [confirmedInput, setConfirmedInput] = useState("");
  const [draftPrompt, setDraftPrompt] = useState<PromptPreview | null>(null);
  const [freshPrompt, setFreshPrompt] = useState<PromptPreview | null>(null);
  const dialogRef = useRef<HTMLDialogElement>(null);
  const inputKey = JSON.stringify([promptRevision, currentChapter || newId, idea.trim(), targetWords, useContext, povMode, povChar, narrative.trim(), contextOptions]);
  const promptStale = !!confirmedPrompt && confirmedInput !== inputKey;

  useEffect(() => {
    let active = true;
    api.getPromptSettings().then((settings) => { if (active) setProjectPreset(settings.generation_preset); })
      .catch((e) => { if (active) setBanner({ kind: "err", msg: String(e) }); });
    return () => { active = false; };
  }, [promptRevision]);

  useEffect(() => {
    setConfirmedPrompt(undefined); setConfirmedInput("");
    setContextOptions((o) => ({ ...o, characters: [], chapters: [], events: [], generation_preset: null }));
  }, [(currentChapter || newId).trim()]);
  useEffect(() => {
    if (draftPrompt && !dialogRef.current?.open) dialogRef.current?.showModal();
  }, [draftPrompt]);

  async function onPreview() {
    const id = (currentChapter || newId).trim();
    if (!id || !idea.trim()) {
      setBanner({ kind: "err", msg: t(!id ? "wb.needChapter" : "wb.needIdea") });
      return;
    }
    setBusy("preview");
    try {
      const fresh = await api.previewPrompt(id, idea.trim(), targetWords, useContext, buildPov(), narrative.trim(), contextOptions);
      setFreshPrompt(fresh);
      setDraftPrompt(confirmedPrompt && confirmedInput === inputKey && confirmedPrompt.source === fresh.source
        ? { ...confirmedPrompt, stats: fresh.stats } : fresh);
    } catch (e) {
      setBanner({ kind: "err", msg: String(e) });
    } finally { setBusy(""); }
  }

  async function onGenerateManual() {
    const id = (currentChapter || newId).trim();
    if (!id) {
      setBanner({ kind: "err", msg: t("wb.needChapter") });
      return;
    }
    if (!manualUser.trim()) {
      setBanner({ kind: "err", msg: t("wb.needManual") });
      return;
    }
    setBusy("generate");
    setBanner({ kind: "info", msg: t("wb.manualGenRunning") });
    try {
      const res = await api.generateChapterManual(id, { system: manualSystem, user: manualUser }, overwrite, manualDraft.budget);
      handleResult(res, t("wb.genOk", { id }));
      if (res.success) {
        await refreshChapters();
        setCurrentChapter(id);
        await loadContent(id);
      }
    } catch (e) {
      setBanner({ kind: "err", msg: String(e) });
    } finally {
      setBusy("");
    }
  }

  function clearManual() {
    if (!window.confirm(t("wb.manualClearConfirm"))) return;
    setManualDraft(emptyManualDraft);
  }

  function closePreview() {
    dialogRef.current?.close();
    setDraftPrompt(null);
    setFreshPrompt(null);
  }

  function confirmPreview() {
    if (!draftPrompt || !freshPrompt) return;
    setConfirmedPrompt({ ...draftPrompt, source: freshPrompt.source, stats: freshPrompt.stats });
    setConfirmedInput(inputKey);
    closePreview();
  }

  function buildPov(): string {
    if (povMode === "limited" || povMode === "first") {
      return povChar.trim() ? `${povMode}:${povChar.trim()}` : povMode;
    }
    return povMode; // "" | "objective" | "omniscient"
  }

  const [generationMode, setGenerationMode] = useState<"template" | "manual">("template");
  const [manualDrafts, setManualDrafts] = useState<Record<string, ManualDraft>>({});
  const manualKey = JSON.stringify([projectRoot, (currentChapter || newId).trim()]);
  const manualDraft = manualDrafts[manualKey] ?? emptyManualDraft;
  const manualSystem = manualDraft.system;
  const manualUser = manualDraft.user;
  const manualTokens = estimateTokens(manualSystem) + estimateTokens(manualUser);
  const manualBudgetValid = Number.isInteger(manualDraft.budget) && manualDraft.budget >= 500 && manualDraft.budget <= 100000;
  const manualOverBudget = manualTokens > manualDraft.budget;
  function setManualDraft(patch: Partial<ManualDraft>) {
    setManualDrafts((drafts) => ({ ...drafts, [manualKey]: { ...(drafts[manualKey] ?? emptyManualDraft), ...patch } }));
  }

  const [content, setContent] = useState("");
  const savedContent = useRef("");
  const [dirty, setDirty] = useState(false);

  const [busy, setBusy] = useState<Busy>("");
  const [titleBusy, setTitleBusy] = useState(false);
  const [loadingChapter, setLoadingChapter] = useState(false);
  const [sourceRevision, setSourceRevision] = useState(0);
  const loadRevision = useRef(0);
  useProjectActivity("workbench", dirty || !!idea.trim() || !!newId.trim() || !!narrative.trim() || !!draftPrompt || Object.values(manualDrafts).some((d) => !!d.system || !!d.user), busy !== "" || loadingChapter);
  const [log, setLog] = useState("");
  const [banner, setBanner] = useState<{ kind: "ok" | "err" | "info"; msg: string } | null>(null);

  async function loadContent(id: string) {
    const revision = ++loadRevision.current;
    if (!id) {
      savedContent.current = "";
      setContent("");
      setDirty(false);
      setLoadingChapter(false);
      return;
    }
    setLoadingChapter(true);
    try {
      const text = await api.readChapter(id);
      if (revision !== loadRevision.current) return;
      setContent(text);
      savedContent.current = text;
      setDirty(false);
    } catch (e) {
      if (revision === loadRevision.current) { setContent(""); setBanner({ kind: "err", msg: String(e) }); }
    } finally { if (revision === loadRevision.current) setLoadingChapter(false); }
  }

  useEffect(() => {
    loadContent(currentChapter);
    return () => { loadRevision.current += 1; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentChapter]);

  function handleResult(res: CommandResult, okMsg: string) {
    setLog(res.log);
    setBanner(res.success ? { kind: "ok", msg: okMsg } : { kind: "err", msg: t("wb.execFail") });
  }

  function nextChapterId(): string {
    let max = 0;
    for (const c of chapters) {
      const m = c.match(/(\d+)/);
      if (m) max = Math.max(max, parseInt(m[1], 10));
    }
    return "ch" + String(max + 1).padStart(3, "0");
  }

  async function onGenerate() {
    const id = (currentChapter || newId).trim();
    if (!id) {
      setBanner({ kind: "err", msg: t("wb.needChapter") });
      return;
    }
    if (!idea.trim()) {
      setBanner({ kind: "err", msg: t("wb.needIdea") });
      return;
    }
    if (promptStale) {
      setBanner({ kind: "err", msg: t("pr.stale") });
      return;
    }
    setBusy("generate");
    setBanner({ kind: "info", msg: t("wb.generating") });
    try {
      const res = await api.generateChapter(
        id,
        idea.trim(),
        targetWords,
        useContext,
        overwrite,
        buildPov(),
        narrative.trim(),
        confirmedPrompt,
        contextOptions
      );
      handleResult(res, t("wb.genOk", { id }));
      if (res.success) {
        await refreshChapters();
        setCurrentChapter(id);
        await loadContent(id);
      }
    } catch (e) {
      setBanner({ kind: "err", msg: String(e) });
    } finally {
      setBusy("");
    }
  }

  async function onSave() {
    if (!currentChapter) return;
    setBusy("save");
    try {
      await api.saveChapter(currentChapter, content, savedContent.current);
      savedContent.current = content;
      setDirty(false);
      setBanner({ kind: "ok", msg: t("wb.saved") });
    } catch (e) {
      setBanner({ kind: "err", msg: String(e) });
    } finally {
      setBusy("");
    }
  }

  async function onCheck() {
    if (!currentChapter) return;
    setBusy("consistency");
    setBanner({ kind: "info", msg: t("wb.checking") });
    try {
      const res = await api.checkConsistency(currentChapter);
      handleResult(res, t("wb.checkOk"));
    } catch (e) {
      setBanner({ kind: "err", msg: String(e) });
    } finally {
      setBusy("");
    }
  }

  async function onUpdateMemory() {
    if (!currentChapter) return;
    setBusy("memory");
    setBanner({ kind: "info", msg: t("wb.memRunning") });
    try {
      const res = await api.updateMemory(currentChapter);
      handleResult(res, t("wb.memOk"));
    } catch (e) {
      setBanner({ kind: "err", msg: String(e) });
    } finally {
      setBusy("");
    }
  }

  async function onReset() {
    if (!currentChapter) return;
    const warn = includeMemory
      ? t("wb.resetConfirmMem", { id: currentChapter })
      : t("wb.resetConfirmNo", { id: currentChapter });
    if (!window.confirm(`${warn}\n${catalog?.chapter_file ?? ""}`)) return;
    setBusy("reset");
    try {
      const res = await api.resetChapter(currentChapter, includeMemory);
      handleResult(res, t("wb.resetOk", { id: currentChapter }));
      if (res.success) {
        await refreshChapters();
        setContent("");
        savedContent.current = "";
        setDirty(false);
      }
    } catch (e) {
      setBanner({ kind: "err", msg: String(e) });
    } finally {
      setBusy("");
    }
  }

  const running = busy !== "" || titleBusy || loadingChapter;
  const currentDetails = chapterDetails.find((chapter) => chapter.id === currentChapter);

  return (
    <div className={`workbench${directoryOpen ? "" : " directory-collapsed"}${aiOpen ? "" : " ai-collapsed"}`}>
      <div className="workspace-toolbar" inert={narrow && aiOpen && !focusMode}>
        <button className="icon-button" title={t("ui.toggleDirectory")} aria-label={t("ui.toggleDirectory")} aria-expanded={directoryOpen} onClick={() => setDirectoryOpen(!directoryOpen)}><Icon name="panel" /></button>
        <span className="workspace-caption">{t("nav.workbench")}</span>
        <span className="spacer" />
        <button className="ghost" aria-pressed={focusMode} onClick={() => onFocusChange(!focusMode)}><Icon name="focus" />{t(focusMode ? "ui.exitFocus" : "ui.focus")}</button>
        <button ref={aiToggleRef} className={aiOpen ? "assistant-toggle selected" : "assistant-toggle"} aria-expanded={aiOpen} onClick={() => setAiOpen(!aiOpen)}><Icon name="spark" />{t("ui.ai")}</button>
      </div>
      <ChapterDirectory open={directoryOpen} inert={narrow && aiOpen && !focusMode} running={running}
        projectSwitcher={projectSwitcher} chapters={chapters} chapterDetails={chapterDetails}
        currentChapter={currentChapter} selectChapter={selectChapter} newId={newId} setNewId={setNewId}
        nextChapterId={nextChapterId} onStartNew={() => { selectChapter(""); setAiOpen(true); }} />
      {aiOpen && <button className="assistant-scrim" aria-label={t("ui.close")} onClick={closeAi} />}
      <aside ref={assistantRef} role={narrow ? "dialog" : undefined} aria-modal={narrow ? true : undefined} className="assistant-panel" hidden={!aiOpen} aria-label={t("ui.ai")} onKeyDown={(e) => { if (e.key === "Escape") { e.preventDefault(); closeAi(); } }}>
        <div className="assistant-heading"><div><span className="eyebrow">SODARIE</span><h2><Icon name="spark" />{t("ui.ai")}</h2></div><button ref={aiCloseRef} className="icon-button" aria-label={t("ui.close")} onClick={closeAi}><Icon name="close" /></button></div>
        <p className="assistant-intro">{t("ui.aiHint")}</p>
        <div className="generation-scroll">
        <div className="target-chapter">{currentDetails?.title || currentChapter || newId || t("ui.newChapter")}</div>
        <Field label={t("wb.generationMode")}>
          <select value={generationMode} disabled={running} onChange={(e) => setGenerationMode(e.target.value as "template" | "manual")}>
            <option value="template">{t("wb.modeTemplate")}</option>
            <option value="manual">{t("wb.modeManual")}</option>
          </select>
        </Field>
        <fieldset className="generation-fields" disabled={running}>
        {generationMode === "template" ? <>
        <Field label={t("wb.idea")}>
          <textarea
            className="idea"
            value={idea}
            placeholder={t("wb.ideaPlaceholder")}
            onChange={(e) => setIdea(e.target.value)}
          />
        </Field>

        <div className="row">
          <Field label={t("wb.targetWords")}>
            <input
              type="number"
              value={targetWords}
              min={500}
              step={500}
              onChange={(e) => setTargetWords(parseInt(e.target.value || "0", 10))}
            />
          </Field>
          <div className="checks">
            <label>
              <input type="checkbox" checked={useContext} onChange={(e) => setUseContext(e.target.checked)} />
              {t("wb.useContext")}
            </label>
          </div>
        </div>

        <details className="generation-options"><summary>{t("ui.advanced")}</summary>
        <Field label={t("pr.chapterPreset")} hint={t("pr.chapterPresetHint")}>
          <select disabled={running} value={contextOptions.generation_preset ?? ""}
            onChange={(e) => setContextOptions((o) => ({ ...o, generation_preset: (e.target.value || null) as GenerationPreset | null }))}>
            <option value="">{t("pr.followDefault")} · {t(`pr.${projectPreset}`)}</option>
            <option value="general">{t("pr.general")}</option>
            <option value="compact">{t("pr.compact")}</option>
            <option value="benchmark">{t("pr.benchmark")}</option>
          </select>
        </Field>

        <ContextControls chapterId={(currentChapter || newId).trim()} options={contextOptions}
          onChange={setContextOptions} revision={promptRevision + sourceRevision} catalog={catalog} setCatalog={setCatalog} disabled={running} />

        <details className="narrative">
          <summary>{t("wb.narrativeSection")}</summary>
          <div className="row">
            <Field label={t("wb.pov")}>
              <select value={povMode} onChange={(e) => setPovMode(e.target.value)}>
                <option value="">{t("wb.povFollow")}</option>
                <option value="limited">{t("wb.povLimited")}</option>
                <option value="objective">{t("wb.povObjective")}</option>
                <option value="first">{t("wb.povFirst")}</option>
                <option value="omniscient">{t("wb.povOmniscient")}</option>
              </select>
            </Field>
            <Field label={t("wb.povCharacter")} hint={t("wb.povCharacterHint")}>
              <input
                value={povChar}
                disabled={povMode !== "limited" && povMode !== "first"}
                onChange={(e) => setPovChar(e.target.value)}
              />
            </Field>
          </div>
          <Field label={t("wb.narrative")} hint={t("wb.narrativeHint")}>
            <input
              value={narrative}
              placeholder={t("wb.narrativePlaceholder")}
              onChange={(e) => setNarrative(e.target.value)}
            />
          </Field>
        </details>
        </details>

        </> : <div className="manual-prompt-editor">
          <h3>{t("wb.modeManual")}</h3>
          <p className="muted">{t("wb.manualHint")}</p>
          <Field label={t("wb.manualSystem")}>
            <textarea
              className="prompt-system"
              value={manualSystem}
              placeholder={t("wb.manualSystem")}
              onChange={(e) => setManualDraft({ system: e.target.value })}
            />
          </Field>
          <Field label={t("wb.manualUser")}>
            <textarea
              className="prompt-user"
              value={manualUser}
              placeholder={t("wb.manualUser")}
              onChange={(e) => setManualDraft({ user: e.target.value })}
            />
          </Field>
          <Field label={t("cx.budget")} hint={t("wb.manualBudgetHint")}>
            <input type="number" min={500} max={100000} step={500} value={manualDraft.budget || ""}
              onChange={(e) => setManualDraft({ budget: Number(e.target.value) })} />
          </Field>
          <p className="muted">{t("cx.length")}: {Array.from(manualSystem + manualUser).length} {t("wb.words")} · ≈ {manualTokens} tokens / {manualDraft.budget}</p>
          <p className="field-hint">{t("wb.manualDraftHint")}</p>
          {!manualBudgetValid ? <Banner kind="err">{t("wb.manualBudgetHint")}</Banner> : null}
          {manualOverBudget ? <Banner kind="err">{t("cx.overBudget")}</Banner> : null}
        </div>}
        <label className="check-row">
          <input type="checkbox" checked={overwrite} onChange={(e) => setOverwrite(e.target.checked)} />
          {t("wb.overwrite")}
        </label>
        </fieldset>
        </div>
        <div className="generation-footer">
        {generationMode === "template" ? <>
          <div className="prompt-status muted">{t(confirmedPrompt ? "pr.manual" : "pr.auto")}</div>
          {promptStale ? <Banner kind="info">{t("pr.stale")}</Banner> : null}
          <div className="actions wrap">
            <button onClick={onPreview} disabled={running}>{busy === "preview" ? <Spinner /> : null}{t("pr.preview")}</button>
            {confirmedPrompt ? <button disabled={running} onClick={() => setConfirmedPrompt(undefined)}>{t("pr.resetAuto")}</button> : null}
            <button className="primary" onClick={onGenerate} disabled={running}>
              {busy === "generate" ? <Spinner /> : null} {t("wb.generate")}
            </button>
          </div>
        </> : <div className="actions wrap">
          <button className="primary" onClick={onGenerateManual} disabled={running || !manualUser.trim() || !manualBudgetValid || manualOverBudget}>
            {busy === "generate" ? <Spinner /> : null} {t("wb.manualGenerate")}
          </button>
          <button className="ghost" onClick={clearManual} disabled={running || (!manualSystem && !manualUser)}>{t("wb.manualClear")}</button>
        </div>}
        {banner ? <Banner kind={banner.kind}>{banner.msg}</Banner> : null}
        <LogBox log={log} />
        </div>
      </aside>

      <section inert={narrow && aiOpen && !focusMode} className="manuscript" aria-label={t("wb.bodyTitle")}>
        <details className="title-editor" hidden={!currentDetails}><summary>{t("ui.titleEdit")}</summary>
        {currentDetails && <ChapterTitlePanel key={currentChapter} chapter={currentDetails} projectRoot={projectRoot}
          content={loadingChapter ? "" : content} disabled={busy !== "" || loadingChapter} onBusyChange={setTitleBusy}
          onSaved={async () => {
            await refreshChapters();
            setConfirmedPrompt(undefined); setConfirmedInput("");
            setSourceRevision((n) => n + 1);
            setCatalog(await api.contextCatalog(currentChapter));
          }} />}
        </details>
        <div className="panel-head manuscript-heading">
          <h2>
            <span className="eyebrow">{currentChapter || "MANUSCRIPT"}</span>
            <span className="manuscript-title">{currentDetails?.title || t("wb.bodyTitle")}</span>
          </h2>
          <div className="panel-actions">
            <span className={`save-state${dirty ? " is-dirty" : ""}`} role="status">{currentChapter ? t(dirty ? "ui.unsaved" : "ui.saved") : ""}</span>
            <button className="primary" onClick={onSave} disabled={!currentChapter || running || !dirty}>
              {busy === "save" ? <Spinner /> : null} {t("wb.save")}
              {dirty ? " *" : ""}
            </button>
          </div>
        </div>
        {!currentChapter && <div className="manuscript-empty"><Icon name="workbench" /><h2>{t("ui.welcome")}</h2><p>{t("ui.welcomeHint")}</p></div>}
        <textarea
          aria-label={t("wb.bodyTitle")}
          hidden={!currentChapter}
          className="chapter-body"
          value={content}
          placeholder={currentChapter ? t("wb.bodyHas") : t("wb.bodyNone")}
          onChange={(e) => {
            setContent(e.target.value);
            setDirty(true);
          }}
          disabled={!currentChapter || loadingChapter || busy === "generate" || busy === "save" || busy === "reset"}
        />
        {(!aiOpen || focusMode) && banner && <Banner kind={banner.kind}>{banner.msg}</Banner>}
        {loadingChapter && <span role="status" className="muted">{t("ui.loading")}</span>}
        <div className="wordcount">
          {Array.from(content).length} {t("wb.words")}
        </div>

        <details className="chapter-tools"><summary>{t("ui.tools")}</summary>
        <div className="actions wrap">
          <button onClick={onCheck} disabled={!currentChapter || running}>
            {busy === "consistency" ? <Spinner /> : null} {t("wb.check")}
          </button>
          <button onClick={onUpdateMemory} disabled={!currentChapter || running}>
            {busy === "memory" ? <Spinner /> : null} {t("wb.updateMemory")}
          </button>
        </div>
        <details className="danger-zone"><summary>{t("ui.danger")}</summary><div className="actions wrap">
          <label className="danger-check">
            <input type="checkbox" checked={includeMemory} onChange={(e) => setIncludeMemory(e.target.checked)} />
            {t("wb.includeMemory")}
          </label>
          <button className="danger" onClick={onReset} disabled={!currentChapter || running}>
            {busy === "reset" ? <Spinner /> : null} {t("wb.reset")}
          </button>
        </div>
        </details></details>
      </section>

      <dialog ref={dialogRef} className="prompt-dialog" aria-labelledby="prompt-dialog-title"
        onCancel={() => { setDraftPrompt(null); setFreshPrompt(null); }}>
        {draftPrompt ? <>
          <h2 id="prompt-dialog-title">{t("pr.preview")}</h2>
          <p className="muted">{t("pr.previewHint")}</p>
          {freshPrompt && (draftPrompt.source !== freshPrompt.source || promptStale) ?
            <Banner kind="info">{t("pr.changed")}</Banner> : null}
          <ContextReport preview={draftPrompt} />
          <Field label={t("pr.system")}><textarea className="prompt-system" value={draftPrompt.prompts.system}
            onChange={(e) => setDraftPrompt({ ...draftPrompt, prompts: { ...draftPrompt.prompts, system: e.target.value } })} /></Field>
          <Field label={t("pr.user")}><textarea className="prompt-user" value={draftPrompt.prompts.user}
            onChange={(e) => setDraftPrompt({ ...draftPrompt, prompts: { ...draftPrompt.prompts, user: e.target.value } })} /></Field>
          <div className="actions wrap">
            <button onClick={() => freshPrompt && setDraftPrompt(freshPrompt)}>{t("pr.rebuild")}</button>
            <span className="spacer" />
            <button onClick={closePreview}>{t("pr.cancel")}</button>
            <button className="primary" disabled={!draftPrompt.prompts.system.trim() || !draftPrompt.prompts.user.trim() || estimateTokens(draftPrompt.prompts.system) + estimateTokens(draftPrompt.prompts.user) > draftPrompt.stats.budget} onClick={confirmPreview}>{t("pr.confirm")}</button>
          </div>
        </> : null}
      </dialog>
    </div>
  );
}
