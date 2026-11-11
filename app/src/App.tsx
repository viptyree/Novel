import { useCallback, useEffect, useRef, useState } from "react";
import { api, ChapterDetails, OpenProjectOptions } from "./api";
import ProjectSwitcher from "./components/ProjectSwitcher";
import Icon, { IconName } from "./components/Icon";
import { ProjectActivityProvider, useProjectActivities } from "./projectActivity";
import { useUi, Lang, Theme } from "./i18n";
import logo from "./assets/logo.png";
import ChapterWorkbench from "./components/ChapterWorkbench";
import Reports from "./components/Reports";
import MemoryEditor from "./components/MemoryEditor";
import SettingsPanel from "./components/SettingsPanel";
import PromptSettingsPanel from "./components/PromptSettingsPanel";
import Guide from "./components/Guide";
import "./App.css";

type Tab = "workbench" | "reports" | "memory" | "settings" | "prompts" | "guide";

const NAV: { key: Tab; labelKey: string; icon: IconName }[] = [
  { key: "workbench", labelKey: "nav.workbench", icon: "workbench" },
  { key: "reports", labelKey: "nav.reports", icon: "reports" },
  { key: "memory", labelKey: "nav.memory", icon: "memory" },
  { key: "prompts", labelKey: "nav.prompts", icon: "prompts" },
  { key: "settings", labelKey: "nav.settings", icon: "settings" },
  { key: "guide", labelKey: "nav.guide", icon: "guide" },
];

const GITHUB_URL = "https://github.com/DuckTraDo";

function GithubMark() {
  return (
    <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" fill="currentColor">
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8z" />
    </svg>
  );
}

const THEMES: { key: Theme; icon: string; labelKey: string }[] = [
  { key: "light", icon: "☀", labelKey: "theme.light" },
  { key: "dark", icon: "☾", labelKey: "theme.dark" },
  { key: "warm", icon: "◑", labelKey: "theme.warm" },
];

export default function App() {
  return <ProjectActivityProvider><AppContent /></ProjectActivityProvider>;
}

function AppContent() {
  const { t, lang, setLang, theme, setTheme } = useUi();
  const activities = useProjectActivities();
  const [focusMode, setFocusMode] = useState(false);
  const [tab, setTab] = useState<Tab>("workbench");
  const [chapterDetails, setChapterDetails] = useState<ChapterDetails[]>([]);
  const chapters = chapterDetails.map((chapter) => chapter.id);
  const [currentChapter, setCurrentChapter] = useState("");
  const [sourceKey, setSourceKey] = useState("");
  const [root, setRoot] = useState("");
  const [error, setError] = useState("");
  const [promptRevision, setPromptRevision] = useState(0);
  const [switching, setSwitching] = useState(false);
  const [projectRevision, setProjectRevision] = useState(0);
  const switchPending = useRef(false);
  const projectEpoch = useRef(0);

  async function openProject(options: OpenProjectOptions): Promise<boolean> {
    if (switchPending.current) return false;
    if ([...activities.values()].some((a) => a.busy)) throw new Error(t("pj.busy"));
    if ([...activities.values()].some((a) => a.dirty) && !window.confirm(t("pj.discardConfirm"))) return false;
    switchPending.current = true;
    setSwitching(true);
    try {
      const nextRoot = await api.openProject(options);
      projectEpoch.current += 1;
      // Clear old project state immediately, even if loading the target fails.
      setRoot(nextRoot); setCurrentChapter(""); setChapterDetails([]); setSourceKey(""); setError("");
      setProjectRevision((n) => n + 1);
      setPromptRevision((n) => n + 1);
      try {
        const [nextChapters, source] = await Promise.all([api.listChapterDetails(), api.getSourceSettings()]);
        setChapterDetails(nextChapters); setSourceKey(source.directory);
      } catch (e) { setError(String(e)); }
      return true;
    } finally { switchPending.current = false; setSwitching(false); }
  }

  const refreshChapters = useCallback(async () => {
    try {
      setChapterDetails(await api.listChapterDetails());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    const epoch = projectEpoch.current;
    Promise.allSettled([api.getPipelineRoot(), api.listChapterDetails(), api.getSourceSettings()]).then(([project, list, source]) => {
      if (cancelled || epoch !== projectEpoch.current) return;
      if (project.status === "fulfilled") setRoot(project.value);
      if (list.status === "fulfilled") setChapterDetails(list.value);
      if (source.status === "fulfilled") setSourceKey(source.value.directory);
      const failed = [project, list, source].find((r) => r.status === "rejected");
      if (failed?.status === "rejected") setError(String(failed.reason));
    });
    return () => { cancelled = true; };
  }, []);

  const currentLabel = NAV.find((n) => n.key === tab)?.labelKey ?? "";

  function selectChapter(id: string) {
    if (id === currentChapter) return;
    if ([...activities.values()].some((state) => state.busy)) { setError(t("pj.busy")); return; }
    if (["workbench", "chapter-title"].some((key) => activities.get(key)?.dirty)
      && !window.confirm(t("ct.switchConfirm"))) return;
    setError(""); setCurrentChapter(id);
  }

  return (
    <div className={`app${focusMode && tab === "workbench" ? " focus-mode" : ""}`}>
      <nav className="sidebar">
        <div className="brand" title="Sodarie Novel">
          <img className="brand-logo" src={logo} alt="Sodarie Novel" />
          <div>
            <div className="brand-title">Sodarie Novel</div>
            <div className="brand-sub">{t("brand.subtitle")}</div>
          </div>
        </div>

        <div className="nav-list">
          {NAV.map((n) => (
            <button
              key={n.key}
              className={tab === n.key ? "nav active" : "nav"}
              title={t(n.labelKey)} aria-label={t(n.labelKey)} aria-current={tab === n.key ? "page" : undefined}
              onClick={() => setTab(n.key)}
            >
              <Icon name={n.icon} /><span className="nav-label">{t(n.labelKey)}</span>
            </button>
          ))}
        </div>

        <div className="sidebar-foot">
          <div className="chapters-count">
            {t("sidebar.chapters")} · {chapters.length}
          </div>
          <button title="DuckTraDo · GitHub" aria-label="DuckTraDo · GitHub" className="madeby" onClick={() => api.openUrl(GITHUB_URL)}>
            <GithubMark />
            <span className="madeby-text">
              <span className="madeby-label">{t("madeBy")}</span>
              <span className="madeby-name">DuckTraDo</span>
            </span>
            <span className="madeby-arrow">↗</span>
          </button>
        </div>
      </nav>

      <main className="content">
        <header className="topbar">
          <div className="topbar-project"><h1 className="topbar-title">{tab === "workbench" ? "Sodarie Novel" : t(currentLabel)}</h1>
            {tab !== "workbench" && <ProjectSwitcher root={root} onOpen={openProject} />}
          </div>
          <div className="topbar-controls">
            <details className="appearance-menu" onKeyDown={(e) => { if (e.key === "Escape") { e.currentTarget.open = false; e.currentTarget.querySelector("summary")?.focus(); } }} onBlur={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node | null)) e.currentTarget.open = false; }}>
              <summary><Icon name="settings" />{t("ui.appearance")}</summary>
              <div className="appearance-popover">
                <span className="field-label">{t("topbar.theme")}</span>
                <div className="seg" role="group" aria-label={t("topbar.theme")}>
                  {THEMES.map((th) => <button key={th.key} className={theme === th.key ? "seg-btn active" : "seg-btn"} aria-pressed={theme === th.key} onClick={() => setTheme(th.key)}>{t(th.labelKey)}</button>)}
                </div>
                <span className="field-label">{t("topbar.lang")}</span>
                <div className="seg" role="group" aria-label={t("topbar.lang")}>
                  {(["zh", "en"] as Lang[]).map((l) => <button key={l} className={lang === l ? "seg-btn active" : "seg-btn"} aria-pressed={lang === l} onClick={() => setLang(l)}>{l === "zh" ? "中文" : "English"}</button>)}
                </div>
              </div>
            </details>
          </div>
        </header>

        <fieldset className={`page project-page ${tab === "workbench" ? "workspace-page" : "document-page"}`} disabled={switching} key={projectRevision}>
          {error ? <div className="banner banner-err">{error}</div> : null}

          <div className="workbench-host" hidden={tab !== "workbench"}>
            <ChapterWorkbench
              key={`${root}:${sourceKey}`}
              focusMode={focusMode}
              onFocusChange={setFocusMode}
              projectSwitcher={<ProjectSwitcher root={root} onOpen={openProject} />}
              promptRevision={promptRevision}
              chapters={chapters}
              chapterDetails={chapterDetails}
              projectRoot={root}
              selectChapter={selectChapter}
              currentChapter={currentChapter}
              setCurrentChapter={setCurrentChapter}
              refreshChapters={refreshChapters}
            />
          </div>
          {tab === "reports" && (
            <Reports
              chapters={chapters}
              chapterDetails={chapterDetails}
              currentChapter={currentChapter}
              setCurrentChapter={selectChapter}
            />
          )}
          <div hidden={tab !== "memory"}><MemoryEditor /></div>
          <div hidden={tab !== "prompts"}><PromptSettingsPanel key={root} onSaved={() => setPromptRevision((n) => n + 1)} /></div>
          <div hidden={tab !== "settings"}><SettingsPanel root={root} onSaved={async () => {
            setPromptRevision((n) => n + 1);
            const nextRoot = await api.getPipelineRoot();
            const source = await api.getSourceSettings();
            if (nextRoot !== root || source.directory !== sourceKey) { setRoot(nextRoot); setSourceKey(source.directory); setCurrentChapter(""); }
            await refreshChapters();
          }} /></div>
          {tab === "guide" && <Guide />}
        </fieldset>
      </main>
    </div>
  );
}
