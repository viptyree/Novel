import { useLayoutEffect, useState } from "react";
import { api, ChapterDetails } from "../api";
import { useT } from "../i18n";
import { useProjectActivity } from "../projectActivity";
import { Banner, Field, Spinner } from "./common";

interface Props {
  projectRoot: string;
  chapter: ChapterDetails;
  content: string;
  disabled: boolean;
  onBusyChange: (busy: boolean) => void;
  onSaved: () => Promise<void>;
}

export default function ChapterTitlePanel({ projectRoot, chapter, content, disabled, onBusyChange, onSaved }: Props) {
  const t = useT();
  const [saved, setSaved] = useState(chapter);
  const [title, setTitle] = useState(chapter.title);
  const [candidates, setCandidates] = useState<string[]>([]);
  const [candidateSource, setCandidateSource] = useState("");
  const [busy, setBusy] = useState<"" | "generate" | "save">("");
  const [banner, setBanner] = useState<{ kind: "ok" | "err"; msg: string } | null>(null);
  const dirty = title !== saved.title;
  const running = busy !== "";
  useProjectActivity("chapter-title", dirty, running);
  useLayoutEffect(() => { onBusyChange(running); return () => onBusyChange(false); }, [running, onBusyChange]);

  async function generate() {
    if (disabled || running || !content.trim()) return;
    const snapshot = content;
    setBusy("generate"); setBanner(null);
    try {
      const next = await api.generateChapterTitles(projectRoot, chapter.id, snapshot, [...candidates, saved.title].filter(Boolean));
      setCandidates(next); setCandidateSource(snapshot);
    } catch (e) { setBanner({ kind: "err", msg: String(e) }); }
    finally { setBusy(""); }
  }

  async function save() {
    if (disabled || running || !title.trim()) return;
    setBusy("save"); setBanner(null);
    try {
      const next = await api.saveChapterTitle(projectRoot, chapter.id, title.trim(), saved.path);
      setSaved(next); setTitle(next.title);
      setBanner({ kind: "ok", msg: t("ct.saved") });
      await onSaved();
    } catch (e) { setBanner({ kind: "err", msg: String(e) }); }
    finally { setBusy(""); }
  }

  return <div className="chapter-title-panel">
    <Field label={t("ct.title")} hint={t("ct.hint")}>
      <div className="chapter-title-input">
        <input aria-label={t("ct.title")} value={title} maxLength={40} placeholder={t("ct.placeholder")}
          disabled={disabled || running} onChange={(e) => { setTitle(e.target.value); setBanner(null); }} />
        <button disabled={disabled || running || !content.trim()} onClick={generate}>
          {busy === "generate" && <Spinner />}{t(candidates.length ? "ct.regenerate" : "ct.generate")}
        </button>
        <button disabled={disabled || running || !title.trim() || !dirty} onClick={save}>
          {busy === "save" && <Spinner />}{t("ct.save")}{dirty ? " *" : ""}
        </button>
      </div>
    </Field>
    {candidates.length > 0 && <div className="chapter-title-candidates" role="group" aria-label={t("ct.candidates")}>
      <span className="muted">{t("ct.candidates")}</span>
      {candidates.map((candidate) => <button key={candidate} disabled={disabled || running}
        aria-pressed={candidate === title} className={candidate === title ? "selected" : ""}
        onClick={() => { setTitle(candidate); setBanner(null); }}>{candidate}</button>)}
    </div>}
    {candidates.length > 0 && candidateSource !== content && <Banner kind="info">{t("ct.stale")}</Banner>}
    {banner && <Banner kind={banner.kind}>{banner.msg}</Banner>}
    <p className="muted source-path">{t("ct.file")}: {saved.path}</p>
  </div>;
}
