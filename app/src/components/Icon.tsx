export type IconName = "workbench" | "reports" | "memory" | "prompts" | "settings" | "guide" | "panel" | "spark" | "focus" | "plus" | "close" | "search";

const paths: Record<IconName, string> = {
  workbench: "M4 20h16M6 16l1-5L17 1l5 5-10 10-6 1M15 3l5 5",
  reports: "M6 3h12v18H6zM9 8h6M9 12h6M9 16h4",
  memory: "M4 4h6l2 2 2-2h6v15h-6l-2 2-2-2H4zM12 6v15",
  prompts: "M4 4h16v13H9l-5 4zM8 8h8M8 12h5",
  settings: "M4 7h16M4 17h16M8 4v6M16 14v6",
  guide: "M9 9a3 3 0 1 1 5 2c-2 1-2 2-2 3M12 17h.01M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20",
  panel: "M3 4h18v16H3zM9 4v16",
  spark: "m12 3 2.5 6.5L21 12l-6.5 2.5L12 21l-2.5-6.5L3 12l6.5-2.5z",
  focus: "M8 3H3v5M16 3h5v5M3 16v5h5M21 16v5h-5",
  plus: "M12 5v14M5 12h14",
  close: "m6 6 12 12M6 18 18 6",
  search: "M10 3a7 7 0 1 0 0 14 7 7 0 0 0 0-14m5 12 6 6",
};

export default function Icon({ name }: { name: IconName }) {
  return <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={paths[name]} /></svg>;
}
