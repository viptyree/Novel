import { useEffect, useRef, useState } from "react";
import { marked } from "marked";
import { api, ChapterDetails, ReportKind } from "../api";
import { useT } from "../i18n";
import { Banner } from "./common";

interface Props {
  chapters: string[];
  chapterDetails: ChapterDetails[];
  currentChapter: string;
  setCurrentChapter: (id: string) => void;
}

const KINDS: { key: ReportKind; labelKey: string }[] = [
  { key: "generation", labelKey: "rp.generation" },
  { key: "consistency", labelKey: "rp.consistency" },
  { key: "memory", labelKey: "rp.memory" },
];

export default function Reports({ chapters, chapterDetails, currentChapter, setCurrentChapter }: Props) {
  const t = useT();
  const [kind, setKind] = useState<ReportKind>("consistency");
  const [html, setHtml] = useState("");
  const request = useRef(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [empty, setEmpty] = useState(false);

  async function load() {
    const revision = ++request.current;
    setError(""); setHtml(""); setEmpty(false);
    if (!currentChapter) { setLoading(false); return; }
    setLoading(true);
    try {
      const md = await api.readReport(currentChapter, kind);
      const rendered = await marked.parse(md);
      if (revision !== request.current) return;
      setHtml(rendered); setEmpty(!md.trim());
    } catch (e) {
      if (revision === request.current) setError(String(e));
    } finally { if (revision === request.current) setLoading(false); }
  }

  useEffect(() => {
    void load();
    return () => { request.current += 1; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentChapter, kind]);

  return (
    <div className="reports">
      <div className="reports-bar">
        <select aria-label={t("rp.selectChapter")} value={currentChapter} onChange={(e) => setCurrentChapter(e.target.value)}>
          <option value="">{t("rp.selectChapter")}</option>
          {chapters.map((c) => (
            <option key={c} value={c}>
              {t("ct.chapter", { number: Number(c.slice(2)) })}{chapterDetails.find((chapter) => chapter.id === c)?.title ? ` · ${chapterDetails.find((chapter) => chapter.id === c)?.title}` : ""}
            </option>
          ))}
        </select>
        <div className="tabs">
          {KINDS.map((k) => (
            <button
              key={k.key}
              className={kind === k.key ? "tab active" : "tab"}
              onClick={() => setKind(k.key)}
            >
              {t(k.labelKey)}
            </button>
          ))}
        </div>
        <button className="ghost" onClick={load}>
          {t("rp.refresh")}
        </button>
      </div>

      {!currentChapter ? (
        <Banner kind="info">{t("rp.pickChapter")}</Banner>
      ) : loading ? (
        <Banner kind="info">{t("ui.loading")}</Banner>
      ) : error ? (
        <Banner kind="err">{t("ui.reportError")} {error}</Banner>
      ) : empty ? (
        <Banner kind="info">{t("rp.empty")}</Banner>
      ) : (
        <div className="markdown" dangerouslySetInnerHTML={{ __html: html }} />
      )}
    </div>
  );
}
