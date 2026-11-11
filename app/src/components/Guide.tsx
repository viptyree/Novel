import { useEffect, useRef, useState } from "react";
import { marked } from "marked";
import { useUi } from "../i18n";
import guideZh from "../guide.md?raw";
import guideEn from "../guide.en.md?raw";

export default function Guide() {
  const { lang, t } = useUi();
  const [html, setHtml] = useState("");
  const [headings, setHeadings] = useState<{ id: string; text: string }[]>([]);
  const article = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let active = true;
    Promise.resolve(marked.parse(lang === "en" ? guideEn : guideZh)).then((value) => { if (active) setHtml(value); });
    return () => { active = false; };
  }, [lang]);
  useEffect(() => {
    const items = Array.from(article.current?.querySelectorAll("h2") ?? []).map((node, i) => {
      node.id = `guide-section-${i}`;
      return { id: node.id, text: node.textContent ?? "" };
    });
    setHeadings(items);
  }, [html]);
  return <div className="guide">
    <nav className="guide-toc" aria-label={t("ui.contents")}>
      <h2>{t("ui.contents")}</h2>
      {headings.map((heading) => <a key={heading.id} href={`#${heading.id}`} onClick={(e) => { e.preventDefault(); document.getElementById(heading.id)?.scrollIntoView({ block: "start" }); }}>{heading.text}</a>)}
    </nav>
    <div ref={article} className="markdown" dangerouslySetInnerHTML={{ __html: html }} />
  </div>;
}
