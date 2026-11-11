import { ReactNode, useState } from "react";
import { ChapterDetails } from "../api";
import { useT } from "../i18n";
import Icon from "./Icon";
import { Field } from "./common";

interface Props {
  open: boolean;
  inert: boolean;
  running: boolean;
  projectSwitcher: ReactNode;
  chapters: string[];
  chapterDetails: ChapterDetails[];
  currentChapter: string;
  selectChapter: (id: string) => void;
  newId: string;
  setNewId: (id: string) => void;
  nextChapterId: () => string;
  onStartNew: () => void;
}

export default function ChapterDirectory({ open, inert, running, projectSwitcher, chapters, chapterDetails, currentChapter, selectChapter, newId, setNewId, nextChapterId, onStartNew }: Props) {
  const t = useT();
  const [query, setQuery] = useState("");
  return (<aside inert={inert} className="chapter-directory" hidden={!open} aria-label={t("ui.directory")}>
        {projectSwitcher}
        <div className="directory-heading"><h2>{t("ui.directory")}</h2><span className="count-badge">{chapters.length}</span></div>
        <label className="chapter-search"><Icon name="search" /><input aria-label={t("ui.search")} placeholder={t("ui.search")} value={query} onChange={(e) => setQuery(e.target.value)} /></label>
        <details className="new-chapter">
          <summary><Icon name="plus" />{t("ui.newChapter")}</summary>
          <Field label={t("wb.newId")} hint={t("wb.newIdHint")}>
            <input disabled={running} value={newId} placeholder={t("wb.newIdPlaceholder")} onChange={(e) => setNewId(e.target.value)} />
          </Field>
          <div className="actions wrap">
            <button disabled={running} onClick={() => setNewId(nextChapterId())}>{t("wb.autoNumber")}</button>
            <button className="primary" disabled={running || !newId.trim()} onClick={onStartNew}>{t("ui.ai")}</button>
          </div>
        </details>
        <div className="chapter-list">
          {chapterDetails.filter((c) => `${c.id} ${c.title} ${Number(c.id.slice(2))}`.toLocaleLowerCase().includes(query.toLocaleLowerCase())).map((c) => <button key={c.id} disabled={running} className={`chapter-item${currentChapter === c.id ? " active" : ""}`} aria-current={currentChapter === c.id ? "true" : undefined} onClick={() => selectChapter(c.id)}>
            <span className="chapter-number">{t("ct.chapter", { number: Number(c.id.slice(2)) })}</span><span className="chapter-name">{c.title || c.id}</span>
          </button>)}
          {query && !chapterDetails.some((c) => `${c.id} ${c.title} ${Number(c.id.slice(2))}`.toLocaleLowerCase().includes(query.toLocaleLowerCase())) && <p className="empty-hint">{t("ui.noMatches")}</p>}
          {!chapters.length && <p className="empty-hint">{t("ui.welcomeHint")}</p>}
        </div>
      </aside>);
}
